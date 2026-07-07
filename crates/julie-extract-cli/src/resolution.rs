//! Workspace reference-resolution tier chain (pure logic, no DB, no I/O).
//!
//! This module is the resolver *policy* seam described in the design doc
//! (`docs/plans/2026-07-06-workspace-reference-resolution-design.md`,
//! §"Resolution tiers" and §"Module placement & interface"). It owns language
//! semantics (kind compatibility, the tier-2 import gate, the tier-3 receiver
//! chain) and never touches storage: the artifact crate stays pure storage and
//! only supplies the row types this module consumes.
//!
//! The caller-facing surface is intentionally small:
//! * [`UnresolvedEdge`] — one pending relationship OR one bare identifier,
//!   abstracted to the fields the tier chain needs.
//! * [`WorkspaceCandidateIndex`] — built once per pass from in-memory symbol /
//!   type-fact / import rows.
//! * [`TierOutcome`] — the result of running the chain for one edge.
//! * [`resolve_one`] — the pure entry point.
//!
//! ## Tier chain (verbatim from the design's tier table)
//!
//! | Tier | Signal | Confidence |
//! |------|--------|------------|
//! | 1 same-file | already materialized at extraction time (not run here) | 0.95 |
//! | 2 import-guided | candidate reachable through an import in the source file | 0.85 |
//! | 3 receiver-typed | receiver → scoped symbol → `type_facts` → type → member | 0.75 (0.65 inferred) |
//! | 4 unique-language-global | exactly one kind-compatible candidate workspace-wide | 0.55 |
//!
//! Every tier is an **independent filter** over kind-compatible, same-language
//! candidates. The edge resolves at the FIRST tier (in order) whose candidate set
//! is exactly one. If no tier yields exactly one, the outcome is `Ambiguous` when
//! any tier yielded >= 2 and `Missing` when every attempted tier yielded 0. There
//! is NO best-guess selection anywhere: a wrong edge is worse than a missing one.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use julie_extract_artifact::resolution_store::{IdentifierWorkItem, PendingWorkItem};
use julie_extractors::SymbolKind;

// ---------------------------------------------------------------------------
// Confidence + method constants (contract — see the design's tier table)
// ---------------------------------------------------------------------------

/// Tier-1 same-file confidence (materialized at extraction time, not by this
/// module; defined here so the constant set is complete and single-sourced).
pub const CONFIDENCE_TIER1: f64 = 0.95;
/// Tier-2 import-guided confidence.
pub const CONFIDENCE_TIER2: f64 = 0.85;
/// Tier-3 receiver-typed confidence (concrete type fact).
pub const CONFIDENCE_TIER3: f64 = 0.75;
/// Tier-3 receiver-typed confidence when the receiver's `type_facts.is_inferred`.
pub const CONFIDENCE_TIER3_INFERRED: f64 = 0.65;
/// Tier-4 unique-language-global confidence.
pub const CONFIDENCE_TIER4: f64 = 0.55;

/// `method` string stamped on a tier-2 resolution.
pub const METHOD_TIER2: &str = "tier2_import";
/// `method` string stamped on a tier-3 resolution.
pub const METHOD_TIER3: &str = "tier3_receiver";
/// `method` string stamped on a tier-4 resolution.
pub const METHOD_TIER4: &str = "tier4_global";

/// Languages whose import-row metadata has a fixture-tested contract that tier 2
/// can key on.
///
/// Import fact shapes vary per language (design §"Resolution tiers", tier-2 row,
/// round-2 finding 5): TypeScript records `source`/`importedName`, Python records
/// nothing usable, Dart stores only the URI. Tier 2 is therefore enabled per
/// language ONLY where a fixture proves the contract; everywhere else it is
/// skipped and the pass reports a capability gap (`reference_resolution.tier2_import`)
/// until F4 normalizes import facts into first-class rows.
///
/// The TypeScript/JavaScript extractor records both the local binding and the
/// module specifier, which is the contract tier 2 relies on. Membership here is
/// the policy switch; the per-language fixture evidence that gates it lands in
/// Task 6 (`fixtures/extraction/<lang>/…` import-contract cases). Adding a
/// language to this list without that fixture evidence is a data-quality-bar
/// violation.
pub const TIER2_IMPORT_LANGUAGES: &[&str] = &["typescript", "javascript"];

/// Type-like symbol kinds: the target set for `uses`/`extends`/`implements`/type
/// edges and identifier `type_usage` (design §"Resolution tiers", kind
/// compatibility). Domain-specific type carriers (`Delegate` in C#) are included
/// because they name types the grammar treats as type references.
const TYPE_LIKE_KINDS: &[SymbolKind] = &[
    SymbolKind::Class,
    SymbolKind::Interface,
    SymbolKind::Struct,
    SymbolKind::Enum,
    SymbolKind::Type,
    SymbolKind::Trait,
    SymbolKind::Union,
    SymbolKind::Delegate,
];

// ---------------------------------------------------------------------------
// Reference kind + origin
// ---------------------------------------------------------------------------

/// Where an [`UnresolvedEdge`] came from. The origin selects the tier chain:
/// pending rows run tiers 2→4 (tier 1 is already materialized), identifiers run a
/// reduced chain with no tier 3 (no receiver context exists on identifiers today).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeOrigin {
    /// A pending relationship row (`pending_relationships`).
    Pending,
    /// A bare identifier row (`identifiers`).
    Identifier,
}

/// The semantic reference kind, abstracted across relationship kinds (pending
/// rows) and identifier kinds. This is the axis the kind-compatibility map and the
/// tier-4 restrictions key on.
///
/// `uses`, `extends`, `implements` and any other type edge all collapse to
/// [`ReferenceKind::TypeUsage`] because they share the same compatible-kind set
/// and the same tier enablement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceKind {
    /// A call: relationship `calls` or identifier `call`. Targets
    /// Function/Method/Constructor (tiers 1–3); tier 4 restricts to
    /// Function/Constructor (method calls are disabled at tier 4).
    Call,
    /// An instantiation: relationship `instantiates`. Targets
    /// Class/Struct/Constructor.
    Instantiates,
    /// A type reference: relationship `uses`/`extends`/`implements` or identifier
    /// `type_usage`. Targets type-like kinds.
    TypeUsage,
    /// A member access: identifier `member_access`. Targets Property/Field/Method
    /// and is enabled at tiers 1–3 only (member names collide too heavily for a
    /// global-uniqueness signal to mean anything).
    MemberAccess,
}

