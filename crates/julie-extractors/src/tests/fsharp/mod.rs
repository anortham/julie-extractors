use crate::base::SymbolKind;
use crate::language::{detect_language_for_source, get_tree_sitter_language, language_spec};
use crate::pipeline::extract_canonical;
use std::collections::HashMap;
use std::path::Path;

#[test]
fn fsharp_extensions_select_one_artifact_language_case_insensitively() {
    for file_path in ["src/model.fs", "src/script.FSX", "src/api.FSI"] {
        assert_eq!(
            detect_language_for_source(file_path, ""),
            Some("fsharp"),
            "expected {file_path} to resolve to fsharp"
        );
    }
    for file_path in ["src/model.fsharp", "src/model.fsc", "src/model.txt"] {
        assert_eq!(
            detect_language_for_source(file_path, ""),
            None,
            "expected unsupported suffix {file_path} to remain unresolved"
        );
    }
}

#[test]
fn public_fsharp_parser_lookup_returns_the_implementation_grammar() {
    let language = get_tree_sitter_language("fsharp").expect("fsharp parser should be public");
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&language)
        .expect("parser language should load");
    let tree = parser
        .parse("module Values =\n  let answer = 42\n", None)
        .expect("implementation source should parse");
    assert!(!tree.root_node().has_error());
}

#[test]
fn fsharp_foundational_declarations_have_stable_structure_and_metadata() {
    let source = r#"/// Domain module
[<AutoOpen>]
module Domain =
  /// Person record
  [<Sealed>]
  type Person = {
    Name: string
    [<DefaultValue>]
    Age: int
  }

  type Shape =
    | Circle of radius: float
    | Empty

  type Calculator(value: int) =
    /// Value property
    [<Obsolete>]
    member _.Value = value
    static member Create() = Calculator(0)

  /// Answer value
  [<Literal>]
  let Answer: int = 42

  let add x y = x + y
"#;
    let first = extract_canonical("src/domain.fs", source, Path::new("/workspace"))
        .expect("valid F# should extract");
    let second = extract_canonical("src/domain.fs", source, Path::new("/workspace"))
        .expect("valid F# should extract deterministically");

    assert!(
        first.parse_diagnostics.is_empty(),
        "valid F# should parse cleanly: {:?}",
        first.parse_diagnostics
    );
    assert_eq!(first.symbols, second.symbols);
    assert_eq!(
        first
            .symbols
            .iter()
            .map(|symbol| symbol.name.as_str())
            .collect::<Vec<_>>(),
        [
            "Domain",
            "Person",
            "Name",
            "Age",
            "Shape",
            "Circle",
            "radius",
            "Empty",
            "Calculator",
            "Value",
            "Create",
            "Answer",
            "add",
        ]
    );
    assert!(
        first
            .symbols
            .iter()
            .all(|symbol| symbol.language == "fsharp")
    );
    let by_name: HashMap<_, _> = first
        .symbols
        .iter()
        .map(|symbol| (symbol.name.as_str(), symbol))
        .collect();
    assert_eq!(by_name["Domain"].kind, SymbolKind::Module);
    assert_eq!(by_name["Person"].kind, SymbolKind::Struct);
    assert_eq!(by_name["Name"].kind, SymbolKind::Field);
    assert_eq!(by_name["Shape"].kind, SymbolKind::Union);
    assert_eq!(by_name["Circle"].kind, SymbolKind::EnumMember);
    assert_eq!(by_name["Calculator"].kind, SymbolKind::Class);
    assert_eq!(by_name["Value"].kind, SymbolKind::Property);
    assert_eq!(by_name["Create"].kind, SymbolKind::Method);
    assert_eq!(by_name["Answer"].kind, SymbolKind::Variable);
    assert_eq!(by_name["add"].kind, SymbolKind::Function);
    for name in ["Person", "Calculator", "Answer", "add"] {
        assert!(
            by_name[name].body_hash.is_some(),
            "{name} should have a body hash"
        );
    }
    assert_eq!(
        by_name["Person"].parent_id,
        Some(by_name["Domain"].id.clone())
    );
    assert_eq!(
        by_name["Name"].parent_id,
        Some(by_name["Person"].id.clone())
    );
    assert_eq!(by_name["Age"].parent_id, Some(by_name["Person"].id.clone()));
    assert_eq!(
        by_name["Value"].parent_id,
        Some(by_name["Calculator"].id.clone())
    );
    assert_eq!(
        by_name["Create"].parent_id,
        Some(by_name["Calculator"].id.clone())
    );
    assert_eq!(
        by_name["Domain"].doc_comment.as_deref(),
        Some("Domain module")
    );
    assert_eq!(
        by_name["Person"].doc_comment.as_deref(),
        Some("Person record")
    );
    assert_eq!(
        by_name["Answer"].doc_comment.as_deref(),
        Some("Answer value")
    );
    assert_eq!(
        by_name["Value"].doc_comment.as_deref(),
        Some("Value property")
    );
    assert_eq!(by_name["Domain"].annotations[0].annotation_key, "autoopen");
    assert_eq!(by_name["Person"].annotations[0].annotation_key, "sealed");
    assert_eq!(by_name["Answer"].annotations[0].annotation_key, "literal");
    assert_eq!(by_name["Value"].annotations[0].annotation_key, "obsolete");
    assert_eq!(by_name["Age"].annotations[0].annotation_key, "defaultvalue");
    assert!(by_name["Domain"].start_byte < by_name["Domain"].end_byte);
    assert!(by_name["Person"].start_byte < by_name["Person"].end_byte);
}

#[test]
fn fsharp_signature_files_use_signature_grammar_and_same_language_identity() {
    let source = r#"namespace Domain
type Person = { Name: string; Age: int }
module Values =
  val Answer : int
  val Add : int -> int -> int
"#;
    let results = extract_canonical("src/api.fsi", source, Path::new("/workspace"))
        .expect("signature F# should extract");
    assert!(
        results.parse_diagnostics.is_empty(),
        "valid F# signature should parse cleanly: {:?}",
        results.parse_diagnostics
    );
    assert!(results.symbols.iter().any(|symbol| symbol.name == "Domain"));
    assert!(results.symbols.iter().any(|symbol| symbol.name == "Person"));
    assert!(results.symbols.iter().any(|symbol| symbol.name == "Answer"));
    assert!(results.symbols.iter().any(|symbol| symbol.name == "Add"));
    assert!(
        results
            .symbols
            .iter()
            .all(|symbol| symbol.language == "fsharp")
    );
}

#[test]
fn malformed_fsharp_declarations_keep_parse_diagnostics() {
    let source = "module Broken =\n  type Person = { Name: string\n";
    let results = extract_canonical("src/broken.fs", source, Path::new("/workspace"))
        .expect("malformed F# should produce degraded extraction results");
    assert!(
        !results.parse_diagnostics.is_empty(),
        "malformed F# should retain parser diagnostics"
    );
}

#[test]
fn fsharp_language_spec_advertises_foundational_symbols() {
    let spec = language_spec("fsharp").expect("fsharp language spec should be registered");
    assert_eq!(spec.extensions, &["fs", "fsx", "fsi"]);
    assert_eq!(spec.parser_crate, "tree-sitter-fsharp");
    assert!(spec.capabilities.symbols);
}
