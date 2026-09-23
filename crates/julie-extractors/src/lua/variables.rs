/// Variable and assignment extraction
///
/// Handles extraction of:
/// - Local variable declarations: `local x = 5`
/// - Variable assignments: `x = 5`
/// - Assignment statements: `x, y = 1, 2`
/// - Property assignments: `obj.prop = value`
/// - Module property assignments: `M.PI = 3.14159`
use super::core::{self, ValueOwners};
use super::helpers;
use super::parameters;
use super::scope;
use crate::base::{BaseExtractor, Symbol, SymbolKind, SymbolOptions, Visibility};
use std::collections::HashMap;
use tree_sitter::Node;

/// Collect non-comma children from an expression_list node.
pub(super) fn collect_expression_nodes<'a>(expr_list: Node<'a>) -> Vec<Node<'a>> {
    let mut cursor = expr_list.walk();
    expr_list
        .children(&mut cursor)
        .filter(|child| child.kind() != ",")
        .collect()
}

/// Infer SymbolKind and data type from an expression node.
///
/// `is_field` controls whether function definitions become Method (true) or Function (false).
/// Returns (kind, data_type) where kind is the override (if any) and data_type is the
/// inferred type string.
pub(super) fn infer_kind_and_type(
    base: &BaseExtractor,
    expression: Node,
    is_field: bool,
) -> (SymbolKind, String) {
    match expression.kind() {
        "function_definition" | "function" | "function_expression" => {
            let kind = if is_field {
                SymbolKind::Method
            } else {
                SymbolKind::Function
            };
            (kind, "function".to_string())
        }
        "expression_list" if helpers::contains_function_definition(expression) => {
            let kind = if is_field {
                SymbolKind::Method
            } else {
                SymbolKind::Function
            };
            (kind, "function".to_string())
        }
        _ => {
            let data_type = helpers::infer_type_from_expression(base, expression);
            let kind = if data_type == "import" {
                SymbolKind::Import
            } else if is_field {
                SymbolKind::Field
            } else {
                SymbolKind::Variable
            };
            (kind, data_type)
        }
    }
}

/// Create a variable-like symbol for `name_node` bound to `value`.
///
/// A function value spans from the name to the closing `end`, carries the
/// function body, and owns its parameters. Function and table values are
/// registered in `owners` so the traversal parents their contents to this symbol.
#[allow(clippy::too_many_arguments)]
pub(super) fn push_variable_symbol(
    symbols: &mut Vec<Symbol>,
    base: &mut BaseExtractor,
    owners: &mut ValueOwners,
    name_node: &Node,
    name: String,
    kind: SymbolKind,
    data_type: String,
    signature: String,
    parent_id: Option<String>,
    visibility: Visibility,
    doc_comment: Option<String>,
    value: Option<Node>,
) {
    let mut metadata = HashMap::new();
    metadata.insert("dataType".to_string(), data_type.into());
    let require = value
        .filter(|_| kind == SymbolKind::Import)
        .and_then(|value| core::require_import(base, value));
    if let Some(require) = &require {
        metadata.extend(require.metadata());
    }

    let options = SymbolOptions {
        signature: Some(signature),
        parent_id: parent_id.clone(),
        visibility: Some(visibility),
        metadata: Some(metadata),
        annotations: helpers::doc_annotations(doc_comment.as_deref()),
        doc_comment,
    };

    let function = value.and_then(scope::function_value);
    let symbol = match function {
        Some(function) => base.create_symbol_from_span(
            &function,
            scope::span_between(name_node, &function),
            name,
            kind,
            options,
        ),
        None => base.create_symbol(name_node, name, kind, options),
    };
    let symbol_id = symbol.id.clone();
    symbols.push(symbol);

    if let Some(function) = function {
        symbols.extend(parameters::extract_parameter_symbols(
            base, function, &symbol_id,
        ));
        owners.insert(function.id(), symbol_id);
    } else if let Some(value) = value.filter(|value| value.kind() == "table_constructor") {
        owners.insert(value.id(), symbol_id);
    } else if let (Some(require), Some(call)) = (require, value) {
        core::record_require_pending(base, &symbol_id, require, call, parent_id.as_deref());
    }
}

fn variable_list_names(variable_list: Node) -> Vec<Node> {
    let mut cursor = variable_list.walk();
    variable_list
        .children_by_field_name("name", &mut cursor)
        .collect()
}

