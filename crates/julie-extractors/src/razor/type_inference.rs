/// Type inference for Razor symbols
use crate::base::Symbol;
use std::collections::HashMap;

impl super::RazorExtractor {
    /// Types stated in symbol metadata. Symbols without a stated type, and
    /// `var`/`void`, get no entry; declared type facts come from `base`.
    pub fn infer_types(&self, symbols: &[Symbol]) -> HashMap<String, String> {
        symbols
            .iter()
            .filter_map(|symbol| {
                let metadata = symbol.metadata.as_ref()?;
                let stated = ["propertyType", "fieldType", "variableType", "returnType"]
                    .iter()
                    .find_map(|key| metadata.get(*key).and_then(|value| value.as_str()))?;
                (!matches!(stated, "" | "var" | "void"))
                    .then(|| (symbol.id.clone(), stated.to_string()))
            })
            .collect()
    }
}
