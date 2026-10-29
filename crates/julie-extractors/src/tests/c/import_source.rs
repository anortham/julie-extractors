use crate::base::{ExtractionResults, RelationshipKind, SymbolKind};
use crate::extract_canonical;
use std::path::Path;

fn extract(source: &str) -> ExtractionResults {
    extract_canonical("src/main.c", source, Path::new("/tmp/test")).expect("C extraction succeeds")
}

fn imports(result: &ExtractionResults) -> Vec<(String, String, String, String, String)> {
    result
        .symbols
        .iter()
        .filter(|symbol| symbol.kind == SymbolKind::Import)
        .map(|symbol| {
            let metadata = symbol.metadata.as_ref().expect("include metadata");
            let text = |key: &str| {
                metadata
                    .get(key)
                    .and_then(|value| value.as_str())
                    .unwrap_or_default()
                    .to_string()
            };
            (
                symbol.name.clone(),
                text("path"),
                text("isSystemHeader"),
                text("source"),
                text("form"),
            )
        })
        .collect()
}

fn row(name: &str, system: &str) -> (String, String, String, String, String) {
    (
        name.to_string(),
        name.to_string(),
        system.to_string(),
        name.to_string(),
        "#include".to_string(),
    )
}

#[test]
fn include_rows_carry_source_and_form() {
    let result = extract(
        "#include \"util/helpers.h\"\n#include <stdio.h>\n#include \"../common/defs.h\"\n#include <sys/types.h>\n",
    );

    assert_eq!(
        imports(&result),
        vec![
            row("util/helpers.h", "false"),
            row("stdio.h", "true"),
            row("../common/defs.h", "false"),
            row("sys/types.h", "true"),
        ]
    );
}

#[test]
fn include_with_trailing_comment_reads_the_path_node() {
    let result =
        extract("#include \"../common/defs.h\" // see <x>\n#include <stdlib.h> /* \"legacy\" */\n");

    assert_eq!(
        imports(&result),
        vec![row("../common/defs.h", "false"), row("stdlib.h", "true")]
    );
    let include_paths: Vec<_> = result
        .relationships
        .iter()
        .filter(|relationship| relationship.kind == RelationshipKind::Imports)
        .map(|relationship| relationship.to_symbol_id.as_str())
        .collect();
    assert_eq!(
        include_paths,
        vec!["header:../common/defs.h", "header:stdlib.h"]
    );
}

#[test]
fn macro_include_writes_no_import_row() {
    let result = extract("#include CONFIG_HEADER\nint main(void) { return 0; }\n");

    assert!(imports(&result).is_empty());
}
