//! Relationship extraction for Erlang.
//!
//! Erlang's edges split cleanly by whether the target can live in the same
//! file. A `.erl` file declares exactly one module, so only an unqualified call
//! to a function defined here is resolvable in-file; every other edge names
//! another module or another file and is emitted as a structured pending row
//! carrying the module/path context a resolver needs.
//!
//! | Source form | Edge |
//! | --- | --- |
//! | `helper(X)` with `helper/1` defined here | resolved `Calls` |
//! | `ledger:record(X)` | pending `Calls`, namespace `["ledger"]` |
//! | `reverse(X)` under `-import(lists, [reverse/1])` | pending `Calls`, namespace `["lists"]`, import context `import` |
//! | `-behaviour(gen_server)` | pending `Implements` from the module symbol |
//! | `-include("x.hrl")` / `-include_lib("app/include/x.hrl")` | pending `Imports` from the module symbol |
//! | `-import(lists, [...])` | pending `Imports` from the module symbol |
//!
//! An unqualified call that resolves to neither a same-file function nor an
//! `-import` is an auto-imported BIF (`length/1`, `self/0`) or a function from
//! an included header. Emitting a pending edge for it would ask a resolver to
//! bind `length` to whatever workspace function happens to share the name, so
//! it emits nothing. Type signatures spell type names with the same `call` node
//! a real call uses, so the walk starts from the two executable declaration
//! kinds — `fun_decl` and `pp_define` — exactly as the identifier layer does.

use std::collections::HashMap;

use tree_sitter::Node;

use super::ErlangExtractor;
use super::definition_forms;
use super::helpers::{
    NameArity, arg_count, find_child_by_type, first_atom_text, named_children, unquote_atom,
};
use super::identifiers::{ImportedFunctions, imported_functions};
use crate::base::{Relationship, RelationshipKind, Symbol, SymbolKind, UnresolvedTarget};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

const LOCAL_CALL_CONFIDENCE: f32 = 0.9;
const REMOTE_CALL_CONFIDENCE: f32 = 0.7;
const ATTRIBUTE_TARGET_CONFIDENCE: f32 = 0.9;

/// Function symbols keyed by Erlang's `(name, arity)` identity, so a call to
/// `helper/2` never binds to `helper/1`.
type FunctionIndex<'a> = HashMap<NameArity, &'a Symbol>;

pub(super) fn extract_relationships(
    extractor: &mut ErlangExtractor,
    declarations: &[Node],
    symbols: &[Symbol],
) -> Vec<Relationship> {
    let imports = imported_functions(extractor, declarations);
    let functions = function_index(symbols);
    let containing_symbols = extractor.base.containing_symbol_index(symbols);
    let module_id = symbols
        .iter()
        .find(|symbol| symbol.kind == SymbolKind::Module)
        .map(|symbol| symbol.id.clone());

    let mut relationships = Vec::new();
    for declaration in declarations {
        match declaration.kind() {
            "behaviour_attribute" => {
                emit_behaviour(extractor, declaration, module_id.as_deref());
            }
            "pp_include" => emit_include(extractor, declaration, module_id.as_deref(), "include"),
            "pp_include_lib" => {
                emit_include(extractor, declaration, module_id.as_deref(), "include_lib")
            }
            "import_attribute" => emit_import(extractor, declaration, module_id.as_deref()),
            "fun_decl" => {
                let scope = clause_scope(extractor, declaration, &functions);
                walk_calls(
                    extractor,
                    *declaration,
                    scope.as_deref(),
                    &functions,
                    &imports,
                    &mut relationships,
                    0,
                );
            }
            "pp_define" => {
                let scope = containing_symbols
                    .find(*declaration)
                    .map(|symbol| symbol.id.clone());
                for child in named_children(declaration) {
                    if child.kind() == "macro_lhs" {
                        continue;
                    }
                    walk_calls(
                        extractor,
                        child,
                        scope.as_deref(),
                        &functions,
                        &imports,
                        &mut relationships,
                        0,
                    );
                }
            }
            _ => {}
        }
    }

    relationships
}

fn function_index(symbols: &[Symbol]) -> FunctionIndex<'_> {
    symbols
        .iter()
        .filter(|symbol| symbol.kind == SymbolKind::Function)
        .filter_map(|symbol| Some(((symbol.name.clone(), symbol_arity(symbol)?), symbol)))
        .collect()
}

fn symbol_arity(symbol: &Symbol) -> Option<u32> {
    symbol
        .metadata
        .as_ref()?
        .get("arity")?
        .as_u64()
        .map(|arity| arity as u32)
}

/// A multi-clause function is a run of sibling `fun_decl` nodes but a single
/// symbol, so every clause resolves its scope through the shared name/arity
/// identity rather than through span containment.
fn clause_scope(
    extractor: &ErlangExtractor,
    declaration: &Node,
    functions: &FunctionIndex,
) -> Option<String> {
    let clause = definition_forms::function_clause(extractor, declaration)?;
    functions
        .get(&clause.identity)
        .map(|symbol| symbol.id.clone())
}

fn walk_calls(
    extractor: &mut ErlangExtractor,
    node: Node,
    scope: Option<&str>,
    functions: &FunctionIndex,
    imports: &ImportedFunctions,
    relationships: &mut Vec<Relationship>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }

    match node.kind() {
        "remote" => emit_remote_call(extractor, node, scope),
        "call" if node.parent().map(|parent| parent.kind()) != Some("remote") => {
            emit_local_call(extractor, node, scope, functions, imports, relationships)
        }
        _ => {}
    }

    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    for child in named_children(&node) {
        walk_calls(
            extractor,
            child,
            scope,
            functions,
            imports,
            relationships,
            child_depth,
        );
    }
}

