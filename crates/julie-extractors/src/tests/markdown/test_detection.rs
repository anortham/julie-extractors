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

fn code_block<'a>(symbols: &'a [Symbol], info_string: Option<&str>) -> &'a Symbol {
    symbols
        .iter()
        .find(|symbol| {
            symbol
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("markdown_kind"))
                .and_then(|value| value.as_str())
                == Some("code_block")
                && symbol
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.get("info_string"))
                    .and_then(|value| value.as_str())
                    == info_string
        })
        .expect("expected fenced code block")
}

fn is_test_case(symbol: &Symbol) -> bool {
    let Some(metadata) = symbol.metadata.as_ref() else {
        return false;
    };
    metadata.get("is_test").and_then(|value| value.as_bool()) == Some(true)
        && !metadata.contains_key("test_case")
}

#[test]
fn rustdoc_fences_mark_executable_cases() {
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
"#;
    let symbols = extract_symbols(source);

    for info_string in [
        Some("rust"),
        Some("rust,no_run"),
        Some("rust,compile_fail"),
        None,
        Some("compile_fail"),
    ] {
        assert!(is_test_case(code_block(&symbols, info_string)));
    }
}

#[test]
fn rustdoc_ignore_and_non_rust_fences_are_not_test_cases() {
    let source = r#"```rust,ignore
fn ignored_rust() {}
```

```ignore
fn ignored_default() {}
```

```python
print("not Rust")
```

```javascript
console.log("not Rust")
```
"#;
    let symbols = extract_symbols(source);

    for info_string in [
        Some("rust,ignore"),
        Some("ignore"),
        Some("python"),
        Some("javascript"),
    ] {
        assert!(!is_test_case(code_block(&symbols, info_string)));
    }
}
