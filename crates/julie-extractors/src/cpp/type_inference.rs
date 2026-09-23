//! Type inference for C++ variables and fields from their signatures.

use crate::base::{Symbol, SymbolKind};
use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;

static VARIABLE_TYPE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:(?:static|extern|const|constexpr|mutable)\s+)*(.+?)\s+(\w+)(?:\s*=.*)?$")
        .unwrap()
});

/// Infer variable and field types from their signatures; return types are
/// recorded from the declaration during symbol extraction.
pub(super) fn infer_types(symbols: &[Symbol]) -> HashMap<String, String> {
    symbols
        .iter()
        .filter(|symbol| matches!(symbol.kind, SymbolKind::Variable | SymbolKind::Field))
        .filter_map(|symbol| Some((symbol.id.clone(), infer_variable_type(symbol)?)))
        .collect()
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