/// `?MODULE:helper(X)` spells its module as a macro rather than an atom, so no
/// module name is available and the call emits nothing.
fn emit_remote_call(extractor: &mut ErlangExtractor, node: Node, scope: Option<&str>) {
    let Some(scope) = scope else {
        return;
    };
    let Some(module) = find_child_by_type(&node, "remote_module")
        .and_then(|wrapper| first_atom_text(&extractor.base, &wrapper))
    else {
        return;
    };
    let Some(callee) =
        find_child_by_type(&node, "call").and_then(|call| find_child_by_type(&call, "atom"))
    else {
        return;
    };
    let name = unquote_atom(&extractor.base.get_node_text(&callee));

    let target = UnresolvedTarget {
        display_name: format!("{module}:{name}"),
        terminal_name: name,
        receiver: None,
        namespace_path: vec![module],
        import_context: None,
    };
    let pending = extractor.base.create_pending_relationship_at_target(
        scope.to_string(),
        target,
        RelationshipKind::Calls,
        &callee,
        Some(scope.to_string()),
        Some(REMOTE_CALL_CONFIDENCE),
    );
    extractor.base.add_structured_pending_relationship(pending);
}

fn emit_local_call(
    extractor: &mut ErlangExtractor,
    node: Node,
    scope: Option<&str>,
    functions: &FunctionIndex,
    imports: &ImportedFunctions,
    relationships: &mut Vec<Relationship>,
) {
    let Some(scope) = scope else {
        return;
    };
    let Some(callee) = find_child_by_type(&node, "atom") else {
        return;
    };
    let name = unquote_atom(&extractor.base.get_node_text(&callee));
    let arity = find_child_by_type(&node, "expr_args")
        .map(|args| arg_count(&args))
        .unwrap_or(0);
    let identity = (name.clone(), arity);

    if let Some(target) = functions.get(&identity) {
        relationships.push(extractor.base.create_relationship_at_target(
            scope.to_string(),
            target.id.clone(),
            RelationshipKind::Calls,
            &callee,
            Some(LOCAL_CALL_CONFIDENCE),
            None,
        ));
        return;
    }

    let Some(module) = imports.get(&identity).cloned() else {
        return;
    };
    let target = UnresolvedTarget {
        display_name: name.clone(),
        terminal_name: name,
        receiver: None,
        namespace_path: vec![module],
        import_context: Some("import".to_string()),
    };
    let pending = extractor.base.create_pending_relationship_at_target(
        scope.to_string(),
        target,
        RelationshipKind::Calls,
        &callee,
        Some(scope.to_string()),
        Some(REMOTE_CALL_CONFIDENCE),
    );
    extractor.base.add_structured_pending_relationship(pending);
}

/// A `.erl` file declares one module, so a `-behaviour` target is always in
/// another file: matching the behaviour name against a same-file function or
/// type symbol would invent an edge the source never declared.
fn emit_behaviour(extractor: &mut ErlangExtractor, node: &Node, module_id: Option<&str>) {
    let Some(module_id) = module_id else {
        return;
    };
    let Some(atom) = find_child_by_type(node, "atom") else {
        return;
    };
    let name = unquote_atom(&extractor.base.get_node_text(&atom));

    let pending = extractor.base.create_pending_relationship_at_target(
        module_id.to_string(),
        UnresolvedTarget::simple(name),
        RelationshipKind::Implements,
        &atom,
        Some(module_id.to_string()),
        Some(ATTRIBUTE_TARGET_CONFIDENCE),
    );
    extractor.base.add_structured_pending_relationship(pending);
}

/// `-include_lib` resolves through an application's lib directory while
/// `-include` resolves against the include path, so the attribute name is
/// recorded as the target's import context.
fn emit_include(
    extractor: &mut ErlangExtractor,
    node: &Node,
    module_id: Option<&str>,
    attribute: &str,
) {
    let Some(module_id) = module_id else {
        return;
    };
    let Some(string) = find_child_by_type(node, "string") else {
        return;
    };
    let path = unquote_string(&extractor.base.get_node_text(&string));
    if path.is_empty() {
        return;
    }

    let mut segments: Vec<String> = path
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(str::to_string)
        .collect();
    let Some(terminal_name) = segments.pop() else {
        return;
    };

    let target = UnresolvedTarget {
        display_name: path,
        terminal_name,
        receiver: None,
        namespace_path: segments,
        import_context: Some(attribute.to_string()),
    };
    let pending = extractor.base.create_pending_relationship_at_target(
        module_id.to_string(),
        target,
        RelationshipKind::Imports,
        &string,
        Some(module_id.to_string()),
        Some(ATTRIBUTE_TARGET_CONFIDENCE),
    );
    extractor.base.add_structured_pending_relationship(pending);
}

fn emit_import(extractor: &mut ErlangExtractor, node: &Node, module_id: Option<&str>) {
    let Some(module_id) = module_id else {
        return;
    };
    let Some(atom) = find_child_by_type(node, "atom") else {
        return;
    };
    let name = unquote_atom(&extractor.base.get_node_text(&atom));

    let target = UnresolvedTarget {
        display_name: name.clone(),
        terminal_name: name,
        receiver: None,
        namespace_path: Vec::new(),
        import_context: Some("import".to_string()),
    };
    let pending = extractor.base.create_pending_relationship_at_target(
        module_id.to_string(),
        target,
        RelationshipKind::Imports,
        &atom,
        Some(module_id.to_string()),
        Some(ATTRIBUTE_TARGET_CONFIDENCE),
    );
    extractor.base.add_structured_pending_relationship(pending);
}

fn unquote_string(text: &str) -> String {
    text.trim().trim_matches('"').to_string()
}