impl ReferenceKind {
    /// Map a pending relationship `kind` string to a resolvable reference kind.
    /// Returns `None` for relationship kinds the workspace chain does not resolve
    /// (e.g. `imports`, `references`, `contains`) — the caller records those as
    /// no-context.
    pub fn from_relationship_kind(kind: &str) -> Option<Self> {
        match kind {
            "calls" => Some(ReferenceKind::Call),
            "instantiates" => Some(ReferenceKind::Instantiates),
            "uses" | "extends" | "implements" => Some(ReferenceKind::TypeUsage),
            _ => None,
        }
    }

    /// Map an identifier `kind` string to a resolvable reference kind.
    pub fn from_identifier_kind(kind: &str) -> Option<Self> {
        match kind {
            "call" => Some(ReferenceKind::Call),
            "type_usage" => Some(ReferenceKind::TypeUsage),
            "member_access" => Some(ReferenceKind::MemberAccess),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// UnresolvedEdge
// ---------------------------------------------------------------------------

/// One reference site to resolve — a pending relationship or a bare identifier,
/// reduced to the fields the tier chain needs. Deliberately free of span-join
/// assumptions (a pending row's span is the whole call/expression node, not the
/// callee identifier — that matters to Task 5's identifier propagation join, not
/// to this tier logic).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnresolvedEdge {
    /// Pending vs identifier — selects the tier chain.
    pub origin: EdgeOrigin,
    /// The semantic reference kind.
    pub kind: ReferenceKind,
    /// Language of the reference site. Every tier filters candidates to this
    /// language.
    pub language: String,
    /// The source file's id (`file_id`).
    pub file_id: String,
    /// The terminal (rightmost) name of the reference — the callee/type/member
    /// name being resolved.
    pub terminal_name: String,
    /// The receiver expression's name for member/method references, if any.
    pub receiver: Option<String>,
    /// The caller's scope symbol id: `caller_scope_symbol_id` for pending rows,
    /// `containing_symbol_id` for identifiers. Anchors the tier-3 scope walk.
    pub caller_scope_symbol_id: Option<String>,
    /// Free-string import context recorded by the extractor. Corroborating
    /// evidence only, never the sole tier-2 key (design §"Resolution tiers").
    pub import_context: Option<String>,
}

impl UnresolvedEdge {
    /// Build an edge from a pending relationship work item. Returns `None` when
    /// the relationship kind is not one the workspace chain resolves.
    pub fn from_pending(item: &PendingWorkItem) -> Option<Self> {
        let kind = ReferenceKind::from_relationship_kind(&item.kind)?;
        Some(UnresolvedEdge {
            origin: EdgeOrigin::Pending,
            kind,
            language: item.language.clone(),
            file_id: item.file_id.clone(),
            terminal_name: item.target_terminal_name.clone(),
            receiver: item.target_receiver.clone(),
            caller_scope_symbol_id: item.caller_scope_symbol_id.clone(),
            import_context: item.target_import_context.clone(),
        })
    }

    /// Build an edge from an identifier work item. Returns `None` when the
    /// identifier kind is not one the reduced chain resolves.
    pub fn from_identifier(item: &IdentifierWorkItem) -> Option<Self> {
        let kind = ReferenceKind::from_identifier_kind(&item.kind)?;
        Some(UnresolvedEdge {
            origin: EdgeOrigin::Identifier,
            kind,
            language: item.language.clone(),
            file_id: item.file_id.clone(),
            terminal_name: item.name.clone(),
            // Identifiers carry no receiver context today (F1 adds it).
            receiver: None,
            caller_scope_symbol_id: item.containing_symbol_id.clone(),
            import_context: None,
        })
    }
}

// ---------------------------------------------------------------------------
// TierOutcome
// ---------------------------------------------------------------------------

/// The result of running the tier chain for a single edge.
#[derive(Debug, Clone, PartialEq)]
pub enum TierOutcome {
    /// Resolved at `tier` with `confidence` and a `method` provenance string.
    Resolved {
        target_symbol_id: String,
        tier: u8,
        confidence: f64,
        method: String,
    },
    /// Some tier yielded >= 2 kind-compatible candidates and no tier yielded
    /// exactly one. `candidates` is ordered by `symbol_id` for determinism.
    Ambiguous { candidates: Vec<String> },
    /// Every attempted tier yielded zero candidates.
    Missing,
    /// No tier was applicable to this edge (e.g. identifier `member_access`, which
    /// has no reduced chain today), or the edge carried no terminal name.
    NoContext,
}

// ---------------------------------------------------------------------------
// Candidate index input rows (owned; Task 5 maps DB rows into these)
// ---------------------------------------------------------------------------

/// A workspace symbol the resolver may resolve *to*.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateSymbol {
    pub symbol_id: String,
    pub file_id: String,
    pub language: String,
    pub name: String,
    pub kind: SymbolKind,
    pub parent_symbol_id: Option<String>,
}

/// A `type_facts` row: the resolved type of a symbol (receiver typing for tier 3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeFact {
    pub symbol_id: String,
    pub resolved_type: String,
    pub is_inferred: bool,
}

/// An import brought into a source file, reduced to the fields tier 2 keys on.
///
/// `module_file_id` is the defining file the module specifier resolves to, when
/// the extractor/Task-5 could map it; `imported_name` is the original exported
/// name when it differs from the local binding (aliased imports).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportRecord {
    pub file_id: String,
    pub local_name: String,
    pub imported_name: Option<String>,
    pub module_file_id: Option<String>,
}

// ---------------------------------------------------------------------------
// WorkspaceCandidateIndex
// ---------------------------------------------------------------------------

/// In-memory candidate index built once per resolution pass. Owns the candidate
/// rows and precomputes the name/parent/file lookups the tier chain needs.
pub struct WorkspaceCandidateIndex {
    symbols: Vec<CandidateSymbol>,
    by_id: HashMap<String, usize>,
    by_name: HashMap<String, Vec<usize>>,
    children_by_parent: HashMap<String, Vec<usize>>,
    top_level_by_file: HashMap<String, Vec<usize>>,
    type_facts_by_symbol: HashMap<String, Vec<TypeFact>>,
    imports_by_file: HashMap<String, Vec<ImportRecord>>,
}

