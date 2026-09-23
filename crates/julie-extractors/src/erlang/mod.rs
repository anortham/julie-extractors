/// Erlang language extractor with support for:
/// - Modules (`-module`), records (`-record`) and their fields
/// - Functions grouped by name/arity across clauses, with export-driven visibility
/// - Macros (`-define`), types (`-type`/`-opaque`), behaviour callbacks (`-callback`)
/// - EDoc `%%` comment blocks and OTP 27 `-doc` / `-moduledoc` attributes
///
/// Erlang declarations are all top-level: `source_file` children are attributes
/// and `fun_decl` nodes, with no nesting to recurse through. Extraction is
/// therefore a pre-scan (exports, clause counts, `-moduledoc`) followed by a
/// single ordered pass over those children.
///
/// A file with parse errors contributes extra declarations recovered by
/// [`recovery`], merged into that same ordered list so the symbol, type,
/// relationship, and identifier walks all see one declaration set.
use std::collections::{HashMap, HashSet};

use tree_sitter::{Node, Tree};

use crate::base::{
    BaseExtractor, Identifier, NormalizedSpan, ParseDiagnostic, ParseDiagnosticKind, Relationship,
    Symbol, TestRole,
};
use crate::test_detection::ErlangTestModule;
use helpers::{NameArity, function_arity_entries, named_children, wild_attribute_name};

mod attributes;
mod definition_forms;
mod doc;
mod helpers;
mod identifiers;
mod lexical;
mod mfa;
mod parameters;
mod recovery;
mod relationships;
pub(crate) mod term_config;
mod test_fixtures;
mod type_facts;
mod types;

const EXPORT_ALL_OPTION: &str = "export_all";
const MODULE_DOC_ATTRIBUTE: &str = "moduledoc";
const EUNIT_HEADER: &str = "eunit/include/eunit.hrl";
const PROPER_HEADER: &str = "proper/include/proper.hrl";

/// The primary tree's top-level children plus the declarations [`recovery`]
/// rescued from re-parses, in source order.
///
/// A recovered node is admitted only when it is a real declaration kind, starts
/// at column 0 the way every top-level Erlang form does, is not literal text,
/// and does not repeat a declaration already admitted at that offset. Offsets
/// are comparable across trees because recovery blanks rather than removes the
/// text before its resume point, so a recovered node's byte range still
/// addresses the original file.
///
/// The form that failed to parse is never itself readmitted: recovery only
/// resumes strictly after an error starts, so its own head — the one whose
/// argument list would yield an invented arity — is never a resume point.
fn merge_declarations<'tree>(
    primary: &'tree Tree,
    recovery: &'tree recovery::Recovery,
) -> Vec<Node<'tree>> {
    let root = primary.root_node();
    let mut declarations = named_children(&root);
    let mut claimed: HashSet<usize> = declarations
        .iter()
        .filter(|node| recovery::RECOVERABLE_DECLARATION_KINDS.contains(&node.kind()))
        .map(|node| node.start_byte())
        .collect();

    for tree in &recovery.trees {
        let root = tree.root_node();
        for node in named_children(&root) {
            if !recovery::RECOVERABLE_DECLARATION_KINDS.contains(&node.kind())
                || node.start_position().column != 0
                || recovery.is_literal_text(node.start_byte())
                || !claimed.insert(node.start_byte())
            {
                continue;
            }
            declarations.push(node);
        }
    }

    declarations.sort_by_key(|node| node.start_byte());
    declarations
}

/// A declaration paired with the byte offset where its own text ends.
///
/// A damaged parse can leave a `fun_decl` that swallows the forms after it while
/// recovery also rescues those forms precisely. The rescued forms own their
/// bytes, so the damaged node ends where the next declaration starts. Top-level
/// forms in a clean file never overlap, so there `end` is the node's own end.
#[derive(Clone, Copy)]
pub(super) struct Bounded<'tree> {
    pub(super) node: Node<'tree>,
    pub(super) end: usize,
}

fn bounded<'tree>(declarations: &[Node<'tree>]) -> Vec<Bounded<'tree>> {
    (0..declarations.len())
        .map(|index| Bounded {
            node: declarations[index],
            end: bounded_end(declarations, index),
        })
        .collect()
}

fn bounded_end(declarations: &[Node], index: usize) -> usize {
    let node = declarations[index];
    declarations
        .get(index + 1)
        .map(|next| next.start_byte())
        .filter(|&next_start| next_start < node.end_byte())
        .unwrap_or(node.end_byte())
}

