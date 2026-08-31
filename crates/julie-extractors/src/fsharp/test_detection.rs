use crate::base::{Symbol, SymbolKind, TestRole};
use crate::test_detection::apply_test_role;
use std::collections::HashMap;

pub(super) fn apply_test_roles(symbols: &mut [Symbol]) {
    for symbol in symbols {
        let Some(role) = test_role(symbol) else {
            continue;
        };
        let metadata = symbol.metadata.get_or_insert_with(HashMap::new);
        apply_test_role(metadata, role);
    }
}

fn test_role(symbol: &Symbol) -> Option<TestRole> {
    if !is_callable(&symbol.kind) {
        return None;
    }

    symbol
        .annotations
        .iter()
        .find_map(|annotation| match annotation.annotation_key.as_str() {
            "fact" | "xunit.fact" | "global.xunit.fact" => Some(TestRole::TestCase),
            "theory" | "xunit.theory" | "global.xunit.theory" => Some(TestRole::ParameterizedTest),
            _ => None,
        })
}

fn is_callable(kind: &SymbolKind) -> bool {
    matches!(
        kind,
        SymbolKind::Function | SymbolKind::Method | SymbolKind::Constructor
    )
}
