//! Phase-0 grammar gate for Erlang and XML.
//!
//! Temporary scaffolding: these smoke tests exist only to prove the newly pinned
//! `tree-sitter-erlang` and `tree-sitter-xml` grammars load and parse under the workspace
//! `tree-sitter = "=0.26.11"` runtime. Tasks 2 and 3 replace them with real extractor tests;
//! delete this module once those land.

use tree_sitter::{Language, Parser};

fn parse_with(language: Language, code: &str) -> tree_sitter::Tree {
    let mut parser = Parser::new();
    parser
        .set_language(&language)
        .expect("grammar incompatible with the pinned tree-sitter runtime");
    parser.parse(code, None).expect("parse returned no tree")
}

fn assert_no_error_nodes(tree: &tree_sitter::Tree) {
    let mut cursor = tree.walk();
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        assert!(
            !node.is_error() && !node.is_missing(),
            "unexpected {} node at {:?}",
            node.kind(),
            node.start_position()
        );
        for child in node.children(&mut cursor) {
            stack.push(child);
        }
    }
}

#[test]
fn erlang_grammar_parses_module_export_and_record() {
    let code = r#"-module(bank).
-export([open/1, balance/1]).

-record(account, {id :: integer(), balance = 0 :: integer()}).

open(Id) ->
    #account{id = Id}.

balance(#account{balance = B}) ->
    B.
"#;

    let tree = parse_with(tree_sitter_erlang::LANGUAGE.into(), code);

    assert_eq!(tree.root_node().kind(), "source_file");
    assert_no_error_nodes(&tree);
}

#[test]
fn xml_grammar_parses_nested_elements_and_attributes() {
    let code = r#"<?xml version="1.0" encoding="UTF-8"?>
<project name="julie" version="2.20.0">
  <dependencies>
    <dependency scope="test">tempfile</dependency>
  </dependencies>
</project>
"#;

    let tree = parse_with(tree_sitter_xml::LANGUAGE_XML.into(), code);

    assert_eq!(tree.root_node().kind(), "document");
    assert_no_error_nodes(&tree);
}