impl WorkspaceCandidateIndex {
    /// Build the index from in-memory rows. All lookup vectors are sorted by
    /// `symbol_id` so candidate enumeration is deterministic before the
    /// exactly-one test.
    pub fn build(
        mut symbols: Vec<CandidateSymbol>,
        type_facts: Vec<TypeFact>,
        imports: Vec<ImportRecord>,
    ) -> Self {
        // Sort symbols by id so every derived vector inherits a stable order.
        symbols.sort_by(|a, b| a.symbol_id.cmp(&b.symbol_id));

        let mut by_id = HashMap::new();
        let mut by_name: HashMap<String, Vec<usize>> = HashMap::new();
        let mut children_by_parent: HashMap<String, Vec<usize>> = HashMap::new();
        let mut top_level_by_file: HashMap<String, Vec<usize>> = HashMap::new();

        for (idx, sym) in symbols.iter().enumerate() {
            by_id.insert(sym.symbol_id.clone(), idx);
            by_name.entry(sym.name.clone()).or_default().push(idx);
            match &sym.parent_symbol_id {
                Some(parent) => children_by_parent
                    .entry(parent.clone())
                    .or_default()
                    .push(idx),
                None => top_level_by_file
                    .entry(sym.file_id.clone())
                    .or_default()
                    .push(idx),
            }
        }

        let mut type_facts_by_symbol: HashMap<String, Vec<TypeFact>> = HashMap::new();
        for fact in type_facts {
            type_facts_by_symbol
                .entry(fact.symbol_id.clone())
                .or_default()
                .push(fact);
        }

        let mut imports_by_file: HashMap<String, Vec<ImportRecord>> = HashMap::new();
        for import in imports {
            imports_by_file
                .entry(import.file_id.clone())
                .or_default()
                .push(import);
        }

        WorkspaceCandidateIndex {
            symbols,
            by_id,
            by_name,
            children_by_parent,
            top_level_by_file,
            type_facts_by_symbol,
            imports_by_file,
        }
    }

    fn symbol_by_id(&self, id: &str) -> Option<&CandidateSymbol> {
        self.by_id.get(id).map(|&idx| &self.symbols[idx])
    }

    fn by_name(&self, name: &str) -> impl Iterator<Item = &CandidateSymbol> + '_ {
        self.by_name
            .get(name)
            .into_iter()
            .flat_map(move |idxs| idxs.iter().map(move |&idx| &self.symbols[idx]))
    }

    /// Children of `parent_id` named `name`. Returns a `Vec` (eager) so the
    /// borrow is tied only to `self`, not to the caller-supplied `name`.
    fn children_named(&self, parent_id: &str, name: &str) -> Vec<&CandidateSymbol> {
        self.children_by_parent
            .get(parent_id)
            .into_iter()
            .flat_map(|idxs| idxs.iter().map(|&idx| &self.symbols[idx]))
            .filter(|sym| sym.name == name)
            .collect()
    }

    /// File top-level (parentless) symbols named `name`.
    fn top_level_named(&self, file_id: &str, name: &str) -> Vec<&CandidateSymbol> {
        self.top_level_by_file
            .get(file_id)
            .into_iter()
            .flat_map(|idxs| idxs.iter().map(|&idx| &self.symbols[idx]))
            .filter(|sym| sym.name == name)
            .collect()
    }

    fn type_facts(&self, symbol_id: &str) -> &[TypeFact] {
        self.type_facts_by_symbol
            .get(symbol_id)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    fn imports(&self, file_id: &str) -> &[ImportRecord] {
        self.imports_by_file
            .get(file_id)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
}

// ---------------------------------------------------------------------------
// Tier identity
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tier {
    Import,
    Receiver,
    Global,
}

impl Tier {
    fn number(self) -> u8 {
        match self {
            Tier::Import => 2,
            Tier::Receiver => 3,
            Tier::Global => 4,
        }
    }

    fn method(self) -> &'static str {
        match self {
            Tier::Import => METHOD_TIER2,
            Tier::Receiver => METHOD_TIER3,
            Tier::Global => METHOD_TIER4,
        }
    }
}

