use crate::ExtractionResults;
use crate::base::{Symbol, SymbolKind};
use crate::pipeline::extract_canonical;
use std::path::Path;

fn extract(source: &str) -> ExtractionResults {
    extract_canonical("src/Main.scala", source, Path::new("/repo"))
        .expect("canonical Scala extraction must succeed")
}

fn symbol<'a>(results: &'a ExtractionResults, name: &str, kind: SymbolKind) -> &'a Symbol {
    results
        .symbols
        .iter()
        .find(|symbol| symbol.name == name && symbol.kind == kind)
        .unwrap_or_else(|| panic!("expected {kind:?} symbol {name}"))
}

fn assert_body(source: &str, symbol: &Symbol, expected: &str) {
    let span = symbol
        .body_span
        .expect("body-bearing symbol needs a body span");
    let declaration = source
        .get(symbol.start_byte as usize..symbol.end_byte as usize)
        .expect("symbol declaration span must select source text");
    let expected_start = declaration
        .find(expected)
        .expect("expected body must appear in source");
    let expected_start = symbol.start_byte as usize + expected_start;
    assert_eq!(
        span.start_byte as usize, expected_start,
        "{} body start",
        symbol.name
    );
    assert_eq!(
        span.end_byte as usize,
        expected_start + expected.len(),
        "{} body end",
        symbol.name
    );
    assert_eq!(
        source.get(span.start_byte as usize..span.end_byte as usize),
        Some(expected)
    );
    assert!(symbol.body_hash.is_some());
}

fn load_hash(source: &str) -> String {
    symbol(&extract(source), "load", SymbolKind::Method)
        .body_hash
        .clone()
        .expect("concrete method needs a body hash")
}

#[test]
fn canonical_body_spans_keep_nested_owners_utf8_crlf_and_hash_boundaries() {
    let source = r#"object Outer {
  abstract class Inner(seed: Int) {
    def this() = { this(0) }
    def load(): String = {
      val café = "ready"
      child()
      café
    }
    def child(): Unit = { }
    def expression(): Int = 42
    def declaration(): Unit
  }
  class Empty { }
}
"#
    .replace('\n', "\r\n");
    let results = extract(&source);
    let outer = symbol(&results, "Outer", SymbolKind::Class);
    let inner = symbol(&results, "Inner", SymbolKind::Class);
    let constructor = symbol(&results, "Inner", SymbolKind::Constructor);
    let load = symbol(&results, "load", SymbolKind::Method);
    let child = symbol(&results, "child", SymbolKind::Method);
    let expression = symbol(&results, "expression", SymbolKind::Method);
    let declaration = symbol(&results, "declaration", SymbolKind::Method);
    let empty = symbol(&results, "Empty", SymbolKind::Class);

    fn find_function<'tree>(
        node: tree_sitter::Node<'tree>,
        source: &str,
        marker: &str,
    ) -> Option<tree_sitter::Node<'tree>> {
        if node.kind().contains("function")
            && source
                .get(node.start_byte()..node.end_byte())
                .is_some_and(|text| text.contains(marker))
        {
            return Some(node);
        }
        let mut cursor = node.walk();
        node.named_children(&mut cursor)
            .find_map(|child| find_function(child, source, marker))
    }
    let mut parser = super::init_test_parser();
    let tree = parser.parse(&source, None).expect("Scala source parses");
    let declaration_node =
        find_function(tree.root_node(), &source, "def declaration").expect("declaration node");
    let ast_fields = ["def load", "def expression"].map(|marker| {
        let function = find_function(tree.root_node(), &source, marker).expect("function node");
        let body = function.child_by_field_name("body").map(|body| {
            (
                body.kind(),
                source.get(body.start_byte()..body.end_byte()).unwrap_or(""),
            )
        });
        (marker, function.kind(), body)
    });

    assert_eq!(inner.parent_id.as_deref(), Some(outer.id.as_str()));
    assert_eq!(load.parent_id.as_deref(), Some(inner.id.as_str()));
    assert_eq!(child.parent_id.as_deref(), Some(inner.id.as_str()));
    assert_eq!(expression.parent_id.as_deref(), Some(inner.id.as_str()));
    assert_body(
        &source,
        outer,
        "{\r\n  abstract class Inner(seed: Int) {\r\n    def this() = { this(0) }\r\n    def load(): String = {\r\n      val café = \"ready\"\r\n      child()\r\n      café\r\n    }\r\n    def child(): Unit = { }\r\n    def expression(): Int = 42\r\n    def declaration(): Unit\r\n  }\r\n  class Empty { }\r\n}",
    );
    assert_body(
        &source,
        inner,
        "{\r\n    def this() = { this(0) }\r\n    def load(): String = {\r\n      val café = \"ready\"\r\n      child()\r\n      café\r\n    }\r\n    def child(): Unit = { }\r\n    def expression(): Int = 42\r\n    def declaration(): Unit\r\n  }",
    );
    assert_body(&source, constructor, "{ this(0) }");
    assert_body(
        &source,
        load,
        "{\r\n      val café = \"ready\"\r\n      child()\r\n      café\r\n    }",
    );
    assert_body(&source, child, "{ }");
    assert_body(&source, expression, "42");
    assert_body(&source, empty, "{ }");
    assert!(
        declaration.body_span.is_none(),
        "bodyless declaration must have no span or hash; span={:?} hash={:?} slice={:?} AST={} body_field={:?} definition_fields={ast_fields:?}",
        declaration.body_span,
        declaration.body_hash,
        declaration
            .body_span
            .and_then(|span| source.get(span.start_byte as usize..span.end_byte as usize)),
        declaration_node.kind(),
        declaration_node
            .child_by_field_name("body")
            .map(|body| body.kind())
    );
    assert!(declaration.body_hash.is_none());

    let with_comment = source.replace(
        "      child()\r\n",
        "      child() // comment-only change\r\n",
    );
    let with_code_change = source.replace("      child()\r\n", "      other()\r\n");
    assert_eq!(load_hash(&source), load_hash(&with_comment));
    assert_ne!(load_hash(&source), load_hash(&with_code_change));
}
