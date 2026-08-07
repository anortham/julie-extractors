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
/// Tier-3 static-type-receiver confidence. Below a concrete type fact (0.75)
/// because the binding rests on type-name uniqueness rather than a recorded type,
/// and above tier 4 (0.55) because the receiver corroborates the target.
pub const CONFIDENCE_TIER3_STATIC: f64 = 0.70;
/// Tier-4 unique-language-global confidence.
pub const CONFIDENCE_TIER4: f64 = 0.55;

/// `method` string stamped on a tier-2 resolution.
pub const METHOD_TIER2: &str = "tier2_import";
/// `method` string stamped on a tier-3 resolution.
pub const METHOD_TIER3: &str = "tier3_receiver";
/// `method` string stamped on a static-type-receiver resolution. Distinct from
/// [`METHOD_TIER3`] because no `type_facts` row participates: reporting these as
/// `tier3_receiver` would misattribute them to type-fact evidence.
pub const METHOD_TIER3_STATIC: &str = "tier3_static_type";
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

/// Languages with fixture-proven `tier3_static_type` support.
///
/// The tier runs for every language — its refusals are language-agnostic and
/// safe — but it can only *produce* an edge where the extractor supplies two
/// facts beyond the receiver: `visibility` on the type symbol (so an unreachable
/// same-named helper in another file is refused) and static reachability on the
/// member (`isStatic` metadata or a standalone `static` word in the signature,
/// so an instance member a type name cannot reach is refused). Where either is
/// missing the tier silently yields nothing — Python spells its modifier
/// `@staticmethod`, and Rust associated functions carry no modifier at all —
/// so membership here is the honesty gate that
/// `reference_resolution.tier3_static_type` keys on.
///
/// Every entry requires a
/// `fixtures/extraction/resolution_contract/<language>/static_type_receiver/`
/// case; adding a language without one is a data-quality-bar violation.
pub const TIER3_STATIC_TYPE_LANGUAGES: &[&str] = &["csharp", "typescript", "javascript"];

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
/// reduced chain that skips tier 2 and reaches the receiver tiers only where the
/// identifier actually carries a receiver. See [`applicable_tiers`].
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
    VariableRef,
}

impl ReferenceKind {
    /// Map a pending relationship `kind` string to a resolvable reference kind.
    /// Returns `None` for relationship kinds the workspace chain does not resolve
    /// (e.g. `imports`, `references`, `contains`). The pending resolver skips
    /// those rows and the report classifies supported reference kinds as
    /// unattempted.
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
            "variable_ref" => Some(ReferenceKind::VariableRef),
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
#[derive(Debug, Clone, PartialEq)]
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
    /// The dotted qualification standing in front of `receiver`, when the source
    /// wrote one: `Some.Namespace.Fixture.Create()` records `Some.Namespace`.
    /// Tier 3b uses it to tell a fully-qualified reference to a workspace type
    /// from a foreign type that merely shares its simple name.
    pub receiver_qualifier: Option<String>,
    pub source_confidence: f64,
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
            receiver_qualifier: namespace_path_qualifier(&item.target_namespace_json),
            source_confidence: item.confidence,
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
            receiver: item.receiver.clone(),
            caller_scope_symbol_id: item.containing_symbol_id.clone(),
            import_context: item.import_context.clone(),
            receiver_qualifier: item.receiver_qualifier.clone(),
            source_confidence: item.confidence,
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
    /// Declared visibility as the extractor recorded it. The static-type receiver
    /// tier uses this to refuse cross-file bindings to non-public types, which is
    /// what keeps a file-scoped homonym of a framework type from hijacking every
    /// same-named reference in the workspace.
    pub visibility: Option<String>,
    /// Declaration signature, read by the static-type receiver tier to tell a
    /// statically reachable member from an instance one. Used as fallback when
    /// [`Self::is_static`] is `None` (e.g. languages that have not yet emitted
    /// the normalized metadata key).
    pub signature: Option<String>,
    /// Normalized static reachability from `symbols.metadata_json.isStatic`.
    /// `Some(true|false)` wins over signature scanning; `None` falls back to
    /// [`contains_static_modifier`] on the declaration signature.
    pub is_static: Option<bool>,
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
    pub source: Option<String>,
    pub module_file_id: Option<String>,
    /// TypeScript `import type` / type-only specifier. Must not corroborate runtime edges.
    pub is_type_only: bool,
    pub is_default: bool,
    pub is_namespace: bool,
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
    /// Importing file -> every workspace path its relative module specifiers could
    /// bind to, whether or not one exists today. The reverse of
    /// [`import_module_candidates`], and the only key by which a delta can notice
    /// that creating or deleting a file re-pointed a specifier: module selection
    /// turns on PATH existence, so it changes without any symbol name changing.
    ///
    /// Empty for an index built by [`WorkspaceCandidateIndex::build`]; populated by
    /// [`load_index`], which is the only caller that has the importing paths.
    module_candidates_by_file: HashMap<String, BTreeSet<String>>,
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
            module_candidates_by_file: HashMap::new(),
        }
    }

    fn symbol_by_id(&self, id: &str) -> Option<&CandidateSymbol> {
        self.by_id.get(id).map(|&idx| &self.symbols[idx])
    }

    /// The name of the symbol with `id`, if present (used by relationship
    /// propagation to find the co-located identifier by name).
    fn symbol_name(&self, id: &str) -> Option<&str> {
        self.symbol_by_id(id).map(|symbol| symbol.name.as_str())
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

    /// Files whose relative module specifiers could bind to one of `changed_paths`.
    ///
    /// The name expansions below are blind to this: which file `./util` selects depends
    /// on which candidate PATH exists, so adding or deleting `src/util.ts` re-points
    /// every importer of `./util` without touching a single symbol name. When the
    /// new file's exports are disjoint from the import's binding, no touched name
    /// matches and the importer would otherwise keep a target the full pass has
    /// already abandoned.
    fn files_importing_module_candidates(
        &self,
        changed_paths: &HashSet<String>,
    ) -> BTreeSet<String> {
        let mut files = BTreeSet::new();
        for (file_id, candidates) in &self.module_candidates_by_file {
            if candidates.iter().any(|path| changed_paths.contains(path)) {
                files.insert(file_id.clone());
            }
        }
        files
    }

    /// Both sides of every import either side of which is one of `names`.
    ///
    /// Tier 2 gates on `local_name` but looks candidates up by `imported_name`, so an
    /// aliased import ties a reference row to an export under a name that row never
    /// carries. A name-keyed recheck therefore has to carry the alias's other half:
    /// touching `Foo` must also recheck the rows written against the local `Bar`.
    fn import_names_linked_to(&self, names: &HashSet<&str>) -> BTreeSet<String> {
        let mut linked = BTreeSet::new();
        for imports in self.imports_by_file.values() {
            for import in imports {
                let imported = import.imported_name.as_deref();
                if names.contains(import.local_name.as_str())
                    || imported.is_some_and(|name| names.contains(name))
                {
                    linked.insert(import.local_name.clone());
                    if let Some(name) = imported {
                        linked.insert(name.to_string());
                    }
                }
            }
        }
        linked
    }

    /// Names of the symbols whose `type_facts.resolved_type` is one of `names`.
    ///
    /// Tier 3 reaches its target through the receiver's RESOLVED TYPE, a name that
    /// appears in neither `target_terminal_name` nor `target_receiver` — the only two
    /// columns the delta worklists match on. A name-keyed recheck therefore has to
    /// carry the receivers bound to a touched type, or the members typed by it are
    /// never revisited when that type's meaning changes.
    fn receiver_names_bound_to_types(&self, names: &HashSet<&str>) -> BTreeSet<String> {
        let mut receivers = BTreeSet::new();
        for (symbol_id, facts) in &self.type_facts_by_symbol {
            if facts
                .iter()
                .any(|fact| names.contains(fact.resolved_type.as_str()))
                && let Some(symbol) = self.symbol_by_id(symbol_id)
            {
                receivers.insert(symbol.name.clone());
            }
        }
        receivers
    }
}

// ---------------------------------------------------------------------------
// Tier identity
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tier {
    Local,
    Import,
    Receiver,
    StaticType,
    Global,
}

impl Tier {
    fn number(self) -> u8 {
        match self {
            Tier::Local => 1,
            Tier::Import => 2,
            Tier::Receiver | Tier::StaticType => 3,
            Tier::Global => 4,
        }
    }

