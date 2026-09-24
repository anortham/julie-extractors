use crate::base::types::TypeNameRules;
use crate::base::{BaseExtractor, ContainingSymbolIndex, Symbol, SymbolKind};
use crate::javascript::type_facts::{pattern_binding_shadows, reassigned_names, var_loop_heads};
use std::collections::{HashMap, HashSet};
use tree_sitter::Node;

use super::relationships::{
    LocalCall, component_id_scopes, find_containing_component, object_owner, object_owner_map,
    resolve_scoped_callee,
};

pub(super) const TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &[],
    reference_prefixes: &[],
    generic_open: &['<'],
};

pub(super) fn record_property_type(base: &mut BaseExtractor, symbol_id: &str, property_node: Node) {
    let Some(type_node) = property_node.child_by_field_name("type") else {
        return;
    };
    let declared = base.get_node_text(&type_node);
    if declared == "alias" {
        return;
    }
    let base_text = property_base_name(base, type_node);
    base.record_declared_type_fact_with_declared(
        symbol_id,
        &base_text,
        &declared,
        &TYPE_NAME_RULES,
        false,
    );
}

pub(super) fn record_new_expression_fact(
    base: &mut BaseExtractor,
    symbol_id: &str,
    value_node: Node,
) {
    crate::javascript::type_facts::record_new_expression_fact(
        base,
        symbol_id,
        value_node,
        &TYPE_NAME_RULES,
    );
}

fn property_base_name(base: &BaseExtractor, type_node: Node) -> String {
    if type_node.kind() == "ui_list_property_type"
        && let Some(name_node) = type_node.named_child(0)
    {
        return base.get_node_text(&name_node);
    }
    base.get_node_text(&type_node)
}

/// Record the type an annotation field (`type` on a parameter, `return_type`
/// on a function) states, when it names a single type.
pub(super) fn record_annotation_type(
    base: &mut BaseExtractor,
    symbol_id: &str,
    annotated_node: Node,
    field: &str,
) {
    let Some(annotation) = annotated_node.child_by_field_name(field) else {
        return;
    };
    let mut cursor = annotation.walk();
    let Some(type_node) = annotation.named_children(&mut cursor).last() else {
        return;
    };
    if !matches!(
        type_node.kind(),
        "type_identifier" | "nested_type_identifier" | "predefined_type" | "generic_type"
    ) {
        return;
    }
    let declared = base.get_node_text(&type_node);
    record_named_type(base, symbol_id, &declared);
}

/// Record a type the source names directly: an object's type or the component
/// an `id` names.
pub(super) fn record_named_type(base: &mut BaseExtractor, symbol_id: &str, type_name: &str) {
    base.record_declared_type_fact_with_declared(
        symbol_id,
        type_name,
        type_name,
        &TYPE_NAME_RULES,
        false,
    );
}

