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

use crate::base::{BaseExtractor, Identifier, NormalizedSpan, Relationship, Symbol};
use crate::test_detection::ErlangTestModule;
use helpers::{NameArity, function_arity_entries, named_children, wild_attribute_name};

mod attributes;
mod definition_forms;
mod doc;
mod helpers;
mod identifiers;
mod lexical;
mod recovery;
mod relationships;
mod types;

const EXPORT_ALL_OPTION: &str = "export_all";
const MODULE_DOC_ATTRIBUTE: &str = "moduledoc";
const EUNIT_HEADER: &str = "eunit/include/eunit.hrl";

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

/// Declarations to walk for identifiers and relationships: those not already
/// covered by an earlier declaration.
///
/// A damaged parse can leave a `fun_decl` that swallows the forms after it while
/// recovery also rescues one of those forms precisely. Both are real symbols, but
/// walking both would attribute the overlapping bytes twice. Top-level forms in a
/// clean file never overlap, so this is the identity there.
fn walkable<'tree>(declarations: &[Node<'tree>]) -> Vec<Node<'tree>> {
    let mut walkable = Vec::with_capacity(declarations.len());
    let mut covered_end = 0;

    for declaration in declarations {
        if declaration.end_byte() <= covered_end {
            continue;
        }
        covered_end = declaration.end_byte();
        walkable.push(*declaration);
    }

    walkable
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
    /// Declared `-spec`, `-callback`, `-type` and `-opaque` forms.
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
        self.base.symbol_map.clear();
        self.exported_functions.clear();
        self.exported_types.clear();
        self.exports_everything = false;

        self.with_declarations(tree, Self::extract_symbols_from)
    }

    fn extract_symbols_from(&mut self, declarations: &[Node]) -> Vec<Symbol> {
        self.collect_exports(declarations);
        self.test_module = self.classify_test_module(declarations);
        self.declared_types = types::collect(&self.base, declarations);

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
                "type_alias" | "opaque" => attributes::extract_type(self, declaration, parent_id),
                "callback" => attributes::extract_callback(self, declaration, parent_id),
                "fun_decl" => {
                    let clause = definition_forms::function_clause(self, declaration);
                    clause.and_then(|clause| {
                        if !emitted.insert(clause.identity.clone()) {
                            return None;
                        }
                        let clause_count =
                            clause_counts.get(&clause.identity).copied().unwrap_or(1);
                        let extent =
                            self.clause_run_extent(declarations, index, &clause.identity)?;
                        Some(definition_forms::extract_function(
                            self,
                            declaration,
                            extent,
                            &clause,
                            clause_count,
                            parent_id,
                        ))
                    })
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
        self.with_declarations(tree, |extractor, declarations| {
            relationships::extract_relationships(extractor, &walkable(declarations), symbols)
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
        self.with_declarations(tree, |extractor, declarations| {
            identifiers::extract_identifiers(extractor, &walkable(declarations), symbols)
        })
    }

    /// Declared `-spec`, `-callback`, `-type` and `-opaque` forms, matched to
    /// the symbols they annotate by `(name, arity)`.
    pub fn infer_types(&self, symbols: &[Symbol]) -> HashMap<String, String> {
        types::infer_types(&self.declared_types, symbols)
    }

    fn collect_exports(&mut self, declarations: &[Node]) {
        for declaration in declarations {
            match declaration.kind() {
                "export_attribute" => {
                    self.exported_functions
                        .extend(function_arity_entries(&self.base, declaration));
                }
                "export_type_attribute" => {
                    self.exported_types
                        .extend(function_arity_entries(&self.base, declaration));
                }
                "compile_options_attribute" if self.declares_export_all(declaration) => {
                    self.exports_everything = true;
                }
                _ => {}
            }
        }
    }

    /// EUnit owns `*_tests` modules and any module that pulls in `eunit.hrl`;
    /// Common Test owns `*_SUITE` modules.
    fn classify_test_module(&self, declarations: &[Node]) -> ErlangTestModule {
        let module_name = declarations
            .iter()
            .find(|declaration| declaration.kind() == "module_attribute")
            .and_then(|declaration| helpers::first_atom_text(&self.base, declaration))
            .unwrap_or_default();

        let includes_eunit = declarations
            .iter()
            .filter(|declaration| matches!(declaration.kind(), "pp_include" | "pp_include_lib"))
            .filter_map(|declaration| helpers::find_child_by_type(declaration, "string"))
            .any(|string| self.base.get_node_text(&string).contains(EUNIT_HEADER));

        ErlangTestModule::classify(&module_name, includes_eunit)
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

    /// Span of a whole function: from the `fun_decl` at `first` through the end
    /// of the last clause in the contiguous sibling run that shares its
    /// name/arity.
    ///
    /// Erlang requires a function's clauses to be adjacent, so the run ends at
    /// the first declaration that is not another clause of the same function.
    /// Without this the symbol would cover clause one alone, and its body hash
    /// would not move when a later clause changed.
    fn clause_run_extent(
        &self,
        declarations: &[Node],
        first: usize,
        identity: &NameArity,
    ) -> Option<NormalizedSpan> {
        let start_byte = declarations.get(first)?.start_byte();
        let mut end_byte = declarations[first].end_byte();

        for declaration in declarations.get(first + 1..)? {
            if declaration.kind() != "fun_decl" {
                break;
            }
            let Some(clause) = definition_forms::function_clause(self, declaration) else {
                break;
            };
            if &clause.identity != identity {
                break;
            }
            end_byte = end_byte.max(declaration.end_byte());
        }

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
            .filter(|declaration| declaration.kind() == "wild_attribute")
            .find(|declaration| {
                wild_attribute_name(&self.base, declaration).as_deref()
                    == Some(MODULE_DOC_ATTRIBUTE)
            })
            .and_then(|declaration| doc::module_doc_text(self, declaration))
    }
}