/// Extract local variable declarations: `local x = 5`, `local x, y = 1, 2`, `local x`
pub(super) fn extract_local_variable_declaration(
    symbols: &mut Vec<Symbol>,
    base: &mut BaseExtractor,
    owners: &mut ValueOwners,
    node: Node,
    parent_id: Option<&str>,
) {
    let assignment_statement = helpers::find_child_by_type(&node, "assignment_statement");
    let Some(variable_list) = assignment_statement
        .and_then(|assignment| helpers::find_child_by_type(&assignment, "variable_list"))
        .or_else(|| helpers::find_child_by_type(&node, "variable_list"))
    else {
        return;
    };
    let expressions: Vec<Node> = assignment_statement
        .and_then(|assignment| helpers::find_child_by_type(&assignment, "expression_list"))
        .map(collect_expression_nodes)
        .unwrap_or_default();

    let signature = base.get_node_text(&node);
    for (i, name_node) in variable_list_names(variable_list).into_iter().enumerate() {
        if name_node.kind() != "identifier" {
            continue;
        }
        let name = base.get_node_text(&name_node);
        let expression = expressions.get(i).copied();
        let (kind, data_type) = expression
            .map(|expr| infer_kind_and_type(base, expr, false))
            .unwrap_or((SymbolKind::Variable, String::new()));
        let doc_comment = helpers::doc_comment(base, &node);

        push_variable_symbol(
            symbols,
            base,
            owners,
            &name_node,
            name,
            kind,
            data_type,
            signature.clone(),
            parent_id.map(|s| s.to_string()),
            Visibility::Private,
            doc_comment,
            expression,
        );
    }
}

/// A global binding (`name = v` with no local in scope) already has a symbol.
fn has_global_binding(symbols: &[Symbol], name: &str) -> bool {
    symbols.iter().any(|symbol| {
        symbol.name == name
            && symbol.visibility == Some(Visibility::Public)
            && matches!(
                symbol.kind,
                SymbolKind::Variable | SymbolKind::Function | SymbolKind::Import
            )
    })
}

/// Extract assignment statements: `x = 5`, `x, y = 1, 2`, `M.a.b = v`
pub(super) fn extract_assignment_statement(
    symbols: &mut Vec<Symbol>,
    base: &mut BaseExtractor,
    owners: &mut ValueOwners,
    node: Node,
    parent_id: Option<&str>,
) {
    let Some(variable_list) = helpers::find_child_by_type(&node, "variable_list") else {
        return;
    };
    let expressions: Vec<Node> = helpers::find_child_by_type(&node, "expression_list")
        .map(collect_expression_nodes)
        .unwrap_or_default();
    let signature = base.get_node_text(&node);

    for (i, target) in variable_list_names(variable_list).into_iter().enumerate() {
        let expression = expressions.get(i).copied();
        let (name_node, name, owner_id, is_field) = match target.kind() {
            "identifier" => {
                let name = base.get_node_text(&target);
                if scope::is_local_binding_in_scope(base, node, &name)
                    || has_global_binding(symbols, &name)
                {
                    continue;
                }
                (target, name, parent_id.map(str::to_string), false)
            }
            "dot_index_expression" => {
                let Some(field) = target.child_by_field_name("field") else {
                    continue;
                };
                let owner_id = target
                    .child_by_field_name("table")
                    .and_then(|table| scope::resolve_table_symbol_id(base, table, symbols));
                (target, base.get_node_text(&field), owner_id, true)
            }
            _ => continue,
        };

        if is_field
            && expression.is_none_or(|expr| scope::function_value(expr).is_none())
            && owner_id.as_deref().is_some_and(|owner_id| {
                symbols.iter().any(|symbol| {
                    symbol.name == name && symbol.parent_id.as_deref() == Some(owner_id)
                })
            })
        {
            continue;
        }

        let (kind, data_type) = expression
            .map(|expr| infer_kind_and_type(base, expr, is_field))
            .unwrap_or_else(|| {
                let kind = if is_field {
                    SymbolKind::Field
                } else {
                    SymbolKind::Variable
                };
                (kind, String::new())
            });
        let doc_comment = helpers::doc_comment(base, &node);

        push_variable_symbol(
            symbols,
            base,
            owners,
            &name_node,
            name,
            kind,
            data_type,
            signature.clone(),
            owner_id,
            Visibility::Public,
            doc_comment,
            expression,
        );
    }
}