/// Whether tier 2 (import-guided) is enabled for `language`. Data-driven gate
/// over [`TIER2_IMPORT_LANGUAGES`]; Task 5 uses this to record a
/// `reference_resolution.tier2_import` capability gap for languages that skip
/// tier 2.
pub fn tier2_enabled(language: &str) -> bool {
    TIER2_IMPORT_LANGUAGES.contains(&language)
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Resolve a single edge against the workspace candidate index by running its
/// tier chain. Pure: no DB, no I/O, deterministic.
///
/// Each applicable tier is an independent filter; the edge resolves at the FIRST
/// tier (in order) whose kind-compatible, same-language candidate set is exactly
/// one. If none yields exactly one: `Ambiguous` when any tier yielded >= 2,
/// `Missing` when every attempted tier yielded 0, `NoContext` when no tier was
/// applicable (or every applicable tier was gated off). No best-guess selection.
pub fn resolve_one(edge: &UnresolvedEdge, index: &WorkspaceCandidateIndex) -> TierOutcome {
    if edge.terminal_name.is_empty() {
        return TierOutcome::NoContext;
    }
    let tiers = applicable_tiers(edge);
    if tiers.is_empty() {
        return TierOutcome::NoContext;
    }

    let mut attempted_any = false;
    let mut first_ambiguous: Option<Vec<String>> = None;

    for tier in tiers {
        // Tier-2 language gate: skipped (not attempted) where no fixture-tested
        // import contract exists. Task 5 records the capability gap.
        if tier == Tier::Import && !tier2_enabled(&edge.language) {
            continue;
        }
        attempted_any = true;

        let candidates = tier_candidates(tier, edge, index);
        match candidates.as_slice() {
            [] => {}
            [only] => {
                return TierOutcome::Resolved {
                    target_symbol_id: only.symbol_id.clone(),
                    tier: tier.number(),
                    confidence: only.confidence,
                    method: tier.method().to_string(),
                };
            }
            many => {
                if first_ambiguous.is_none() {
                    first_ambiguous = Some(many.iter().map(|c| c.symbol_id.clone()).collect());
                }
            }
        }
    }

    match first_ambiguous {
        Some(candidates) => TierOutcome::Ambiguous { candidates },
        None if attempted_any => TierOutcome::Missing,
        None => TierOutcome::NoContext,
    }
}

// ---------------------------------------------------------------------------
// Tier chain internals
// ---------------------------------------------------------------------------

/// One kind-compatible candidate produced by a tier, already carrying that tier's
/// confidence (tier 3 varies it by `is_inferred`).
struct TierCandidate {
    symbol_id: String,
    confidence: f64,
}

/// The ordered tier chain for an edge (design §"Resolution tiers" + §"Data flow"
/// step 4). Pending rows run tiers 2→4 (tier 1 already materialized); identifiers
/// run a reduced chain with no tier 3.
fn applicable_tiers(edge: &UnresolvedEdge) -> Vec<Tier> {
    use EdgeOrigin::*;
    use ReferenceKind::*;
    match (edge.origin, edge.kind) {
        // Pending: full workspace chain (tier 4 disabled only for member_access).
        (Pending, Call | Instantiates | TypeUsage) => {
            vec![Tier::Import, Tier::Receiver, Tier::Global]
        }
        (Pending, MemberAccess) => vec![Tier::Import, Tier::Receiver],
        // Identifiers: reduced chains, no tier 3. `Instantiates` is not an
        // identifier kind (never constructed) but is covered for exhaustiveness.
        (Identifier, Call | TypeUsage) => vec![Tier::Import, Tier::Global],
        (Identifier, MemberAccess | Instantiates) => vec![],
    }
}

fn tier_candidates(
    tier: Tier,
    edge: &UnresolvedEdge,
    index: &WorkspaceCandidateIndex,
) -> Vec<TierCandidate> {
    match tier {
        Tier::Import => tier2_candidates(edge, index),
        Tier::Receiver => tier3_candidates(edge, index),
        Tier::Global => tier4_candidates(edge, index),
    }
}

/// Tier 2: candidates reachable through an import in the source file. Two keys
/// (design §"Resolution tiers"): (A) an import whose local binding matches the
/// terminal name (aliases key on `imported_name`), and (B) an import whose module
/// resolves to the candidate's defining file, referenced by the candidate's own
/// name. Both require same language and kind compatibility.
fn tier2_candidates(edge: &UnresolvedEdge, index: &WorkspaceCandidateIndex) -> Vec<TierCandidate> {
    let kinds = tier123_compatible_kinds(edge.kind);
    let mut set: BTreeSet<String> = BTreeSet::new();

    for import in index.imports(&edge.file_id) {
        // Branch A: named / aliased import brings `terminal_name` into scope.
        if import.local_name == edge.terminal_name {
            let target_name = import
                .imported_name
                .as_deref()
                .unwrap_or(&import.local_name);
            for cand in index.by_name(target_name) {
                let module_ok = import
                    .module_file_id
                    .as_deref()
                    .map_or(true, |m| m == cand.file_id);
                if cand.language == edge.language && kinds.contains(&cand.kind) && module_ok {
                    set.insert(cand.symbol_id.clone());
                }
            }
        }
        // Branch B: module import reaches a candidate by its own terminal name
        // (namespace / wildcard imports where the local binding differs).
        if let Some(module_file) = import.module_file_id.as_deref() {
            for cand in index.by_name(&edge.terminal_name) {
                if cand.language == edge.language
                    && kinds.contains(&cand.kind)
                    && cand.file_id == module_file
                {
                    set.insert(cand.symbol_id.clone());
                }
            }
        }
    }

    set.into_iter()
        .map(|symbol_id| TierCandidate {
            symbol_id,
            confidence: CONFIDENCE_TIER2,
        })
        .collect()
}

/// Tier 3: receiver name → scoped symbol → `type_facts.resolved_type` → unique
/// same-language type symbol → member with the terminal name. Confidence drops to
/// 0.65 when the contributing type fact `is_inferred`.
fn tier3_candidates(edge: &UnresolvedEdge, index: &WorkspaceCandidateIndex) -> Vec<TierCandidate> {
    let Some(receiver) = edge.receiver.as_deref() else {
        return Vec::new();
    };
    if receiver.is_empty() {
        return Vec::new();
    }

    let receiver_symbols = resolve_receiver_symbols(edge, index, receiver);
    if receiver_symbols.is_empty() {
        return Vec::new();
    }

    let member_kinds = tier123_compatible_kinds(edge.kind);
    // symbol_id -> is_inferred; BTreeMap keeps the result ordered by symbol_id.
    // When a member is reachable via both a concrete and an inferred type fact,
    // prefer the concrete (higher) confidence.
    let mut members: BTreeMap<String, bool> = BTreeMap::new();

    for receiver_symbol in receiver_symbols {
        for fact in index.type_facts(&receiver_symbol.symbol_id) {
            let Some(type_symbol) = unique_type_symbol(index, &fact.resolved_type, &edge.language)
            else {
                continue;
            };
            for member in index.children_named(&type_symbol.symbol_id, &edge.terminal_name) {
                if member.language == edge.language && member_kinds.contains(&member.kind) {
                    let entry = members
                        .entry(member.symbol_id.clone())
                        .or_insert(fact.is_inferred);
                    if !fact.is_inferred {
                        *entry = false;
                    }
                }
            }
        }
    }

    members
        .into_iter()
        .map(|(symbol_id, is_inferred)| TierCandidate {
            symbol_id,
            confidence: if is_inferred {
                CONFIDENCE_TIER3_INFERRED
            } else {
                CONFIDENCE_TIER3
            },
        })
        .collect()
}

/// Tier 4: exactly one kind-compatible candidate in the same language
/// workspace-wide. Tier-4 kind compatibility is stricter for calls
/// (Function/Constructor only — method calls disabled) and empty for
/// member_access (never reached — member_access excludes tier 4 from its chain).
fn tier4_candidates(edge: &UnresolvedEdge, index: &WorkspaceCandidateIndex) -> Vec<TierCandidate> {
    let kinds = tier4_compatible_kinds(edge.kind);
    if kinds.is_empty() {
        return Vec::new();
    }
    let mut set: BTreeSet<String> = BTreeSet::new();
    for cand in index.by_name(&edge.terminal_name) {
        if cand.language == edge.language && kinds.contains(&cand.kind) {
            set.insert(cand.symbol_id.clone());
        }
    }
    set.into_iter()
        .map(|symbol_id| TierCandidate {
            symbol_id,
            confidence: CONFIDENCE_TIER4,
        })
        .collect()
}

/// Resolve the receiver name to symbol(s) in scope: walk the caller's scope chain
/// (nearest scope first — locals, then enclosing-type fields as an ancestor's
/// children), then fall back to file top-level symbols. Returns the set found at
/// the first non-empty precedence level, ordered by `symbol_id`.
fn resolve_receiver_symbols<'a>(
    edge: &UnresolvedEdge,
    index: &'a WorkspaceCandidateIndex,
    receiver: &str,
) -> Vec<&'a CandidateSymbol> {
    let mut scope = edge.caller_scope_symbol_id.clone();
    while let Some(scope_id) = scope {
        let hits: Vec<&CandidateSymbol> = index
            .children_named(&scope_id, receiver)
            .into_iter()
            .filter(|s| s.language == edge.language)
            .collect();
        if !hits.is_empty() {
            return sorted_by_id(hits);
        }
        scope = index
            .symbol_by_id(&scope_id)
            .and_then(|s| s.parent_symbol_id.clone());
    }

    let hits: Vec<&CandidateSymbol> = index
        .top_level_named(&edge.file_id, receiver)
        .into_iter()
        .filter(|s| s.language == edge.language)
        .collect();
    sorted_by_id(hits)
}

