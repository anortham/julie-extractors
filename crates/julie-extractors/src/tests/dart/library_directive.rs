use crate::base::{ExtractionResults, Symbol, SymbolKind};
use crate::extract_canonical;
use std::path::Path;

fn extract(path: &str, code: &str) -> ExtractionResults {
    extract_canonical(path, code, Path::new("/tmp/test")).expect("dart extraction")
}

fn named<'a>(result: &'a ExtractionResults, name: &str) -> Vec<&'a Symbol> {
    result.symbols.iter().filter(|s| s.name == name).collect()
}

fn library<'a>(result: &'a ExtractionResults, name: &str) -> &'a Symbol {
    let found = named(result, name);
    assert_eq!(found.len(), 1, "{name}: {:#?}", result.symbols);
    let symbol = found[0];
    assert_eq!(symbol.kind, SymbolKind::Namespace, "{symbol:#?}");
    assert_eq!(
        symbol.metadata.as_ref().and_then(|m| m.get("type")),
        Some(&serde_json::json!("library")),
        "{symbol:#?}"
    );
    symbol
}

#[test]
fn library_directive_before_a_declaration_is_a_library_namespace() {
    let result = extract(
        "lib/post.dart",
        "/// The post library.\nlibrary post;\n\nclass Post {}\n",
    );
    let post = library(&result, "post");
    assert_eq!(post.doc_comment.as_deref(), Some("/// The post library."));
    assert_eq!(post.signature.as_deref(), Some("library post;"));
    assert_eq!(post.start_line, 2);
    assert_eq!(post.parent_id, None);
    assert!(
        named(&result, "Post")
            .iter()
            .any(|s| s.kind == SymbolKind::Class)
    );
}

#[test]
fn library_directive_reads_the_same_with_a_comment_an_annotation_or_nothing_before_it() {
    for (code, doc) in [
        ("library post;\n\nclass Post {}\n", None),
        ("// plain\nlibrary post;\n\nvoid main() {}\n", None),
        ("@deprecated\nlibrary post;\nclass Post {}\n", None),
        (
            "/// Docs.\n@deprecated\nlibrary post;\nint count = 0;\n",
            Some("/// Docs."),
        ),
        ("/// Docs.\nlibrary post;\n", Some("/// Docs.")),
    ] {
        let result = extract("lib/post.dart", code);
        let post = library(&result, "post");
        assert_eq!(post.doc_comment.as_deref(), doc, "{code}");
        assert!(
            result.identifiers.iter().all(|i| i.name != "library"),
            "{code}: {:#?}",
            result.identifiers
        );
        assert!(
            result.types.values().all(|t| t.resolved_type != "library"),
            "{code}: {:#?}",
            result.types
        );
        assert!(
            result
                .pending_relationships
                .iter()
                .all(|p| p.callee_name != "library"),
            "{code}: {:#?}",
            result.pending_relationships
        );
        assert!(
            result
                .structured_pending_relationships
                .iter()
                .all(|p| p.target.terminal_name != "library"),
            "{code}"
        );
    }
}

#[test]
fn dotted_library_names_keep_every_segment() {
    for code in [
        "/// Dotted.\nlibrary a.b.c;\n\nclass Post {}\n",
        "library a.b.c;\nimport 'x.dart';\n",
    ] {
        let result = extract("lib/abc.dart", code);
        library(&result, "a.b.c");
    }
}

#[test]
fn unnamed_library_directive_writes_no_symbol() {
    for code in [
        "library;\n",
        "/// Unnamed.\nlibrary;\nclass Post {}\n",
        "@deprecated\nlibrary;\n",
    ] {
        let result = extract("lib/unnamed.dart", code);
        assert!(
            result
                .symbols
                .iter()
                .all(|s| s.kind != SymbolKind::Namespace && s.kind != SymbolKind::Variable),
            "{code}: {:#?}",
            result.symbols
        );
    }
}

#[test]
fn ordinary_top_level_variables_stay_variables() {
    let result = extract(
        "lib/vars.dart",
        "library vars;\n\nLibrary library = Library();\nString name;\nfinal libraryName = 'x';\n",
    );
    library(&result, "vars");
    for name in ["library", "name", "libraryName"] {
        let found = named(&result, name);
        assert_eq!(found.len(), 1, "{name}: {:#?}", result.symbols);
        assert!(
            matches!(found[0].kind, SymbolKind::Variable | SymbolKind::Constant),
            "{name}: {:#?}",
            found[0]
        );
    }
}
