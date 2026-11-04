use crate::base::{Literal, Symbol};
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

fn capture_symbols_and_literals(html: &str) -> (Vec<Symbol>, Vec<Literal>) {
    let results = crate::extract_canonical("test.html", html, std::path::Path::new("/tmp/test"))
        .expect("canonical HTML extraction");
    (results.symbols, results.literals)
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

#[test]
fn attribute_literals_exclude_comment_and_text_lookalikes_with_host_bytes() {
    let html = "<!-- café href=\"/comment.svg\" -->\r\n<a href=\"/workers\" title=\"/title.svg\">href=\"/text.svg\"</a>\r\n";
    let (symbols, literals) = capture_symbols_and_literals(html);

    assert_eq!(literals.len(), 2);
    for (text, carrier) in [("/workers", "a.href"), ("/title.svg", "a.title")] {
        let literal = literals
            .iter()
            .find(|literal| literal.literal_text == text)
            .unwrap_or_else(|| panic!("missing HTML literal {text}"));
        let raw = format!("\"{text}\"");
        let start = html.find(&raw).unwrap();

        assert_eq!(literal.carrier.as_deref(), Some(carrier));
        assert_eq!(literal.language, "html");
        assert_eq!(literal.arg_position, 0);
        assert_eq!(literal.start_byte as usize, start);
        assert_eq!(literal.end_byte as usize, start + raw.len());
        assert_eq!(
            &html[literal.start_byte as usize..literal.end_byte as usize],
            raw
        );
        let owner = symbols
            .iter()
            .find(|symbol| Some(symbol.id.as_str()) == literal.containing_symbol_id.as_deref())
            .expect("literal owner");
        assert_eq!(owner.name, "a");
    }
}