/// Record an inferred type fact for each local whose initializer calls a
/// same-file function with a return annotation, named bare in an enclosing
/// object's scope or through an id of the call's own component. An id of an
/// enclosing component is visible from an inline component only under
/// `pragma ComponentBehavior: Bound`, so it records nothing. A callee or
/// receiver name the file assigns anywhere records nothing. Runs after the
/// symbol walk, so a written local type wins.
pub(super) fn record_call_initializer_facts(
    base: &mut BaseExtractor,
    root: Node,
    symbols: &[Symbol],
) {
    let by_id: HashMap<&str, &Symbol> = symbols
        .iter()
        .map(|symbol| (symbol.id.as_str(), symbol))
        .collect();
    let class_symbols = ContainingSymbolIndex::from_iter(
        symbols
            .iter()
            .filter(|symbol| symbol.kind == SymbolKind::Class),
    );
    let object_owners = object_owner_map(symbols);
    let var_loops = var_loop_heads(root);
    let reassigned = reassigned_names(base, root);
    let scoped_names: HashSet<(&str, &str)> = symbols
        .iter()
        .filter_map(|symbol| Some((symbol.parent_id.as_deref()?, symbol.name.as_str())))
        .collect();
    let facts: Vec<(String, String, String)> = symbols
        .iter()
        .filter(|local| local.kind == SymbolKind::Variable)
        .filter_map(|local| {
            let call = root
                .descendant_for_byte_range(local.start_byte as usize, local.end_byte as usize)
                .filter(|node| node.kind() == "variable_declarator")?
                .child_by_field_name("value")
                .filter(|value| value.kind() == "call_expression" && !has_optional_chain(*value))?;
            let caller = by_id.get(local.parent_id.as_deref()?)?;
            let function = call.child_by_field_name("function")?;
            let (function_name, receiver) = match function.kind() {
                "identifier" => (base.get_node_text(&function), None),
                "member_expression" if !has_optional_chain(function) => {
                    let object = function
                        .child_by_field_name("object")
                        .filter(|object| object.kind() == "identifier")?;
                    (
                        base.get_node_text(&function.child_by_field_name("property")?),
                        Some(base.get_node_text(&object)),
                    )
                }
                _ => return None,
            };
            let bound = receiver.as_deref().unwrap_or(&function_name);
            if reassigned.contains(bound)
                || reassigned.contains(&function_name)
                || pattern_binding_shadows(base, &var_loops, bound, call)
                || declared_in_enclosing_functions(bound, caller, &scoped_names, &by_id)
            {
                return None;
            }
            let component = find_containing_component(call, &class_symbols)?;
            if receiver.as_deref().is_some_and(|receiver| {
                component_id_scopes(receiver, symbols, component).is_empty()
            }) {
                return None;
            }
            let callee = resolve_scoped_callee(
                &LocalCall {
                    node: call,
                    function_name: &function_name,
                    receiver: receiver.as_deref(),
                    caller,
                },
                symbols,
                component,
                &object_owners,
            )
            .filter(|callee| {
                receiver.is_some() || scope_object_admits_bare_call(call, callee, &object_owners)
            })?;
            let fact = base.type_info.get(&callee.id).filter(|fact| {
                !matches!(fact.resolved_type.as_str(), "void" | "undefined" | "var")
            })?;
            let declared = fact
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("declared"))
                .and_then(|declared| declared.as_str())
                .unwrap_or(&fact.resolved_type);
            Some((
                local.id.clone(),
                fact.resolved_type.clone(),
                declared.to_string(),
            ))
        })
        .collect();
    for (symbol_id, resolved, declared) in facts {
        base.record_declared_type_fact_with_declared(
            &symbol_id,
            &resolved,
            &declared,
            &TYPE_NAME_RULES,
            true,
        );
    }
}

/// Whether a bare call's scope object (the nearest enclosing object) leaves
/// no member unknown that could shadow `callee`: it declares the callee, or
/// it is its component's root, whose own declarations win. Any other object
/// also carries the members of its type, which may come from another file,
/// from Qt, or from a same-file inline component, so it resolves nothing.
fn scope_object_admits_bare_call(
    call: Node,
    callee: &Symbol,
    object_owners: &HashMap<u32, &Symbol>,
) -> bool {
    let mut current = call.parent();
    while let Some(node) = current {
        if matches!(
            node.kind(),
            "ui_object_definition" | "ui_object_definition_binding"
        ) {
            return object_owner(node, object_owners).is_some_and(|owner| {
                owner.kind == SymbolKind::Class
                    || callee.parent_id.as_deref() == Some(owner.id.as_str())
            });
        }
        current = node.parent();
    }
    false
}

/// Whether the caller or a function enclosing it declares `name` as a
/// parameter, local, or nested function. `scoped_names` holds the
/// `(parent id, name)` of every symbol.
fn declared_in_enclosing_functions(
    name: &str,
    caller: &Symbol,
    scoped_names: &HashSet<(&str, &str)>,
    by_id: &HashMap<&str, &Symbol>,
) -> bool {
    let mut scope = Some(caller);
    while let Some(function) = scope.filter(|scope| scope.kind == SymbolKind::Function) {
        if scoped_names.contains(&(function.id.as_str(), name)) {
            return true;
        }
        scope = function
            .parent_id
            .as_deref()
            .and_then(|parent| by_id.get(parent).copied());
    }
    false
}

fn has_optional_chain(node: Node) -> bool {
    node.children(&mut node.walk())
        .any(|child| matches!(child.kind(), "optional_chain" | "?."))
}
