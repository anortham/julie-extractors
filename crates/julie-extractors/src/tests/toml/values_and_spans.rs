use crate::base::{ExtractionResults, Symbol};
use crate::extract_canonical;
use crate::tests::helpers::{facts_with_pattern, metadata_str};
use std::path::Path;

fn extract(source: &str) -> ExtractionResults {
    extract_canonical("config.toml", source, Path::new("/tmp/test")).unwrap()
}

fn symbol<'a>(results: &'a ExtractionResults, name: &str) -> &'a Symbol {
    results
        .symbols
        .iter()
        .find(|s| s.name == name)
        .unwrap_or_else(|| panic!("missing {name}: {:#?}", results.symbols))
}

fn body<'a>(source: &'a str, symbol: &Symbol) -> Option<&'a str> {
    let span = symbol.body_span?;
    source.get(span.start_byte as usize..span.end_byte as usize)
}

#[test]
fn trailing_comment_does_not_replace_the_value() {
    let source = r#"[database]
url = "postgres://localhost/app" # primary DSN
pool = { max = 10 } # connection pool
"#;
    let results = extract(source);

    let url = symbol(&results, "url");
    assert_eq!(
        url.signature.as_deref(),
        Some(r#"url = "postgres://localhost/app""#)
    );
    assert_eq!(url.doc_comment.as_deref(), Some("postgres://localhost/app"));
    assert!(results.literals.iter().any(|literal| {
        literal.literal_text == "postgres://localhost/app"
            && literal.carrier.as_deref() == Some("database.url")
    }));
    let key_values = facts_with_pattern(&results, "toml.key_value.v1");
    let kind_of = |key: &str| {
        key_values
            .iter()
            .find(|fact| metadata_str(fact, "key") == Some(key))
            .and_then(|fact| metadata_str(fact, "value_kind"))
    };
    assert_eq!(kind_of("url"), Some("string"));
    assert_eq!(kind_of("pool"), Some("inline_table"));
    assert!(
        key_values
            .iter()
            .any(|fact| metadata_str(fact, "key_path") == Some("database.pool.max"))
    );
}

#[test]
fn body_spans_come_from_grammar_nodes() {
    let source = r#"[env]
RUST_LOG = "info"
APP_HOME = { value = "target/home", relative = true }
# comment for the next table

[build]
description = "build step (release mode)"
"#;
    let results = extract(source);

    let env = symbol(&results, "env");
    assert_eq!(
        body(source, env),
        Some("RUST_LOG = \"info\"\nAPP_HOME = { value = \"target/home\", relative = true }")
    );
    assert_eq!(env.end_line, 3, "the table ends at its last pair");
    assert_eq!(
        body(source, symbol(&results, "APP_HOME")),
        Some(r#"{ value = "target/home", relative = true }"#)
    );
    assert_eq!(
        body(source, symbol(&results, "description")),
        Some(r#""build step (release mode)""#)
    );
    assert_eq!(
        body(source, symbol(&results, "build")),
        Some(r#"description = "build step (release mode)""#)
    );
}

#[test]
fn empty_table_has_no_body_span() {
    let results = extract("[empty]\n\n[next]\nkey = 1\n");

    let empty = symbol(&results, "empty");
    assert!(empty.body_span.is_none() && empty.body_hash.is_none());
    assert_eq!(empty.end_line, 1);
}