    fn method(self) -> &'static str {
        match self {
            Tier::Local => METHOD_TIER1,
            Tier::Import => METHOD_TIER2,
            Tier::Receiver => METHOD_TIER3,
            Tier::StaticType => METHOD_TIER3_STATIC,
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

/// Languages with ES module import/export scope, including JSX/TSX aliases.
///
/// Used for fail-closed policies (tier-4 same-file only, static-type import
/// corroboration) even when the language id is not yet on the tier-2 allowlist.
/// Wrong cross-file edges must not slip through via a dialect alias.
pub fn es_module_language(language: &str) -> bool {
    matches!(language, "javascript" | "jsx" | "typescript" | "tsx")
}

/// Whether `language` has fixture-proven static-type-receiver support. Data-driven
/// gate over [`TIER3_STATIC_TYPE_LANGUAGES`]; the capability snapshot records a
/// `reference_resolution.tier3_static_type` gap for every language that fails it.
pub fn tier3_static_type_proven(language: &str) -> bool {
    TIER3_STATIC_TYPE_LANGUAGES.contains(&language)
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
                    confidence: only.confidence.min(edge.source_confidence),
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
/// run only the tiers that their extracted context can support.
fn applicable_tiers(edge: &UnresolvedEdge) -> Vec<Tier> {
    use EdgeOrigin::*;
    use ReferenceKind::*;
    match (edge.origin, edge.kind) {
        // Pending: full workspace chain (tier 4 disabled only for member_access).
        // Receiver-qualified pending calls skip Import: bare import binding
        // does not encode the receiver, so Import would ignore it.
        (Pending, Call) if edge.receiver.is_some() => {
            // No Global: terminal-name uniqueness without the receiver is unsafe.
            vec![Tier::Receiver, Tier::StaticType]
        }
        (Pending, Call | Instantiates | TypeUsage) => {
            vec![Tier::Import, Tier::Receiver, Tier::StaticType, Tier::Global]
        }
        (Pending, MemberAccess) if edge.receiver.is_some() => {
            vec![Tier::Receiver, Tier::StaticType]
        }
        (Pending, MemberAccess) => vec![Tier::Import, Tier::Receiver, Tier::StaticType],
        (Pending, VariableRef) => vec![],
        // `Instantiates` is not an identifier kind (never constructed) but is
        // covered for exhaustiveness.
        // Calls with a receiver need typed-receiver / static-type paths. Import and
        // Global are omitted: both ignore the receiver and can bind wrong edges.
        (Identifier, Call) if edge.receiver.is_some() => {
            vec![Tier::Receiver, Tier::StaticType]
        }
        (Identifier, Call | TypeUsage) => {
            vec![Tier::Import, Tier::StaticType, Tier::Global]
        }
        // A member access with no receiver carries no context to resolve from, so
        // it stays NoContext rather than being reported as a failed lookup.
        (Identifier, MemberAccess) if edge.receiver.is_some() => {
            vec![Tier::Receiver, Tier::StaticType]
        }
        (Identifier, MemberAccess) => vec![],
        (Identifier, VariableRef) => vec![Tier::Local],
        (Identifier, Instantiates) => vec![],
    }
}

fn tier_candidates(
    tier: Tier,
    edge: &UnresolvedEdge,
    index: &WorkspaceCandidateIndex,
) -> Vec<TierCandidate> {
    match tier {
        Tier::Local => tier1_candidates(edge, index),
        Tier::Import => tier2_candidates(edge, index),
        Tier::Receiver => tier3_candidates(edge, index),
        Tier::StaticType => static_type_candidates(edge, index),
        Tier::Global => tier4_candidates(edge, index),
    }
}

/// Static-type receiver: the receiver names a type directly (`SomeEnum.Value`,
/// `Fixture.Create()`) rather than a variable whose type must be inferred, so the
/// member is read straight off that type's children.
///
/// [`resolve_receiver_symbols`] cannot bind these — it searches the caller's scope
/// chain and then file top-level, and a referenced type usually lives in another
/// file. This filter closes that gap without consulting `type_facts`.
///
/// Two refusals keep type-name uniqueness from becoming the tier-4 failure keyed on
/// a different column. A workspace type whose simple name collides with an external
/// one would otherwise hijack every same-named reference in the workspace:
///
/// * **Nested types never bind.** A nested `Foo.File` must not answer for `File.X`.
/// * **Non-public types bind only inside their own file.** A file-scoped or private
///   helper is unreachable from elsewhere, so a cross-file reference to that name
///   means some other type. This deliberately over-refuses `internal` types, which
///   costs recall and never produces a wrong edge.
fn static_type_candidates(
    edge: &UnresolvedEdge,
    index: &WorkspaceCandidateIndex,
) -> Vec<TierCandidate> {
    let Some(receiver) = edge.receiver.as_deref() else {
        return Vec::new();
    };
    if receiver.is_empty() {
        return Vec::new();
    }
    if scope_binds_receiver_name(edge, receiver, index) {
        return Vec::new();
    }
    let Some(type_symbol) = resolve_static_type_symbol(edge, index, receiver) else {
        return Vec::new();
    };
    if !static_receiver_is_reachable(edge, type_symbol, index) {
        return Vec::new();
    }
    if !static_type_import_corroborated(edge, type_symbol, index) {
        return Vec::new();
    }

    let member_kinds = tier123_compatible_kinds(edge.kind);
    let cross_file = type_symbol.file_id != edge.file_id;
    let mut set: BTreeSet<String> = BTreeSet::new();
    for member in index.children_named(&type_symbol.symbol_id, &edge.terminal_name) {
        if member.language == edge.language
            && member_kinds.contains(&member.kind)
            && is_statically_reachable(member)
            && (!cross_file || member_is_cross_file_visible(member))
        {
            set.insert(member.symbol_id.clone());
        }
    }
    set.into_iter()
        .map(|symbol_id| TierCandidate {
            symbol_id,
            confidence: CONFIDENCE_TIER3_STATIC,
        })
        .collect()
}

/// Cross-file static access only binds members that are publicly visible.
fn member_is_cross_file_visible(member: &CandidateSymbol) -> bool {
    match member.visibility.as_deref() {
        None | Some("public") | Some("open") | Some("internal") => true,
        Some("private") | Some("protected") | Some("fileprivate") => false,
        _ => true,
    }
}

/// Whether `member` can actually be reached through its type's name. Enum members
/// and constants always can. Anything else prefers normalized `isStatic` metadata
/// when present; otherwise the declaration signature must contain a standalone
/// `static` modifier. `Type.InstanceMethod()` does not compile, so binding it
/// would claim evidence the source does not support.
fn is_statically_reachable(member: &CandidateSymbol) -> bool {
    if matches!(
        member.kind,
        SymbolKind::EnumMember | SymbolKind::Constant | SymbolKind::Enum
    ) {
        return true;
    }
    match member.is_static {
        Some(true) => true,
        Some(false) => false,
        None => member
            .signature
            .as_deref()
            .is_some_and(contains_static_modifier),
    }
}

/// Parse `isStatic` from a symbol's `metadata_json`. Prefers a JSON boolean;
/// tolerates the strings `"true"` / `"false"`. Any other shape is ignored.
fn parse_is_static_metadata(metadata_json: Option<&str>) -> Option<bool> {
    let raw = metadata_json?;
    let value: serde_json::Value = serde_json::from_str(raw).ok()?;
    match value.get("isStatic")? {
        serde_json::Value::Bool(b) => Some(*b),
        serde_json::Value::String(s) => match s.as_str() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

/// `static` as a standalone word in the signature's *modifier prefix*.
///
/// Scanning the whole signature is unsound: an extractor puts parameters,
/// field initializers, and expression bodies in there too, and any of them can
/// contain the bare word — `public int Total => Kinds.Sum(static k => k.Total)`
/// and `public string Kind = "static"` are both instance members. So the scan
/// stops at the first `(`, `<`, `=`, `{`, or `"`, which is where modifiers can no
/// longer appear, after skipping any leading attribute group.
fn contains_static_modifier(signature: &str) -> bool {
    modifier_prefix(signature)
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .any(|word| word == "static")
}

fn modifier_prefix(signature: &str) -> &str {
    let declaration = strip_leading_attributes(signature);
    let end = declaration
        .find(['(', '<', '=', '{', '"'])
        .unwrap_or(declaration.len());
    &declaration[..end]
}

/// Skip balanced `[...]` groups leading the signature: C# and Java put
/// annotations ahead of the modifiers, and an annotation argument can carry any
/// text at all (`[Description("use the static form")]`).
fn strip_leading_attributes(signature: &str) -> &str {
    let mut rest = signature.trim_start();
    while rest.starts_with('[') {
        let mut depth = 0usize;
        let mut close = None;
        for (index, character) in rest.char_indices() {
            match character {
                '[' => depth += 1,
                ']' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        close = Some(index + character.len_utf8());
                        break;
                    }
                }
                _ => {}
            }
        }
        match close {
            Some(index) => rest = rest[index..].trim_start(),
            None => break,
        }
    }
    rest
}

/// The dotted qualification a pending row recorded, from its stored
/// `target_namespace_json` array. Empty arrays and unparseable values yield `None`
/// — an absent qualifier means "unqualified", which is what an empty path means.
fn namespace_path_qualifier(namespace_json: &str) -> Option<String> {
    let segments: Vec<String> = serde_json::from_str(namespace_json).ok()?;
    (!segments.is_empty()).then(|| segments.join("."))
}

/// The namespace a symbol is declared in, as a dotted path, by walking its
/// ancestors. C# records one `Namespace` symbol whose own name may already be
/// dotted (`namespace App.Deep;`), and block syntax can nest several.
fn declared_namespace_path(symbol: &CandidateSymbol, index: &WorkspaceCandidateIndex) -> String {
    let mut segments = Vec::new();
    let mut next = symbol.parent_symbol_id.as_deref();
    while let Some(parent_id) = next {
        let Some(parent) = index.symbol_by_id(parent_id) else {
            break;
        };
        if matches!(parent.kind, SymbolKind::Namespace | SymbolKind::Module) {
            segments.push(parent.name.clone());
        }
        next = parent.parent_symbol_id.as_deref();
    }
    segments.reverse();
    segments.join(".")
}

/// Whether an explicitly written qualification can name `type_symbol`.
///
/// A source qualification is a *suffix* of the full namespace path: inside
/// `Miller.Server`, `Hosting.LeaderIdentityFile` and
/// `Miller.Server.Hosting.LeaderIdentityFile` both name the same type. So the
/// written qualifier has to match the tail of the declared path segment-for-
/// segment. A qualifier naming anything else — `External.Fixture.Create()` — means
/// a foreign type that merely shares the simple name, and must not bind.
///
/// `global::` is an alias for the root namespace, not a segment, so it is dropped
/// before comparison.
fn qualifier_matches_namespace(qualifier: &str, declared: &str) -> bool {
    let written: Vec<&str> = qualifier
        .split('.')
        .filter(|segment| !segment.is_empty() && *segment != "global")
        .collect();
    if written.is_empty() {
        return true;
    }
    let declared: Vec<&str> = declared
        .split('.')
        .flat_map(|part| part.split('.'))
        .filter(|segment| !segment.is_empty())
        .collect();
    declared.len() >= written.len() && declared[declared.len() - written.len()..] == written[..]
}

/// Whether an enclosing callable declares a parameter named `receiver`.
///
/// Such a parameter shadows the type of the same name, so `Fixture.Create()` is an
/// instance access on the parameter, not a static access on the type. Walks the
/// scope chain through callables and stops at the first type-like ancestor — a
/// class is not a binding scope for this purpose, and a record's primary
/// constructor parameters are members reached through `this`, not shadowing locals.
fn scope_binds_receiver_name(
    edge: &UnresolvedEdge,
    receiver: &str,
    index: &WorkspaceCandidateIndex,
) -> bool {
    let mut next = edge.caller_scope_symbol_id.as_deref();
    while let Some(scope_id) = next {
        let Some(scope) = index.symbol_by_id(scope_id) else {
            return false;
        };
        if is_type_like(&scope.kind) {
            return false;
        }
        // Prefer emitted local/parameter symbols (R2+) over signature parsing alone.
        if index
            .children_named(scope_id, receiver)
            .into_iter()
            .any(|child| child.kind == SymbolKind::Variable)
        {
            return true;
        }
        if let Some(signature) = scope.signature.as_deref()
            && parameter_names(signature).any(|name| name == receiver)
        {
            return true;
        }
        next = scope.parent_symbol_id.as_deref();
    }
    false
}

/// The declared parameter names in a callable's signature: the contents of the
/// first top-level `(...)`, split on top-level commas, each yielding the last
/// identifier before any default value. Nesting-aware so
/// `Dictionary<string, int> map` reads as one parameter.
fn parameter_names(signature: &str) -> impl Iterator<Item = &str> {
    let list = top_level_parameter_list(signature).unwrap_or("");
    split_top_level(list, ',').filter_map(|parameter| {
        let declaration = split_top_level(parameter, '=').next().unwrap_or(parameter);
        declaration
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .rfind(|token| !token.is_empty())
    })
}

fn top_level_parameter_list(signature: &str) -> Option<&str> {
    let open = signature.find('(')?;
    let mut depth = 0usize;
    for (index, character) in signature[open..].char_indices() {
        match character {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&signature[open + 1..open + index]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Split on `separator` only where no bracket is open, so generic arguments and
/// nested calls stay inside the segment they belong to.
fn split_top_level(text: &str, separator: char) -> impl Iterator<Item = &str> {
    let mut segments = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (index, character) in text.char_indices() {
        match character {
            '<' | '(' | '[' | '{' => depth += 1,
            '>' | ')' | ']' | '}' => depth = depth.saturating_sub(1),
            _ if character == separator && depth == 0 => {
                segments.push(&text[start..index]);
                start = index + character.len_utf8();
            }
            _ => {}
        }
    }
    segments.push(&text[start..]);
    segments.into_iter()
}

/// Whether `type_symbol` is a legitimate static receiver for a reference in
/// `edge`'s file. See [`static_type_candidates`] for why each refusal exists.
///
/// "Nested" means enclosed by another *type*. A namespace or module parent does
/// not nest a type — C# block-namespace syntax parents every type that way — so
/// only a type-like parent disqualifies the receiver.
fn static_receiver_is_reachable(
    edge: &UnresolvedEdge,
    type_symbol: &CandidateSymbol,
    index: &WorkspaceCandidateIndex,
) -> bool {
    let nested_in_type = type_symbol
        .parent_symbol_id
        .as_deref()
        .and_then(|parent| index.symbol_by_id(parent))
        .is_some_and(|parent| is_type_like(&parent.kind));
    if nested_in_type {
        return false;
    }
    if let Some(qualifier) = edge.receiver_qualifier.as_deref()
        && !qualifier_matches_namespace(qualifier, &declared_namespace_path(type_symbol, index))
    {
        return false;
    }
    if type_symbol.file_id == edge.file_id {
        return true;
    }
    matches!(type_symbol.visibility.as_deref(), Some("public"))
}

fn tier1_candidates(edge: &UnresolvedEdge, index: &WorkspaceCandidateIndex) -> Vec<TierCandidate> {
    let kinds = tier123_compatible_kinds(edge.kind);
    let mut scope = edge.caller_scope_symbol_id.clone();
    while let Some(scope_id) = scope {
        let hits = index
            .children_named(&scope_id, &edge.terminal_name)
            .into_iter()
            .filter(|candidate| {
                candidate.language == edge.language && kinds.contains(&candidate.kind)
            })
            .map(|candidate| TierCandidate {
                symbol_id: candidate.symbol_id.clone(),
                confidence: CONFIDENCE_TIER1,
            })
            .collect::<Vec<_>>();
        if !hits.is_empty() {
            return hits;
        }
        scope = index
            .symbol_by_id(&scope_id)
            .and_then(|symbol| symbol.parent_symbol_id.clone());
    }

    index
        .top_level_named(&edge.file_id, &edge.terminal_name)
        .into_iter()
        .filter(|candidate| candidate.language == edge.language && kinds.contains(&candidate.kind))
        .map(|candidate| TierCandidate {
            symbol_id: candidate.symbol_id.clone(),
            confidence: CONFIDENCE_TIER1,
        })
        .collect()
}

/// Tier 2: candidates reachable through an import whose **local binding**
/// matches the terminal name (design §"Resolution tiers"). Aliases key on
/// `imported_name` for the candidate lookup. Module-wide Branch B was removed:
/// a named/default import must not authorize every export in the module.
fn tier2_candidates(edge: &UnresolvedEdge, index: &WorkspaceCandidateIndex) -> Vec<TierCandidate> {
    let kinds = tier123_compatible_kinds(edge.kind);
    let mut set: BTreeSet<String> = BTreeSet::new();

    for import in index.imports(&edge.file_id) {
        if import.is_type_only || import.is_namespace {
            // Type-only: no value/runtime edges. Namespace: members need a
            // receiver (`NS.member`); bare names are not introduced into scope.
            continue;
        }
        if import.is_default {
            // Default import local names are arbitrary (`import Foo from "./m"`).
            // Without default-export provenance on candidates, name-matching a
            // named export is a wrong edge. Fail closed until extractors mark
            // default-export declarations.
            continue;
        }
        if import.local_name != edge.terminal_name {
            continue;
        }
        // Named aliases key on the original export; non-aliased use local name.
        let target_name = import
            .imported_name
            .as_deref()
            .unwrap_or(import.local_name.as_str());
        for cand in index.by_name(target_name) {
            let module_ok = match (import.source.as_deref(), import.module_file_id.as_deref()) {
                (Some(_), Some(module_file)) => module_file == cand.file_id,
                (Some(_), None) => false,
                (None, Some(module_file)) => module_file == cand.file_id,
                (None, None) => true,
            };
            if cand.language == edge.language && kinds.contains(&cand.kind) && module_ok {
                set.insert(cand.symbol_id.clone());
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
///
/// For module languages with a tier-2 import contract (TS/JS), candidates are
/// restricted to the same file as the edge. Cross-file names require import
/// evidence — unique workspace globals would reintroduce unimported-export edges.
fn tier4_candidates(edge: &UnresolvedEdge, index: &WorkspaceCandidateIndex) -> Vec<TierCandidate> {
    let kinds = tier4_compatible_kinds(edge.kind);
    if kinds.is_empty() {
        return Vec::new();
    }
    let same_file_only = es_module_language(&edge.language);
    let mut set: BTreeSet<String> = BTreeSet::new();
    for cand in index.by_name(&edge.terminal_name) {
        if cand.language == edge.language
            && kinds.contains(&cand.kind)
            && (!same_file_only || cand.file_id == edge.file_id)
        {
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
    unique_named_type_symbol(index, type_name, language, is_type_like)
}

/// Type-name receiver for the static tier. Module languages only accept runtime
/// value types (class/enum); interface/type-alias receivers cannot host static
/// calls at runtime, so they never bind here.
fn unique_static_type_symbol<'a>(
    index: &'a WorkspaceCandidateIndex,
    type_name: &str,
    language: &str,
) -> Option<&'a CandidateSymbol> {
    unique_named_type_symbol(index, type_name, language, |kind| {
        is_static_type_receiver_kind(language, kind)
    })
}

fn unique_named_type_symbol<'a, F>(
    index: &'a WorkspaceCandidateIndex,
    type_name: &str,
    language: &str,
    kind_ok: F,
) -> Option<&'a CandidateSymbol>
where
    F: Fn(&SymbolKind) -> bool,
{
    let mut found: Option<&CandidateSymbol> = None;
    for cand in index.by_name(type_name) {
        if cand.language == language && kind_ok(&cand.kind) {
            if found.is_some() {
                return None;
            }
            found = Some(cand);
        }
    }
    found
}

/// Runtime value types that can appear as `Type.staticMember` receivers.
/// TypeScript/JavaScript modules: class and enum only (interfaces/type aliases
/// erase at runtime). Other languages keep the full type-like set.
fn is_static_type_receiver_kind(language: &str, kind: &SymbolKind) -> bool {
    if es_module_language(language) {
        matches!(kind, SymbolKind::Class | SymbolKind::Enum)
    } else {
        is_type_like(kind)
    }
}

/// Resolve the type symbol named by a static-type receiver, including aliased
/// imports (`import { Fixture as F }` → receiver `F` → type `Fixture`).
fn resolve_static_type_symbol<'a>(
    edge: &UnresolvedEdge,
    index: &'a WorkspaceCandidateIndex,
    receiver: &str,
) -> Option<&'a CandidateSymbol> {
    if let Some(type_symbol) = unique_static_type_symbol(index, receiver, &edge.language) {
        return Some(type_symbol);
    }
    // Alias path for ES modules including jsx/tsx dialect aliases.
    if !es_module_language(&edge.language) {
        return None;
    }
    // Aliased import: local binding is the receiver, imported name is the type.
    let mut found: Option<&CandidateSymbol> = None;
    for import in index.imports(&edge.file_id) {
        if import.is_type_only || import.is_namespace || import.local_name != receiver {
            continue;
        }
        let type_name = import
            .imported_name
            .as_deref()
            .unwrap_or(import.local_name.as_str());
        if type_name == receiver {
            continue; // already tried unique_static_type_symbol(receiver)
        }
        let Some(type_symbol) = unique_static_type_symbol(index, type_name, &edge.language) else {
            continue;
        };
        if let Some(module) = import.module_file_id.as_deref()
            && module != type_symbol.file_id
        {
            continue;
        }
        if found.is_some() {
            return None;
        }
        found = Some(type_symbol);
    }
    found
}

/// ES-module languages refuse cross-file static-type edges unless the receiver
/// name is imported from the type's defining file. Includes JSX/TSX even when
/// tier-2 resolution is not yet certified for those language ids. C# and
/// similar namespace languages rely on uniqueness + visibility alone.
fn static_type_import_corroborated(
    edge: &UnresolvedEdge,
    type_symbol: &CandidateSymbol,
    index: &WorkspaceCandidateIndex,
) -> bool {
    if type_symbol.file_id == edge.file_id {
        return true;
    }
    if !es_module_language(&edge.language) {
        return true;
    }
    let Some(receiver) = edge.receiver.as_deref() else {
        return false;
    };
    index
        .imports(&edge.file_id)
        .iter()
        .any(|import| import_binds_static_type(import, receiver, type_symbol))
}

/// Whether `import` can name `type_symbol` as local binding `receiver`.
fn import_binds_static_type(
    import: &ImportRecord,
    receiver: &str,
    type_symbol: &CandidateSymbol,
) -> bool {
    if import.is_type_only || import.is_namespace {
        return false;
    }
    if import.local_name != receiver {
        return false;
    }
    let Some(module) = import.module_file_id.as_deref() else {
        return false;
    };
    if module != type_symbol.file_id {
        return false;
    }
    if import.is_default {
        // Default import local names are arbitrary. Without default-export
        // provenance, refuse rather than bind a same-named named export.
        return false;
    }
    let imported = import
        .imported_name
        .as_deref()
        .unwrap_or(import.local_name.as_str());
    imported == type_symbol.name
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
        // `Constant` and `EnumMember` carry static member access (`SomeEnum.Value`,
        // `Limits.Max`). Omitting them left those references unresolvable at every
        // tier, since no other kind matches an enum case or a named constant.
        ReferenceKind::MemberAccess => &[
            SymbolKind::Property,
            SymbolKind::Field,
            SymbolKind::Method,
            SymbolKind::Constant,
            SymbolKind::EnumMember,
        ],
        ReferenceKind::VariableRef => &[
            SymbolKind::Variable,
            SymbolKind::Constant,
            SymbolKind::Field,
            SymbolKind::Property,
        ],
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
        ReferenceKind::MemberAccess | ReferenceKind::VariableRef => &[],
    }
}

// ===========================================================================
// Workspace pass (DB-facing)
// ===========================================================================
//
// Everything above is pure tier logic. This section is the ONLY DB-facing part
// of the module: it builds the candidate index from the artifact tables, runs
// the tier chain over the Full/Delta worklists, propagates resolved edges to
// co-located identifiers, and demotes on regression. All *writes* go exclusively
// through Task 1's `resolution_store` primitives (the sanctioned overlay write
// path); the raw `SELECT`s here are read-only index/locator loads (Task 1 ships
// worklists and record/demote primitives, not index loaders, so the resolver
// policy crate owns these read queries — keeping language semantics out of the
// storage crate per design §"Module placement & interface").

use std::collections::HashSet;

use julie_extract_artifact::resolution_store::{
    self, Outcome, ResolutionCounts, ResolutionReportRow, ResolutionStatus, ResolutionWriteBuffer,
};
use julie_extract_artifact::writer::{ResolutionHookError, ResolutionScopeInput};
use rusqlite::{Connection, Transaction};

/// Value stamped into `reference_resolution_version` metadata. Bump when the
/// resolver's observable output contract changes so Miller can gate on it.
pub const RESOLUTION_VERSION: i64 = 6;

/// `method` string stamped on an identifier filled from a tier-1 (extraction-time,
/// same-file) `relationships` row. Tier 1 is materialized at extraction; the
/// workspace pass only *propagates* it onto the co-located identifier.
pub const METHOD_TIER1: &str = "tier1_local";

/// Per-pass report the hook closure captures (Task 3's return contract: the
/// writer consumes only [`ResolutionCounts`]; this richer report never travels
/// through the writer). It carries the aggregated per-language/per-tier/per-outcome
/// rows plus the durable status the CLI writes to `artifact_metadata` after commit.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolutionReport {
    /// Aggregated per-language/per-tier/per-outcome counts (whole-artifact snapshot
    /// taken at the end of the pass — the honest current resolution state). `None`
    /// on a scoped delta: the aggregate is an O(workspace) query, and recomputing it
    /// per single-file update is what regressed the delta gate from 82 ms to 180 ms
    /// (docs/findings/2026-08-05-single-file-delta-172ms-attribution.md). Durable
    /// status/version/revision never depend on these rows.
    pub rows: Option<Vec<ResolutionReportRow>>,
    /// Durable status: `complete` on a clean Full pass with no gated languages,
    /// `partial` after a Delta-only pass or when any processed language is
    /// tier-2-gated, `failed` when the hook errored (set by the CLI, not here).
    pub status: ResolutionStatus,
    /// Resolution contract version ([`RESOLUTION_VERSION`]).
    pub version: i64,
    /// Revision id of the last Full resolve (this revision on a Full pass; the
    /// previously-recorded value on a Delta pass).
    pub last_full_revision: i64,
    /// Languages processed this pass whose tier 2 is gated off (import-guided
    /// resolution unavailable). Drives the `partial` status and the scan-report
    /// `gated_languages` list.
    pub tier2_gated_languages: BTreeSet<String>,
}

/// Run the workspace resolution pass inside the writer's open transaction. This is
/// the closure body the CLI installs at every artifact-mutating flow. Any storage
/// error is mapped to a non-fatal [`ResolutionHookError`] (design §"Failure
/// semantics"): the writer rolls the overlay writes back and the scan still commits.
pub fn resolve_workspace(
    tx: &Transaction<'_>,
    scope: &ResolutionScopeInput,
) -> Result<(ResolutionCounts, ResolutionReport), ResolutionHookError> {
    resolve_workspace_with_crossover(tx, scope, DELTA_SCOPE_CROSSOVER)
}

/// `resolve_workspace` with the delta-scope crossover overridden.
///
/// Exists for the perf sweep that SETS the crossover: with promotion live, every
/// measurement past the threshold times a Full pass and reports it as the scoped
/// cost, so the sweep would only ever rediscover the threshold it was given. Passing
/// a value above 1.0 keeps the scoped path scoped and yields the curve the shipped
/// constant is chosen from. Not a tuning knob — production goes through
/// `resolve_workspace`.
pub fn resolve_workspace_with_crossover(
    tx: &Transaction<'_>,
    scope: &ResolutionScopeInput,
    crossover: f64,
) -> Result<(ResolutionCounts, ResolutionReport), ResolutionHookError> {
    run_resolution(tx, scope, crossover).map_err(|err| ResolutionHookError::new(err.to_string()))
}

/// Write the durable `reference_resolution_*` metadata after the write commits.
///
/// Called by the CLI on the committed connection (autocommit) rather than inside
/// the hook, so a `failed` status survives even though the writer rolls back the
/// hook's in-transaction writes on error (the overlay + any in-tx metadata write
/// would otherwise be discarded together). Non-fatal: a metadata write failure is
/// swallowed — the keys simply stay at their prior value and the next scan
/// backfills. Returns whether metadata was written.
pub fn finalize_resolution_metadata(
    conn: &Connection,
    write_result: &julie_extract_artifact::model::WriteResult,
    report: Option<&ResolutionReport>,
) -> bool {
    if let Some(_message) = &write_result.resolution.failed {
        // The hook errored: its overlay writes were rolled back. Record a durable
        // `failed` status, preserving the last known good `last_full_revision`.
        let last_full_revision = resolution_store::read_resolution_metadata(conn)
            .ok()
            .flatten()
            .map(|meta| meta.last_full_revision)
            .unwrap_or(0);
        return resolution_store::write_resolution_metadata(
            conn,
            ResolutionStatus::Failed,
            RESOLUTION_VERSION,
            last_full_revision,
        )
        .is_ok();
    }
    if let Some(report) = report {
        return resolution_store::write_resolution_metadata(
            conn,
            report.status,
            report.version,
            report.last_full_revision,
        )
        .is_ok();
    }
    false
}

/// Mark an empty artifact as fully upgraded when no resolution hook was needed.
pub fn finalize_empty_resolution_upgrade(conn: &Connection) -> bool {
    let revision = current_revision(conn).unwrap_or(0);
    resolution_store::write_resolution_metadata(
        conn,
        ResolutionStatus::Complete,
        RESOLUTION_VERSION,
        revision,
    )
    .is_ok()
}

/// Keep a failed upgrade unavailable to single-file mutations until a full scan succeeds.
pub fn finalize_resolution_upgrade_failure(conn: &Connection) -> bool {
    let last_full_revision = resolution_store::read_resolution_metadata(conn)
        .ok()
        .flatten()
        .map(|metadata| metadata.last_full_revision)
        .unwrap_or(0);
    resolution_store::write_resolution_metadata(
        conn,
        ResolutionStatus::Failed,
        RESOLUTION_VERSION,
        last_full_revision,
    )
    .is_ok()
}

fn run_resolution(
    tx: &Transaction<'_>,
    scope: &ResolutionScopeInput,
    crossover: f64,
) -> rusqlite::Result<(ResolutionCounts, ResolutionReport)> {
    let revision = current_revision(tx)?;
    let prior = resolution_store::read_resolution_metadata(tx)?;
    // v3-artifact backfill: a v3 artifact opened by a new binary gets the overlay
    // tables via the additive schema create but has no resolution metadata yet.
    // Any scan then forces a Full resolve so the whole workspace is backfilled
    // (design §"Contract & rollout" item 2 — the WRITE path).
    let requested_full = scope.is_full_scan || prior.is_none();

    // Build the candidate index ONCE per hook invocation (design §"Performance &
    // determinism"). The index is always whole-workspace: a delta edge may resolve
    // to a symbol in an unchanged file.
    let index = load_index(tx)?;
    // The identifier locator + covered-set are only consulted for same-file
    // co-location joins, so a delta scopes both to the files it touches (FINDING 1:
    // an O(delta) load rather than reloading every identifier each incremental
    // scan). A full pass loads the whole workspace.
    //
    // The delta scope is computed BEFORE the branch is final: the selection can still
    // cover most of the workspace, and past the crossover a scoped pass is strictly
    // worse than Full — same rows, plus chunked `IN` clauses and per-file bookkeeping.
    let mut effective_full = requested_full;
    let (locator, covered, delta) = if requested_full {
        let locator = IdentifierLocator::load_scoped(tx, None)?;
        let covered = covered_identifiers(tx, &index, &locator, None)?;
        (locator, covered, DeltaScope::none())
    } else {
        let delta = delta_scope_files(tx, scope, &index, revision)?;
        if delta_scope_crosses_over(tx, scope.changed_file_ids.len(), &delta, crossover)? {
            effective_full = true;
            let locator = IdentifierLocator::load_scoped(tx, None)?;
            let covered = covered_identifiers(tx, &index, &locator, None)?;
            (locator, covered, DeltaScope::none())
        } else {
            // Built from the files of the SELECTED ROWS, not from `recheck_files`: a
            // row the name arm selects in an unchanged file still needs its
            // co-location join, and a file the locator never loaded answers `None`.
            let file_refs: Vec<&str> = delta
                .selected_row_files
                .iter()
                .map(String::as_str)
                .collect();
            let locator = IdentifierLocator::load_scoped(tx, Some(&file_refs))?;
            let covered = covered_identifiers(tx, &index, &locator, Some(&file_refs))?;
            (locator, covered, delta)
        }
    };

    let mut counts = ResolutionCounts::default();
    let mut gated: BTreeSet<String> = BTreeSet::new();

    if effective_full {
        resolve_full(
            tx,
            &index,
            &locator,
            &covered,
            revision,
            &mut counts,
            &mut gated,
        )?;
    } else {
        resolve_delta(
            tx,
            scope,
            &delta,
            &index,
            &locator,
            &covered,
            revision,
            &mut counts,
            &mut gated,
        )?;
    }

    // The workspace-wide aggregate only runs on passes that re-derived the whole
    // workspace; a scoped delta would pay O(workspace) for rows it did not change.
    let rows = if effective_full {
        Some(resolution_store::resolution_report(tx)?)
    } else {
        None
    };
    // What makes the overlay current for the whole workspace is that every file was
    // hash-checked, NOT that resolution re-derived every row: a scoped pass over a
    // whole-corpus write reaches everything that moved, which is the property the
    // equivalence gate holds. Keying these two on the dispatch switch instead is what
    // would pin `status` to `partial` and freeze `last_full_revision` the moment a
    // whole-repo scan started scoping. The converse holds too: `effective_full` is
    // NOT sufficient — a single-file update promoted past the crossover re-derives
    // every resolution row from artifact state but hash-checked one file, so it must
    // not claim corpus currency. The identifier-denominated crossover makes that
    // promotion routine on dense files; before it, this leg was almost unreachable
    // and the old `effective_full ||` here went unnoticed.
    let corpus_current = scope.whole_corpus;
    // `gated` accumulates only over items the sweep actually visited, so a scoped pass
    // sees only the languages in scope — enough for the report, which describes what
    // THIS pass did, but not enough to claim `complete` for the workspace. Only that
    // claim reads the workspace-wide set, and only when a scoped pass is about to make
    // it; the reported set is left exactly as observed.
    let status_gated_clear = if effective_full {
        gated.is_empty()
    } else {
        gated.is_empty() && workspace_tier2_gated_languages(tx)?.is_empty()
    };
    let status = if corpus_current && status_gated_clear {
        ResolutionStatus::Complete
    } else {
        ResolutionStatus::Partial
    };
    let last_full_revision = if corpus_current {
        revision
    } else {
        prior
            .map(|meta| meta.last_full_revision)
            .unwrap_or(revision)
    };
    Ok((
        counts,
        ResolutionReport {
            rows,
            status,
            version: RESOLUTION_VERSION,
            last_full_revision,
            tier2_gated_languages: gated,
        },
    ))
}

// ---------------------------------------------------------------------------
// Full / Delta orchestration
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn resolve_full(
    tx: &Transaction<'_>,
    index: &WorkspaceCandidateIndex,
    locator: &IdentifierLocator,
    covered: &HashSet<String>,
    revision: i64,
    counts: &mut ResolutionCounts,
    gated: &mut BTreeSet<String>,
) -> rusqlite::Result<()> {
    // Overlay writes are buffered and flushed at each phase boundary (below). The
    // flush points are exactly the places where the ORIGINAL immediate-write code
    // let a later worklist SELECT observe an earlier write, so behavior is
    // bit-identical while the count of statement-ends inside the open savepoint
    // drops from ~O(rows) to O(phases × chunks). See the buffer's ordering contract.
    let mut buf = ResolutionWriteBuffer::new();

    // 0. Recheck already-resolved overlays against the whole current workspace.
    // A full pass must not preserve stale rows if a prior unique target became
    // ambiguous or disappeared.
    // The demoted co-locations are discarded here: a full pass re-derives every
    // identifier from `worklist_full_identifiers`, so they need no separate repair.
    let _ = recheck_resolved_pending_items(
        &mut buf,
        &resolution_store::worklist_resolved_pending(tx)?,
        index,
        locator,
        gated,
    )?;
    // Flush demotions so the next worklist SELECT (resolved identifiers, then the
    // unresolved-pending fill) sees the demoted rows as unresolved — matches the
    // original immediate-demote ordering.
    buf.flush(tx)?;
    // Read ownership *after* the demotion flush so an identifier whose pending was
    // just demoted re-enters the recheck instead of keeping a stale target.
    let propagation_covered = propagation_covered_identifiers(tx, index, locator, None)?;
    recheck_resolved_identifier_items(
        &mut buf,
        &resolution_store::worklist_resolved_identifiers(tx)?,
        index,
        &propagation_covered,
        revision,
        counts,
        gated,
    )?;
    buf.flush(tx)?;
    // 1. Resolve every unresolved pending row; propagate resolved ones.
    let pending = resolution_store::worklist_full_pending(tx)?;
    resolve_pending_items(&mut buf, &pending, index, locator, revision, counts, gated)?;
    // Flush pending resolutions + their co-located identifier writes before the
    // tier-1 propagation and the generic identifier worklist read them.
    buf.flush(tx)?;
    // 2. Propagate tier-1 (extraction-time, same-file) relationships workspace-wide.
    propagate_relationships(tx, &mut buf, index, locator, None, revision, counts)?;
    buf.flush(tx)?;
    // 3. Generic identifier chain for every identifier propagation did not resolve.
    let owned = propagation_owned_identifiers(tx, covered)?;
    let identifiers = resolution_store::worklist_full_identifiers(tx)?;
    resolve_identifier_items(
        &mut buf,
        &identifiers,
        index,
        &owned,
        revision,
        counts,
        gated,
    )?;
    buf.flush(tx)?;
    Ok(())
}

/// Union two worklists, keeping the first occurrence of each key and restoring the
/// primary-key order both queries were issued in. Matches `chunked_by`'s discipline
/// so a merged worklist stays as deterministic as a single-query one.
fn merge_by_key<T, K, F>(primary: Vec<T>, secondary: Vec<T>, key: F) -> Vec<T>
where
    F: Fn(&T) -> K,
    K: Ord + Clone + std::hash::Hash,
{
    let mut seen: HashSet<K> = HashSet::new();
    let mut merged = Vec::with_capacity(primary.len() + secondary.len());
    for item in primary.into_iter().chain(secondary) {
        let k = key(&item);
        if seen.insert(k) {
            merged.push(item);
        }
    }
    merged.sort_by_key(&key);
    merged
}

#[allow(clippy::too_many_arguments)]
fn resolve_delta(
    tx: &Transaction<'_>,
    scope: &ResolutionScopeInput,
    delta: &DeltaScope,
    index: &WorkspaceCandidateIndex,
    locator: &IdentifierLocator,
    covered: &HashSet<String>,
    revision: i64,
    counts: &mut ResolutionCounts,
    gated: &mut BTreeSet<String>,
) -> rusqlite::Result<()> {
    let names: Vec<&str> = delta.recheck_names.iter().map(String::as_str).collect();
    let files: Vec<&str> = scope.changed_file_ids.iter().map(String::as_str).collect();

    // Buffered writes, flushed at each phase boundary (below). The flush points are
    // exactly where the original immediate-write code let a later worklist SELECT
    // observe an earlier write, so behavior is bit-identical.
    let mut buf = ResolutionWriteBuffer::new();

    // --- Demotion sweep -----------------------------------------------------
    // Resolved pending/identifiers keyed by a recheck name: re-run the chain; if the
    // outcome no longer yields the same single target, demote or overwrite with the
    // current outcome. The fill sweep below re-resolves demoted pending rows if a new
    // single candidate exists.
    // The file-keyed arms carry only the rows no name can select: those in a changed
    // file, and those in a file whose module specifier the revision re-pointed.
    let recheck_files: Vec<&str> = delta.recheck_files.iter().map(String::as_str).collect();
    let demoted_co_locations = recheck_resolved_pending_items(
        &mut buf,
        &merge_by_key(
            resolution_store::worklist_resolved_pending_by_names(tx, &names)?,
            resolution_store::worklist_resolved_pending_in_files(tx, &recheck_files)?,
            |item| item.pending.pending_relationship_id.clone(),
        ),
        index,
        locator,
        gated,
    )?;
    // Flush demotions so the resolved-identifier sweep and the unresolved-pending
    // fill worklists (both filter on the overlay tables) see them.
    buf.flush(tx)?;
    // Read ownership *after* the demotion flush so an identifier whose pending was
    // just demoted re-enters the recheck instead of keeping a stale target.
    //
    // Read over `selected_row_files`, not `recheck_files`: the recheck worklists match
    // by name across the whole workspace, so an identifier in an UNCHANGED file can
    // enter them. Reading ownership over the file-keyed arms' set alone would leave
    // that identifier looking unowned, and the generic chain would overwrite a correct
    // propagated target.
    let selected_row_files: Vec<&str> = delta
        .selected_row_files
        .iter()
        .map(String::as_str)
        .collect();
    let propagation_covered =
        propagation_covered_identifiers(tx, index, locator, Some(&selected_row_files))?;
    recheck_resolved_identifier_items(
        &mut buf,
        &merge_by_key(
            resolution_store::worklist_resolved_identifiers_by_names(tx, &names)?,
            resolution_store::worklist_resolved_identifiers_in_files(tx, &recheck_files)?,
            |item| item.identifier.identifier_id.clone(),
        ),
        index,
        &propagation_covered,
        revision,
        counts,
        gated,
    )?;
    buf.flush(tx)?;

    // --- Fill sweep ---------------------------------------------------------
    // Unresolved pending matching a recheck name, PLUS every unresolved pending in a
    // recheck file.
    //
    // An edge whose tier keys on a name the reference row does not carry — an aliased
    // import's `imported_name`, a receiver's resolved type — rides the name arm here,
    // because `recheck_names` carries both relations.
    let mut pending = resolution_store::worklist_unresolved_pending_by_names(tx, &names)?;
    let mut seen: HashSet<String> = pending
        .iter()
        .map(|item| item.pending_relationship_id.clone())
        .collect();
    for item in unresolved_pending_in_files(tx, &recheck_files)? {
        if seen.insert(item.pending_relationship_id.clone()) {
            pending.push(item);
        }
    }
    resolve_pending_items(&mut buf, &pending, index, locator, revision, counts, gated)?;
    // Flush pending resolutions + co-located identifier writes before propagation
    // and the never-attempted identifier worklists read the overlay tables.
    buf.flush(tx)?;

    // Tier-1 relationships in the changed files (their rows were re-extracted).
    propagate_relationships(tx, &mut buf, index, locator, Some(&files), revision, counts)?;
    buf.flush(tx)?;

    // Never-attempted identifiers matching a recheck name or in a recheck file.
    let mut identifiers =
        resolution_store::worklist_never_attempted_identifiers_by_names(tx, &names)?;
    let mut seen_ids: HashSet<String> = identifiers
        .iter()
        .map(|item| item.identifier_id.clone())
        .collect();
    for item in resolution_store::worklist_never_attempted_identifiers_by_files(tx, &recheck_files)?
    {
        if seen_ids.insert(item.identifier_id.clone()) {
            identifiers.push(item);
        }
    }
    // Repair the co-locations the demotion sweep deleted. Selected by name because the
    // worklists key on names, then narrowed back to the exact demoted ids: an unrelated
    // identifier sharing the name could sit outside `selected_row_files`, where the
    // ownership read below cannot see it and the generic chain would overwrite a
    // propagated target. A row that propagation has since re-resolved is gone from this
    // worklist already, which is the same answer a full pass reaches by ownership.
    let demoted_names: Vec<&str> = demoted_co_locations
        .iter()
        .map(|demoted| demoted.name.as_str())
        .collect();
    let demoted_ids: HashSet<&str> = demoted_co_locations
        .iter()
        .map(|demoted| demoted.identifier_id.as_str())
        .collect();
    for item in resolution_store::worklist_never_attempted_identifiers_by_names(tx, &demoted_names)?
    {
        if demoted_ids.contains(item.identifier_id.as_str())
            && seen_ids.insert(item.identifier_id.clone())
        {
            identifiers.push(item);
        }
    }
    let owned = propagation_owned_identifiers(tx, covered)?;
    resolve_identifier_items(
        &mut buf,
        &identifiers,
        index,
        &owned,
        revision,
        counts,
        gated,
    )?;
    buf.flush(tx)?;
    Ok(())
}

/// An identifier whose overlay row the pending recheck deleted through co-location.
///
/// This deletion is the delta's OWN write, not a workspace change, so no selection key
/// leads back to it: the identifier is named after the pending's terminal name, which a
/// receiver-keyed or file-keyed recheck never carries. A full pass re-derives it from
/// its whole-workspace worklist; a scoped pass has to be told.
struct DemotedCoLocation {
    identifier_id: String,
    name: String,
}

/// Re-check resolved pending rows, demoting the ones whose chain no longer yields the
/// same single target. Returns the identifiers demoted alongside them, which the caller
/// must re-derive (see [`DemotedCoLocation`]).
fn recheck_resolved_pending_items(
    buf: &mut ResolutionWriteBuffer,
    items: &[resolution_store::ResolvedPendingWorkItem],
    index: &WorkspaceCandidateIndex,
    locator: &IdentifierLocator,
    gated: &mut BTreeSet<String>,
) -> rusqlite::Result<Vec<DemotedCoLocation>> {
    let mut demoted_co_locations = Vec::new();
    for resolved in items {
        if !tier2_enabled(&resolved.pending.language) {
            gated.insert(resolved.pending.language.clone());
        }
        let keep = match UnresolvedEdge::from_pending(&resolved.pending) {
            Some(edge) => matches!(
                resolve_one(&edge, index),
                TierOutcome::Resolved { ref target_symbol_id, tier, .. }
                    if *target_symbol_id == resolved.target_symbol_id
                        && i64::from(tier) == resolved.tier
            ),
            None => false,
        };
        if !keep {
            let pending = &resolved.pending;
            buf.demote_pending(&pending.pending_relationship_id);
            // Clear the co-located identifier too: `demote_pending` only removes the
            // pending overlay, but the propagated identifier resolution must go with
            // it. A later fill sweep re-propagates if the edge re-resolves.
            if let Some(identifier_id) = locator.locate(
                &pending.file_id,
                &pending.target_terminal_name,
                pending.start_byte,
                pending.end_byte,
                pending.start_line,
            ) {
                buf.demote_identifier(&identifier_id);
                demoted_co_locations.push(DemotedCoLocation {
                    identifier_id,
                    name: pending.target_terminal_name.clone(),
                });
            }
        }
    }
    Ok(demoted_co_locations)
}

fn recheck_resolved_identifier_items(
    buf: &mut ResolutionWriteBuffer,
    items: &[resolution_store::ResolvedIdentifierWorkItem],
    index: &WorkspaceCandidateIndex,
    covered: &HashSet<String>,
    revision: i64,
    counts: &mut ResolutionCounts,
    gated: &mut BTreeSet<String>,
) -> rusqlite::Result<()> {
    for resolved in items {
        if covered.contains(&resolved.identifier.identifier_id) {
            continue;
        }
        record_identifier_edge(buf, &resolved.identifier, index, revision, counts, gated)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Per-item resolution + propagation
// ---------------------------------------------------------------------------

fn resolve_pending_items(
    buf: &mut ResolutionWriteBuffer,
    items: &[PendingWorkItem],
    index: &WorkspaceCandidateIndex,
    locator: &IdentifierLocator,
    revision: i64,
    counts: &mut ResolutionCounts,
    gated: &mut BTreeSet<String>,
) -> rusqlite::Result<()> {
    for item in items {
        let Some(edge) = UnresolvedEdge::from_pending(item) else {
            continue; // unsupported relationship kind: no overlay for pending rows.
        };
        if !tier2_enabled(&item.language) {
            gated.insert(item.language.clone());
        }
        // Only RESOLVED pending rows get an overlay; ambiguous/missing/no-context
        // pending simply stay unresolved (design §"Resolution tiers").
        if let TierOutcome::Resolved {
            target_symbol_id,
            tier,
            confidence,
            method,
        } = resolve_one(&edge, index)
        {
            buf.record_pending_resolution(
                &item.pending_relationship_id,
                &target_symbol_id,
                tier,
                confidence,
                &method,
                revision,
            );
            counts.pending_resolutions += 1;
            // Propagate onto the co-located identifier by span (line fallback only
            // when exactly one identifier matches — never into an ambiguous join).
            if let Some(identifier_id) = locator.locate(
                &item.file_id,
                &item.target_terminal_name,
                item.start_byte,
                item.end_byte,
                item.start_line,
            ) {
                buf.record_identifier_outcome(
                    &identifier_id,
                    Outcome::Resolved,
                    Some(&target_symbol_id),
                    Some(tier),
                    Some(confidence),
                    Some(&method),
                    None,
                    revision,
                );
                counts.identifier_resolutions += 1;
            }
        }
    }
    Ok(())
}

fn resolve_identifier_items(
    buf: &mut ResolutionWriteBuffer,
    items: &[IdentifierWorkItem],
    index: &WorkspaceCandidateIndex,
    covered: &HashSet<String>,
    revision: i64,
    counts: &mut ResolutionCounts,
    gated: &mut BTreeSet<String>,
) -> rusqlite::Result<()> {
    for item in items {
        if covered.contains(&item.identifier_id) {
            continue; // propagation already wrote this identifier's target.
        }
        record_identifier_edge(buf, item, index, revision, counts, gated)?;
    }
    Ok(())
}

/// Narrow a co-location set down to the identifiers propagation actually resolved.
///
/// [`covered_identifiers`] answers "is a pending/relationship edge sitting on this
/// span", which is not the same question as "did that edge produce a target". When
/// the covering edge failed, the identifier used to be skipped anyway and no
/// `identifier_resolutions` row was written at all — so the site showed up as
/// neither resolved nor missing, and the resolution report understated its own gap.
///
/// Run after the pending and propagation phases have flushed, so the overlay
/// reflects this pass. Identifiers not returned here fall through to the generic
/// identifier chain, whose kind compatibility differs from the pending chain's and
/// can therefore still resolve some of them.
fn propagation_owned_identifiers(
    conn: &Connection,
    covered: &HashSet<String>,
) -> rusqlite::Result<HashSet<String>> {
    let ids: Vec<&str> = covered.iter().map(String::as_str).collect();
    let mut owned = HashSet::new();
    for chunk in ids.chunks(FILE_QUERY_CHUNK) {
        let sql = format!(
            "SELECT identifier_id FROM identifier_resolutions \
             WHERE outcome = 'resolved' AND identifier_id IN ({})",
            placeholders(chunk.len())
        );
        let mut stmt = conn.prepare(&sql)?;
        let params = rusqlite::params_from_iter(chunk.iter());
        let rows = stmt.query_map(params, |row| row.get::<_, String>(0))?;
        for row in rows {
            owned.insert(row?);
        }
    }
    Ok(owned)
}

/// Run the reduced identifier chain for one identifier and record its outcome
/// (resolved/ambiguous/missing/no_context are all recorded — design §"Data flow"
/// step 4). Idempotent upsert, so re-running demotes a regressed resolution.
fn record_identifier_edge(
    buf: &mut ResolutionWriteBuffer,
    item: &IdentifierWorkItem,
    index: &WorkspaceCandidateIndex,
    revision: i64,
    counts: &mut ResolutionCounts,
    gated: &mut BTreeSet<String>,
) -> rusqlite::Result<()> {
    let Some(edge) = UnresolvedEdge::from_identifier(item) else {
        // Unsupported identifier kind: record no-context so it stops re-entering
        // the never-attempted worklist for its kind.
        buf.record_identifier_outcome(
            &item.identifier_id,
            Outcome::NoContext,
            None,
            None,
            None,
            None,
            None,
            revision,
        );
        counts.identifier_resolutions += 1;
        return Ok(());
    };
    if applicable_tiers(&edge).contains(&Tier::Import) && !tier2_enabled(&item.language) {
        gated.insert(item.language.clone());
    }
    let (outcome, target, tier, confidence, method, candidates) = match resolve_one(&edge, index) {
        TierOutcome::Resolved {
            target_symbol_id,
            tier,
            confidence,
            method,
        } => (
            Outcome::Resolved,
            Some(target_symbol_id),
            Some(tier),
            Some(confidence),
            Some(method),
            None,
        ),
        TierOutcome::Ambiguous { candidates } => (
            Outcome::Ambiguous,
            None,
            None,
            None,
            None,
            Some(candidates.len() as i64),
        ),
        TierOutcome::Missing => (Outcome::Missing, None, None, None, None, None),
        TierOutcome::NoContext => (Outcome::NoContext, None, None, None, None, None),
    };
    buf.record_identifier_outcome(
        &item.identifier_id,
        outcome,
        target.as_deref(),
        tier,
        confidence,
        method.as_deref(),
        candidates,
        revision,
    );
    counts.identifier_resolutions += 1;
    Ok(())
}

/// Propagate tier-1 (extraction-time, same-file) `relationships` edges onto their
/// co-located identifiers. `file_filter` restricts to changed files on a delta
/// pass; `None` covers the whole workspace on a full pass.
fn propagate_relationships(
    tx: &Transaction<'_>,
    buf: &mut ResolutionWriteBuffer,
    index: &WorkspaceCandidateIndex,
    locator: &IdentifierLocator,
    file_filter: Option<&[&str]>,
    revision: i64,
    counts: &mut ResolutionCounts,
) -> rusqlite::Result<()> {
    let base = "SELECT to_symbol_id, file_id, kind, start_line, start_byte, end_byte, confidence \
                FROM relationships";
    let rows = match file_filter {
        Some(files) => {
            if files.is_empty() {
                return Ok(());
            }
            let sql = format!("{base} WHERE file_id IN ({})", placeholders(files.len()));
            let mut stmt = tx.prepare(&sql)?;
            stmt.query_map(
                rusqlite::params_from_iter(files.iter()),
                map_relationship_row,
            )?
            .collect::<Result<Vec<_>, _>>()?
        }
        None => {
            let mut stmt = tx.prepare(base)?;
            stmt.query_map([], map_relationship_row)?
                .collect::<Result<Vec<_>, _>>()?
        }
    };
    for (to_symbol_id, file_id, kind, start_line, start_byte, end_byte, confidence) in rows {
        if ReferenceKind::from_relationship_kind(&kind).is_none() {
            continue;
        }
        let Some(name) = index.symbol_name(&to_symbol_id) else {
            continue;
        };
        let name = name.to_string();
        if let Some(identifier_id) =
            locator.locate(&file_id, &name, start_byte, end_byte, start_line)
        {
            buf.record_identifier_outcome(
                &identifier_id,
                Outcome::Resolved,
                Some(&to_symbol_id),
                Some(1),
                Some(confidence.min(CONFIDENCE_TIER1)),
                Some(METHOD_TIER1),
                None,
                revision,
            );
            counts.identifier_resolutions += 1;
        }
    }
    Ok(())
}

/// Max file ids bound into one `IN (...)` clause for the delta-scoped loads. Kept
/// under SQLite's `SQLITE_MAX_VARIABLE_NUMBER` (default 32766); larger deltas are
/// chunked.
const FILE_QUERY_CHUNK: usize = 16000;

/// Map an `identifiers` row to `(file_id, IdentifierLocation)` for the locator.
fn map_identifier_location(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<(String, IdentifierLocation)> {
    Ok((
        row.get::<_, String>(1)?,
        IdentifierLocation {
            identifier_id: row.get(0)?,
            name: row.get(2)?,
            start_line: row.get(3)?,
            start_byte: row.get(4)?,
            end_byte: row.get(5)?,
        },
    ))
}

type RelationshipRow = (String, String, String, i64, Option<i64>, Option<i64>, f64);

fn map_relationship_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RelationshipRow> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get::<_, Option<i64>>(3)?.unwrap_or(-1),
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
    ))
}

// ---------------------------------------------------------------------------
// Index / locator loading (read-only)
// ---------------------------------------------------------------------------

fn current_revision(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT COALESCE(MAX(revision_id), 0) FROM extraction_revisions",
        [],
        |row| row.get(0),
    )
}

fn load_index(conn: &Connection) -> rusqlite::Result<WorkspaceCandidateIndex> {
    let (imports, module_candidates_by_file) = load_import_records(conn)?;
    let mut index = WorkspaceCandidateIndex::build(
        load_candidate_symbols(conn)?,
        load_type_facts(conn)?,
        imports,
    );
    index.module_candidates_by_file = module_candidates_by_file;
    Ok(index)
}

fn load_candidate_symbols(conn: &Connection) -> rusqlite::Result<Vec<CandidateSymbol>> {
    let mut stmt = conn.prepare(
        "SELECT symbol_id, file_id, language, name, kind, parent_symbol_id, visibility, \
                signature, metadata_json \
         FROM symbols ORDER BY symbol_id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, Option<String>>(5)?,
            row.get::<_, Option<String>>(6)?,
            row.get::<_, Option<String>>(7)?,
            row.get::<_, Option<String>>(8)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (
            symbol_id,
            file_id,
            language,
            name,
            kind,
            parent_symbol_id,
            visibility,
            signature,
            metadata_json,
        ) = row?;
        // Skip rows whose kind string is not a known SymbolKind (Task 4 contract).
        let Some(kind) = SymbolKind::try_from_string(&kind) else {
            continue;
        };
        out.push(CandidateSymbol {
            symbol_id,
            file_id,
            language,
            name,
            kind,
            parent_symbol_id,
            visibility,
            signature,
            is_static: parse_is_static_metadata(metadata_json.as_deref()),
        });
    }
    Ok(out)
}

fn load_type_facts(conn: &Connection) -> rusqlite::Result<Vec<TypeFact>> {
    let mut stmt = conn.prepare("SELECT symbol_id, resolved_type, is_inferred FROM type_facts")?;
    let rows = stmt.query_map([], |row| {
        Ok(TypeFact {
            symbol_id: row.get(0)?,
            resolved_type: row.get(1)?,
            is_inferred: row.get::<_, i64>(2)? != 0,
        })
    })?;
    rows.collect()
}

/// Import records plus, per importing file, the module-candidate paths its relative
/// specifiers could bind to. Both come out of one pass because the candidate list is
/// already computed here to pick `module_file_id`; recomputing it for the delta scope
/// would mean a second scan of every import symbol.
type ImportLoad = (Vec<ImportRecord>, HashMap<String, BTreeSet<String>>);

fn load_import_records(conn: &Connection) -> rusqlite::Result<ImportLoad> {
    // Imports are `kind='import'` symbols; there is no dedicated imports table
    // (Task 4 handoff). Resolve relative module specifiers here so tier 2 only
    // trusts aliased imports whose source module maps to a concrete workspace file.
    let files_by_path = load_file_ids_by_path(conn)?;
    let mut stmt = conn.prepare(
        "SELECT file_id, path, language, name, metadata_json FROM symbols WHERE kind = 'import'",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, Option<String>>(4)?,
        ))
    })?;
    let mut out = Vec::new();
    let mut candidates_by_file: HashMap<String, BTreeSet<String>> = HashMap::new();
    for row in rows {
        let (file_id, path, language, name, metadata_json) = row?;
        let (local_name, imported_name, source, is_type_only, is_default, is_namespace) =
            import_binding(&name, metadata_json.as_deref());
        let candidates = import_module_candidates(&path, source.as_deref(), &language);
        let module_file_id = select_module_file(&candidates, &files_by_path, &language);
        if !candidates.is_empty() {
            candidates_by_file
                .entry(file_id.clone())
                .or_default()
                .extend(candidates);
        }
        out.push(ImportRecord {
            file_id,
            local_name,
            imported_name,
            source,
            module_file_id,
            is_type_only,
            is_default,
            is_namespace,
        });
    }
    Ok((out, candidates_by_file))
}

fn load_file_ids_by_path(conn: &Connection) -> rusqlite::Result<HashMap<String, (String, String)>> {
    let mut stmt = conn.prepare("SELECT path, file_id, language FROM files")?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            (row.get::<_, String>(1)?, row.get::<_, String>(2)?),
        ))
    })?;
    let mut out = HashMap::new();
    for row in rows {
        let (path, file) = row?;
        out.insert(path, file);
    }
    Ok(out)
}

