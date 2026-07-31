//! Identifier extraction for Erlang: call sites, fun references, macro usage,
//! and record/field references.
//!
//! Node kinds come from `tree-sitter-erlang` 0.20.0 parse trees. Executable code
//! lives in `fun_decl` clauses and in `pp_define` macro bodies; every other
//! top-level form is a declaration. Type signatures matter here: `-spec`,
//! `-type`, `-opaque`, `-callback`, and record field types spell `integer()`
//! with the very same `call` node a real call uses, so the walk starts from the
//! two executable declaration kinds instead of the whole tree.
//!
//! Kind assignment:
//! - a call — local, remote, imported, or a parameterized macro — is `Call`
//! - a module qualifier (`lists:`, `fun lists:reverse/1`) is `TypeUsage`,
//!   mirroring how the Elixir layer records a remote receiver as a separate row
//! - a fun reference (`fun g/1`) and a bare macro read (`?LIMIT`) name a value
//!   rather than invoking it, so they are `VariableRef` and stay distinguishable
//!   from the `Call` row a real invocation of the same name produces
//! - a record name is `TypeUsage` and a record field is `MemberAccess`

use std::collections::HashMap;

use tree_sitter::{Node, Tree};

use super::ErlangExtractor;
use super::definition_forms;
use super::helpers::{
    NameArity, arg_count, child_named_kinds, find_child_by_type, first_atom_text,
    function_arity_entries, named_children, unquote_atom,
};
use crate::base::{Identifier, IdentifierKind, Symbol};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

/// `(name, arity)` pairs made local by `-import(Module, [...])`, keyed to the
/// module that owns them.
type ImportedFunctions = HashMap<NameArity, String>;

pub(super) fn extract_identifiers(
    extractor: &mut ErlangExtractor,
    tree: &Tree,
    symbols: &[Symbol],
) -> Vec<Identifier> {
    let root = tree.root_node();
    let declarations = named_children(&root);
    let imports = imported_functions(extractor, &declarations);
    let symbol_map: HashMap<String, &Symbol> = symbols
        .iter()
        .map(|symbol| (symbol.id.clone(), symbol))
        .collect();
    let mut clause_scopes: HashMap<NameArity, Option<String>> = HashMap::new();

    for declaration in &declarations {
        match declaration.kind() {
            "fun_decl" => {
                let scope = function_scope(extractor, declaration, &symbol_map, &mut clause_scopes);
                walk(extractor, *declaration, scope.as_deref(), &imports, 0);
            }
            "pp_define" => {
                let scope = containing_symbol_id(extractor, declaration, &symbol_map);
                for child in named_children(declaration) {
                    if child.kind() == "macro_lhs" {
                        continue;
                    }
                    walk(extractor, child, scope.as_deref(), &imports, 0);
                }
            }
            _ => {}
        }
    }

    extractor.base.identifiers.clone()
}

/// A multi-clause function is a run of sibling `fun_decl` nodes but a single
/// symbol spanning only the first clause, so later clauses reuse the scope
/// resolved for the first clause of the same name/arity.
fn function_scope(
    extractor: &ErlangExtractor,
    declaration: &Node,
    symbol_map: &HashMap<String, &Symbol>,
    clause_scopes: &mut HashMap<NameArity, Option<String>>,
) -> Option<String> {
    let Some(clause) = definition_forms::function_clause(extractor, declaration) else {
        return containing_symbol_id(extractor, declaration, symbol_map);
    };
    clause_scopes
        .entry(clause.identity)
        .or_insert_with(|| containing_symbol_id(extractor, declaration, symbol_map))
        .clone()
}

fn containing_symbol_id(
    extractor: &ErlangExtractor,
    node: &Node,
    symbol_map: &HashMap<String, &Symbol>,
) -> Option<String> {
    extractor
        .base
        .find_containing_symbol_from_map(node, symbol_map)
        .map(|symbol| symbol.id.clone())
}

fn walk(
    extractor: &mut ErlangExtractor,
    node: Node,
    scope: Option<&str>,
    imports: &ImportedFunctions,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    emit_identifiers(extractor, node, scope, imports);

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    for child in named_children(&node) {
        walk(extractor, child, scope, imports, child_depth);
    }
}

fn emit_identifiers(
    extractor: &mut ErlangExtractor,
    node: Node,
    scope: Option<&str>,
    imports: &ImportedFunctions,
) {
    match node.kind() {
        "call" => emit_call(extractor, node, scope, imports),
        "remote" => {
            emit_module_qualifier(extractor, find_child_by_type(&node, "remote_module"), scope)
        }
        "internal_fun" => emit_fun_reference(extractor, node, scope),
        "external_fun" => {
            emit_module_qualifier(extractor, find_child_by_type(&node, "module"), scope);
            emit_fun_reference(extractor, node, scope);
        }
        "macro_call_expr" => emit_macro_usage(extractor, node, scope),
        "record_expr" | "record_update_expr" | "record_index_expr" | "record_field_expr" => {
            emit_record_reference(extractor, node, scope)
        }
        _ => {}
    }
}

