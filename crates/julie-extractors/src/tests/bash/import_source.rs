use crate::base::{ExtractionResults, SymbolKind};
use crate::extract_canonical;
use std::path::Path;

fn extract(path: &str, source: &str) -> ExtractionResults {
    extract_canonical(path, source, Path::new("/tmp/test")).expect("Bash extraction succeeds")
}

fn imports(result: &ExtractionResults) -> Vec<(String, Option<String>, String)> {
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
                    .map(str::to_string)
            };
            (
                symbol.name.clone(),
                text("source"),
                text("form").unwrap_or_default(),
            )
        })
        .collect()
}

fn row(name: &str, source: Option<&str>, form: &str) -> (String, Option<String>, String) {
    (
        name.to_string(),
        source.map(str::to_string),
        form.to_string(),
    )
}

#[test]
fn source_and_dot_rows_carry_the_path_as_written() {
    let result = extract(
        "scripts/run.sh",
        r#"source ./lib/common.sh
. "$DIR/env.sh"
source "${ROOT}/lib/x.sh"
. '../shared/paths.sh'
alias ll='ls -l'
"#,
    );

    assert_eq!(
        imports(&result),
        vec![
            row("common", Some("./lib/common.sh"), "source"),
            row("env", Some("$DIR/env.sh"), "."),
            row("x", Some("${ROOT}/lib/x.sh"), "source"),
            row("paths", Some("../shared/paths.sh"), "."),
            row("ll", None, "alias"),
        ]
    );
    let source_paths: Vec<_> = result
        .symbols
        .iter()
        .filter_map(|symbol| symbol.metadata.as_ref()?.get("sourcePath")?.as_str())
        .collect();
    assert_eq!(
        source_paths,
        vec![
            "./lib/common.sh",
            "$DIR/env.sh",
            "${ROOT}/lib/x.sh",
            "../shared/paths.sh"
        ]
    );
}

#[test]
fn partly_quoted_paths_drop_their_quotes_in_source() {
    let result = extract(
        "scripts/run.sh",
        r#". "$DIR"/common.sh
source "$(dirname "$0")/base-test.sh"
source $HOME/'my lib'/tools.sh
"#,
    );

    assert_eq!(
        imports(&result),
        vec![
            row("common", Some("$DIR/common.sh"), "."),
            row(
                "base-test",
                Some("$(dirname \"$0\")/base-test.sh"),
                "source"
            ),
            row("tools", Some("$HOME/my lib/tools.sh"), "source"),
        ]
    );
}

#[test]
fn bats_load_rows_carry_the_load_form() {
    let result = extract(
        "test/run.bats",
        "load test_helper\nbats_load_library bats-support\n@test \"runs\" {\n  run true\n}\n",
    );

    assert_eq!(
        imports(&result),
        vec![
            row("test_helper", Some("test_helper"), "load"),
            row("bats-support", Some("bats-support"), "bats_load_library"),
        ]
    );
}
