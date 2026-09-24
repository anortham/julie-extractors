//! Type facts for Erlang variables.
//!
//! Head record patterns (`#foo{} = X`) are syntax-stated. A body match
//! `X = Value` records an inferred fact when `Value` is a record literal or
//! record update (`Old#foo{..}`) of a same-file `-record`, or a call to a same-file function whose `-spec`
//! clauses all return one base type: `load()`, `?MODULE:load()`, or
//! `this_module:load()`. `X = Y = Value` gives both variables that type.
//! `{ok, X} = load()` binds `X` and gives it `T` when every `{ok, T}`
//! alternative of the spec agrees and no other alternative can match. A
//! variable bound by several matches in one function keeps a fact only when
//! every match gives it the same type. Calls to other modules, funs, macros,
//! `catch`, atom literals, `no_return()`, and type-variable, union, tuple, or
//! list returns record nothing.

use std::collections::{HashMap, HashSet};

use tree_sitter::Node;

use super::ErlangExtractor;
use super::helpers::{NameArity, arg_count, first_atom_text, named_children, unquote_atom};
use super::types::{DeclaredType, DeclaredTypes, SpecReturn, is_ok_atom};
use crate::base::types::TypeNameRules;
use crate::base::{BaseExtractor, Symbol, SymbolKind, SymbolOptions};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

pub(super) const ERLANG_TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &[],
    reference_prefixes: &[],
    generic_open: &[],
};

pub(super) fn record_record_fact(
    base: &mut BaseExtractor,
    symbol_id: &str,
    record_name: &str,
    is_inferred: bool,
) {
    base.record_declared_type_fact_with_declared(
        symbol_id,
        record_name,
        record_name,
        &ERLANG_TYPE_NAME_RULES,
        is_inferred,
    );
}

fn same_file_record_names(base: &BaseExtractor, declarations: &[Node]) -> HashSet<String> {
    declarations
        .iter()
        .filter(|declaration| declaration.kind() == "record_decl")
        .filter_map(|declaration| first_atom_text(base, declaration))
        .collect()
}

pub(super) fn record_expr_name(base: &BaseExtractor, node: &Node) -> Option<String> {
    if node.kind() != "record_expr" {
        return None;
    }
    let name_node = node.child_by_field_name("name")?;
    first_atom_text(base, &name_node)
}

/// What body-match inference resolves against, built once per file.
pub(super) struct InitializerScope {
    records: HashSet<String>,
    returns: HashMap<NameArity, SpecReturn>,
    module_name: Option<String>,
}

impl InitializerScope {
    /// `defined` holds the name/arity of every function the file defines; a
    /// `-spec` with no same-file definition is ignored.
    pub(super) fn build(
        base: &BaseExtractor,
        declarations: &[Node],
        declared: &DeclaredTypes,
        defined: &HashMap<NameArity, usize>,
    ) -> Self {
        Self {
            records: same_file_record_names(base, declarations),
            returns: declared
                .spec_returns()
                .filter(|(identity, _)| defined.contains_key(*identity))
                .map(|(identity, returned)| (identity.clone(), returned.clone()))
                .collect(),
            module_name: declarations
                .iter()
                .find(|declaration| declaration.kind() == "module_attribute")
                .and_then(|declaration| first_atom_text(base, declaration)),
        }
    }

    fn value_type(
        &self,
        extractor: &ErlangExtractor,
        value: Node,
        depth: u32,
    ) -> Option<DeclaredType> {
        if !should_visit_tree_depth(depth) {
            return None;
        }
        match value.kind() {
            "record_expr" | "record_update_expr" => value
                .child_by_field_name("name")
                .and_then(|name| first_atom_text(&extractor.base, &name))
                .filter(|name| self.records.contains(name))
                .map(|name| DeclaredType {
                    declared: name.clone(),
                    name,
                }),
            "call" | "remote" => self.call_return(extractor, value)?.value.clone(),
            "match_expr" => self.value_type(
                extractor,
                value.child_by_field_name("rhs")?,
                child_tree_depth(depth)?,
            ),
            _ => None,
        }
    }

    /// The `T` that `{ok, X} = value` binds `X` to.
    fn ok_payload_type(
        &self,
        extractor: &ErlangExtractor,
        value: Node,
        depth: u32,
    ) -> Option<DeclaredType> {
        if !should_visit_tree_depth(depth) {
            return None;
        }
        match value.kind() {
            "match_expr" => self.ok_payload_type(
                extractor,
                value.child_by_field_name("rhs")?,
                child_tree_depth(depth)?,
            ),
            _ => self.call_return(extractor, value)?.ok_payload.clone(),
        }
    }

    fn call_return(&self, extractor: &ErlangExtractor, value: Node) -> Option<&SpecReturn> {
        match value.kind() {
            "call" => self.local_call_return(extractor, value),
            "remote" => self.remote_call_return(extractor, value),
            _ => None,
        }
    }

