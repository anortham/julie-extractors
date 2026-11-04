use crate::base::body::body_hash;
use crate::base::{BaseExtractor, NormalizedSpan, SymbolKind, SymbolOptions};
use tree_sitter::Parser;

#[test]
fn programming_parameters_do_not_own_type_or_decorator_bodies() {
    for (path, source) in [
        (
            "sample.ts",
            "function keep(callback: () => void, input: { quantity: number }) { callback(); } class Worker { constructor(@Inject() public readonly store: Logger) {} }",
        ),
        (
            "sample.tsx",
            "function keep(callback: () => void, input: { quantity: number }) { callback(); } class Worker { constructor(@Inject() public readonly store: Logger) {} }",
        ),
        (
            "sample.kt",
            "fun keep(predicate: (Int) -> Boolean): Boolean { return predicate(1) }",
        ),
        (
            "sample.zig",
            "fn keep(server: *httpz.Server(void)) void { _ = server; }",
        ),
    ] {
        let result =
            crate::pipeline::extract_canonical(path, source, std::path::Path::new("/repo"))
                .expect(path);
        assert!(
            result.parse_diagnostics.is_empty(),
            "{path}: {:?}",
            result.parse_diagnostics
        );
        let parameters: Vec<_> = result
            .symbols
            .iter()
            .filter(|symbol| {
                symbol
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.get("role"))
                    .and_then(serde_json::Value::as_str)
                    == Some("parameter")
            })
            .collect();
        assert!(!parameters.is_empty(), "{path}");
        for parameter in parameters {
            assert!(parameter.body_span.is_none(), "{path}: {parameter:?}");
            assert!(parameter.body_hash.is_none(), "{path}: {parameter:?}");
        }
        let callable = result
            .symbols
            .iter()
            .find(|symbol| {
                symbol.name == "keep"
                    && matches!(symbol.kind, SymbolKind::Function | SymbolKind::Method)
            })
            .expect(path);
        assert!(callable.body_span.is_some(), "{path}");
        assert!(callable.body_hash.is_some(), "{path}");
    }
}

#[test]
fn create_symbol_records_body_span_for_tree_sitter_body_node() {
    let content = "fn hello() {\n    let value = 1;\n}\n";
    let mut extractor = rust_extractor(content);
    let tree = parse_rust(content);
    let function = tree
        .root_node()
        .child(0)
        .expect("rust fixture should contain a function item");

    let symbol = extractor.create_symbol(
        &function,
        "hello".to_string(),
        SymbolKind::Function,
        SymbolOptions::default(),
    );

    let body_span = symbol
        .body_span
        .expect("function symbols should carry a body span");
    assert_eq!(
        &content[body_span.start_byte as usize..body_span.end_byte as usize],
        "{\n    let value = 1;\n}"
    );
    assert!(symbol.body_hash.is_some(), "body span requires body hash");
}

#[test]
fn csharp_interpolation_comments_do_not_change_method_body_hashes() {
    let hashes: Vec<_> = ["", "/* short */", "/* a much longer comment */"]
        .into_iter()
        .map(|comment| {
            let source = format!(
                "class Sample {{ string Format(string name) {{ return $\"value {{{comment}name}}\"; }} }}"
            );
            let result = crate::pipeline::extract_canonical(
                "Sample.cs",
                &source,
                std::path::Path::new("/repo"),
            )
            .expect("C# extraction");
            assert!(result.parse_diagnostics.is_empty(), "{:?}", result.parse_diagnostics);
            result.symbols.into_iter().find(|symbol| symbol.name == "Format"
                && symbol.kind == SymbolKind::Method).expect("Format method")
                .body_hash.expect("method hash")
        })
        .collect();
    assert!(
        hashes.windows(2).all(|pair| pair[0] == pair[1]),
        "{hashes:?}"
    );
}

#[test]
fn body_hash_ignores_whitespace_only_formatting_changes() {
    let compact = symbol_for_rust_function("fn hello(){let value=1;}\n");
    let spaced = symbol_for_rust_function("fn hello() {\n    let value = 1;\n}\n");

    assert_eq!(compact.body_hash, spaced.body_hash);
}

#[test]
fn body_hash_ignores_comment_only_changes() {
    let plain = symbol_for_rust_function("fn hello() {\n    let value = 1;\n}\n");
    let commented = symbol_for_rust_function(
        "fn hello() {\n    // only a comment\n    let value = 1; /* also only a comment */\n}\n",
    );

    assert!(
        plain.body_hash.is_some(),
        "plain Rust function needs body_hash"
    );
    assert!(
        commented.body_hash.is_some(),
        "commented Rust function needs body_hash"
    );
    assert_eq!(plain.body_hash, commented.body_hash);
}