/// Best-effort local-binding / imported-name split from an import symbol. Falls
/// back to the symbol name as the local binding. Alias keys (`alias`,
/// `local_name`), imported-name keys (`imported_name`, `imported`), and `source`
/// are read from `metadata_json` when present; per-language import metadata is
/// not a normalized contract yet (F4), so this stays defensive.
fn import_binding(
    name: &str,
    metadata_json: Option<&str>,
) -> (String, Option<String>, Option<String>, bool, bool, bool) {
    let Some(raw) = metadata_json else {
        return (name.to_string(), None, None, false, false, false);
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return (name.to_string(), None, None, false, false, false);
    };
    let string_field = |key: &str| {
        value
            .get(key)
            .and_then(serde_json::Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    let bool_field = |key: &str| value.get(key).and_then(|v| v.as_bool()).unwrap_or(false);
    let local_name = string_field("alias")
        .or_else(|| string_field("local_name"))
        .unwrap_or_else(|| name.to_string());
    let imported_name = string_field("imported_name")
        .or_else(|| string_field("imported"))
        // The TS/JS extractor records the imported name under camelCase `importedName`
        // (fixture-confirmed, Task 6). Without this an aliased import misses tier 2.
        .or_else(|| string_field("importedName"))
        .or_else(|| {
            // If an alias was recorded, the symbol name is the imported name.
            if local_name != name {
                Some(name.to_string())
            } else {
                None
            }
        });
    let source = string_field("source");
    let is_type_only = bool_field("isTypeOnly") || bool_field("is_type_only");
    let is_default = bool_field("isDefault") || bool_field("is_default");
    let is_namespace = bool_field("isNamespace") || bool_field("is_namespace");
    (
        local_name,
        imported_name,
        source,
        is_type_only,
        is_default,
        is_namespace,
    )
}

/// Every workspace path a relative specifier could bind to, in priority order and
/// independent of which of them exist. Split from selection because the delta scope
/// needs the paths a specifier WOULD accept — a file that does not exist yet, or no
/// longer does, is exactly the one that re-points it.
fn import_module_candidates(
    importing_path: &str,
    source: Option<&str>,
    language: &str,
) -> Vec<String> {
    let Some(source) = source else {
        return Vec::new();
    };
    if !(source.starts_with("./") || source.starts_with("../")) {
        return Vec::new();
    }
    let base = importing_path.rsplit_once('/').map_or("", |(base, _)| base);
    let Some(module_path) = normalize_relative_module_path(base, source) else {
        return Vec::new();
    };
    module_path_candidates(&module_path, language)
}

fn select_module_file(
    candidates: &[String],
    files_by_path: &HashMap<String, (String, String)>,
    language: &str,
) -> Option<String> {
    for candidate in candidates {
        if let Some((file_id, file_language)) = files_by_path.get(candidate)
            && file_language == language
        {
            return Some(file_id.clone());
        }
    }
    None
}

fn normalize_relative_module_path(base: &str, source: &str) -> Option<String> {
    let mut parts: Vec<&str> = if base.is_empty() {
        Vec::new()
    } else {
        base.split('/').collect()
    };
    for part in source.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other),
        }
    }
    Some(parts.join("/"))
}

