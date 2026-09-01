// C# Type Inference

use crate::base::types::TypeNameRules;
use crate::base::{BaseExtractor, Symbol};
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

/// Infer types for all symbols
pub fn infer_types(symbols: &[Symbol]) -> HashMap<String, String> {
    let mut type_map = HashMap::new();

    // Pass 1: declared types and signature parsing.
    for symbol in symbols {
        let inferred_type = match symbol.kind {
            crate::base::SymbolKind::Method | crate::base::SymbolKind::Function => {
                infer_method_return_type(symbol)
            }
            crate::base::SymbolKind::Property => infer_property_type(symbol),
            crate::base::SymbolKind::Field | crate::base::SymbolKind::Constant => {
                infer_field_type(symbol)
            }
            crate::base::SymbolKind::Variable => declared_or_signature_type(symbol),
            _ => None,
        };

        if let Some(inferred_type) = inferred_type {
            type_map.insert(symbol.id.clone(), inferred_type);
        }
    }

    // Pass 2: `var x = other;` copies a same-scope identifier's known type.
    let by_name: HashMap<&str, Vec<&Symbol>> = {
        let mut map: HashMap<&str, Vec<&Symbol>> = HashMap::new();
        for symbol in symbols {
            map.entry(symbol.name.as_str()).or_default().push(symbol);
        }
        map
    };

    for symbol in symbols {
        if symbol.kind != crate::base::SymbolKind::Variable {
            continue;
        }
        if type_map.contains_key(&symbol.id) {
            continue;
        }
        let Some(init) = symbol
            .metadata
            .as_ref()
            .and_then(|m| m.get("initializer"))
            .and_then(|v| v.as_str())
        else {
            continue;
        };
        let init = init.trim();
        if !is_simple_identifier(init) {
            continue;
        }
        let Some(candidates) = by_name.get(init) else {
            continue;
        };
        // Prefer a sibling under the same parent (same callable).
        let parent = symbol.parent_id.as_deref();
        let mut resolved: Option<String> = None;
        for candidate in candidates {
            if candidate.id == symbol.id {
                continue;
            }
            if parent.is_some() && candidate.parent_id.as_deref() != parent {
                continue;
            }
            if let Some(ty) = type_map.get(&candidate.id) {
                resolved = Some(ty.clone());
                break;
            }
            if let Some(ty) = declared_or_signature_type(candidate) {
                resolved = Some(ty);
                break;
            }
        }
        if let Some(ty) = resolved {
            type_map.insert(symbol.id.clone(), ty);
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

fn declared_or_signature_type(symbol: &Symbol) -> Option<String> {
    if let Some(metadata) = symbol.metadata.as_ref()
        && let Some(declared) = metadata
            .get("variableType")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty() && *s != "var" && *s != "using" && *s != "implicit_type")
    {
        return Some(declared.to_string());
    }
    infer_variable_type_from_signature(symbol)
}

fn infer_method_return_type(symbol: &Symbol) -> Option<String> {
    let signature = symbol.signature.as_ref()?;
    let method_pos = find_method_declaration_start(signature, &symbol.name)?;
    let before = signature[..method_pos].trim_end();

    let parts: Vec<&str> = before.split_whitespace().collect();
    let modifiers = [
        "public",
        "private",
        "protected",
        "internal",
        "static",
        "virtual",
        "override",
        "abstract",
        "async",
        "sealed",
    ];

    for part in parts.iter().rev() {
        if !modifiers.contains(part) && !part.is_empty() {
            return Some(part.to_string());
        }
    }

    None
}

/// Locate the method declaration token in a signature string.
///
/// Requires an exact identifier match followed by `(` so attribute arguments,
/// default parameter strings, and return types like `GetNameHandler` cannot
/// satisfy a search for method `Name`.
fn find_method_declaration_start(signature: &str, method_name: &str) -> Option<usize> {
    let needle = format!("{method_name}(");
    let mut search_from = 0;

    while search_from < signature.len() {
        let rel = signature[search_from..].find(&needle)?;
        let abs = search_from + rel;
        if abs == 0 || !is_ident_char(signature.as_bytes()[abs - 1]) {
            return Some(abs);
        }
        search_from = abs + 1;
    }

    None
}

fn is_ident_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn infer_property_type(symbol: &Symbol) -> Option<String> {
    let signature = symbol.signature.as_ref()?;
    let parts: Vec<&str> = signature.split_whitespace().collect();
    let modifiers = [
        "public",
        "private",
        "protected",
        "internal",
        "static",
        "virtual",
        "override",
        "abstract",
    ];

    for part in &parts {
        if !modifiers.contains(part) && !part.is_empty() {
            return Some(part.to_string());
        }
    }

    None
}

fn infer_field_type(symbol: &Symbol) -> Option<String> {
    let signature = symbol.signature.as_ref()?;
    let parts: Vec<&str> = signature.split_whitespace().collect();
    let modifiers = [
        "public",
        "private",
        "protected",
        "internal",
        "static",
        "readonly",
        "const",
        "volatile",
    ];

    for part in &parts {
        if !modifiers.contains(part) && !part.is_empty() {
            return Some(part.to_string());
        }
    }

    None
}

fn infer_variable_type_from_signature(symbol: &Symbol) -> Option<String> {
    let signature = symbol.signature.as_ref()?;
    // Signatures look like: "int total = 0", "string name", "params string[] args".
    let without_init = signature.split('=').next()?.trim();
    let parts: Vec<&str> = without_init.split_whitespace().collect();
    if parts.len() < 2 {
        return None;
    }
    let name = symbol.name.as_str();
    if parts.last().copied() != Some(name) {
        return None;
    }
    let type_parts: Vec<&str> = parts[..parts.len() - 1]
        .iter()
        .copied()
        .filter(|part| {
            !matches!(
                *part,
                "public"
                    | "private"
                    | "protected"
                    | "internal"
                    | "static"
                    | "readonly"
                    | "const"
                    | "ref"
                    | "out"
                    | "in"
                    | "params"
                    | "this"
                    | "scoped"
            )
        })
        .collect();
    let joined = type_parts.join(" ");
    if joined.is_empty() || joined == "var" {
        None
    } else {
        Some(joined)
    }
}
