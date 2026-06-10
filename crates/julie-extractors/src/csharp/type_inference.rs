// C# Type Inference

use crate::base::Symbol;
use std::collections::HashMap;

/// Infer types for all symbols
pub fn infer_types(symbols: &[Symbol]) -> HashMap<String, String> {
    let mut type_map = HashMap::new();

    for symbol in symbols {
        let inferred_type = match symbol.kind {
            crate::base::SymbolKind::Method | crate::base::SymbolKind::Function => {
                infer_method_return_type(symbol)
            }
            crate::base::SymbolKind::Property => infer_property_type(symbol),
            crate::base::SymbolKind::Field | crate::base::SymbolKind::Constant => {
                infer_field_type(symbol)
            }
            crate::base::SymbolKind::Variable => infer_variable_type(symbol),
            _ => None,
        };

        if let Some(inferred_type) = inferred_type {
            type_map.insert(symbol.id.clone(), inferred_type);
        }
    }

    type_map
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

fn infer_variable_type(_symbol: &Symbol) -> Option<String> {
    None
}
