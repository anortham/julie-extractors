use crate::base::Symbol;
use crate::markdown::MarkdownExtractor;
use std::path::PathBuf;
use tree_sitter::Parser;

fn extract_symbols(source: &str) -> Vec<Symbol> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_md::LANGUAGE.into())
        .expect("Markdown grammar should load");
    let tree = parser.parse(source, None).expect("Markdown should parse");
    let mut extractor = MarkdownExtractor::new(
        "markdown".to_string(),
        "test.md".to_string(),
        source.to_string(),
        &PathBuf::from("/tmp/test"),
    );
    extractor.extract_symbols(&tree)
}

fn code_blocks(symbols: &[Symbol]) -> Vec<&Symbol> {
    symbols
        .iter()
        .filter(|symbol| {
            symbol
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("markdown_kind"))
                .and_then(|value| value.as_str())
                == Some("code_block")
        })
        .collect()
}

fn carries_test_role(symbol: &Symbol) -> bool {
    let Some(metadata) = symbol.metadata.as_ref() else {
        return false;
    };
    metadata.contains_key("is_test")
        || metadata.contains_key("test_role")
        || metadata.contains_key("test_container")
        || metadata.contains_key("test_lifecycle")
}

#[test]
fn fenced_code_blocks_never_carry_a_test_role() {
    let source = r#"```rust
fn explicit() {}
```

```rust,no_run
fn no_run_case() {}
```

```rust,compile_fail
let value: i32 = "not an integer";
```

```
fn default_rust() {}
```

```compile_fail
let value: i32 = "not an integer";
```

```rust,ignore
fn ignored_rust() {}
```

```python
print("not Rust")
```
"#;
    let symbols = extract_symbols(source);
    let blocks = code_blocks(&symbols);

    assert_eq!(blocks.len(), 7);
    for block in blocks {
        assert!(
            !carries_test_role(block),
            "markdown code block must carry no test role: {:?}",
            block.metadata
        );
    }
}
