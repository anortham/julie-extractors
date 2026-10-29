use crate::base::{ExtractionResults, SymbolKind};
use crate::extract_canonical;
use std::path::Path;

fn extract(source: &str) -> ExtractionResults {
    extract_canonical("styles/site.css", source, Path::new("/tmp/test"))
        .expect("CSS extraction succeeds")
}

fn imports(result: &ExtractionResults) -> Vec<(String, String, String, String, String)> {
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
            (
                symbol.name.clone(),
                text("url"),
                text("source"),
                text("form"),
                text("media"),
            )
        })
        .collect()
}

fn row(path: &str, media: &str) -> (String, String, String, String, String) {
    (
        path.to_string(),
        path.to_string(),
        path.to_string(),
        "@import".to_string(),
        media.to_string(),
    )
}

#[test]
fn import_rows_carry_source_and_form() {
    let result = extract(
        r#"@import "base/reset.css";
@import url("theme.css");
@import url(plain.css);
@import '../shared/print.css' print, screen and (min-width: 600px);
@import url("layered.css") layer(base) supports(display: grid);
.page { color: red; }
"#,
    );

    assert_eq!(
        imports(&result),
        vec![
            row("base/reset.css", ""),
            row("theme.css", ""),
            row("plain.css", ""),
            row(
                "../shared/print.css",
                "print, screen and (min-width: 600px)"
            ),
            row("layered.css", "layer(base) supports(display: grid)"),
        ]
    );
}