pub struct ErlangExtractor {
    pub(crate) base: BaseExtractor,
    /// `(name, arity)` pairs listed in `-export([...])`.
    pub(crate) exported_functions: HashSet<NameArity>,
    /// `(name, arity)` pairs listed in `-export_type([...])`.
    pub(crate) exported_types: HashSet<NameArity>,
    /// `-compile(export_all)`, standalone or inside a compile-options list.
    pub(crate) exports_everything: bool,
    /// Which test frameworks, if any, own this module.
    pub(crate) test_module: ErlangTestModule,
    /// Common Test case names listed by literal `all/0` and `groups/0` bodies;
    /// `None` when the suite computes them, so the export rule applies.
    pub(crate) common_test_cases: Option<HashSet<String>>,
    /// Record names listed in `-export_record([...])`.
    pub(crate) exported_records: HashSet<String>,
    /// A `.hrl` header exists to be included elsewhere, so its declarations
    /// are public.
    pub(crate) is_header: bool,
    /// Roles the EUnit fixture tuples of `*_test_` generators give the funs
    /// they name: setup, cleanup, and instantiated tests.
    pub(crate) eunit_fixture_roles: HashMap<NameArity, TestRole>,
    /// Declared `-spec`, `-callback`, `-type`, `-opaque` and `-nominal` forms.
    declared_types: types::DeclaredTypes,
    /// Re-parses produced by [`recovery`] for a file with parse errors. Owned
    /// here so every walk sees the same recovered declarations; empty for a file
    /// that parsed clean.
    recovery: Option<recovery::Recovery>,
}

impl ErlangExtractor {
    pub fn new(
        language: String,
        file_path: String,
        content: String,
        workspace_root: &std::path::Path,
    ) -> Self {
        Self {
            base: BaseExtractor::new(language, file_path, content, workspace_root),
            exported_functions: HashSet::new(),
            exported_types: HashSet::new(),
            exports_everything: false,
            test_module: ErlangTestModule::default(),
            common_test_cases: None,
            exported_records: HashSet::new(),
            is_header: false,
            eunit_fixture_roles: HashMap::new(),
            declared_types: types::DeclaredTypes::default(),
            recovery: None,
        }
    }

