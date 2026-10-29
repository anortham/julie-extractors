use crate::base::{ExtractionResults, SymbolKind};
use crate::extract_canonical;
use std::path::Path;

fn extract(source: &str) -> ExtractionResults {
    extract_canonical("src/main.cpp", source, Path::new("/tmp/test"))
        .expect("C++ extraction succeeds")
}

fn imports(result: &ExtractionResults) -> Vec<(String, String, String)> {
    result
        .symbols
        .iter()
        .filter(|symbol| symbol.kind == SymbolKind::Import)
        .map(|symbol| {
            let metadata = symbol.metadata.as_ref().expect("import metadata");
            let text = |key: &str| {
                metadata
                    .get(key)
                    .and_then(|value| value.as_str())
                    .unwrap_or_default()
                    .to_string()
            };
            (symbol.name.clone(), text("source"), text("form"))
        })
        .collect()
}

fn row(name: &str, source: &str, form: &str) -> (String, String, String) {
    (name.to_string(), source.to_string(), form.to_string())
}

#[test]
fn include_rows_carry_source_and_form() {
    let result =
        extract("#include \"core/math.hpp\"\n#include <vector>\n#include \"../shared/io.hpp\"\n");

    assert_eq!(
        imports(&result),
        vec![
            row("core/math.hpp", "core/math.hpp", "#include"),
            row("vector", "vector", "#include"),
            row("../shared/io.hpp", "../shared/io.hpp", "#include"),
        ]
    );
    let system_flags: Vec<_> = result
        .symbols
        .iter()
        .filter_map(|symbol| symbol.metadata.as_ref()?.get("isSystemHeader"))
        .filter_map(|value| value.as_str())
        .collect();
    assert_eq!(system_flags, vec!["false", "true", "false"]);
}

#[test]
fn using_and_namespace_alias_rows_carry_source_and_form() {
    let result = extract(
        "using namespace std;\nusing std::vector;\nnamespace fs = std::filesystem;\nusing Index = int;\n",
    );

    assert_eq!(
        imports(&result),
        vec![
            row("std", "std", "using namespace"),
            row("vector", "std::vector", "using"),
            row("fs", "std::filesystem", "namespace"),
        ]
    );
}
