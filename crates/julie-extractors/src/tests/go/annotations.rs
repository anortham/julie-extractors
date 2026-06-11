use crate::base::SymbolKind;
use crate::go::GoExtractor;
use crate::tests::helpers::init_parser;
use std::path::PathBuf;

#[test]
fn field_tags_emit_per_key_annotation_markers() {
    let code = r#"
package main

type Worker struct {
    ID int `json:"id" db:"worker_id"`
}
"#;
    let tree = init_parser(code, "go");
    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = GoExtractor::new(
        "go".to_string(),
        "test.go".to_string(),
        code.to_string(),
        &workspace_root,
    );
    let symbols = extractor.extract_symbols(&tree);

    let id_field = symbols
        .iter()
        .find(|symbol| symbol.name == "ID" && symbol.kind == SymbolKind::Field)
        .expect("ID field");
    let keys: Vec<_> = id_field
        .annotations
        .iter()
        .map(|marker| marker.annotation_key.as_str())
        .collect();
    assert_eq!(keys, vec!["json", "db"]);

    let worker = symbols
        .iter()
        .find(|symbol| symbol.name == "Worker" && symbol.kind == SymbolKind::Struct)
        .expect("Worker struct");
    assert_eq!(worker.annotations.len(), 1);
    assert_eq!(worker.annotations[0].annotation_key, "field_tags");
}

#[test]
fn compiler_directives_attach_to_functions_not_doc_comments() {
    let code = r#"
package main

//go:noinline
func Evaluate(count int) int {
    return count
}

// helper does work.
func helper(count int) int {
    return count
}
"#;
    let tree = init_parser(code, "go");
    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = GoExtractor::new(
        "go".to_string(),
        "test.go".to_string(),
        code.to_string(),
        &workspace_root,
    );
    let symbols = extractor.extract_symbols(&tree);

    let evaluate = symbols
        .iter()
        .find(|symbol| symbol.name == "Evaluate")
        .expect("Evaluate");
    assert_eq!(evaluate.annotations.len(), 1);
    assert_eq!(evaluate.annotations[0].annotation_key, "noinline");
    assert_eq!(evaluate.annotations[0].annotation, "go:noinline");
    assert!(evaluate.doc_comment.is_none());

    let helper = symbols
        .iter()
        .find(|symbol| symbol.name == "helper")
        .expect("helper");
    assert!(helper.annotations.is_empty());
    assert!(helper.doc_comment.is_some());
}