/// The single same-language, type-like symbol named `type_name`, or `None` when
/// zero or more than one exist (partial classes / cross-file duplicates stay
/// non-unique, so tier 3 declines rather than guesses).
fn unique_type_symbol<'a>(
    index: &'a WorkspaceCandidateIndex,
    type_name: &str,
    language: &str,
) -> Option<&'a CandidateSymbol> {
    let mut found: Option<&CandidateSymbol> = None;
    for cand in index.by_name(type_name) {
        if cand.language == language && is_type_like(&cand.kind) {
            if found.is_some() {
                return None;
            }
            found = Some(cand);
        }
    }
    found
}

fn sorted_by_id(mut symbols: Vec<&CandidateSymbol>) -> Vec<&CandidateSymbol> {
    symbols.sort_by(|a, b| a.symbol_id.cmp(&b.symbol_id));
    symbols
}

// ---------------------------------------------------------------------------
// Kind compatibility (design §"Resolution tiers", kind-compatibility map)
// ---------------------------------------------------------------------------

fn is_type_like(kind: &SymbolKind) -> bool {
    TYPE_LIKE_KINDS.contains(kind)
}

/// Kind compatibility for tiers 1–3.
fn tier123_compatible_kinds(kind: ReferenceKind) -> &'static [SymbolKind] {
    match kind {
        ReferenceKind::Call => &[
            SymbolKind::Function,
            SymbolKind::Method,
            SymbolKind::Constructor,
        ],
        ReferenceKind::Instantiates => &[
            SymbolKind::Class,
            SymbolKind::Struct,
            SymbolKind::Constructor,
        ],
        ReferenceKind::TypeUsage => TYPE_LIKE_KINDS,
        ReferenceKind::MemberAccess => {
            &[SymbolKind::Property, SymbolKind::Field, SymbolKind::Method]
        }
    }
}

