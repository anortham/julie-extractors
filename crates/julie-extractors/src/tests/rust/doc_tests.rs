use crate::base::{StructuralFact, collect_rust_doc_test_facts};
use crate::extract_canonical;
use crate::rust::RustExtractor;
use std::path::Path;
use std::path::PathBuf;
use tree_sitter::Parser;

fn rust_doc_facts(source: &str) -> (Vec<StructuralFact>, crate::ExtractionResults) {
    let results = extract_canonical("src/lib.rs", source, Path::new("/test/workspace"))
        .expect("Rust canonical extraction should succeed");
    let facts = results
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == "rust.doc_test.v1")
        .cloned()
        .collect();
    (facts, results)
}

fn assert_fact_contract(fact: &StructuralFact, mode: &str) {
    assert_eq!(fact.pattern_id, "rust.doc_test.v1");
    assert_eq!(fact.capture_name, "doc_test");
    assert_eq!(fact.node_kind, "rustdoc_fence");
    assert_eq!(fact.confidence, 1.0);
    assert_eq!(
        fact.metadata
            .as_ref()
            .and_then(|metadata| metadata.get("mode"))
            .and_then(|value| value.as_str()),
        Some(mode)
    );
    assert_eq!(
        fact.metadata
            .as_ref()
            .and_then(|metadata| metadata.get("pattern_version"))
            .and_then(|value| value.as_i64()),
        Some(1)
    );
    assert_eq!(
        fact.metadata
            .as_ref()
            .and_then(|metadata| metadata.get("query_family"))
            .and_then(|value| value.as_str()),
        Some("testing")
    );
}

#[test]
fn rust_doc_test_facts_emit_outer_and_inner_docs_with_each_mode() {
    let source = r#"//! Module documentation.
//!
//! ```rust,no_run
//! let value = 1;
//! assert_eq!(value, 1);
//! ```

/// Function documentation.
///
/// ```
/// assert_eq!(1 + 1, 2);
/// ```
///
/// ```rust,ignore
/// panic!("ignored");
/// ```
///
/// ```rust,compile_fail
/// let value: i32 = "not an integer";
/// ```
pub fn documented() {}
"#;

    let (facts, results) = rust_doc_facts(source);
    assert_eq!(facts.len(), 4, "tree facts: {facts:?}");

    let modes = facts
        .iter()
        .map(|fact| {
            fact.metadata
                .as_ref()
                .and_then(|metadata| metadata.get("mode"))
                .and_then(|value| value.as_str())
                .expect("doc-test facts carry a mode")
        })
        .collect::<Vec<_>>();
    assert_eq!(modes, vec!["no_run", "run", "ignore", "compile_fail"]);

    for (fact, mode) in facts
        .iter()
        .zip(["no_run", "run", "ignore", "compile_fail"])
    {
        assert_fact_contract(fact, mode);
        assert!(fact.start_byte < fact.end_byte);
        assert!(fact.start_line <= fact.end_line);
    }

    let documented_id = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "documented")
        .map(|symbol| symbol.id.as_str())
        .expect("documented function symbol");
    assert_eq!(
        facts[1].containing_symbol_id.as_deref(),
        Some(documented_id)
    );
    assert_eq!(
        facts[2].containing_symbol_id.as_deref(),
        Some(documented_id)
    );
    assert_eq!(
        facts[3].containing_symbol_id.as_deref(),
        Some(documented_id)
    );
    assert_eq!(facts[0].containing_symbol_id, None);
}