fn module_path_candidates(module_path: &str, language: &str) -> Vec<String> {
    let file_name = module_path
        .rsplit_once('/')
        .map_or(module_path, |(_, file)| file);
    if file_name.contains('.') {
        return vec![module_path.to_string()];
    }
    let extensions: &[&str] = match language {
        "typescript" => &["ts", "tsx", "js", "jsx"],
        "javascript" => &["js", "jsx", "ts", "tsx"],
        _ => &[],
    };
    let mut candidates = Vec::new();
    for ext in extensions {
        candidates.push(format!("{module_path}.{ext}"));
    }
    for ext in extensions {
        candidates.push(format!("{module_path}/index.{ext}"));
    }
    candidates
}

/// Unresolved pending rows in any of `file_ids` (delta fill scope — no by-files
/// pending worklist exists in the storage crate, so the resolver loads it here).
///
/// Chunked: the delta fill sweep passes the WIDENED scope, not the changed files, so
/// this binds one variable per file in the workspace-reachable union rather than the
/// handful a single-file update touches. Over `SQLITE_MAX_VARIABLE_NUMBER` SQLite
/// fails the prepare, and it would fail AFTER the source write committed.
fn unresolved_pending_in_files(
    conn: &Connection,
    file_ids: &[&str],
) -> rusqlite::Result<Vec<PendingWorkItem>> {
    let mut out = Vec::new();
    for chunk in file_ids.chunks(FILE_QUERY_CHUNK) {
        let sql = format!(
            "SELECT pr.pending_relationship_id, pr.from_symbol_id, pr.caller_scope_symbol_id, \
                    pr.file_id, pr.path, f.language, pr.kind, pr.target_display_name, \
                    pr.target_terminal_name, pr.target_receiver, pr.target_namespace_json, \
                    pr.target_import_context, pr.start_line, pr.start_byte, pr.end_byte, \
                    pr.confidence \
             FROM pending_relationships pr \
             JOIN files f ON f.file_id = pr.file_id \
             WHERE pr.file_id IN ({}) \
               AND pr.pending_relationship_id NOT IN \
                   (SELECT pending_relationship_id FROM pending_resolutions) \
             ORDER BY pr.pending_relationship_id",
            placeholders(chunk.len())
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(chunk.iter()), |row| {
            Ok(PendingWorkItem {
                pending_relationship_id: row.get(0)?,
                from_symbol_id: row.get(1)?,
                caller_scope_symbol_id: row.get(2)?,
                file_id: row.get(3)?,
                path: row.get(4)?,
                language: row.get(5)?,
                kind: row.get(6)?,
                target_display_name: row.get(7)?,
                target_terminal_name: row.get(8)?,
                target_receiver: row.get(9)?,
                target_namespace_json: row.get(10)?,
                target_import_context: row.get(11)?,
                start_line: row.get(12)?,
                start_byte: row.get(13)?,
                end_byte: row.get(14)?,
                confidence: row.get(15)?,
            })
        })?;
        for row in rows {
            out.push(row?);
        }
    }
    // Each chunk is ordered; the whole is not until the chunks are merged.
    out.sort_by(|a, b| a.pending_relationship_id.cmp(&b.pending_relationship_id));
    Ok(out)
}