/// A `call` whose callee is not an `atom` is a dynamic call through a variable
/// (`Fun(X)`) and names no symbol. An auto-imported BIF (`length/1`, `self/0`)
/// is left as a bare call: attributing it to `erlang` would invent a module
/// reference the source never wrote and no workspace symbol can resolve.
fn emit_call(
    extractor: &mut ErlangExtractor,
    node: Node,
    scope: Option<&str>,
    imports: &ImportedFunctions,
) {
    let Some(atom) = find_child_by_type(&node, "atom") else {
        return;
    };
    let name = unquote_atom(&extractor.base.get_node_text(&atom));
    extractor.base.create_identifier(
        &atom,
        name.clone(),
        IdentifierKind::Call,
        scope.map(String::from),
    );

    if node.parent().map(|parent| parent.kind()) == Some("remote") {
        return;
    }
    let arity = find_child_by_type(&node, "expr_args")
        .map(|args| arg_count(&args))
        .unwrap_or(0);
    if let Some(module) = imports.get(&(name, arity)).cloned() {
        extractor.base.create_identifier(
            &atom,
            module,
            IdentifierKind::TypeUsage,
            scope.map(String::from),
        );
    }
}

/// Records the module of `lists:reverse(X)`, `fun lists:reverse/1`, and an
/// `-import`-ed call as its own row, anchored on the module atom where the
/// source spells one and on the callee atom where `-import` leaves it implicit.
fn emit_module_qualifier(
    extractor: &mut ErlangExtractor,
    wrapper: Option<Node>,
    scope: Option<&str>,
) {
    let Some(wrapper) = wrapper else {
        return;
    };
    let Some(atom) = find_child_by_type(&wrapper, "atom") else {
        return;
    };
    let name = unquote_atom(&extractor.base.get_node_text(&atom));
    extractor.base.create_identifier(
        &atom,
        name,
        IdentifierKind::TypeUsage,
        scope.map(String::from),
    );
}

fn emit_fun_reference(extractor: &mut ErlangExtractor, node: Node, scope: Option<&str>) {
    let Some(atom) = find_child_by_type(&node, "atom") else {
        return;
    };
    let name = unquote_atom(&extractor.base.get_node_text(&atom));
    extractor.base.create_identifier(
        &atom,
        name,
        IdentifierKind::VariableRef,
        scope.map(String::from),
    );
}

fn emit_macro_usage(extractor: &mut ErlangExtractor, node: Node, scope: Option<&str>) {
    let Some(var) = find_child_by_type(&node, "var") else {
        return;
    };
    let kind = if find_child_by_type(&node, "macro_call_args").is_some() {
        IdentifierKind::Call
    } else {
        IdentifierKind::VariableRef
    };
    let name = extractor.base.get_node_text(&var);
    extractor
        .base
        .create_identifier(&var, name, kind, scope.map(String::from));
}

fn emit_record_reference(extractor: &mut ErlangExtractor, node: Node, scope: Option<&str>) {
    emit_wrapped_atom(
        extractor,
        find_child_by_type(&node, "record_name"),
        IdentifierKind::TypeUsage,
        scope,
    );
    emit_wrapped_atom(
        extractor,
        find_child_by_type(&node, "record_field_name"),
        IdentifierKind::MemberAccess,
        scope,
    );
    for field in child_named_kinds(&node, "record_field") {
        emit_wrapped_atom(extractor, Some(field), IdentifierKind::MemberAccess, scope);
    }
}

fn emit_wrapped_atom(
    extractor: &mut ErlangExtractor,
    wrapper: Option<Node>,
    kind: IdentifierKind,
    scope: Option<&str>,
) {
    let Some(wrapper) = wrapper else {
        return;
    };
    let Some(atom) = find_child_by_type(&wrapper, "atom") else {
        return;
    };
    let name = unquote_atom(&extractor.base.get_node_text(&atom));
    extractor
        .base
        .create_identifier(&atom, name, kind, scope.map(String::from));
}

fn imported_functions(extractor: &ErlangExtractor, declarations: &[Node]) -> ImportedFunctions {
    let mut imports = ImportedFunctions::new();
    for declaration in declarations {
        if declaration.kind() != "import_attribute" {
            continue;
        }
        let Some(module) = first_atom_text(&extractor.base, declaration) else {
            continue;
        };
        for entry in function_arity_entries(&extractor.base, declaration) {
            imports.insert(entry, module.clone());
        }
    }
    imports
}
