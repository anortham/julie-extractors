use crate::base::{Symbol, SymbolKind, TestRole};
use crate::test_detection::{
    apply_callable_test_metadata, apply_test_role, mark_dotnet_test_containers,
};
use std::collections::HashMap;

/// Applies the .NET test roles (xUnit, NUnit, MSTest) to F# callables, marks
/// Expecto `[<Tests>]` values as tests, and classifies .NET test containers.
pub(super) fn apply_test_roles(symbols: &mut [Symbol]) {
    for symbol in symbols.iter_mut() {
        let keys: Vec<String> = symbol
            .annotations
            .iter()
            .map(|annotation| {
                annotation
                    .annotation_key
                    .rsplit('.')
                    .next()
                    .unwrap_or_default()
                    .to_string()
            })
            .collect();
        let metadata = symbol.metadata.get_or_insert_with(HashMap::new);
        if symbol.kind == SymbolKind::Variable {
            if keys.iter().any(|key| key == "tests") {
                apply_test_role(metadata, TestRole::TestCase);
            }
            continue;
        }
        apply_callable_test_metadata(
            "fsharp",
            &symbol.name,
            &symbol.file_path,
            &symbol.kind,
            &keys,
            None,
            metadata,
        );
        if keys.iter().any(|key| is_parameterized(key)) && metadata.contains_key("is_test") {
            apply_test_role(metadata, TestRole::ParameterizedTest);
        }
    }
    mark_dotnet_test_containers(symbols);
}

fn is_parameterized(key: &str) -> bool {
    matches!(
        key,
        "theory" | "datatestmethod" | "testcase" | "testcasesource"
    )
}
