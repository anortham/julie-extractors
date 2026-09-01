/// Type inference for Elixir from @spec annotations.
///
/// Associates @spec return types with their corresponding function symbols.
use crate::base::Symbol;
use std::collections::HashMap;

/// Infer types from collected @spec annotations.
///
/// `specs` maps function name → return type string (collected during attribute extraction).
/// Only a return type that reduces to a single base name is recorded: `integer()` becomes
/// `integer`, `GenServer.on_start()` becomes `GenServer.on_start`, and tuples, lists, unions,
/// maps, and type variables record nothing.
pub(super) fn infer_types(
    specs: &HashMap<String, String>,
    symbols: &[Symbol],
) -> HashMap<String, String> {
    let mut type_map = HashMap::new();

    for symbol in symbols {
        if let Some(base) = specs
            .get(&symbol.name)
            .and_then(|return_type| spec_base_type_name(return_type))
        {
            type_map.insert(symbol.id.clone(), base);
        }
    }

    type_map
}

fn spec_base_type_name(return_type: &str) -> Option<String> {
    let text = return_type.trim();
    let name = text.strip_suffix("()").unwrap_or(text);
    let is_dotted_name = !name.is_empty()
        && name.split('.').all(|segment| {
            segment
                .chars()
                .next()
                .is_some_and(|c| c.is_alphabetic() || c == '_')
                && segment.chars().all(|c| c.is_alphanumeric() || c == '_')
        });
    is_dotted_name.then(|| name.to_string())
}