fn placeholders(count: usize) -> String {
    vec!["?"; count].join(", ")
}

/// Fraction of the workspace's IDENTIFIER ROWS a delta scope may cover before Full is
/// the better plan.
///
/// Denominated in identifier rows, not files: the scoped pass's cost is the identifier
/// rows it loads and resolves, and the widening unions preferentially select large
/// files, so file share systematically understates cost on real corpora. Measured on
/// Miller (1,420 files, C#-dense): a 737-changed-file scan delta covered 52% of FILES
/// — under the old file-denominated guard it stayed scoped — while holding 99.7% of
/// IDENTIFIERS and paying 26.0 s scoped vs 11.6 s Full
/// (miller `spike/index-store-ph0/resolution-growth/results.md:50`). Single-changed-file
/// scopes are exempt from promotion entirely — see `delta_scope_crosses_over`.
///
/// Measured by `resolution_perf::delta_scope_crossover_sweep`, which fails if this
/// value promotes to Full later than the crossing it observes. The sweep's fixture is
/// uniform-density (identifier share tracks file share there), so its 2026-08-05
/// curve stands: a scoped pass runs at 0.07x Full for one changed file, 0.37x at a
/// quarter of the corpus and 0.96x at 70%, then loses: 1.08x at 80%, 1.37x at the
/// whole corpus. 0.7 is the last point where scoping still wins.
///
/// The sweep must disable promotion to measure this — with it live, every point past
/// the threshold times a Full pass and the measurement just echoes its own input, which
/// is how an earlier reading put the crossing at 50%.
///
/// A wrong value here only ever converts a scoped pass into a Full one, so erring low
/// costs time and never correctness.
pub const DELTA_SCOPE_CROSSOVER: f64 = 0.7;

/// Whether a delta scope has widened far enough that Full is cheaper.
///
/// Past the crossover a scoped pass does everything Full does and then pays extra for
/// it: the same rows, plus chunked `IN` clauses, per-file worklist bookkeeping, and a
/// locator built from an explicit file list instead of one unfiltered query.
///
/// Compares the scope's identifier rows against the workspace's, because that is the
/// quantity the pass actually pays for (see [`DELTA_SCOPE_CROSSOVER`]) — EXCEPT for a
/// single-changed-file scope, which never promotes. Both halves are measured
/// (2026-08-07, miller corpus, 381k identifiers): a 737-changed-file scan delta at
/// 99.7% identifier share paid 26.0 s scoped vs 11.6 s Full (the per-changed-file
/// worklist dominates), while a 1-changed-file save whose scope widened to 90–93% of
/// identifiers paid the SAME or less on the scoped path as on Full across repeated
/// A/B runs (miller `spike/index-store-ph1/julie-path-audit/`,
/// `spike/index-store-ph0/resolution-growth/results.md:50-51`). Promotion exists to
/// shed worklist overhead, and a one-file worklist has none worth shedding — a dense
/// single-file save gains nothing from Full and must keep its metadata semantics
/// (`corpus_current` stays false either way, but the cheaper path is the scoped one).
///
/// A workspace with no identifier rows at all falls back to the file-count share:
/// both passes are near-free there, and the promotion contract (past-crossover scopes
/// re-derive the workspace aggregate) should not hinge on an empty table.
///
/// Under row scoping the selection has two arms, so the measure sums both: the rows in
/// the whole-file arm plus the rows the name arm matches. A row in both is counted
/// twice, which can only overstate the share and so only ever promotes EARLIER — the
/// same direction the threshold already errs in. The whole curve is re-measured by
/// `resolution_perf::delta_scope_crossover_sweep` under row scoping.
fn delta_scope_crosses_over(
    conn: &Connection,
    changed_file_count: usize,
    delta: &DeltaScope,
    crossover: f64,
) -> rusqlite::Result<bool> {
    if changed_file_count <= 1 || (delta.recheck_files.is_empty() && delta.recheck_names.is_empty())
    {
        return Ok(false);
    }
    let total_identifiers: i64 =
        conn.query_row("SELECT COUNT(*) FROM identifiers", [], |row| row.get(0))?;
    if total_identifiers <= 0 {
        let total_files: i64 =
            conn.query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0))?;
        if total_files <= 0 {
            return Ok(false);
        }
        return Ok(delta.recheck_files.len() as f64 >= total_files as f64 * crossover);
    }
    let mut scope_identifiers: i64 = 0;
    for chunk in delta.recheck_files.chunks(FILE_QUERY_CHUNK) {
        let sql = format!(
            "SELECT COUNT(*) FROM identifiers WHERE file_id IN ({})",
            placeholders(chunk.len())
        );
        let chunk_count: i64 =
            conn.query_row(&sql, rusqlite::params_from_iter(chunk.iter()), |row| {
                row.get(0)
            })?;
        scope_identifiers += chunk_count;
    }
    for chunk in delta.recheck_names.chunks(FILE_QUERY_CHUNK) {
        let sql = format!(
            "SELECT COUNT(*) FROM identifiers WHERE name IN ({})",
            placeholders(chunk.len())
        );
        let chunk_count: i64 =
            conn.query_row(&sql, rusqlite::params_from_iter(chunk.iter()), |row| {
                row.get(0)
            })?;
        scope_identifiers += chunk_count;
    }
    Ok(scope_identifiers as f64 >= total_identifiers as f64 * crossover)
}

/// Languages present in the artifact's pending rows that tier 2 does not cover.
///
/// `tier2_enabled` is a pure allowlist test, so the workspace-wide answer is exactly
/// the distinct languages carrying pending work — no need to persist what a previous
/// pass observed.
fn workspace_tier2_gated_languages(conn: &Connection) -> rusqlite::Result<BTreeSet<String>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT f.language FROM pending_relationships pr \
         JOIN files f ON f.file_id = pr.file_id",
    )?;
    let mut gated = BTreeSet::new();
    for row in stmt.query_map([], |row| row.get::<_, String>(0))? {
        let language = row?;
        if !tier2_enabled(&language) {
            gated.insert(language);
        }
    }
    Ok(gated)
}

/// Paths whose EXISTENCE this revision changed — the only changes that can re-point a
/// module specifier.
///
/// `updated` is excluded deliberately. `file_id` is `stable_id("file", [&path])`, so a
/// rewrite leaves every candidate path exactly as it was and no specifier can select a
/// different file; widening on it would pull in every importer of a module on every
/// edit to that module, which is pure cost for a resolution that provably cannot move.
/// `deleted` and `unsupported` both drop the `files` row, so both stop satisfying a
/// candidate — they are structural even though only one is a user-visible delete.
///
/// Read from `revision_file_changes` rather than `files` because a removed file's row
/// is already gone when the hook runs; the writer inserts this revision's row, path
/// included, ahead of the hook inside the same transaction. Keyed on the
/// `revision_id` primary-key prefix, so it never scans the whole history.
fn structurally_changed_paths(
    conn: &Connection,
    revision: i64,
    file_ids: &[&str],
) -> rusqlite::Result<HashSet<String>> {
    let mut paths = HashSet::new();
    // One bind slot is spent on the revision, so this chunk is one narrower.
    for chunk in file_ids.chunks(FILE_QUERY_CHUNK - 1) {
        let sql = format!(
            "SELECT path FROM revision_file_changes \
             WHERE revision_id = ? AND change_kind IN ('inserted', 'deleted', 'unsupported') \
               AND file_id IN ({})",
            placeholders(chunk.len())
        );
        let mut params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(chunk.len() + 1);
        params.push(&revision);
        for id in chunk {
            params.push(id);
        }
        let mut stmt = conn.prepare(&sql)?;
        for row in stmt.query_map(params.as_slice(), |row| row.get::<_, String>(0))? {
            paths.insert(row?);
        }
    }
    Ok(paths)
}