#[test]
fn body_hash_ignores_vbnet_comment_only_changes() {
    let plain = symbol_for_vbnet_method(
        r#"Public Class Sample
    Public Sub Hello()
        Dim value = 1
    End Sub
End Class
"#,
    );
    let commented = symbol_for_vbnet_method(
        r#"Public Class Sample
    Public Sub Hello()
        ' only a comment
        REM also only a comment
        rEm mixed-case comment
        REM
        Dim value = 1 : REM inline comment
    End Sub
End Class
"#,
    );

    assert!(
        plain.body_hash.is_some(),
        "plain VB.NET method needs body_hash"
    );
    assert!(
        commented.body_hash.is_some(),
        "commented VB.NET method needs body_hash"
    );
    assert_eq!(plain.body_hash, commented.body_hash);
}

#[test]
fn body_hash_normalizer_ignores_vbnet_comment_syntax_inside_span() {
    let plain = body_hash_for_language("Dim value = 1\n", "vbnet");
    let commented = body_hash_for_language(
        "' only a comment\nREM also only a comment\nrEm mixed-case comment\nREM\nDim value = 1\n",
        "vbnet",
    );

    assert_eq!(plain, commented);
}

#[test]
fn body_hash_normalizer_ignores_vbnet_inline_rem_comments_after_colon() {
    let plain = body_hash_for_language("Dim value = 1\n", "vbnet");
    let commented = body_hash_for_language("Dim value = 1 : REM inline comment\n", "vbnet");

    assert_eq!(plain, commented);
}

#[test]
fn body_hash_normalizer_keeps_vbnet_rem_prefix_identifiers() {
    let rem_variable = body_hash_for_language("Dim Remote = 1\n", "vbnet");
    let value_variable = body_hash_for_language("Dim value = 1\n", "vbnet");

    assert_ne!(rem_variable, value_variable);
}

#[test]
fn body_hash_changes_when_executable_tokens_change() {
    let one = symbol_for_rust_function("fn hello() {\n    let value = 1;\n}\n");
    let two = symbol_for_rust_function("fn hello() {\n    let value = 2;\n}\n");

    assert_ne!(one.body_hash, two.body_hash);
}

#[test]
fn body_hash_keeps_comment_markers_inside_strings() {
    let slash = symbol_for_rust_function(
        r#"fn hello() {
    let value = "// not a comment";
}
"#,
    );
    let block = symbol_for_rust_function(
        r#"fn hello() {
    let value = "/* not a comment */";
}
"#,
    );

    assert_ne!(slash.body_hash, block.body_hash);
}

fn symbol_for_rust_function(content: &str) -> crate::base::Symbol {
    let mut extractor = rust_extractor(content);
    let tree = parse_rust(content);
    let function = tree
        .root_node()
        .child(0)
        .expect("rust fixture should contain a function item");

    extractor.create_symbol(
        &function,
        "hello".to_string(),
        SymbolKind::Function,
        SymbolOptions::default(),
    )
}

fn symbol_for_vbnet_method(content: &str) -> crate::base::Symbol {
    crate::pipeline::extract_canonical("test.vb", content, std::path::Path::new("/repo"))
        .expect("vbnet extraction should succeed")
        .symbols
        .into_iter()
        .find(|symbol| symbol.name == "Hello" && symbol.kind == SymbolKind::Method)
        .expect("vbnet fixture should contain Hello method")
}

fn body_hash_for_language(source: &str, language: &str) -> String {
    body_hash(
        source,
        NormalizedSpan {
            start_line: 1,
            start_column: 0,
            end_line: source.lines().count() as u32,
            end_column: 0,
            start_byte: 0,
            end_byte: source.len() as u32,
        },
        language,
    )
    .expect("body hash should be computable")
}

fn rust_extractor(content: &str) -> BaseExtractor {
    BaseExtractor::new(
        "rust".to_string(),
        "test.rs".to_string(),
        content.to_string(),
        std::path::Path::new("/tmp/test"),
    )
}

fn parse_rust(content: &str) -> tree_sitter::Tree {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .expect("rust parser should load");
    parser
        .parse(content, None)
        .expect("rust fixture should parse")
}
