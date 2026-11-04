use super::extract_symbols;

fn root_body_hash(source: &str) -> String {
    extract_symbols(source)
        .into_iter()
        .find(|symbol| {
            symbol.language == "html"
                && symbol
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.get("tagName"))
                    .and_then(serde_json::Value::as_str)
                    == Some("div")
                && symbol
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.get("attributes"))
                    .and_then(|attributes| attributes.get("id"))
                    .and_then(serde_json::Value::as_str)
                    == Some("root")
        })
        .and_then(|symbol| symbol.body_hash)
        .expect("root div body hash")
}

#[test]
fn html_host_body_hash_ignores_host_and_embedded_comments_after_quoted_text() {
    let template = r#"<div id="root">π it's "literal" text<!-- @@HTML_COMMENT@@ --><script>function render(value) { return `value ${value@@JS_COMMENT@@}`; }</script></div>"#;
    let baseline = template
        .replace("@@HTML_COMMENT@@", "base host comment")
        .replace("@@JS_COMMENT@@", "");
    let short = template
        .replace("@@HTML_COMMENT@@", "short host comment")
        .replace("@@JS_COMMENT@@", " /* c */");
    let long = template
        .replace("@@HTML_COMMENT@@", "long host comment that must be ignored")
        .replace(
            "@@JS_COMMENT@@",
            " /* long interpolation comment whose words must disappear */",
        );

    assert_eq!(root_body_hash(&baseline), root_body_hash(&short));
    assert_eq!(root_body_hash(&baseline), root_body_hash(&long));

    let changed_text = baseline.replace("literal", "updated");
    assert_ne!(root_body_hash(&baseline), root_body_hash(&changed_text));
    let changed_script_literal = baseline.replace("`value", "`payload");
    assert_ne!(
        root_body_hash(&baseline),
        root_body_hash(&changed_script_literal)
    );
}

#[test]
fn html_host_body_hash_ignores_embedded_css_comments_and_keeps_css_values() {
    let baseline = "<div id=\"root\"><style>.card { color: red; }</style></div>";
    let short = "<div id=\"root\"><style>.card { color: red; /* c */ }</style></div>";
    let long =
        "<div id=\"root\"><style>.card { color: red; /* long stylesheet comment */ }</style></div>";

    assert_eq!(root_body_hash(baseline), root_body_hash(short));
    assert_eq!(root_body_hash(baseline), root_body_hash(long));

    let changed = baseline.replace("red", "blue");
    assert_ne!(root_body_hash(baseline), root_body_hash(&changed));
}