/// Identifier ids whose overlay a co-located edge currently owns: a relationship
/// (always a materialized tier-1 edge) or a pending row that is resolved right
/// now. Rechecking one of these would overwrite a span-propagated target with a
/// weaker generic guess, and the pending recheck already demotes them when the
/// covering edge goes stale.
///
/// This is deliberately narrower than [`covered_identifiers`]. An identifier
/// merely *co-located* with a failed pending edge is owned by nobody, so it must
/// stay in the recheck worklist — otherwise a generic resolution written on that
/// span could never be demoted when the workspace changes under it.
fn propagation_covered_identifiers(
    conn: &Connection,
    index: &WorkspaceCandidateIndex,
    locator: &IdentifierLocator,
    files: Option<&[&str]>,
) -> rusqlite::Result<HashSet<String>> {
    let mut covered = HashSet::new();

    let pending = query_scoped_rows(
        conn,
        "file_id, target_terminal_name, start_byte, end_byte, start_line",
        "pending_relationships pr \
         JOIN pending_resolutions res \
           ON res.pending_relationship_id = pr.pending_relationship_id",
        files,
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<i64>>(2)?,
                row.get::<_, Option<i64>>(3)?,
                row.get::<_, i64>(4)?,
            ))
        },
    )?;
    for (file_id, name, start_byte, end_byte, start_line) in pending {
        if let Some(id) = locator.locate(&file_id, &name, start_byte, end_byte, start_line) {
            covered.insert(id);
        }
    }

    let relationships = query_scoped_rows(
        conn,
        "to_symbol_id, file_id, kind, start_line, start_byte, end_byte, confidence",
        "relationships",
        files,
        map_relationship_row,
    )?;
    for (to_symbol_id, file_id, kind, start_line, start_byte, end_byte, _) in relationships {
        if ReferenceKind::from_relationship_kind(&kind).is_none() {
            continue;
        }
        if let Some(name) = index.symbol_name(&to_symbol_id) {
            let name = name.to_string();
            if let Some(id) = locator.locate(&file_id, &name, start_byte, end_byte, start_line) {
                covered.insert(id);
            }
        }
    }
    Ok(covered)
}

/// Identifier ids that are co-located with a pending row or a resolvable
/// relationship — the identifiers propagation *may* claim. Whether it actually
/// did is a separate question, answered by [`propagation_owned_identifiers`]
/// after the propagation phase and by [`propagation_covered_identifiers`] before it.
fn covered_identifiers(
    conn: &Connection,
    index: &WorkspaceCandidateIndex,
    locator: &IdentifierLocator,
    files: Option<&[&str]>,
) -> rusqlite::Result<HashSet<String>> {
    let mut covered = HashSet::new();

    let pending = query_scoped_rows(
        conn,
        "file_id, target_terminal_name, start_byte, end_byte, start_line",
        "pending_relationships",
        files,
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<i64>>(2)?,
                row.get::<_, Option<i64>>(3)?,
                row.get::<_, i64>(4)?,
            ))
        },
    )?;
    for (file_id, name, start_byte, end_byte, start_line) in pending {
        if let Some(id) = locator.locate(&file_id, &name, start_byte, end_byte, start_line) {
            covered.insert(id);
        }
    }

    let relationships = query_scoped_rows(
        conn,
        "to_symbol_id, file_id, kind, start_line, start_byte, end_byte, confidence",
        "relationships",
        files,
        map_relationship_row,
    )?;
    for (to_symbol_id, file_id, kind, start_line, start_byte, end_byte, _) in relationships {
        if ReferenceKind::from_relationship_kind(&kind).is_none() {
            continue;
        }
        if let Some(name) = index.symbol_name(&to_symbol_id) {
            let name = name.to_string();
            if let Some(id) = locator.locate(&file_id, &name, start_byte, end_byte, start_line) {
                covered.insert(id);
            }
        }
    }
    Ok(covered)
}