    fn local_call_return(&self, extractor: &ErlangExtractor, call: Node) -> Option<&SpecReturn> {
        let callee = call
            .child_by_field_name("expr")
            .filter(|callee| callee.kind() == "atom")?;
        let identity = (
            unquote_atom(&extractor.base.get_node_text(&callee)),
            arg_count(&call.child_by_field_name("args")?),
        );
        self.returns.get(&identity)
    }

    fn remote_call_return(&self, extractor: &ErlangExtractor, remote: Node) -> Option<&SpecReturn> {
        let own_module = self.module_name.as_deref()?;
        let module = remote
            .child_by_field_name("module")?
            .child_by_field_name("module")?;
        let names_own_module = match module.kind() {
            "atom" => unquote_atom(&extractor.base.get_node_text(&module)) == own_module,
            "macro_call_expr" => super::relationships::is_module_macro(extractor, &module),
            _ => false,
        };
        let call = remote
            .child_by_field_name("fun")
            .filter(|call| call.kind() == "call")?;
        names_own_module
            .then(|| self.local_call_return(extractor, call))
            .flatten()
    }
}

/// One variable bound by `Var = Value` or `{ok, Var} = Value` matches in a
/// function body: its first binding site and the type every match agrees on.
struct Binding<'tree> {
    var: Node<'tree>,
    value_type: Option<DeclaredType>,
}

pub(super) fn extract_body_locals(
    extractor: &mut ErlangExtractor,
    clauses: &[Node],
    callable_id: &str,
    scope: &InitializerScope,
    seen: &mut HashSet<String>,
) -> Vec<Symbol> {
    let mut bindings = Vec::new();
    let mut by_name = HashMap::new();
    for declaration in clauses {
        let Some(clause) = super::helpers::find_child_by_type(declaration, "function_clause")
        else {
            continue;
        };
        let Some(body) = clause.child_by_field_name("body") else {
            continue;
        };
        walk_body(extractor, body, scope, &mut bindings, &mut by_name, 0);
    }
    bindings
        .into_iter()
        .filter_map(|binding| emit_local(extractor, binding, callable_id, seen))
        .collect()
}

fn walk_body<'tree>(
    extractor: &ErlangExtractor,
    node: Node<'tree>,
    scope: &InitializerScope,
    bindings: &mut Vec<Binding<'tree>>,
    by_name: &mut HashMap<String, usize>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    if node.kind() == "match_expr"
        && let Some((var, value_type)) = match_binding(extractor, node, scope, depth)
    {
        let name = extractor.base.get_node_text(&var);
        match by_name.get(&name) {
            Some(&index) => {
                let binding: &mut Binding = &mut bindings[index];
                if binding.value_type != value_type {
                    binding.value_type = None;
                }
            }
            None => {
                by_name.insert(name, bindings.len());
                bindings.push(Binding { var, value_type });
            }
        }
    }

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    for child in named_children(&node) {
        walk_body(extractor, child, scope, bindings, by_name, child_depth);
    }
}

fn match_binding<'tree>(
    extractor: &ErlangExtractor,
    node: Node<'tree>,
    scope: &InitializerScope,
    depth: u32,
) -> Option<(Node<'tree>, Option<DeclaredType>)> {
    let lhs = node.child_by_field_name("lhs")?;
    let rhs = node.child_by_field_name("rhs")?;

    if lhs.kind() == "var" {
        return Some((lhs, scope.value_type(extractor, rhs, depth)));
    }
    if lhs.kind() == "tuple" {
        let mut cursor = lhs.walk();
        let elements: Vec<Node> = lhs.children_by_field_name("expr", &mut cursor).collect();
        return match elements.as_slice() {
            [tag, var] if is_ok_atom(&extractor.base, tag) && var.kind() == "var" => {
                Some((*var, scope.ok_payload_type(extractor, rhs, depth)))
            }
            _ => None,
        };
    }

    let record = record_expr_name(&extractor.base, &lhs)?;
    (rhs.kind() == "var").then(|| {
        let value_type = scope.records.contains(&record).then(|| DeclaredType {
            declared: record.clone(),
            name: record,
        });
        (rhs, value_type)
    })
}

fn emit_local(
    extractor: &mut ErlangExtractor,
    binding: Binding,
    callable_id: &str,
    seen: &mut HashSet<String>,
) -> Option<Symbol> {
    let name = extractor.base.get_node_text(&binding.var);
    if name.is_empty() || name == "_" || !seen.insert(name.clone()) {
        return None;
    }
    let signature = name.clone();
    let symbol = extractor.base.create_symbol(
        &binding.var,
        name,
        SymbolKind::Variable,
        SymbolOptions {
            signature: Some(signature),
            parent_id: Some(callable_id.to_string()),
            ..Default::default()
        },
    );
    if let Some(value_type) = binding.value_type {
        extractor.base.record_declared_type_fact_with_declared(
            &symbol.id,
            &value_type.name,
            &value_type.declared,
            &ERLANG_TYPE_NAME_RULES,
            true,
        );
    }
    Some(symbol)
}