#[test]
fn rust_doc_test_facts_preserve_multiple_fence_spans_and_ids() {
    let source = r#"/// Examples:
///
/// ```rust
/// let first = 1;
/// assert_eq!(first, 1);
/// ```
///
/// ```rust,no_run
/// let second = 2;
/// assert_eq!(second, 2);
/// ```
pub fn examples() {}
"#;

    let (facts, _) = rust_doc_facts(source);
    let (facts_again, _) = rust_doc_facts(source);
    assert_eq!(facts.len(), 2, "tree facts: {facts:?}");
    assert_fact_contract(&facts[0], "run");
    assert_fact_contract(&facts[1], "no_run");
    assert_ne!(facts[0].id, facts[1].id);
    assert_eq!(
        &source[facts[0].start_byte as usize..facts[0].end_byte as usize],
        "```rust\n/// let first = 1;\n/// assert_eq!(first, 1);\n/// ```"
    );
    assert_eq!(
        &source[facts[1].start_byte as usize..facts[1].end_byte as usize],
        "```rust,no_run\n/// let second = 2;\n/// assert_eq!(second, 2);\n/// ```"
    );
    assert_eq!((facts[0].start_line, facts[0].start_column), (3, 4));
    assert_eq!((facts[0].end_line, facts[0].end_column), (6, 7));
    assert_eq!((facts[1].start_line, facts[1].start_column), (8, 4));
    assert_eq!((facts[1].end_line, facts[1].end_column), (11, 7));
    assert_eq!(
        facts.iter().map(|fact| &fact.id).collect::<Vec<_>>(),
        facts_again.iter().map(|fact| &fact.id).collect::<Vec<_>>()
    );
}

#[test]
fn rust_doc_test_facts_attach_inner_module_docs_to_the_module() {
    let source = r#"mod nested {
    //! Nested module documentation.
    //!
    //! ```rust,no_run
    //! let value = 1;
    //! assert_eq!(value, 1);
    //! ```
}
"#;

    let (facts, results) = rust_doc_facts(source);
    assert_eq!(facts.len(), 1, "tree facts: {facts:?}");
    assert_fact_contract(&facts[0], "no_run");
    let nested_id = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "nested")
        .map(|symbol| symbol.id.as_str())
        .expect("nested module symbol");
    assert_eq!(facts[0].containing_symbol_id.as_deref(), Some(nested_id));
    assert_eq!((facts[0].start_line, facts[0].start_column), (4, 8));
    assert_eq!((facts[0].end_line, facts[0].end_column), (7, 11));
}

#[test]
fn rust_doc_test_facts_ignore_non_executable_and_incomplete_fences() {
    let source = r#"// ordinary comment
// ```rust
// fn ordinary() {}
// ```

/// ```text
/// not executable
/// ```
///
/// ```python
/// print("not Rust")
/// ```
///
/// ```rust
/// let unterminated = true;
pub fn documented() {}
"#;

    let (facts, _) = rust_doc_facts(source);
    assert!(facts.is_empty(), "tree facts: {facts:?}");
}

#[test]
fn rust_doc_test_facts_do_not_pair_across_separate_comment_blocks() {
    let source = r#"/// ```rust
/// first block is unterminated

/// ```rust
/// second block is complete
/// ```
pub fn documented() {}
"#;

    let (facts, _) = rust_doc_facts(source);
    assert_eq!(facts.len(), 1, "tree facts: {facts:?}");
    assert_fact_contract(&facts[0], "run");
    assert_eq!((facts[0].start_line, facts[0].start_column), (4, 4));
    assert_eq!((facts[0].end_line, facts[0].end_column), (6, 7));
}

#[test]
fn rust_doc_test_facts_are_rust_only() {
    let source = "/// ```rust\n/// let value = 1;\n/// ```\npub fn documented() {}\n";
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .expect("Rust grammar should load");
    let tree = parser
        .parse(source, None)
        .expect("Rust source should parse");
    let mut extractor = RustExtractor::new(
        "rust".to_string(),
        "src/lib.rs".to_string(),
        source.to_string(),
        &PathBuf::from("/test/workspace"),
    );
    let symbols = extractor.extract_symbols(&tree);

    assert!(
        collect_rust_doc_test_facts("python", &tree, "src/lib.py", source, &symbols).is_empty()
    );
}

