use crate::ExtractionResults;
use crate::base::{Symbol, SymbolKind};
use crate::pipeline::extract_canonical;
use std::path::Path;

fn extract(source: &str) -> ExtractionResults {
    extract_canonical("src/Main.java", source, Path::new("/repo"))
        .expect("canonical Java extraction must succeed")
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
    let source = r#"abstract class Outer {
    class Inner {
        Inner() { }
        String load() {
            String café = "ready";
            child();
            return café;
        }
        void child() { }
        abstract void declaration();
    }
    interface Contract { void dispatch(); }
}
"#
    .replace('\n', "\r\n");
    let results = extract(&source);
    let outer = symbol(&results, "Outer", SymbolKind::Class);
    let inner = symbol(&results, "Inner", SymbolKind::Class);
    let constructor = symbol(&results, "Inner", SymbolKind::Constructor);
    let load = symbol(&results, "load", SymbolKind::Method);
    let child = symbol(&results, "child", SymbolKind::Method);
    let declaration = symbol(&results, "declaration", SymbolKind::Method);
    let contract = symbol(&results, "Contract", SymbolKind::Interface);
    let dispatch = symbol(&results, "dispatch", SymbolKind::Method);

    assert_eq!(inner.parent_id.as_deref(), Some(outer.id.as_str()));
    assert_eq!(load.parent_id.as_deref(), Some(inner.id.as_str()));
    assert_eq!(child.parent_id.as_deref(), Some(inner.id.as_str()));
    assert_eq!(contract.parent_id.as_deref(), Some(outer.id.as_str()));
    assert_eq!(dispatch.parent_id.as_deref(), Some(contract.id.as_str()));
    assert_body(
        &source,
        outer,
        "{\r\n    class Inner {\r\n        Inner() { }\r\n        String load() {\r\n            String café = \"ready\";\r\n            child();\r\n            return café;\r\n        }\r\n        void child() { }\r\n        abstract void declaration();\r\n    }\r\n    interface Contract { void dispatch(); }\r\n}",
    );
    assert_body(
        &source,
        inner,
        "{\r\n        Inner() { }\r\n        String load() {\r\n            String café = \"ready\";\r\n            child();\r\n            return café;\r\n        }\r\n        void child() { }\r\n        abstract void declaration();\r\n    }",
    );
    assert_body(&source, constructor, "{ }");
    assert_body(
        &source,
        load,
        "{\r\n            String café = \"ready\";\r\n            child();\r\n            return café;\r\n        }",
    );
    assert_body(&source, child, "{ }");
    assert_body(&source, contract, "{ void dispatch(); }");
    assert!(declaration.body_span.is_none());
    assert!(declaration.body_hash.is_none());
    assert!(dispatch.body_span.is_none());
    assert!(dispatch.body_hash.is_none());

    let with_comment = source.replace(
        "            child();\r\n",
        "            child(); // comment-only change\r\n",
    );
    let with_code_change = source.replace("            child();\r\n", "            other();\r\n");
    assert_eq!(load_hash(&source), load_hash(&with_comment));
    assert_ne!(load_hash(&source), load_hash(&with_code_change));
}
