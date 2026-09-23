use crate::base::types::TypeNameRules;
use crate::base::{BaseExtractor, Symbol, TypeInfo};
use std::collections::HashMap;
use tree_sitter::Node;

pub(crate) const CSHARP_TYPE_NAME_RULES: TypeNameRules = TypeNameRules {
    nullable_suffixes: &["?"],
    reference_prefixes: &["ref", "out", "in", "scoped"],
    generic_open: &['<'],
};

/// Record a syntactically stated type for a symbol (`is_inferred=false`).
pub(crate) fn record_declared_type(base: &mut BaseExtractor, symbol_id: &str, type_node: Node) {
    record_type_node(base, symbol_id, type_node, false);
}

/// Record the constructed type of a `var x = new Foo(...)` initializer
/// (`is_inferred=true`). Target-typed `new()` carries no type node and
/// records nothing.
pub(crate) fn record_new_expression_type(
    base: &mut BaseExtractor,
    symbol_id: &str,
    initializer: Node,
) {
    if initializer.kind() != "object_creation_expression" {
        return;
    }
    let Some(type_node) = initializer.child_by_field_name("type") else {
        return;
    };
    record_type_node(base, symbol_id, type_node, true);
}

/// Record a callable's declared return type (`is_inferred=false`). `void`
/// is not a type fact and records nothing.
pub(crate) fn record_return_type(base: &mut BaseExtractor, symbol_id: &str, type_node: Node) {
    if base.get_node_text(&type_node).trim() == "void" {
        return;
    }
    record_type_node(base, symbol_id, type_node, false);
}

fn record_type_node(base: &mut BaseExtractor, symbol_id: &str, type_node: Node, is_inferred: bool) {
    if !names_single_base_type(type_node) {
        return;
    }
    let declared = base.get_node_text(&type_node);
    base.record_declared_type_fact(symbol_id, &declared, &CSHARP_TYPE_NAME_RULES, is_inferred);
}

/// True for type nodes whose text reduces to one base type name. Tuple,
/// pointer, function-pointer, and implicit (`var`) types do not, so they
/// record nothing.
fn names_single_base_type(node: Node) -> bool {
    match node.kind() {
        "predefined_type"
        | "identifier"
        | "generic_name"
        | "qualified_name"
        | "alias_qualified_name" => true,
        "nullable_type" | "ref_type" | "scoped_type" | "array_type" => node
            .child_by_field_name("type")
            .is_some_and(names_single_base_type),
        _ => false,
    }
}

/// Types beyond the recorded declared facts: `var x = other;` copies the
/// recorded type of a same-callable `other`. Declared types come only from
/// `record_*` calls on the syntax tree, never from signature text, so tuple,
/// pointer, and `void` positions stay untyped.
pub fn infer_types(
    symbols: &[Symbol],
    recorded: &HashMap<String, TypeInfo>,
) -> HashMap<String, String> {
    let mut type_map: HashMap<String, String> = recorded
        .iter()
        .map(|(id, info)| (id.clone(), info.resolved_type.clone()))
        .collect();

    let mut by_name: HashMap<&str, Vec<&Symbol>> = HashMap::new();
    for symbol in symbols {
        by_name
            .entry(symbol.name.as_str())
            .or_default()
            .push(symbol);
    }

    for symbol in symbols {
        if symbol.kind != crate::base::SymbolKind::Variable || type_map.contains_key(&symbol.id) {
            continue;
        }
        let Some(init) = symbol
            .metadata
            .as_ref()
            .and_then(|m| m.get("initializer"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|init| is_simple_identifier(init))
        else {
            continue;
        };
        let parent = symbol.parent_id.as_deref();
        let resolved = by_name.get(init).and_then(|candidates| {
            candidates
                .iter()
                .filter(|candidate| candidate.id != symbol.id)
                .filter(|candidate| parent.is_none() || candidate.parent_id.as_deref() == parent)
                .find_map(|candidate| type_map.get(&candidate.id).cloned())
        });
        if let Some(resolved) = resolved {
            type_map.insert(symbol.id.clone(), resolved);
        }
    }

    type_map
}

fn is_simple_identifier(text: &str) -> bool {
    let mut chars = text.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}
