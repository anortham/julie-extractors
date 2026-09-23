use crate::base::Symbol;
use std::collections::HashMap;

pub fn infer_types(symbols: &[Symbol]) -> HashMap<String, String> {
    let mut type_map = HashMap::new();

    for symbol in symbols {
        let inferred_type = match symbol.kind {
            crate::base::SymbolKind::Property => infer_property_type(symbol),
            crate::base::SymbolKind::Field | crate::base::SymbolKind::Constant => {
                infer_field_type(symbol)
            }
            _ => None,
        };

        if let Some(inferred_type) = inferred_type {
            type_map.insert(symbol.id.clone(), inferred_type);
        }
    }

    type_map
}

fn infer_property_type(symbol: &Symbol) -> Option<String> {
    let signature = symbol.signature.as_ref()?;
    let as_pos = signature.rfind(" As ")?;
    let type_str = signature[as_pos + 4..].trim();
    if type_str.is_empty() {
        None
    } else {
        Some(type_str.to_string())
    }
}

fn infer_field_type(symbol: &Symbol) -> Option<String> {
    let signature = symbol.signature.as_ref()?;
    let as_pos = signature.rfind(" As ")?;
    let type_str = signature[as_pos + 4..].trim();
    if type_str.is_empty() {
        None
    } else {
        Some(type_str.to_string())
    }
}