/// Kind compatibility for tier 4. Differs from tiers 1–3 only for calls (methods
/// excluded — method calls disabled at tier 4) and member_access (empty — tier 4
/// disabled entirely).
fn tier4_compatible_kinds(kind: ReferenceKind) -> &'static [SymbolKind] {
    match kind {
        ReferenceKind::Call => &[SymbolKind::Function, SymbolKind::Constructor],
        ReferenceKind::Instantiates => &[
            SymbolKind::Class,
            SymbolKind::Struct,
            SymbolKind::Constructor,
        ],
        ReferenceKind::TypeUsage => TYPE_LIKE_KINDS,
        ReferenceKind::MemberAccess => &[],
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ---- builders -------------------------------------------------------

    fn sym(id: &str, name: &str, kind: SymbolKind, lang: &str, file: &str) -> CandidateSymbol {
        CandidateSymbol {
            symbol_id: id.to_string(),
            file_id: file.to_string(),
            language: lang.to_string(),
            name: name.to_string(),
            kind,
            parent_symbol_id: None,
        }
    }

    fn child(
        id: &str,
        name: &str,
        kind: SymbolKind,
        lang: &str,
        file: &str,
        parent: &str,
    ) -> CandidateSymbol {
        CandidateSymbol {
            parent_symbol_id: Some(parent.to_string()),
            ..sym(id, name, kind, lang, file)
        }
    }

    fn pending_edge(kind: ReferenceKind, lang: &str, file: &str, terminal: &str) -> UnresolvedEdge {
        UnresolvedEdge {
            origin: EdgeOrigin::Pending,
            kind,
            language: lang.to_string(),
            file_id: file.to_string(),
            terminal_name: terminal.to_string(),
            receiver: None,
            caller_scope_symbol_id: None,
            import_context: None,
        }
    }

    fn ident_edge(kind: ReferenceKind, lang: &str, file: &str, terminal: &str) -> UnresolvedEdge {
        UnresolvedEdge {
            origin: EdgeOrigin::Identifier,
            ..pending_edge(kind, lang, file, terminal)
        }
    }

    fn resolved(outcome: &TierOutcome) -> (u8, f64, String, String) {
        match outcome {
            TierOutcome::Resolved {
                target_symbol_id,
                tier,
                confidence,
                method,
            } => (*tier, *confidence, method.clone(), target_symbol_id.clone()),
            other => panic!("expected Resolved, got {other:?}"),
        }
    }

    // ---- tier 4: unique-language-global --------------------------------

    #[test]
    fn tier4_unique_type_resolves() {
        // INVARIANT: exactly one kind-compatible, same-language type symbol
        // workspace-wide resolves a type_usage edge at tier 4 (0.55).
        let index = WorkspaceCandidateIndex::build(
            vec![sym("s1", "Widget", SymbolKind::Class, "rust", "f1")],
            vec![],
            vec![],
        );
        let edge = pending_edge(ReferenceKind::TypeUsage, "rust", "f2", "Widget");
        let (tier, conf, method, target) = resolved(&resolve_one(&edge, &index));
        assert_eq!(tier, 4);
        assert_eq!(conf, CONFIDENCE_TIER4);
        assert_eq!(method, METHOD_TIER4);
        assert_eq!(target, "s1");
    }

    #[test]
    fn tier4_cross_language_collision_is_not_a_candidate() {
        // INVARIANT: same-language filter — a same-name symbol in another
        // language is never a candidate. Only the rust one resolves.
        let index = WorkspaceCandidateIndex::build(
            vec![
                sym("s1", "Widget", SymbolKind::Class, "rust", "f1"),
                sym("s2", "Widget", SymbolKind::Class, "python", "f9"),
            ],
            vec![],
            vec![],
        );
        let edge = pending_edge(ReferenceKind::TypeUsage, "rust", "f2", "Widget");
        let (_, _, _, target) = resolved(&resolve_one(&edge, &index));
        assert_eq!(target, "s1");
    }

    #[test]
    fn tier4_cross_language_only_candidate_is_missing() {
        // INVARIANT (negative): if the only same-name symbol is another language,
        // no tier yields a candidate -> Missing (not a wrong cross-language edge).
        let index = WorkspaceCandidateIndex::build(
            vec![sym("s2", "Widget", SymbolKind::Class, "python", "f9")],
            vec![],
            vec![],
        );
        let edge = pending_edge(ReferenceKind::TypeUsage, "rust", "f2", "Widget");
        assert_eq!(resolve_one(&edge, &index), TierOutcome::Missing);
    }

    #[test]
    fn tier4_overload_stays_ambiguous() {
        // INVARIANT: two same-name Function symbols (overloads) -> tier 4 sees 2,
        // no tier yields exactly one -> Ambiguous. No best-guess.
        let index = WorkspaceCandidateIndex::build(
            vec![
                sym("s2", "run", SymbolKind::Function, "rust", "f1"),
                sym("s1", "run", SymbolKind::Function, "rust", "f2"),
            ],
            vec![],
            vec![],
        );
        let edge = pending_edge(ReferenceKind::Call, "rust", "f3", "run");
        match resolve_one(&edge, &index) {
            TierOutcome::Ambiguous { candidates } => assert_eq!(candidates, vec!["s1", "s2"]),
            other => panic!("expected Ambiguous, got {other:?}"),
        }
    }

    #[test]
    fn tier4_partial_class_stays_ambiguous() {
        // INVARIANT: partial classes (two same-name Class symbols) stay ambiguous
        // at tier 4; coverage loss, never a wrong edge.
        let index = WorkspaceCandidateIndex::build(
            vec![
                sym("s1", "Service", SymbolKind::Class, "csharp", "f1"),
                sym("s2", "Service", SymbolKind::Class, "csharp", "f2"),
            ],
            vec![],
            vec![],
        );
        let edge = pending_edge(ReferenceKind::Instantiates, "csharp", "f3", "Service");
        match resolve_one(&edge, &index) {
            TierOutcome::Ambiguous { candidates } => assert_eq!(candidates, vec!["s1", "s2"]),
            other => panic!("expected Ambiguous, got {other:?}"),
        }
    }

    #[test]
    fn tier4_call_to_method_only_is_disabled() {
        // INVARIANT: tier 4 is disabled for method calls — a unique same-name
        // Method is NOT a tier-4 candidate for a call. -> Missing.
        let index = WorkspaceCandidateIndex::build(
            vec![sym("s1", "doWork", SymbolKind::Method, "rust", "f1")],
            vec![],
            vec![],
        );
        let edge = pending_edge(ReferenceKind::Call, "rust", "f2", "doWork");
        assert_eq!(resolve_one(&edge, &index), TierOutcome::Missing);
    }

    #[test]
    fn tier4_call_to_function_resolves() {
        // INVARIANT (positive counterpart): a unique same-name Function IS a
        // tier-4 candidate for a call.
        let index = WorkspaceCandidateIndex::build(
            vec![sym("s1", "compute", SymbolKind::Function, "rust", "f1")],
            vec![],
            vec![],
        );
        let edge = pending_edge(ReferenceKind::Call, "rust", "f2", "compute");
        let (tier, _, _, target) = resolved(&resolve_one(&edge, &index));
        assert_eq!(tier, 4);
        assert_eq!(target, "s1");
    }

    #[test]
    fn tier4_instantiates_targets_class_struct_constructor() {
        // INVARIANT: instantiates kind-compat is Class/Struct/Constructor; a
        // same-name Function is not a candidate.
        let index = WorkspaceCandidateIndex::build(
            vec![
                sym("s1", "Point", SymbolKind::Struct, "rust", "f1"),
                sym("s2", "Point", SymbolKind::Function, "rust", "f2"),
            ],
            vec![],
            vec![],
        );
        let edge = pending_edge(ReferenceKind::Instantiates, "rust", "f3", "Point");
        let (tier, _, _, target) = resolved(&resolve_one(&edge, &index));
        assert_eq!(tier, 4);
        assert_eq!(target, "s1");
    }

    // ---- tier 2: import-guided -----------------------------------------

    #[test]
    fn tier2_import_hit_resolves() {
        // INVARIANT: a candidate reachable through an import in the source file
        // resolves at tier 2 (0.85), ahead of tier 4.
        let index = WorkspaceCandidateIndex::build(
            vec![sym("s1", "Foo", SymbolKind::Class, "typescript", "mod")],
            vec![],
            vec![ImportRecord {
                file_id: "src".to_string(),
                local_name: "Foo".to_string(),
                imported_name: None,
                module_file_id: Some("mod".to_string()),
            }],
        );
        let edge = pending_edge(ReferenceKind::TypeUsage, "typescript", "src", "Foo");
        let (tier, conf, method, target) = resolved(&resolve_one(&edge, &index));
        assert_eq!(tier, 2);
        assert_eq!(conf, CONFIDENCE_TIER2);
        assert_eq!(method, METHOD_TIER2);
        assert_eq!(target, "s1");
    }

    #[test]
    fn tier2_import_beats_tier4_when_both_would_resolve() {
        // INVARIANT: tiers run in order; tier 2 wins over tier 4 for the same
        // unique candidate.
        let index = WorkspaceCandidateIndex::build(
            vec![sym("s1", "Foo", SymbolKind::Class, "typescript", "mod")],
            vec![],
            vec![ImportRecord {
                file_id: "src".to_string(),
                local_name: "Foo".to_string(),
                imported_name: None,
                module_file_id: Some("mod".to_string()),
            }],
        );
        let edge = pending_edge(ReferenceKind::TypeUsage, "typescript", "src", "Foo");
        let (tier, _, _, _) = resolved(&resolve_one(&edge, &index));
        assert_eq!(tier, 2);
    }

    #[test]
    fn tier2_aliased_import_resolves_by_imported_name() {
        // INVARIANT: an aliased import keys on imported_name for the candidate
        // while the reference uses the local binding.
        let index = WorkspaceCandidateIndex::build(
            vec![sym("s1", "Foo", SymbolKind::Class, "typescript", "mod")],
            vec![],
            vec![ImportRecord {
                file_id: "src".to_string(),
                local_name: "Bar".to_string(),
                imported_name: Some("Foo".to_string()),
                module_file_id: Some("mod".to_string()),
            }],
        );
        let edge = pending_edge(ReferenceKind::TypeUsage, "typescript", "src", "Bar");
        let (tier, _, _, target) = resolved(&resolve_one(&edge, &index));
        assert_eq!(tier, 2);
        assert_eq!(target, "s1");
    }

    #[test]
    fn tier2_gated_off_language_skips_to_tier4() {
        // INVARIANT: tier 2 is language-gated. Python is not on the allowlist, so
        // even with import evidence tier 2 is skipped; the edge resolves at tier 4
        // (unique global) instead — proving the gate short-circuits tier 2, not
        // resolution overall.
        assert!(!tier2_enabled("python"));
        let index = WorkspaceCandidateIndex::build(
            vec![sym("s1", "Foo", SymbolKind::Class, "python", "mod")],
            vec![],
            vec![ImportRecord {
                file_id: "src".to_string(),
                local_name: "Foo".to_string(),
                imported_name: None,
                module_file_id: Some("mod".to_string()),
            }],
        );
        let edge = pending_edge(ReferenceKind::TypeUsage, "python", "src", "Foo");
        let (tier, _, _, target) = resolved(&resolve_one(&edge, &index));
        assert_eq!(tier, 4, "python must skip tier 2 and land at tier 4");
        assert_eq!(target, "s1");
    }

    #[test]
    fn tier2_language_gate_flags() {
        // INVARIANT: the gate is a data-driven allowlist.
        assert!(tier2_enabled("typescript"));
        assert!(tier2_enabled("javascript"));
        assert!(!tier2_enabled("python"));
        assert!(!tier2_enabled("dart"));
    }

    #[test]
    fn tier2_cross_language_import_candidate_excluded() {
        // INVARIANT: same-language filter applies at tier 2 too — an import that
        // points at a different-language candidate does not resolve there.
        let index = WorkspaceCandidateIndex::build(
            vec![sym("s1", "Foo", SymbolKind::Class, "javascript", "mod")],
            vec![],
            vec![ImportRecord {
                file_id: "src".to_string(),
                local_name: "Foo".to_string(),
                imported_name: None,
                module_file_id: Some("mod".to_string()),
            }],
        );
        // Reference site is typescript; candidate is javascript -> not a tier-2 hit.
        let edge = pending_edge(ReferenceKind::TypeUsage, "typescript", "src", "Foo");
        assert_eq!(resolve_one(&edge, &index), TierOutcome::Missing);
    }

    #[test]
    fn tier2_ambiguous_but_tier3_resolves() {
        // INVARIANT (tier independence): tiers 2 and 3 filter on different axes.
        // Two imports named `handle` make tier 2 ambiguous, but receiver typing
        // picks exactly one member at tier 3 -> Resolved at tier 3.
        let index = WorkspaceCandidateIndex::build(
            vec![
                // Two global `handle` methods on two different types (tier-2 ambiguous).
                child(
                    "m1",
                    "handle",
                    SymbolKind::Method,
                    "typescript",
                    "modA",
                    "typeA",
                ),
                child(
                    "m2",
                    "handle",
                    SymbolKind::Method,
                    "typescript",
                    "modB",
                    "typeB",
                ),
                sym("typeA", "Alpha", SymbolKind::Class, "typescript", "modA"),
                sym("typeB", "Beta", SymbolKind::Class, "typescript", "modB"),
                // Local receiver `svc` typed to Alpha in the caller method scope.
                child(
                    "svc",
                    "svc",
                    SymbolKind::Variable,
                    "typescript",
                    "src",
                    "caller",
                ),
                sym(
                    "caller",
                    "callerFn",
                    SymbolKind::Function,
                    "typescript",
                    "src",
                ),
            ],
            vec![TypeFact {
                symbol_id: "svc".to_string(),
                resolved_type: "Alpha".to_string(),
                is_inferred: false,
            }],
            vec![
                ImportRecord {
                    file_id: "src".to_string(),
                    local_name: "handle".to_string(),
                    imported_name: None,
                    module_file_id: Some("modA".to_string()),
                },
                ImportRecord {
                    file_id: "src".to_string(),
                    local_name: "handle".to_string(),
                    imported_name: None,
                    module_file_id: Some("modB".to_string()),
                },
            ],
        );
        let mut edge = pending_edge(ReferenceKind::Call, "typescript", "src", "handle");
        edge.receiver = Some("svc".to_string());
        edge.caller_scope_symbol_id = Some("caller".to_string());
        let (tier, conf, _, target) = resolved(&resolve_one(&edge, &index));
        assert_eq!(tier, 3);
        assert_eq!(conf, CONFIDENCE_TIER3);
        assert_eq!(target, "m1");
    }

    // ---- tier 3: receiver-typed ----------------------------------------

    #[test]
    fn tier3_receiver_local_resolves() {
        // INVARIANT: receiver local -> type_fact -> unique type -> member.
        let index = build_receiver_index(false);
        let mut edge = pending_edge(ReferenceKind::Call, "rust", "src", "doWork");
        edge.receiver = Some("svc".to_string());
        edge.caller_scope_symbol_id = Some("caller".to_string());
        let (tier, conf, method, target) = resolved(&resolve_one(&edge, &index));
        assert_eq!(tier, 3);
        assert_eq!(conf, CONFIDENCE_TIER3);
        assert_eq!(method, METHOD_TIER3);
        assert_eq!(target, "member");
    }

    #[test]
    fn tier3_inferred_type_drops_confidence() {
        // INVARIANT: an inferred receiver type resolves at tier 3 with 0.65.
        let index = build_receiver_index(true);
        let mut edge = pending_edge(ReferenceKind::Call, "rust", "src", "doWork");
        edge.receiver = Some("svc".to_string());
        edge.caller_scope_symbol_id = Some("caller".to_string());
        let (tier, conf, _, _) = resolved(&resolve_one(&edge, &index));
        assert_eq!(tier, 3);
        assert_eq!(conf, CONFIDENCE_TIER3_INFERRED);
    }

    #[test]
    fn tier3_field_receiver_via_enclosing_type() {
        // INVARIANT: the scope walk reaches enclosing-type fields — a field
        // `store` on the caller's class typed to a Repo resolves the member.
        let index = WorkspaceCandidateIndex::build(
            vec![
                sym("cls", "Controller", SymbolKind::Class, "csharp", "src"),
                child(
                    "caller",
                    "Handle",
                    SymbolKind::Method,
                    "csharp",
                    "src",
                    "cls",
                ),
                child("store", "store", SymbolKind::Field, "csharp", "src", "cls"),
                sym("repo", "Repo", SymbolKind::Class, "csharp", "modR"),
                child("save", "Save", SymbolKind::Method, "csharp", "modR", "repo"),
            ],
            vec![TypeFact {
                symbol_id: "store".to_string(),
                resolved_type: "Repo".to_string(),
                is_inferred: false,
            }],
            vec![],
        );
        let mut edge = pending_edge(ReferenceKind::Call, "csharp", "src", "Save");
        edge.receiver = Some("store".to_string());
        edge.caller_scope_symbol_id = Some("caller".to_string());
        let (tier, _, _, target) = resolved(&resolve_one(&edge, &index));
        assert_eq!(tier, 3);
        assert_eq!(target, "save");
    }

    #[test]
    fn tier3_no_type_fact_yields_no_tier3() {
        // INVARIANT (negative): receiver in scope but no type fact -> tier 3
        // yields nothing; with no other tier hit -> Missing.
        let index = WorkspaceCandidateIndex::build(
            vec![
                sym("caller", "callerFn", SymbolKind::Function, "rust", "src"),
                child("svc", "svc", SymbolKind::Variable, "rust", "src", "caller"),
                sym("typeA", "Alpha", SymbolKind::Class, "rust", "modA"),
                child(
                    "member",
                    "doWork",
                    SymbolKind::Method,
                    "rust",
                    "modA",
                    "typeA",
                ),
            ],
            vec![], // no type fact for svc
            vec![],
        );
        let mut edge = pending_edge(ReferenceKind::Call, "rust", "src", "doWork");
        edge.receiver = Some("svc".to_string());
        edge.caller_scope_symbol_id = Some("caller".to_string());
        // doWork is a Method, so tier 4 is disabled for the call -> Missing.
        assert_eq!(resolve_one(&edge, &index), TierOutcome::Missing);
    }

    #[test]
    fn tier3_ambiguous_type_symbol_yields_no_tier3() {
        // INVARIANT: the type name must map to EXACTLY ONE same-language type
        // symbol; two `Alpha` classes make the receiver type non-unique -> no tier
        // 3 resolution.
        let index = WorkspaceCandidateIndex::build(
            vec![
                sym("caller", "callerFn", SymbolKind::Function, "rust", "src"),
                child("svc", "svc", SymbolKind::Variable, "rust", "src", "caller"),
                sym("typeA1", "Alpha", SymbolKind::Class, "rust", "modA"),
                sym("typeA2", "Alpha", SymbolKind::Class, "rust", "modB"),
                child("m1", "doWork", SymbolKind::Method, "rust", "modA", "typeA1"),
                child("m2", "doWork", SymbolKind::Method, "rust", "modB", "typeA2"),
            ],
            vec![TypeFact {
                symbol_id: "svc".to_string(),
                resolved_type: "Alpha".to_string(),
                is_inferred: false,
            }],
            vec![],
        );
        let mut edge = pending_edge(ReferenceKind::Call, "rust", "src", "doWork");
        edge.receiver = Some("svc".to_string());
        edge.caller_scope_symbol_id = Some("caller".to_string());
        assert_eq!(resolve_one(&edge, &index), TierOutcome::Missing);
    }

    fn build_receiver_index(is_inferred: bool) -> WorkspaceCandidateIndex {
        WorkspaceCandidateIndex::build(
            vec![
                sym("caller", "callerFn", SymbolKind::Function, "rust", "src"),
                child("svc", "svc", SymbolKind::Variable, "rust", "src", "caller"),
                sym("typeA", "Service", SymbolKind::Class, "rust", "modA"),
                child(
                    "member",
                    "doWork",
                    SymbolKind::Method,
                    "rust",
                    "modA",
                    "typeA",
                ),
            ],
            vec![TypeFact {
                symbol_id: "svc".to_string(),
                resolved_type: "Service".to_string(),
                is_inferred,
            }],
            vec![],
        )
    }

    // ---- reduced identifier chains -------------------------------------

    #[test]
    fn identifier_member_access_is_no_context() {
        // INVARIANT: identifier member_access has no reduced chain today ->
        // NoContext, even when a unique global Method exists.
        let index = WorkspaceCandidateIndex::build(
            vec![sym("s1", "doWork", SymbolKind::Method, "rust", "f1")],
            vec![],
            vec![],
        );
        let edge = ident_edge(ReferenceKind::MemberAccess, "rust", "f2", "doWork");
        assert_eq!(resolve_one(&edge, &index), TierOutcome::NoContext);
    }

    #[test]
    fn identifier_type_usage_resolves_at_tier4() {
        // INVARIANT: identifier type_usage runs tiers 2 & 4; a unique global type
        // resolves at tier 4.
        let index = WorkspaceCandidateIndex::build(
            vec![sym("s1", "Widget", SymbolKind::Class, "rust", "f1")],
            vec![],
            vec![],
        );
        let edge = ident_edge(ReferenceKind::TypeUsage, "rust", "f2", "Widget");
        let (tier, _, _, target) = resolved(&resolve_one(&edge, &index));
        assert_eq!(tier, 4);
        assert_eq!(target, "s1");
    }

    #[test]
    fn identifier_call_uses_tier4_function_only() {
        // INVARIANT: identifier call runs tiers 2 & 4 (Function/Constructor only).
        // A unique Function resolves; a unique Method would not.
        let index = WorkspaceCandidateIndex::build(
            vec![sym("s1", "compute", SymbolKind::Function, "rust", "f1")],
            vec![],
            vec![],
        );
        let edge = ident_edge(ReferenceKind::Call, "rust", "f2", "compute");
        let (tier, _, _, target) = resolved(&resolve_one(&edge, &index));
        assert_eq!(tier, 4);
        assert_eq!(target, "s1");
    }

    #[test]
    fn identifier_call_method_only_is_missing() {
        // INVARIANT (negative): identifier call to a unique Method -> tier 4
        // disabled for method calls -> Missing.
        let index = WorkspaceCandidateIndex::build(
            vec![sym("s1", "doWork", SymbolKind::Method, "rust", "f1")],
            vec![],
            vec![],
        );
        let edge = ident_edge(ReferenceKind::Call, "rust", "f2", "doWork");
        assert_eq!(resolve_one(&edge, &index), TierOutcome::Missing);
    }

    // ---- outcome bookkeeping -------------------------------------------

    #[test]
    fn all_tiers_zero_is_missing() {
        // INVARIANT: attempted tiers all yield 0 -> Missing.
        let index = WorkspaceCandidateIndex::build(vec![], vec![], vec![]);
        let edge = pending_edge(ReferenceKind::TypeUsage, "rust", "f1", "Nope");
        assert_eq!(resolve_one(&edge, &index), TierOutcome::Missing);
    }

    #[test]
    fn ambiguous_candidates_sorted_by_symbol_id() {
        // INVARIANT: determinism — Ambiguous candidate list is ordered by
        // symbol_id regardless of input order.
        let index = WorkspaceCandidateIndex::build(
            vec![
                sym("zeta", "T", SymbolKind::Class, "rust", "f1"),
                sym("alpha", "T", SymbolKind::Class, "rust", "f2"),
                sym("mid", "T", SymbolKind::Class, "rust", "f3"),
            ],
            vec![],
            vec![],
        );
        let edge = pending_edge(ReferenceKind::TypeUsage, "rust", "f4", "T");
        match resolve_one(&edge, &index) {
            TierOutcome::Ambiguous { candidates } => {
                assert_eq!(candidates, vec!["alpha", "mid", "zeta"]);
            }
            other => panic!("expected Ambiguous, got {other:?}"),
        }
    }

    #[test]
    fn unsupported_relationship_kind_makes_no_edge() {
        // INVARIANT: kinds outside the resolvable set produce no edge; the caller
        // records those as no-context.
        assert!(ReferenceKind::from_relationship_kind("imports").is_none());
        assert!(ReferenceKind::from_relationship_kind("references").is_none());
        assert_eq!(
            ReferenceKind::from_relationship_kind("calls"),
            Some(ReferenceKind::Call)
        );
        assert_eq!(
            ReferenceKind::from_relationship_kind("extends"),
            Some(ReferenceKind::TypeUsage)
        );
    }

    #[test]
    fn unsupported_identifier_kind_makes_no_edge() {
        assert!(ReferenceKind::from_identifier_kind("definition").is_none());
        assert_eq!(
            ReferenceKind::from_identifier_kind("member_access"),
            Some(ReferenceKind::MemberAccess)
        );
    }
}
