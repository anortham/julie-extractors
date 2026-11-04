use crate::base::{ExtractionResults, Symbol};
use crate::extract_canonical;
use std::collections::BTreeSet;
use std::path::Path;

fn extract(source: &str) -> ExtractionResults {
    extract_canonical("styles/literals.css", source, Path::new("/repo")).expect("CSS extraction")
}

fn owner<'a>(results: &'a ExtractionResults, symbol_id: Option<&str>) -> &'a Symbol {
    results
        .symbols
        .iter()
        .find(|symbol| Some(symbol.id.as_str()) == symbol_id)
        .expect("literal owner")
}

#[test]
fn url_literals_keep_host_spans_and_ignore_comment_and_string_lookalikes() {
    let source = concat!(
        "/* café url(comment.svg) */\r\n",
        ".card { background-image: url(\"/assets/icon.svg\"); content: \"url(text.svg)\"; }\r\n",
        "@namespace url(http://www.w3.org/1999/xhtml);\r\n",
    );
    let results = extract(source);

    assert!(results.parse_diagnostics.is_empty());
    assert_eq!(
        results
            .literals
            .iter()
            .map(|literal| literal.literal_text.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["/assets/icon.svg", "http://www.w3.org/1999/xhtml"])
    );

    let icon = results
        .literals
        .iter()
        .find(|literal| literal.literal_text == "/assets/icon.svg")
        .expect("background URL literal");
    assert_eq!(icon.carrier.as_deref(), Some("url"));
    assert_eq!(icon.arg_position, 0);
    assert_eq!(icon.language, "css");
    assert_eq!(
        &source[icon.start_byte as usize..icon.end_byte as usize],
        "\"/assets/icon.svg\""
    );
    assert_eq!(
        owner(&results, icon.containing_symbol_id.as_deref()).name,
        ".card"
    );

    let namespace = results
        .literals
        .iter()
        .find(|literal| literal.literal_text == "http://www.w3.org/1999/xhtml")
        .expect("namespace URL literal");
    assert_eq!(namespace.carrier.as_deref(), Some("url"));
    assert_eq!(
        &source[namespace.start_byte as usize..namespace.end_byte as usize],
        "http://www.w3.org/1999/xhtml"
    );
    assert_eq!(
        owner(&results, namespace.containing_symbol_id.as_deref())
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("atRuleType"))
            .and_then(serde_json::Value::as_str),
        Some("namespace")
    );
}
