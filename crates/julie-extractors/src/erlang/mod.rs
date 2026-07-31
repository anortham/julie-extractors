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
use std::collections::{HashMap, HashSet};

use tree_sitter::{Node, Tree};

use crate::base::{BaseExtractor, Identifier, Relationship, Symbol};
use helpers::{NameArity, function_arity_entries, named_children, wild_attribute_name};

mod attributes;
mod definition_forms;
mod doc;
mod helpers;

const EXPORT_ALL_OPTION: &str = "export_all";
const MODULE_DOC_ATTRIBUTE: &str = "moduledoc";

pub struct ErlangExtractor {
    pub(crate) base: BaseExtractor,
    /// `(name, arity)` pairs listed in `-export([...])`.
    pub(crate) exported_functions: HashSet<NameArity>,
    /// `(name, arity)` pairs listed in `-export_type([...])`.
    pub(crate) exported_types: HashSet<NameArity>,
    /// `-compile(export_all)`, standalone or inside a compile-options list.
    pub(crate) exports_everything: bool,
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
        }
    }

    /// Extract all symbols from Erlang source code.
    pub fn extract_symbols(&mut self, tree: &Tree) -> Vec<Symbol> {
        self.base.symbol_map.clear();
        self.exported_functions.clear();
        self.exported_types.clear();
        self.exports_everything = false;

        let root = tree.root_node();
        let declarations = named_children(&root);
        self.collect_exports(&declarations);

        let clause_counts = self.clause_counts(&declarations);
        let module_doc = self.module_doc(&declarations);

        let mut symbols = Vec::new();
        let mut module_id: Option<String> = None;
        let mut emitted: HashSet<NameArity> = HashSet::new();

        for declaration in &declarations {
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
                        Some(definition_forms::extract_function(
                            self,
                            declaration,
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

    /// Relationship extraction is not part of the Erlang symbol tier yet.
    pub fn extract_relationships(
        &mut self,
        _tree: &Tree,
        _symbols: &[Symbol],
    ) -> Vec<Relationship> {
        Vec::new()
    }

    /// Identifier extraction is not part of the Erlang symbol tier yet.
    pub fn extract_identifiers(&mut self, _tree: &Tree, _symbols: &[Symbol]) -> Vec<Identifier> {
        Vec::new()
    }

    /// Type inference is not part of the Erlang symbol tier yet.
    pub fn infer_types(&self, _symbols: &[Symbol]) -> HashMap<String, String> {
        HashMap::new()
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
