use crate::base::{Symbol, SymbolKind};
use std::collections::HashMap;

/// Type rows for Zig symbols whose kind or metadata states the type category.
pub(super) fn infer_types(symbols: &[Symbol]) -> HashMap<String, String> {
    let mut types = HashMap::new();

    for symbol in symbols {
        if metadata_flag(symbol, "isErrorType") {
            types.insert(symbol.id.clone(), "error".to_string());
        }
        if metadata_flag(symbol, "isTypeAlias") {
            types.insert(symbol.id.clone(), "type".to_string());
        }

        match symbol.kind {
            SymbolKind::Struct if !metadata_flag(symbol, "isErrorType") => {
                types.insert(symbol.id.clone(), "struct".to_string());
            }
            SymbolKind::Enum => {
                types.insert(symbol.id.clone(), "enum".to_string());
            }
            _ => {}
        }
    }

    types
}

fn metadata_flag(symbol: &Symbol, key: &str) -> bool {
    symbol
        .metadata
        .as_ref()
        .and_then(|m| m.get(key))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}