#[test]
fn rust_doc_test_facts_recognize_the_rustdoc_attribute_vocabulary() {
    let source = r#"/// ```should_panic
/// panic!("boom");
/// ```
///
/// ```edition2021
/// let value = 1;
/// ```
///
/// ```ignore-windows
/// let value = 1;
/// ```
///
/// ```no_run,should_panic
/// panic!("boom");
/// ```
///
/// ```test_harness
/// let value = 1;
/// ```
pub fn documented() {}
"#;

    let (facts, _) = rust_doc_facts(source);
    let modes = facts
        .iter()
        .map(|fact| {
            fact.metadata
                .as_ref()
                .and_then(|metadata| metadata.get("mode"))
                .and_then(|value| value.as_str())
                .expect("doc-test facts carry a mode")
        })
        .collect::<Vec<_>>();
    assert_eq!(
        modes,
        vec!["run", "run", "ignore", "no_run", "run"],
        "tree facts: {facts:?}"
    );
}

#[test]
fn rust_doc_test_facts_keep_a_fence_that_mixes_known_and_unknown_tokens() {
    let source = r#"/// ```rust,custom-tooling
/// let value = 1;
/// ```
pub fn documented() {}
"#;

    let (facts, _) = rust_doc_facts(source);
    assert_eq!(facts.len(), 1, "tree facts: {facts:?}");
    assert_fact_contract(&facts[0], "run");
}

#[test]
fn rust_doc_test_facts_extract_fences_from_outer_block_doc_comments() {
    let source = r#"/** Adds numbers.

```
assert_eq!(1 + 1, 2);
```
*/
pub fn add() {}
"#;

    let (facts, results) = rust_doc_facts(source);
    assert_eq!(facts.len(), 1, "tree facts: {facts:?}");
    assert_fact_contract(&facts[0], "run");
    assert_eq!(
        &source[facts[0].start_byte as usize..facts[0].end_byte as usize],
        "```\nassert_eq!(1 + 1, 2);\n```"
    );
    let add_id = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "add")
        .map(|symbol| symbol.id.as_str())
        .expect("add function symbol");
    assert_eq!(facts[0].containing_symbol_id.as_deref(), Some(add_id));
}

#[test]
fn rust_doc_test_facts_strip_star_prefixes_in_block_doc_comments() {
    let source = r#"/**
 * Example:
 * ```rust,no_run
 * let value = 1;
 * ```
 */
pub fn starred() {}
"#;

    let (facts, results) = rust_doc_facts(source);
    assert_eq!(facts.len(), 1, "tree facts: {facts:?}");
    assert_fact_contract(&facts[0], "no_run");
    assert_eq!(
        &source[facts[0].start_byte as usize..facts[0].end_byte as usize],
        "```rust,no_run\n * let value = 1;\n * ```"
    );
    let starred_id = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "starred")
        .map(|symbol| symbol.id.as_str())
        .expect("starred function symbol");
    assert_eq!(facts[0].containing_symbol_id.as_deref(), Some(starred_id));
}

#[test]
fn rust_doc_test_facts_attach_inner_block_doc_fences_to_the_module() {
    let source = r#"mod nested_block {
    /*!
     * ```
     * assert_eq!(2 + 2, 4);
     * ```
     */
}
"#;

    let (facts, results) = rust_doc_facts(source);
    assert_eq!(facts.len(), 1, "tree facts: {facts:?}");
    assert_fact_contract(&facts[0], "run");
    let nested_id = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "nested_block")
        .map(|symbol| symbol.id.as_str())
        .expect("nested_block module symbol");
    assert_eq!(facts[0].containing_symbol_id.as_deref(), Some(nested_id));
}

#[test]
fn rust_doc_test_facts_ignore_fences_in_non_doc_block_comments() {
    let source = r#"/*
```rust
let value = 1;
```
*/

/***
```rust
let value = 2;
```
*/
pub fn not_documented() {}
"#;

    let (facts, _) = rust_doc_facts(source);
    assert!(facts.is_empty(), "tree facts: {facts:?}");
}

#[test]
fn rust_doc_test_facts_do_not_pair_block_doc_fences_across_comment_kinds() {
    let source = r#"/** ```rust
let unterminated = true;
*/
/// ```
pub fn documented() {}
"#;

    let (facts, _) = rust_doc_facts(source);
    assert!(facts.is_empty(), "tree facts: {facts:?}");
}