    /// Run `body` over the file's full declaration list — the primary tree's
    /// top-level children plus anything [`recovery`] rescued from after a parse
    /// error, in source order.
    ///
    /// The recovery result is moved out of `self` for the call so the borrow
    /// checker can see that the declaration nodes borrow it rather than the
    /// extractor, then moved back.
    fn with_declarations<R>(
        &mut self,
        tree: &Tree,
        body: impl FnOnce(&mut Self, &[Node<'_>]) -> R,
    ) -> R {
        let recovery = self
            .recovery
            .take()
            .unwrap_or_else(|| recovery::recover(&self.base.content, tree));
        let result = {
            let declarations = merge_declarations(tree, &recovery);
            body(self, &declarations)
        };
        self.recovery = Some(recovery);
        result
    }

    /// Extract all symbols from Erlang source code.
    pub fn extract_symbols(&mut self, tree: &Tree) -> Vec<Symbol> {
        self.exported_functions.clear();
        self.exported_types.clear();
        self.exported_records.clear();
        self.exports_everything = false;
        self.is_header = self.base.file_path.ends_with(".hrl");

        if let Some(config) = term_config::TermConfig::for_path(&self.base.file_path) {
            return term_config::extract_symbols(self, tree, config);
        }
        self.with_declarations(tree, Self::extract_symbols_from)
    }

    fn extract_symbols_from(&mut self, declarations: &[Node]) -> Vec<Symbol> {
        self.collect_exports(declarations);
        self.test_module = self.classify_test_module(declarations);
        self.common_test_cases = self.common_test_cases(declarations);
        self.eunit_fixture_roles = test_fixtures::eunit_fixture_roles(self, declarations);
        self.declared_types = types::collect(&self.base, declarations);
        let same_file_records = type_facts::same_file_record_names(&self.base, declarations);

        let clause_counts = self.clause_counts(declarations);
        let module_doc = self.module_doc(declarations);

        let mut symbols = Vec::new();
        let mut module_id: Option<String> = None;
        let mut emitted: HashSet<NameArity> = HashSet::new();

        for (index, declaration) in declarations.iter().enumerate() {
            let parent_id = module_id.clone();
            let parent_id = parent_id.as_deref();

            let symbol = match declaration.kind() {
                "module_attribute" => {
                    let symbol = attributes::extract_module(self, declaration, module_doc.clone());
                    if let Some(symbol) = &symbol {
                        module_id = Some(symbol.id.clone());
                    }
                    symbol
                }
                "record_decl" => {
                    attributes::extract_record(self, declaration, &mut symbols, parent_id);
                    None
                }
                "pp_define" => attributes::extract_macro(self, declaration, parent_id),
                "type_alias" | "opaque" | "nominal" => {
                    attributes::extract_type(self, declaration, parent_id)
                }
                "callback" => attributes::extract_callback(self, declaration, parent_id),
                "fun_decl" => {
                    let clause = definition_forms::function_clause(self, declaration);
                    if let Some(clause) = clause
                        && emitted.insert(clause.identity.clone())
                    {
                        let clause_count =
                            clause_counts.get(&clause.identity).copied().unwrap_or(1);
                        let run = self.clause_run(declarations, index, &clause.identity);
                        let clauses: Vec<Node> = run.iter().map(|&i| declarations[i]).collect();
                        if let Some(extent) = self.clause_run_extent(declarations, &run) {
                            symbols.extend(definition_forms::extract_function(
                                self,
                                declaration,
                                extent,
                                &clause,
                                clause_count,
                                parent_id,
                                &clauses,
                                &same_file_records,
                            ));
                        }
                    }
                    None
                }
                _ => None,
            };

            if let Some(symbol) = symbol {
                symbols.push(symbol);
            }
        }

        symbols
    }

    /// Extract same-file call edges, plus structured pending edges for remote
    /// calls, `-behaviour`, `-include`/`-include_lib`, and `-import`.
    pub fn extract_relationships(&mut self, tree: &Tree, symbols: &[Symbol]) -> Vec<Relationship> {
        if term_config::TermConfig::for_path(&self.base.file_path).is_some() {
            term_config::extract_relationships(self, tree, symbols);
            return Vec::new();
        }
        self.with_declarations(tree, |extractor, declarations| {
            relationships::extract_relationships(extractor, &bounded(declarations), symbols)
        })
    }

    pub fn get_pending_relationships(&self) -> Vec<crate::base::PendingRelationship> {
        self.base.get_pending_relationships()
    }

    pub fn get_structured_pending_relationships(
        &self,
    ) -> Vec<crate::base::StructuredPendingRelationship> {
        self.base.get_structured_pending_relationships()
    }

    /// Extract call sites, fun references, macro usages, and record/field
    /// references from function clauses and macro bodies.
    pub fn extract_identifiers(&mut self, tree: &Tree, symbols: &[Symbol]) -> Vec<Identifier> {
        if term_config::TermConfig::for_path(&self.base.file_path).is_some() {
            return Vec::new();
        }
        self.with_declarations(tree, |extractor, declarations| {
            identifiers::extract_identifiers(extractor, &bounded(declarations), symbols)
        })
    }

    /// Diagnostics the parse tree cannot carry: one span when error recovery
    /// stopped on its parse budget rather than because the file was recovered,
    /// so a consumer can tell a complete result from a truncated one. Empty
    /// unless [`extract_symbols`](Self::extract_symbols) has run.
    pub fn parse_diagnostics(&self) -> Vec<ParseDiagnostic> {
        let Some(offset) = self
            .recovery
            .as_ref()
            .and_then(|recovery| recovery.exhausted_at)
        else {
            return Vec::new();
        };
        let Some(span) = NormalizedSpan::from_content_range_with_line_starts(
            &self.base.content,
            self.base.line_starts(),
            offset,
            self.base.content.len(),
        ) else {
            return Vec::new();
        };

        vec![ParseDiagnostic {
            kind: ParseDiagnosticKind::Error,
            message: Some(format!(
                "erlang recovery budget exhausted after {} re-parses; \
                 unresolved parse errors remain from byte {offset}, \
                 so declarations after it may be missing",
                recovery::MAX_RECOVERY_PARSES
            )),
            start_line: span.start_line,
            start_column: span.start_column,
            end_line: span.end_line,
            end_column: span.end_column,
            start_byte: span.start_byte,
            end_byte: span.end_byte,
        }]
    }

    /// Declared `-spec`, `-callback`, `-type` and `-opaque` forms, matched to
    /// the symbols they annotate by `(name, arity)`.
    pub fn infer_types(&self, symbols: &[Symbol]) -> HashMap<String, String> {
        types::infer_types(&self.declared_types, symbols)
    }

    fn collect_exports(&mut self, declarations: &[Node]) {
        let test_only = test_only_declarations(&self.base, declarations);
        for declaration in declarations {
            match declaration.kind() {
                "export_record_attribute" => {
                    for atom in helpers::child_named_kinds(declaration, "atom") {
                        self.exported_records
                            .insert(helpers::unquote_atom(&self.base.get_node_text(&atom)));
                    }
                }
                "export_attribute" => {
                    self.exported_functions
                        .extend(function_arity_entries(&self.base, declaration));
                }
                "export_type_attribute" => {
                    self.exported_types
                        .extend(function_arity_entries(&self.base, declaration));
                }
                "compile_options_attribute"
                    if self.declares_export_all(declaration)
                        && !test_only.contains(&declaration.start_byte()) =>
                {
                    self.exports_everything = true;
                }
                _ => {}
            }
        }
    }

    /// EUnit owns `*_tests` modules and any module that pulls in `eunit.hrl`
    /// outside an `-ifdef(TEST)` block; Common Test owns `*_SUITE` modules;
    /// PropEr owns modules that include `proper.hrl`.
    fn classify_test_module(&self, declarations: &[Node]) -> ErlangTestModule {
        let module_name = declarations
            .iter()
            .find(|declaration| declaration.kind() == "module_attribute")
            .and_then(|declaration| helpers::first_atom_text(&self.base, declaration))
            .unwrap_or_default();

        let test_only = test_only_declarations(&self.base, declarations);
        let includes = |header: &str| {
            declarations
                .iter()
                .filter(|declaration| {
                    matches!(declaration.kind(), "pp_include" | "pp_include_lib")
                        && !test_only.contains(&declaration.start_byte())
                })
                .filter_map(|declaration| helpers::find_child_by_type(declaration, "string"))
                .any(|string| self.base.get_node_text(&string).contains(header))
        };

        ErlangTestModule::classify(
            &module_name,
            includes(EUNIT_HEADER),
            includes(PROPER_HEADER),
        )
    }

    /// Test case names a Common Test suite lists in `all/0` and `groups/0`.
    /// Each must return a literal list; `{group, G}` entries name groups, not
    /// cases, and `{testcase, Name, ...}` names a case.
    fn common_test_cases(&self, declarations: &[Node]) -> Option<HashSet<String>> {
        if !self.test_module.is_common_test() {
            return None;
        }
        let mut cases = HashSet::new();
        let all = self.literal_list_body(declarations, "all")?;
        self.collect_case_entries(&all, &mut cases);
        if declarations
            .iter()
            .any(|declaration| self.is_zero_arity_function(declaration, "groups"))
        {
            let groups = self.literal_list_body(declarations, "groups")?;
            for group in named_children(&groups) {
                if group.kind() == "tuple"
                    && let Some(members) = named_children(&group)
                        .into_iter()
                        .rfind(|child| child.kind() == "list")
                {
                    self.collect_case_entries(&members, &mut cases);
                }
            }
        }
        Some(cases)
    }

    fn collect_case_entries(&self, list: &Node, cases: &mut HashSet<String>) {
        for entry in named_children(list) {
            match entry.kind() {
                "atom" => {
                    cases.insert(helpers::unquote_atom(&self.base.get_node_text(&entry)));
                }
                "tuple" => {
                    let parts = named_children(&entry);
                    let tag = parts
                        .first()
                        .map(|tag| helpers::unquote_atom(&self.base.get_node_text(tag)));
                    if tag.as_deref() == Some("testcase")
                        && let Some(name) = parts.get(1).filter(|name| name.kind() == "atom")
                    {
                        cases.insert(helpers::unquote_atom(&self.base.get_node_text(name)));
                    }
                }
                _ => {}
            }
        }
    }

    fn is_zero_arity_function(&self, declaration: &Node, name: &str) -> bool {
        declaration.kind() == "fun_decl"
            && definition_forms::function_clause(self, declaration)
                .is_some_and(|clause| clause.identity == (name.to_string(), 0))
    }

    /// The list a single-clause zero-arity function returns, when its whole
    /// body is that literal list.
    fn literal_list_body<'tree>(
        &self,
        declarations: &[Node<'tree>],
        name: &str,
    ) -> Option<Node<'tree>> {
        let mut functions = declarations
            .iter()
            .filter(|declaration| self.is_zero_arity_function(declaration, name));
        let function = functions.next()?;
        if functions.next().is_some() {
            return None;
        }
        let clauses = named_children(function);
        let [clause] = clauses.as_slice() else {
            return None;
        };
        let body = clause.child_by_field_name("body")?;
        let exprs = named_children(&body);
        match exprs.as_slice() {
            [list] if list.kind() == "list" => Some(*list),
            _ => None,
        }
    }

    fn declares_export_all(&self, declaration: &Node) -> bool {
        fn option_atoms(base: &BaseExtractor, node: &Node, atoms: &mut Vec<String>) {
            for child in named_children(node) {
                if child.kind() == "atom" {
                    atoms.push(helpers::unquote_atom(&base.get_node_text(&child)));
                } else {
                    option_atoms(base, &child, atoms);
                }
            }
        }

        let mut atoms = Vec::new();
        option_atoms(&self.base, declaration, &mut atoms);
        atoms.iter().any(|atom| atom == EXPORT_ALL_OPTION)
    }

    /// Indices of the `fun_decl` clauses starting at `first` that share one
    /// name/arity identity.
    ///
    /// Erlang requires a function's clauses to be adjacent, so the run ends at
    /// the first declaration that is neither a comment nor another clause of the
    /// same function. Without this the symbol would cover clause one alone, and
    /// its body hash would not move when a later clause changed.
    fn clause_run(&self, declarations: &[Node], first: usize, identity: &NameArity) -> Vec<usize> {
        let mut run = vec![first];
        for (index, declaration) in declarations.iter().enumerate().skip(first + 1) {
            match declaration.kind() {
                "comment" => {}
                "fun_decl"
                    if definition_forms::function_clause(self, declaration)
                        .is_some_and(|clause| &clause.identity == identity) =>
                {
                    run.push(index);
                }
                _ => break,
            }
        }
        run
    }

    /// Span from the first clause through the end of the last one, where a
    /// damaged clause ends before the next declaration recovery rescued.
    fn clause_run_extent(&self, declarations: &[Node], run: &[usize]) -> Option<NormalizedSpan> {
        let start_byte = declarations.get(*run.first()?)?.start_byte();
        let bound = bounded_end(declarations, *run.last()?);
        let end_byte = start_byte + self.base.content.get(start_byte..bound)?.trim_end().len();

        NormalizedSpan::from_content_range_with_line_starts(
            &self.base.content,
            self.base.line_starts(),
            start_byte,
            end_byte,
        )
    }

    fn clause_counts(&self, declarations: &[Node]) -> HashMap<NameArity, usize> {
        let mut counts = HashMap::new();
        for declaration in declarations {
            if declaration.kind() != "fun_decl" {
                continue;
            }
            if let Some(clause) = definition_forms::function_clause(self, declaration) {
                *counts.entry(clause.identity).or_insert(0) += 1;
            }
        }
        counts
    }

    fn module_doc(&self, declarations: &[Node]) -> Option<String> {
        declarations
            .iter()
            .filter(|declaration| {
                (declaration.kind() == "wild_attribute"
                    && wild_attribute_name(&self.base, declaration).as_deref()
                        == Some(MODULE_DOC_ATTRIBUTE))
                    || helpers::doc_macro_name(&self.base, declaration).as_deref()
                        == Some("MODULEDOC")
            })
            .find_map(|declaration| doc::module_doc_text(self, declaration))
    }
}

/// Start bytes of the declarations that only exist in a test build: those
/// inside `-ifdef(TEST)` / `-ifdef(EUNIT)` blocks, or the `-else` branch of an
/// `-ifndef(TEST)`.
fn test_only_declarations(base: &BaseExtractor, declarations: &[Node]) -> HashSet<usize> {
    let mut stack: Vec<Option<bool>> = Vec::new();
    let mut test_only = HashSet::new();
    for declaration in declarations {
        let names_test_macro = || {
            declaration
                .child_by_field_name("name")
                .is_some_and(|name| matches!(base.get_node_text(&name).as_str(), "TEST" | "EUNIT"))
        };
        match declaration.kind() {
            "pp_ifdef" => stack.push(names_test_macro().then_some(true)),
            "pp_ifndef" => stack.push(names_test_macro().then_some(false)),
            "pp_if" => stack.push(None),
            "pp_else" => {
                if let Some(Some(branch)) = stack.last_mut() {
                    *branch = !*branch;
                }
            }
            "pp_endif" => {
                stack.pop();
            }
            _ if stack.contains(&Some(true)) => {
                test_only.insert(declaration.start_byte());
            }
            _ => {}
        }
    }
    test_only
}
