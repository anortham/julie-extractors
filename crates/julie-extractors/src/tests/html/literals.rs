use crate::base::Literal;
use crate::html::HTMLExtractor;
use std::path::PathBuf;
use tree_sitter::Parser;

fn capture_literals(html: &str) -> Vec<Literal> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_html::LANGUAGE.into())
        .expect("html grammar");
    let tree = parser.parse(html, None).expect("parse");
    let mut extractor = HTMLExtractor::new(
        "html".to_string(),
        "test.html".to_string(),
        html.to_string(),
        &PathBuf::from("/tmp/test"),
    );
    let symbols = extractor.extract_symbols(&tree);
    extractor.extract_identifiers(&tree, &symbols);
    extractor.base.take_literals()
}

#[test]
fn attribute_literals_use_tag_and_attribute_carriers() {
    let html = r#"
<html>
  <body>
    <a href="/workers">Workers</a>
    <form action="/submit">
      <button data-action="run">Run</button>
      <input name="worker_id" value="42">
    </form>
    <A HREF="/upper">Upper</A>
  </body>
</html>
"#;

    let literals = capture_literals(html);
    assert_eq!(
        literals
            .iter()
            .find(|literal| literal.literal_text == "/workers")
            .and_then(|literal| literal.carrier.as_deref()),
        Some("a.href")
    );
    assert_eq!(
        literals
            .iter()
            .find(|literal| literal.literal_text == "/submit")
            .and_then(|literal| literal.carrier.as_deref()),
        Some("form.action")
    );
    assert_eq!(
        literals
            .iter()
            .find(|literal| literal.literal_text == "run")
            .and_then(|literal| literal.carrier.as_deref()),
        Some("button.data-action")
    );
    assert_eq!(
        literals
            .iter()
            .find(|literal| literal.literal_text == "worker_id")
            .and_then(|literal| literal.carrier.as_deref()),
        Some("input.name")
    );
    assert_eq!(
        literals
            .iter()
            .find(|literal| literal.literal_text == "/upper")
            .and_then(|literal| literal.carrier.as_deref()),
        Some("a.href")
    );
}
