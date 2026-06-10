//! Elixir ordered/nested typespec type-argument capture (Miller bridge Phase 2).
//!
//! Elixir typespec parameter forms parse as `call` nodes inside `@type` /
//! `@spec` / `@callback` attribute trees, e.g. `list(list(integer()))`. Nested
//! parameterized forms ride along as `children` of the outermost usage — one
//! `TypeArgumentUsage` row per outermost typespec generic.

use crate::base::TypeArgumentUsage;
use crate::elixir::ElixirExtractor;
use std::path::Path;
use std::path::PathBuf;
use tree_sitter::Parser;

const FIXTURE_SOURCE: &str =
    include_str!("../../../../../fixtures/extraction/elixir/basic/source.ex");

fn capture(code: &str) -> Vec<TypeArgumentUsage> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_elixir::LANGUAGE.into())
        .expect("load Elixir grammar");
    let tree = parser.parse(code, None).expect("parse Elixir");
    let mut ext = ElixirExtractor::new(
        "elixir".to_string(),
        "test.ex".to_string(),
        code.to_string(),
        &PathBuf::from("/test/workspace"),
    );
    let symbols = ext.extract_symbols(&tree);
    ext.extract_identifiers(&tree, &symbols);
    ext.get_type_argument_usages()
}

fn top_level(usage: &TypeArgumentUsage) -> Vec<(u32, &str)> {
    usage
        .arguments
        .iter()
        .map(|arg| (arg.ordinal, arg.type_name.as_str()))
        .collect()
}

#[test]
fn nested_typespec_records_one_outermost_argument() {
    let code = r#"defmodule Foo do
  @type worker_index :: list(list(integer()))
end"#;
    let usages = capture(code);
    assert_eq!(
        usages.len(),
        1,
        "exactly one outermost typespec generic (list), got {usages:?}"
    );
    assert_eq!(top_level(&usages[0]), vec![(0, "list")]);
    assert_eq!(
        usages[0].arguments[0]
            .children
            .iter()
            .map(|c| (c.ordinal, c.type_name.as_str()))
            .collect::<Vec<_>>(),
        vec![(0, "integer")],
        "inner list(integer()) child preserved under ordinal 0"
    );
}

#[test]
fn runtime_call_and_zero_argument_typespec_emit_no_rows() {
    let code = r#"defmodule Foo do
  @spec run(integer()) :: integer()
  def run(id), do: Kernel.abs(id)
end"#;
    let usages = capture(code);
    assert!(
        usages.is_empty(),
        "runtime calls and zero-arg typespec primitives must not emit rows, got {usages:?}"
    );
}

fn extract_fixture(source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical(
        "fixtures/extraction/elixir/basic/source.ex",
        source,
        Path::new("/repo"),
    )
    .expect("canonical Elixir extraction should succeed")
}

#[test]
fn basic_fixture_emits_nested_type_arguments_via_canonical_pipeline() {
    let results = extract_fixture(FIXTURE_SOURCE);
    assert_eq!(
        results.type_argument_usages.len(),
        1,
        "fixture should emit one list(list(integer())) usage, got {:?}",
        results.type_argument_usages
    );
    let usage = &results.type_argument_usages[0];
    assert_eq!(top_level(usage), vec![(0, "list")]);
    assert_eq!(
        usage.arguments[0]
            .children
            .iter()
            .map(|c| (c.ordinal, c.type_name.as_str()))
            .collect::<Vec<_>>(),
        vec![(0, "integer")],
        "inner list(integer()) nested argument preserved under ordinal 0"
    );
}

#[test]
fn basic_fixture_spec_and_runtime_calls_emit_no_type_arguments() {
    let results = extract_fixture(FIXTURE_SOURCE);
    assert!(
        !results
            .type_argument_usages
            .iter()
            .any(|usage| usage.identifier_id.contains(":run:")),
        "@spec run(integer()) and runtime calls must not emit type_argument_usages"
    );
}