/// Load rows from `table` (all rows when `files = None`, else only rows whose
/// `file_id` is in `files`, chunked under the SQLite variable limit). Both scoped
/// callers here filter on the `file_id` column.
fn query_scoped_rows<T>(
    conn: &Connection,
    columns: &str,
    table: &str,
    files: Option<&[&str]>,
    map: impl Fn(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
) -> rusqlite::Result<Vec<T>> {
    let mut out = Vec::new();
    match files {
        None => {
            let sql = format!("SELECT {columns} FROM {table}");
            let mut stmt = conn.prepare(&sql)?;
            for row in stmt.query_map([], &map)? {
                out.push(row?);
            }
        }
        Some(files) => {
            for chunk in files.chunks(FILE_QUERY_CHUNK) {
                let sql = format!(
                    "SELECT {columns} FROM {table} WHERE file_id IN ({})",
                    placeholders(chunk.len())
                );
                let mut stmt = conn.prepare(&sql)?;
                for row in stmt.query_map(rusqlite::params_from_iter(chunk.iter()), &map)? {
                    out.push(row?);
                }
            }
        }
    }
    Ok(out)
}

/// What a delta pass re-derives, split by the key each worklist arm matches on.
///
/// The name-keyed arms match only `target_terminal_name` / `target_receiver` /
/// `identifiers.name`. Tiers 2 and 3 key on names those columns never carry (an
/// import's `imported_name`, a receiver's resolved type), which is why the whole
/// FILE holding such a row used to be swept. `recheck_names` carries those relations
/// instead, so the row itself is selected and its file is not.
struct DeltaScope {
    /// Names the by-names arms match: the touched names plus the two keying
    /// relations that a reference row never spells out. One round of expansion, not
    /// a fixpoint — a name reached only through a name that was itself reached
    /// cannot key a row against the touched set.
    recheck_names: Vec<String>,
    /// Files the by-files arms sweep whole. Only the changed files and the
    /// module-candidate importers, which bind by PATH existence and so cannot be
    /// name-keyed at all.
    recheck_files: Vec<String>,
    /// `recheck_files` plus the file of every by-names match. Not a worklist input:
    /// the locator, the covered set and the ownership read are built from it, because
    /// a file outside the locator makes `locate` return `None` and silently drops the
    /// co-location join for a row the name arm did select.
    selected_row_files: Vec<String>,
}

impl DeltaScope {
    /// The empty selection a Full pass carries, so `resolve_delta`'s inputs stay
    /// non-optional.
    fn none() -> Self {
        DeltaScope {
            recheck_names: Vec::new(),
            recheck_files: Vec::new(),
            selected_row_files: Vec::new(),
        }
    }
}

/// Compute the delta selection: which names key its rows, which files it sweeps
/// whole, and which files hold the rows either arm selects.
///
/// The two name expansions read maps `load_index` already holds, so they cost no
/// extra SQL. The four by-names worklists run here only to learn the files their
/// matches live in; `resolve_delta` re-runs them against the overlay it has since
/// flushed.
fn delta_scope_files(
    conn: &Connection,
    scope: &ResolutionScopeInput,
    index: &WorkspaceCandidateIndex,
    revision: i64,
) -> rusqlite::Result<DeltaScope> {
    let touched: HashSet<&str> = scope
        .touched_symbol_names
        .iter()
        .map(String::as_str)
        .collect();
    let mut recheck_names: BTreeSet<String> = scope.touched_symbol_names.iter().cloned().collect();
    recheck_names.extend(index.import_names_linked_to(&touched));
    recheck_names.extend(index.receiver_names_bound_to_types(&touched));
    let names: Vec<&str> = recheck_names.iter().map(String::as_str).collect();

    let changed_ids: Vec<&str> = scope.changed_file_ids.iter().map(String::as_str).collect();
    let structural_paths = structurally_changed_paths(conn, revision, &changed_ids)?;
    let mut recheck_files: BTreeSet<String> = scope.changed_file_ids.iter().cloned().collect();
    recheck_files.extend(index.files_importing_module_candidates(&structural_paths));

    let mut selected_row_files = recheck_files.clone();
    for item in resolution_store::worklist_resolved_pending_by_names(conn, &names)? {
        selected_row_files.insert(item.pending.file_id);
    }
    for item in resolution_store::worklist_unresolved_pending_by_names(conn, &names)? {
        selected_row_files.insert(item.file_id);
    }
    for item in resolution_store::worklist_resolved_identifiers_by_names(conn, &names)? {
        selected_row_files.insert(item.identifier.file_id);
    }
    for item in resolution_store::worklist_never_attempted_identifiers_by_names(conn, &names)? {
        selected_row_files.insert(item.file_id);
    }

    Ok(DeltaScope {
        recheck_names: recheck_names.into_iter().collect(),
        recheck_files: recheck_files.into_iter().collect(),
        selected_row_files: selected_row_files.into_iter().collect(),
    })
}

/// One identifier's location, for span-based propagation joins.
struct IdentifierLocation {
    identifier_id: String,
    name: String,
    start_line: i64,
    start_byte: i64,
    end_byte: i64,
}

/// In-memory identifier index keyed by file, for co-location joins. Built once per
/// pass; each file's list is sorted by `identifier_id` for deterministic matching.
struct IdentifierLocator {
    by_file: HashMap<String, Vec<IdentifierLocation>>,
}

impl IdentifierLocator {
    /// Load identifier locations for co-location joins. `files = None` loads the
    /// whole workspace (Full pass); `Some(files)` loads only those files (Delta
    /// pass — an O(delta) load instead of O(workspace), since every co-location
    /// join is same-file, so a delta only ever locates within the files it
    /// touches). File ids are chunked to stay under the SQLite variable limit.
    fn load_scoped(conn: &Connection, files: Option<&[&str]>) -> rusqlite::Result<Self> {
        let base = "SELECT identifier_id, file_id, name, start_line, start_byte, end_byte \
                    FROM identifiers";
        let mut by_file: HashMap<String, Vec<IdentifierLocation>> = HashMap::new();
        match files {
            None => {
                let sql = format!("{base} ORDER BY identifier_id");
                let mut stmt = conn.prepare(&sql)?;
                let rows = stmt.query_map([], map_identifier_location)?;
                Self::collect(&mut by_file, rows)?;
            }
            Some(files) => {
                for chunk in files.chunks(FILE_QUERY_CHUNK) {
                    let sql = format!(
                        "{base} WHERE file_id IN ({}) ORDER BY identifier_id",
                        placeholders(chunk.len())
                    );
                    let mut stmt = conn.prepare(&sql)?;
                    let rows = stmt.query_map(
                        rusqlite::params_from_iter(chunk.iter()),
                        map_identifier_location,
                    )?;
                    Self::collect(&mut by_file, rows)?;
                }
            }
        }
        Ok(Self { by_file })
    }

    fn collect(
        by_file: &mut HashMap<String, Vec<IdentifierLocation>>,
        rows: impl Iterator<Item = rusqlite::Result<(String, IdentifierLocation)>>,
    ) -> rusqlite::Result<()> {
        for row in rows {
            let (file_id, location) = row?;
            by_file.entry(file_id).or_default().push(location);
        }
        Ok(())
    }

    /// The single identifier co-located with a reference span, or `None` when the
    /// join is empty or ambiguous.
    ///
    /// A pending/relationship span is the whole call/expression node, WIDER than
    /// the callee identifier (Task 2 handoff). So the byte join accepts an
    /// identifier whose span is CONTAINED within the reference span (or shares its
    /// start byte) — never a byte-exact equality that would miss. When the byte
    /// join is empty (NULL spans on `html`/`json`, or a shape it can't match) it
    /// falls back to `(file_id, start_line, name)`, and BOTH joins propagate only
    /// when EXACTLY ONE identifier matches (never into an ambiguous line join).
    /// Byte columns are 0-based; lines 1-based.
    fn locate(
        &self,
        file_id: &str,
        name: &str,
        span_start_byte: Option<i64>,
        span_end_byte: Option<i64>,
        span_start_line: i64,
    ) -> Option<String> {
        let locations = self.by_file.get(file_id)?;
        if let (Some(start), Some(end)) = (span_start_byte, span_end_byte) {
            let mut hit: Option<&IdentifierLocation> = None;
            let mut count = 0usize;
            for location in locations.iter().filter(|loc| loc.name == name) {
                let contained = location.start_byte >= start && location.end_byte <= end;
                let shares_start = location.start_byte == start;
                if contained || shares_start {
                    count += 1;
                    hit = Some(location);
                }
            }
            if count == 1 {
                return Some(hit.unwrap().identifier_id.clone());
            }
            if count > 1 {
                return None; // ambiguous byte join: do not propagate.
            }
        }
        // Line fallback: exactly one same-name identifier on the reference line.
        let mut hit: Option<&IdentifierLocation> = None;
        let mut count = 0usize;
        for location in locations
            .iter()
            .filter(|loc| loc.name == name && loc.start_line == span_start_line)
        {
            count += 1;
            hit = Some(location);
        }
        if count == 1 {
            Some(hit.unwrap().identifier_id.clone())
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// SQLite's default `SQLITE_MAX_VARIABLE_NUMBER` is 32,766. The delta fill sweep
    /// binds one variable per file in the WIDENED scope, so an unchunked query fails
    /// the prepare on a large workspace — after the source write has already committed.
    #[test]
    fn file_scoped_delta_queries_chunk_past_the_sqlite_variable_limit() {
        let conn = Connection::open_in_memory().expect("in-memory artifact opens");
        julie_extract_artifact::schema::create_schema(&conn).expect("schema creates");

        let ids: Vec<String> = (0..40_000).map(|i| format!("file-{i}")).collect();
        let refs: Vec<&str> = ids.iter().map(String::as_str).collect();

        assert!(
            unresolved_pending_in_files(&conn, &refs)
                .expect("chunked pending query runs")
                .is_empty()
        );
        assert!(
            structurally_changed_paths(&conn, 1, &refs)
                .expect("chunked changed-path query runs")
                .is_empty()
        );
    }

    // ---- builders -------------------------------------------------------

    fn sym(id: &str, name: &str, kind: SymbolKind, lang: &str, file: &str) -> CandidateSymbol {
        CandidateSymbol {
            symbol_id: id.to_string(),
            file_id: file.to_string(),
            language: lang.to_string(),
            name: name.to_string(),
            kind,
            parent_symbol_id: None,
            visibility: Some("public".to_string()),
            signature: Some(format!("public static {name}")),
            is_static: None,
        }
    }

    fn instance_member(symbol: CandidateSymbol) -> CandidateSymbol {
        CandidateSymbol {
            signature: Some(format!("public {}", symbol.name)),
            is_static: None,
            ..symbol
        }
    }

    fn with_visibility(mut symbol: CandidateSymbol, visibility: &str) -> CandidateSymbol {
        symbol.visibility = Some(visibility.to_string());
        symbol
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
            receiver_qualifier: None,
            source_confidence: 1.0,
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
                source: None,
                module_file_id: Some("mod".to_string()),
                is_type_only: false,
                is_default: false,
                is_namespace: false,
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
                source: None,
                module_file_id: Some("mod".to_string()),
                is_type_only: false,
                is_default: false,
                is_namespace: false,
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
                source: None,
                module_file_id: Some("mod".to_string()),
                is_type_only: false,
                is_default: false,
                is_namespace: false,
            }],
        );
        let edge = pending_edge(ReferenceKind::TypeUsage, "typescript", "src", "Bar");
        let (tier, _, _, target) = resolved(&resolve_one(&edge, &index));
        assert_eq!(tier, 2);
        assert_eq!(target, "s1");
    }

    #[test]
    fn tier2_named_import_does_not_resolve_unimported_export() {
        // INVARIANT: importing one name from a module does not authorize every
        // export in that module (no module-wide Branch B). A second same-named
        // candidate keeps tier 4 from masking the failure with a unique global.
        let index = WorkspaceCandidateIndex::build(
            vec![
                sym("s1", "imported", SymbolKind::Function, "typescript", "mod"),
                sym(
                    "s2",
                    "notImported",
                    SymbolKind::Function,
                    "typescript",
                    "mod",
                ),
                sym(
                    "s3",
                    "notImported",
                    SymbolKind::Function,
                    "typescript",
                    "other",
                ),
            ],
            vec![],
            vec![ImportRecord {
                file_id: "src".to_string(),
                local_name: "imported".to_string(),
                imported_name: Some("imported".to_string()),
                source: Some("./mod".to_string()),
                module_file_id: Some("mod".to_string()),
                is_type_only: false,
                is_default: false,
                is_namespace: false,
            }],
        );
        let edge = pending_edge(ReferenceKind::Call, "typescript", "src", "notImported");
        match resolve_one(&edge, &index) {
            TierOutcome::Resolved {
                tier,
                method,
                target_symbol_id,
                ..
            } => panic!(
                "unimported same-module export must not resolve via import Branch B \
                 (got tier={tier} method={method} target={target_symbol_id})"
            ),
            TierOutcome::Missing | TierOutcome::Ambiguous { .. } | TierOutcome::NoContext => {}
        }
    }

    #[test]
    fn tier2_default_import_fails_closed_without_export_provenance() {
        // Default import local names are arbitrary. Without default-export
        // provenance, name-matching a named export is a wrong edge — miss.
        let index = WorkspaceCandidateIndex::build(
            vec![
                sym("s1", "Foo", SymbolKind::Class, "typescript", "mod"),
                sym(
                    "s2",
                    "ActualDefault",
                    SymbolKind::Class,
                    "typescript",
                    "mod",
                ),
            ],
            vec![],
            vec![ImportRecord {
                file_id: "src".to_string(),
                local_name: "Foo".to_string(),
                imported_name: Some("default".to_string()),
                source: Some("./mod".to_string()),
                module_file_id: Some("mod".to_string()),
                is_type_only: false,
                is_default: true,
                is_namespace: false,
            }],
        );
        let edge = pending_edge(ReferenceKind::TypeUsage, "typescript", "src", "Foo");
        assert_eq!(
            resolve_one(&edge, &index),
            TierOutcome::Missing,
            "default import must not bind a same-named named export"
        );
    }

    #[test]
    fn tier4_module_language_refuses_cross_file_unique_export() {
        // INVARIANT: TS/JS/JSX/TSX unimported exports must not resolve via unique global.
        for language in ["typescript", "javascript", "tsx", "jsx"] {
            let index = WorkspaceCandidateIndex::build(
                vec![sym(
                    "s1",
                    "notImported",
                    SymbolKind::Function,
                    language,
                    "mod",
                )],
                vec![],
                vec![ImportRecord {
                    file_id: "src".to_string(),
                    local_name: "imported".to_string(),
                    imported_name: Some("imported".to_string()),
                    source: Some("./mod".to_string()),
                    module_file_id: Some("mod".to_string()),
                    is_type_only: false,
                    is_default: false,
                    is_namespace: false,
                }],
            );
            let edge = pending_edge(ReferenceKind::Call, language, "src", "notImported");
            assert_eq!(
                resolve_one(&edge, &index),
                TierOutcome::Missing,
                "unique unimported export must not resolve at tier 4 for {language}"
            );
        }
    }

    #[test]
    fn es_module_language_covers_jsx_tsx_aliases() {
        assert!(es_module_language("javascript"));
        assert!(es_module_language("jsx"));
        assert!(es_module_language("typescript"));
        assert!(es_module_language("tsx"));
        assert!(!es_module_language("csharp"));
        // Tier-2 certification remains narrower until fixtures land.
        assert!(!tier2_enabled("jsx"));
        assert!(!tier2_enabled("tsx"));
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
                source: None,
                module_file_id: Some("mod".to_string()),
                is_type_only: false,
                is_default: false,
                is_namespace: false,
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
                source: None,
                module_file_id: Some("mod".to_string()),
                is_type_only: false,
                is_default: false,
                is_namespace: false,
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
                    source: None,
                    module_file_id: Some("modA".to_string()),
                    is_type_only: false,
                    is_default: false,
                    is_namespace: false,
                },
                ImportRecord {
                    file_id: "src".to_string(),
                    local_name: "handle".to_string(),
                    imported_name: None,
                    source: None,
                    module_file_id: Some("modB".to_string()),
                    is_type_only: false,
                    is_default: false,
                    is_namespace: false,
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

    // ---- static-type receiver ------------------------------------------

    fn static_receiver_index(extra: Vec<CandidateSymbol>) -> WorkspaceCandidateIndex {
        let mut symbols = vec![
            sym("fixture", "Fixture", SymbolKind::Class, "csharp", "modA"),
            child(
                "create",
                "Create",
                SymbolKind::Method,
                "csharp",
                "modA",
                "fixture",
            ),
        ];
        symbols.extend(extra);
        WorkspaceCandidateIndex::build(symbols, vec![], vec![])
    }

    fn static_call_edge(receiver: &str, terminal: &str) -> UnresolvedEdge {
        let mut edge = ident_edge(ReferenceKind::Call, "csharp", "caller_file", terminal);
        edge.receiver = Some(receiver.to_string());
        edge
    }

    #[test]
    fn static_type_receiver_resolves_call_across_files() {
        let index = static_receiver_index(vec![]);
        let (tier, conf, method, target) =
            resolved(&resolve_one(&static_call_edge("Fixture", "Create"), &index));
        assert_eq!(tier, 3);
        assert_eq!(conf, CONFIDENCE_TIER3_STATIC);
        assert_eq!(method, METHOD_TIER3_STATIC);
        assert_eq!(target, "create");
    }

    #[test]
    fn static_type_receiver_resolves_enum_member() {
        let index = WorkspaceCandidateIndex::build(
            vec![
                sym("color", "Color", SymbolKind::Enum, "csharp", "modA"),
                child(
                    "red",
                    "Red",
                    SymbolKind::EnumMember,
                    "csharp",
                    "modA",
                    "color",
                ),
            ],
            vec![],
            vec![],
        );
        let mut edge = ident_edge(ReferenceKind::MemberAccess, "csharp", "caller_file", "Red");
        edge.receiver = Some("Color".to_string());
        let (_, _, method, target) = resolved(&resolve_one(&edge, &index));
        assert_eq!(method, METHOD_TIER3_STATIC);
        assert_eq!(target, "red");
    }

    #[test]
    fn static_type_receiver_resolves_named_constant() {
        let index = WorkspaceCandidateIndex::build(
            vec![
                sym("limits", "Limits", SymbolKind::Class, "csharp", "modA"),
                child(
                    "max",
                    "Max",
                    SymbolKind::Constant,
                    "csharp",
                    "modA",
                    "limits",
                ),
            ],
            vec![],
            vec![],
        );
        let mut edge = ident_edge(ReferenceKind::MemberAccess, "csharp", "caller_file", "Max");
        edge.receiver = Some("Limits".to_string());
        let (_, _, _, target) = resolved(&resolve_one(&edge, &index));
        assert_eq!(target, "max");
    }

    #[test]
    fn static_type_receiver_refuses_non_public_type_from_another_file() {
        // A file-scoped helper cannot be referenced from elsewhere, so a same-named
        // reference in another file means some other (external) type. Binding it
        // would be the framework-homonym wrong edge.
        let index = WorkspaceCandidateIndex::build(
            vec![
                with_visibility(
                    sym("fixture", "Fixture", SymbolKind::Class, "csharp", "modA"),
                    "private",
                ),
                child(
                    "create",
                    "Create",
                    SymbolKind::Method,
                    "csharp",
                    "modA",
                    "fixture",
                ),
            ],
            vec![],
            vec![],
        );
        assert_eq!(
            resolve_one(&static_call_edge("Fixture", "Create"), &index),
            TierOutcome::Missing
        );
    }

    #[test]
    fn static_type_receiver_allows_non_public_type_within_its_own_file() {
        let index = WorkspaceCandidateIndex::build(
            vec![
                with_visibility(
                    sym("fixture", "Fixture", SymbolKind::Class, "csharp", "modA"),
                    "private",
                ),
                child(
                    "create",
                    "Create",
                    SymbolKind::Method,
                    "csharp",
                    "modA",
                    "fixture",
                ),
            ],
            vec![],
            vec![],
        );
        let mut edge = static_call_edge("Fixture", "Create");
        edge.file_id = "modA".to_string();
        let (_, _, _, target) = resolved(&resolve_one(&edge, &index));
        assert_eq!(target, "create");
    }

    #[test]
    fn static_type_receiver_refuses_nested_type() {
        // A nested `Outer.File` must never answer for `File.ReadAllText`.
        let index = WorkspaceCandidateIndex::build(
            vec![
                sym("outer", "Outer", SymbolKind::Class, "csharp", "modA"),
                child("file", "File", SymbolKind::Class, "csharp", "modA", "outer"),
                child(
                    "read",
                    "ReadAllText",
                    SymbolKind::Method,
                    "csharp",
                    "modA",
                    "file",
                ),
            ],
            vec![],
            vec![],
        );
        assert_eq!(
            resolve_one(&static_call_edge("File", "ReadAllText"), &index),
            TierOutcome::Missing
        );
    }

    #[test]
    fn static_type_receiver_refuses_instance_member() {
        let index = WorkspaceCandidateIndex::build(
            vec![
                sym("fixture", "Fixture", SymbolKind::Class, "csharp", "modA"),
                instance_member(child(
                    "create",
                    "Create",
                    SymbolKind::Method,
                    "csharp",
                    "modA",
                    "fixture",
                )),
            ],
            vec![],
            vec![],
        );
        assert_eq!(
            resolve_one(&static_call_edge("Fixture", "Create"), &index),
            TierOutcome::Missing
        );
    }

    #[test]
    fn static_type_receiver_honors_is_static_metadata_true_without_signature_static() {
        let mut member = child(
            "create",
            "Create",
            SymbolKind::Method,
            "csharp",
            "modA",
            "fixture",
        );
        member.signature = Some("public Create()".to_string());
        member.is_static = Some(true);
        let index = WorkspaceCandidateIndex::build(
            vec![
                sym("fixture", "Fixture", SymbolKind::Class, "csharp", "modA"),
                member,
            ],
            vec![],
            vec![],
        );
        let (tier, conf, method, target) =
            resolved(&resolve_one(&static_call_edge("Fixture", "Create"), &index));
        assert_eq!(tier, 3);
        assert_eq!(conf, CONFIDENCE_TIER3_STATIC);
        assert_eq!(method, METHOD_TIER3_STATIC);
        assert_eq!(target, "create");
    }

    #[test]
    fn static_type_receiver_honors_is_static_metadata_false_over_signature() {
        let mut member = child(
            "create",
            "Create",
            SymbolKind::Method,
            "csharp",
            "modA",
            "fixture",
        );
        member.signature = Some("public static Create()".to_string());
        member.is_static = Some(false);
        let index = WorkspaceCandidateIndex::build(
            vec![
                sym("fixture", "Fixture", SymbolKind::Class, "csharp", "modA"),
                member,
            ],
            vec![],
            vec![],
        );
        assert_eq!(
            resolve_one(&static_call_edge("Fixture", "Create"), &index),
            TierOutcome::Missing
        );
    }

    #[test]
    fn static_type_receiver_falls_back_to_signature_when_is_static_metadata_absent() {
        let index = static_receiver_index(vec![]);
        let (tier, conf, method, target) =
            resolved(&resolve_one(&static_call_edge("Fixture", "Create"), &index));
        assert_eq!(tier, 3);
        assert_eq!(conf, CONFIDENCE_TIER3_STATIC);
        assert_eq!(method, METHOD_TIER3_STATIC);
        assert_eq!(target, "create");
        assert!(
            index
                .symbol_by_id("create")
                .is_some_and(|s| s.is_static.is_none()),
            "signature-only path requires is_static=None"
        );
    }

    #[test]
    fn parse_is_static_metadata_accepts_bool_and_string_forms() {
        assert_eq!(
            parse_is_static_metadata(Some(r#"{"isStatic":true}"#)),
            Some(true)
        );
        assert_eq!(
            parse_is_static_metadata(Some(r#"{"isStatic":false}"#)),
            Some(false)
        );
        assert_eq!(
            parse_is_static_metadata(Some(r#"{"isStatic":"true"}"#)),
            Some(true)
        );
        assert_eq!(
            parse_is_static_metadata(Some(r#"{"isStatic":"false"}"#)),
            Some(false)
        );
        assert_eq!(
            parse_is_static_metadata(Some(r#"{"isStatic":"yes"}"#)),
            None
        );
        assert_eq!(parse_is_static_metadata(Some(r#"{"other":true}"#)), None);
        assert_eq!(parse_is_static_metadata(Some("{")), None);
        assert_eq!(parse_is_static_metadata(None), None);
    }

    #[test]
    fn resolution_version_is_six_after_module_scope_tightening() {
        assert_eq!(RESOLUTION_VERSION, 6);
    }

    #[test]
    fn static_modifier_scan_stops_before_bodies_parameters_and_initializers() {
        for signature in [
            "public int TotalCount => Kinds.Sum(static kind => kind.TotalCount)",
            "public IReadOnlyList<Gen> Deletions => [.. Decisions.Where(static d => d.Keep)]",
            "public string Kind = \"static\"",
            "private IEnumerable<Neighbour> Evidence(string id) => dir switch { _ => static F }",
            "[Description(\"prefer the static form\")] public void Run()",
        ] {
            assert!(
                !contains_static_modifier(signature),
                "instance member must not read as static: {signature}"
            );
        }
        for signature in [
            "public static int Create()",
            "public static Task<int> RunAsync<T>()",
            "[McpServerTool(Name = \"pin\")] public static void Pin()",
            "static make()",
            "public static implicit operator Foo(Bar b)",
        ] {
            assert!(
                contains_static_modifier(signature),
                "static member must read as static: {signature}"
            );
        }
    }

    #[test]
    fn static_type_receiver_refuses_member_whose_body_contains_a_static_lambda() {
        let mut member = child(
            "total",
            "TotalCount",
            SymbolKind::Property,
            "csharp",
            "modA",
            "fixture",
        );
        member.signature =
            Some("public int TotalCount => Kinds.Sum(static kind => kind.TotalCount)".to_string());
        let index = WorkspaceCandidateIndex::build(
            vec![
                sym("fixture", "Fixture", SymbolKind::Class, "csharp", "modA"),
                member,
            ],
            vec![],
            vec![],
        );
        let mut edge = ident_edge(
            ReferenceKind::MemberAccess,
            "csharp",
            "caller_file",
            "TotalCount",
        );
        edge.receiver = Some("Fixture".to_string());
        assert_eq!(resolve_one(&edge, &index), TierOutcome::Missing);
    }

    #[test]
    fn static_type_receiver_refuses_member_whose_signature_only_spells_static_inside_a_word() {
        let mut member = child(
            "create",
            "Create",
            SymbolKind::Method,
            "csharp",
            "modA",
            "fixture",
        );
        member.signature = Some("public StaticFactory Create()".to_string());
        let index = WorkspaceCandidateIndex::build(
            vec![
                sym("fixture", "Fixture", SymbolKind::Class, "csharp", "modA"),
                member,
            ],
            vec![],
            vec![],
        );
        assert_eq!(
            resolve_one(&static_call_edge("Fixture", "Create"), &index),
            TierOutcome::Missing
        );
    }

    #[test]
    fn parameter_names_reads_declarations_not_types_or_defaults() {
        assert_eq!(
            parameter_names("public int Run(SomeOtherType Fixture)").collect::<Vec<_>>(),
            vec!["Fixture"]
        );
        assert_eq!(
            parameter_names("void F(Dictionary<string, int> map, int count = 10)")
                .collect::<Vec<_>>(),
            vec!["map", "count"]
        );
        assert_eq!(parameter_names("public int Run()").count(), 0);
    }

    #[test]
    fn static_type_receiver_refuses_receiver_shadowed_by_a_parameter() {
        let mut caller = sym("run", "Run", SymbolKind::Method, "csharp", "caller_file");
        caller.signature = Some("public int Run(SomeOtherType Fixture)".to_string());
        let mut symbols = vec![
            sym("fixture", "Fixture", SymbolKind::Class, "csharp", "modA"),
            child(
                "create",
                "Create",
                SymbolKind::Method,
                "csharp",
                "modA",
                "fixture",
            ),
        ];
        symbols.push(caller);
        let index = WorkspaceCandidateIndex::build(symbols, vec![], vec![]);
        let mut edge = static_call_edge("Fixture", "Create");
        edge.caller_scope_symbol_id = Some("run".to_string());
        assert_eq!(resolve_one(&edge, &index), TierOutcome::Missing);
    }

    #[test]
    fn static_type_receiver_refuses_a_foreign_qualifier_but_keeps_workspace_ones() {
        let namespace = sym("ns", "App.Core", SymbolKind::Namespace, "csharp", "modA");
        let mut type_symbol = sym("fixture", "Fixture", SymbolKind::Class, "csharp", "modA");
        type_symbol.parent_symbol_id = Some("ns".to_string());
        let index = WorkspaceCandidateIndex::build(
            vec![
                namespace,
                type_symbol,
                child(
                    "create",
                    "Create",
                    SymbolKind::Method,
                    "csharp",
                    "modA",
                    "fixture",
                ),
            ],
            vec![],
            vec![],
        );
        for qualifier in ["App.Core", "Core", "global.App.Core"] {
            let mut edge = static_call_edge("Fixture", "Create");
            edge.receiver_qualifier = Some(qualifier.to_string());
            let (_, _, method, target) = resolved(&resolve_one(&edge, &index));
            assert_eq!(
                method, METHOD_TIER3_STATIC,
                "{qualifier} names the workspace type"
            );
            assert_eq!(target, "create");
        }
        for qualifier in ["External", "Other.Core", "App.Other"] {
            let mut edge = static_call_edge("Fixture", "Create");
            edge.receiver_qualifier = Some(qualifier.to_string());
            assert_eq!(
                resolve_one(&edge, &index),
                TierOutcome::Missing,
                "{qualifier} names a foreign type"
            );
        }
    }

    #[test]
    fn static_type_receiver_refuses_external_receiver_with_no_workspace_type() {
        let index = static_receiver_index(vec![sym(
            "combine",
            "Combine",
            SymbolKind::Method,
            "csharp",
            "modB",
        )]);
        assert_eq!(
            resolve_one(&static_call_edge("Path", "Combine"), &index),
            TierOutcome::Missing
        );
    }

    #[test]
    fn static_type_receiver_refuses_overloaded_member() {
        let index = static_receiver_index(vec![child(
            "create2",
            "Create",
            SymbolKind::Method,
            "csharp",
            "modA",
            "fixture",
        )]);
        match resolve_one(&static_call_edge("Fixture", "Create"), &index) {
            TierOutcome::Ambiguous { candidates } => {
                assert_eq!(candidates, vec!["create", "create2"])
            }
            other => panic!("expected Ambiguous, got {other:?}"),
        }
    }

    #[test]
    fn static_type_receiver_refuses_ambiguous_type_name() {
        let index = static_receiver_index(vec![sym(
            "fixture2",
            "Fixture",
            SymbolKind::Class,
            "csharp",
            "modB",
        )]);
        assert_eq!(
            resolve_one(&static_call_edge("Fixture", "Create"), &index),
            TierOutcome::Missing
        );
    }

    #[test]
    fn static_type_receiver_refuses_cross_language_type() {
        let index = WorkspaceCandidateIndex::build(
            vec![
                sym("fixture", "Fixture", SymbolKind::Class, "rust", "modA"),
                child(
                    "create",
                    "Create",
                    SymbolKind::Method,
                    "rust",
                    "modA",
                    "fixture",
                ),
            ],
            vec![],
            vec![],
        );
        assert_eq!(
            resolve_one(&static_call_edge("Fixture", "Create"), &index),
            TierOutcome::Missing
        );
    }

    #[test]
    fn concrete_type_fact_outranks_static_type_receiver() {
        // Both tiers can answer; tier 3's type-fact evidence must win so the
        // recorded confidence and method reflect the stronger signal.
        let index = WorkspaceCandidateIndex::build(
            vec![
                sym("caller", "callerFn", SymbolKind::Method, "csharp", "src"),
                child(
                    "svc",
                    "Service",
                    SymbolKind::Field,
                    "csharp",
                    "src",
                    "caller",
                ),
                sym("typeA", "Service", SymbolKind::Class, "csharp", "modA"),
                child(
                    "member",
                    "doWork",
                    SymbolKind::Method,
                    "csharp",
                    "modA",
                    "typeA",
                ),
            ],
            vec![TypeFact {
                symbol_id: "svc".to_string(),
                resolved_type: "Service".to_string(),
                is_inferred: false,
            }],
            vec![],
        );
        let mut edge = pending_edge(ReferenceKind::Call, "csharp", "src", "doWork");
        edge.receiver = Some("Service".to_string());
        edge.caller_scope_symbol_id = Some("caller".to_string());
        let (tier, conf, method, _) = resolved(&resolve_one(&edge, &index));
        assert_eq!(tier, 3);
        assert_eq!(conf, CONFIDENCE_TIER3);
        assert_eq!(method, METHOD_TIER3);
    }

    #[test]
    fn static_type_receiver_outranks_global_uniqueness() {
        let index = static_receiver_index(vec![sym(
            "free",
            "Create",
            SymbolKind::Function,
            "csharp",
            "modB",
        )]);
        let (_, _, method, target) =
            resolved(&resolve_one(&static_call_edge("Fixture", "Create"), &index));
        assert_eq!(method, METHOD_TIER3_STATIC);
        assert_eq!(target, "create");
    }

    #[test]
    fn member_access_without_receiver_stays_no_context() {
        let index = static_receiver_index(vec![]);
        let edge = ident_edge(
            ReferenceKind::MemberAccess,
            "csharp",
            "caller_file",
            "Create",
        );
        assert_eq!(resolve_one(&edge, &index), TierOutcome::NoContext);
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

    #[test]
    fn identifier_variable_ref_resolves_unique_same_file_value_only() {
        let index = WorkspaceCandidateIndex::build(
            vec![
                child(
                    "value",
                    "counter",
                    SymbolKind::Variable,
                    "rust",
                    "src",
                    "caller",
                ),
                sym("other", "counter", SymbolKind::Variable, "rust", "other"),
            ],
            vec![],
            vec![],
        );
        let mut edge = ident_edge(ReferenceKind::VariableRef, "rust", "src", "counter");
        edge.caller_scope_symbol_id = Some("caller".to_string());
        let (tier, confidence, method, target) = resolved(&resolve_one(&edge, &index));
        assert_eq!(
            (tier, confidence, method.as_str(), target.as_str()),
            (1, 0.95, "tier1_local", "value")
        );
    }

    #[test]
    fn identifier_variable_ref_never_uses_workspace_global_name_only() {
        let index = WorkspaceCandidateIndex::build(
            vec![sym(
                "other",
                "counter",
                SymbolKind::Variable,
                "rust",
                "other",
            )],
            vec![],
            vec![],
        );
        let edge = ident_edge(ReferenceKind::VariableRef, "rust", "src", "counter");
        assert_eq!(resolve_one(&edge, &index), TierOutcome::Missing);
    }

    #[test]
    fn identifier_member_access_with_receiver_uses_tier3_but_not_tier4() {
        let index = build_receiver_index(false);
        let mut edge = ident_edge(ReferenceKind::MemberAccess, "rust", "src", "doWork");
        edge.receiver = Some("svc".to_string());
        edge.caller_scope_symbol_id = Some("caller".to_string());
        let (tier, _, method, target) = resolved(&resolve_one(&edge, &index));
        assert_eq!(
            (tier, method.as_str(), target.as_str()),
            (3, "tier3_receiver", "member")
        );

        let global_only = WorkspaceCandidateIndex::build(
            vec![sym("member", "doWork", SymbolKind::Method, "rust", "other")],
            vec![],
            vec![],
        );
        assert_eq!(resolve_one(&edge, &global_only), TierOutcome::Missing);
    }

    #[test]
    fn resolution_confidence_never_exceeds_source_confidence() {
        let index = WorkspaceCandidateIndex::build(
            vec![sym("target", "Widget", SymbolKind::Class, "rust", "other")],
            vec![],
            vec![],
        );
        let mut edge = ident_edge(ReferenceKind::TypeUsage, "rust", "src", "Widget");
        edge.source_confidence = 0.4;
        let (_, confidence, _, _) = resolved(&resolve_one(&edge, &index));
        assert_eq!(confidence, 0.4);
    }

    #[test]
    fn canonical_reference_kinds_cover_both_evidence_vocabulary_forms() {
        let canonical = julie_extract_artifact::resolution_store::canonical_reference_kind;
        assert_eq!(canonical("identifier", "call"), Some("calls"));
        assert_eq!(canonical("relationship", "calls"), Some("calls"));
        assert_eq!(canonical("identifier", "type_usage"), Some("uses"));
        assert_eq!(canonical("identifier", "member_access"), Some("references"));
        assert_eq!(canonical("identifier", "variable_ref"), Some("references"));
        for kind in [
            "calls",
            "extends",
            "implements",
            "imports",
            "instantiates",
            "references",
            "uses",
        ] {
            assert_eq!(canonical("relationship", kind), Some(kind));
        }
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

    // ---- delta name-set expansion ---------------------------------------

    fn import_row(file: &str, local: &str, imported: Option<&str>) -> ImportRecord {
        ImportRecord {
            file_id: file.to_string(),
            local_name: local.to_string(),
            imported_name: imported.map(str::to_string),
            source: None,
            module_file_id: None,
            is_type_only: false,
            is_default: false,
            is_namespace: false,
        }
    }

    fn type_fact(symbol_id: &str, resolved_type: &str) -> TypeFact {
        TypeFact {
            symbol_id: symbol_id.to_string(),
            resolved_type: resolved_type.to_string(),
            is_inferred: false,
        }
    }

    fn name_set(values: &[&str]) -> BTreeSet<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn import_names_linked_to_carries_the_export_of_a_touched_local_alias() {
        let index = WorkspaceCandidateIndex::build(
            vec![],
            vec![],
            vec![import_row("src", "Bar", Some("Foo"))],
        );
        assert_eq!(
            index.import_names_linked_to(&HashSet::from(["Bar"])),
            name_set(&["Bar", "Foo"])
        );
    }

    #[test]
    fn import_names_linked_to_carries_the_local_of_a_touched_exported_alias() {
        let index = WorkspaceCandidateIndex::build(
            vec![],
            vec![],
            vec![import_row("src", "Bar", Some("Foo"))],
        );
        assert_eq!(
            index.import_names_linked_to(&HashSet::from(["Foo"])),
            name_set(&["Bar", "Foo"])
        );
    }

    #[test]
    fn import_names_linked_to_matches_an_import_with_no_imported_name() {
        let index =
            WorkspaceCandidateIndex::build(vec![], vec![], vec![import_row("src", "Foo", None)]);
        assert_eq!(
            index.import_names_linked_to(&HashSet::from(["Foo"])),
            name_set(&["Foo"])
        );
    }

    #[test]
    fn import_names_linked_to_leaves_out_imports_neither_side_of_which_was_touched() {
        let index = WorkspaceCandidateIndex::build(
            vec![],
            vec![],
            vec![
                import_row("src", "Bar", Some("Foo")),
                import_row("src", "Qux", Some("Quux")),
            ],
        );
        assert_eq!(
            index.import_names_linked_to(&HashSet::from(["Foo"])),
            name_set(&["Bar", "Foo"])
        );
    }

    #[test]
    fn import_names_linked_to_is_empty_for_an_empty_name_set() {
        let index = WorkspaceCandidateIndex::build(
            vec![],
            vec![],
            vec![import_row("src", "Bar", Some("Foo"))],
        );
        assert!(index.import_names_linked_to(&HashSet::new()).is_empty());
    }

    #[test]
    fn receiver_names_bound_to_types_carries_receivers_typed_by_a_touched_type() {
        let index = WorkspaceCandidateIndex::build(
            vec![
                sym("svc", "service", SymbolKind::Variable, "rust", "src"),
                sym("other", "helper", SymbolKind::Variable, "rust", "src"),
            ],
            vec![type_fact("svc", "Widget"), type_fact("other", "Gadget")],
            vec![],
        );
        assert_eq!(
            index.receiver_names_bound_to_types(&HashSet::from(["Widget"])),
            name_set(&["service"])
        );
    }

    #[test]
    fn receiver_names_bound_to_types_skips_facts_whose_symbol_is_not_indexed() {
        let index =
            WorkspaceCandidateIndex::build(vec![], vec![type_fact("missing", "Widget")], vec![]);
        assert!(
            index
                .receiver_names_bound_to_types(&HashSet::from(["Widget"]))
                .is_empty()
        );
    }

    #[test]
    fn receiver_names_bound_to_types_is_empty_for_an_empty_name_set() {
        let index = WorkspaceCandidateIndex::build(
            vec![sym("svc", "service", SymbolKind::Variable, "rust", "src")],
            vec![type_fact("svc", "Widget")],
            vec![],
        );
        assert!(
            index
                .receiver_names_bound_to_types(&HashSet::new())
                .is_empty()
        );
    }
}

// ---------------------------------------------------------------------------
// Workspace-pass tests (DB-facing: metadata finalization + the failed path)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod workspace_tests {
    use super::*;
    use julie_extract_artifact::metadata::ArtifactMetadata;
    use julie_extract_artifact::model::{ResolutionWriteOutcome, WriteResult};
    use julie_extract_artifact::resolution_store::{self, ResolutionStatus};
    use julie_extract_artifact::writer::ArtifactWriter;

    fn metadata() -> ArtifactMetadata {
        ArtifactMetadata {
            artifact_id: "artifact-test".to_string(),
            root_path: "/tmp/root".to_string(),
            binary_version: "0.0.0-test".to_string(),
            hash_algorithm: "blake3".to_string(),
            parser_inventory_fingerprint: "sha256:parser".to_string(),
            capability_snapshot_fingerprint: "sha256:capability".to_string(),
            created_at: "1970-01-01T00:00:00Z".to_string(),
            updated_at: "1970-01-01T00:00:00Z".to_string(),
        }
    }

    fn report(status: ResolutionStatus, last_full_revision: i64) -> ResolutionReport {
        ResolutionReport {
            rows: None,
            status,
            version: RESOLUTION_VERSION,
            last_full_revision,
            tier2_gated_languages: BTreeSet::new(),
        }
    }

    #[test]
    fn finalize_writes_failed_status_when_hook_reported_failure() {
        // INVARIANT: an injected failing hook (the writer surfaces `failed`) makes
        // the CLI record a durable `failed` status even though the hook's
        // in-transaction writes were rolled back. This is why metadata is written
        // post-commit, not inside the hook.
        let writer = ArtifactWriter::open_in_memory(metadata()).unwrap();
        let conn = writer.into_connection();
        let write_result = WriteResult {
            resolution: ResolutionWriteOutcome {
                counts: Default::default(),
                failed: Some("resolver boom".to_string()),
            },
            ..Default::default()
        };
        // A failed hook leaves no captured report (the closure returned Err).
        assert!(finalize_resolution_metadata(&conn, &write_result, None));
        let meta = resolution_store::read_resolution_metadata(&conn)
            .unwrap()
            .expect("failed status must be recorded durably");
        assert_eq!(meta.status, ResolutionStatus::Failed);
        assert_eq!(meta.version, RESOLUTION_VERSION);
    }

    #[test]
    fn finalize_writes_report_status_on_success() {
        // INVARIANT: a successful pass records the report's status/version/revision.
        let writer = ArtifactWriter::open_in_memory(metadata()).unwrap();
        let conn = writer.into_connection();
        let write_result = WriteResult::default();
        assert!(finalize_resolution_metadata(
            &conn,
            &write_result,
            Some(&report(ResolutionStatus::Complete, 7)),
        ));
        let meta = resolution_store::read_resolution_metadata(&conn)
            .unwrap()
            .expect("complete status must be recorded");
        assert_eq!(meta.status, ResolutionStatus::Complete);
        assert_eq!(meta.last_full_revision, 7);
    }

    #[test]
    fn finalize_failure_preserves_last_full_revision() {
        // INVARIANT: a later failure keeps the last known-good `last_full_revision`
        // instead of clobbering it (Miller keeps gating on the last good full pass).
        let writer = ArtifactWriter::open_in_memory(metadata()).unwrap();
        let conn = writer.into_connection();
        // Seed a prior clean Full at revision 5.
        finalize_resolution_metadata(
            &conn,
            &WriteResult::default(),
            Some(&report(ResolutionStatus::Complete, 5)),
        );
        // A subsequent hook failure records `failed` but preserves revision 5.
        let failed = WriteResult {
            resolution: ResolutionWriteOutcome {
                counts: Default::default(),
                failed: Some("boom".to_string()),
            },
            ..Default::default()
        };
        finalize_resolution_metadata(&conn, &failed, None);
        let meta = resolution_store::read_resolution_metadata(&conn)
            .unwrap()
            .unwrap();
        assert_eq!(meta.status, ResolutionStatus::Failed);
        assert_eq!(meta.last_full_revision, 5);
    }

    #[test]
    fn finalize_is_noop_without_report_or_failure() {
        // INVARIANT: a hookless write (no report, no failure) writes no metadata.
        let writer = ArtifactWriter::open_in_memory(metadata()).unwrap();
        let conn = writer.into_connection();
        assert!(!finalize_resolution_metadata(
            &conn,
            &WriteResult::default(),
            None
        ));
        assert!(
            resolution_store::read_resolution_metadata(&conn)
                .unwrap()
                .is_none()
        );
    }
}
