//! Type inference for C++ symbols
//! Infers return types and variable types from signatures and declarations

use crate::base::{Symbol, SymbolKind};
use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;

// Static regexes compiled once for performance
static FUNCTION_RETURN_TYPE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:(?:virtual|static|inline|friend)\s+)*(.+?)\s+(\w+|operator\w*|~\w+)\s*\(")
        .unwrap()
});
static AUTO_RETURN_TYPE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"auto\s+(\w+)\s*\([^)]*\)\s*->\s*(.+?)(?:\s|$)").unwrap());
static VARIABLE_TYPE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:(?:static|extern|const|constexpr|mutable)\s+)*(.+?)\s+(\w+)(?:\s*=.*)?$")
        .unwrap()
});

/// Infer types from C++ type annotations and declarations
pub(super) fn infer_types(symbols: &[Symbol]) -> HashMap<String, String> {
    let mut type_map = HashMap::new();

    for symbol in symbols {
        if matches!(symbol.kind, SymbolKind::Function | SymbolKind::Method) {
            // Extract return type from function signature
            if let Some(return_type) = infer_function_return_type(symbol) {
                type_map.insert(symbol.id.clone(), return_type);
            }
        } else if matches!(symbol.kind, SymbolKind::Variable | SymbolKind::Field) {
            // Extract variable type from signature
            if let Some(variable_type) = infer_variable_type(symbol) {
                type_map.insert(symbol.id.clone(), variable_type);
            }
        }
    }

    type_map
}

/// Infer function return type from signature
fn infer_function_return_type(symbol: &Symbol) -> Option<String> {
    let signature = symbol.signature.as_ref()?;

    // Remove template parameters if present
    let signature = if let Some(template_match) = signature.find("template<") {
        if let Some(newline_pos) = signature[template_match..].find('\n') {
            let end_idx = template_match + newline_pos + 1;
            // SAFETY: Check char boundary before slicing to prevent UTF-8 panic
            if signature.is_char_boundary(end_idx) {
                &signature[end_idx..]
            } else {
                signature
            }
        } else {
            signature
        }
    } else {
        signature
    };

    // Skip constructors and destructors (no return type)
    if matches!(
        symbol.kind,
        SymbolKind::Constructor | SymbolKind::Destructor
    ) {
        return None;
    }

    // Pattern: "returnType functionName(params)"
    if let Some(captures) = FUNCTION_RETURN_TYPE_RE.captures(signature) {
        let return_type = captures.get(1)?.as_str().trim();
        return Some(return_type.to_string());
    }

    // Pattern: "auto functionName(params) -> returnType"
    if let Some(captures) = AUTO_RETURN_TYPE_RE.captures(signature) {
        return Some(captures.get(2)?.as_str().trim().to_string());
    }

    None
}

/// Infer variable type from signature
fn infer_variable_type(symbol: &Symbol) -> Option<String> {
    let signature = symbol.signature.as_ref()?;

    // Pattern: "storageClass? typeSpec variableName initializer?"
    if let Some(captures) = VARIABLE_TYPE_RE.captures(signature) {
        return Some(captures.get(1)?.as_str().trim().to_string());
    }

    None
}
