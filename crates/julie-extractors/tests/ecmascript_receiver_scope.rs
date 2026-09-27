use std::path::Path;

use julie_extractors::{
    ExtractionLevel, ExtractionResults, Identifier, IdentifierKind,
    extract_canonical_for_language_at,
};

fn extract(language: &str, path: &str, source: &str) -> ExtractionResults {
    extract_canonical_for_language_at(
        language,
        path,
        source,
        Path::new("."),
        ExtractionLevel::Full,
    )
    .unwrap()
}

fn call<'a>(results: &'a ExtractionResults, name: &str) -> &'a Identifier {
    results
        .identifiers
        .iter()
        .find(|identifier| identifier.name == name && identifier.kind == IdentifierKind::Call)
        .unwrap_or_else(|| panic!("no call named {name:?}"))
}

#[test]
fn ordinary_functions_stop_inheriting_class_receiver_types() {
    let source = r#"
class Base {}
class Outer extends Base {
  constructor() { this.inConstructor(); }
  instance() {
    this.inMethod();
    const arrow = () => () => this.inNestedArrow();
    function ordinaryDeclaration() { this.inDeclaration(); this.missingFromDeclaration(); }
    const ordinaryExpression = function () { this.inExpression(); };
    function* ordinaryGeneratorDeclaration() { this.inGeneratorDeclaration(); }
    const ordinaryGeneratorExpression = function* () { this.inGeneratorExpression(); };
    const arrowInsideOrdinaryFunction = function () { return () => this.inOrdinaryArrow(); };
  }
  static staticMethod() { this.inStaticMethod(); super.inSuper(); }
  static staticField = () => this.inStaticField();
}
class Sibling { instance() { this.inSibling(); } }
Outer.prototype.prototypeMethod = function () { this.inPrototypeAssignment(); };
"#;

    let mut results_by_language = Vec::new();
    for (language, path) in [
        ("javascript", "sample.js"),
        ("jsx", "sample.jsx"),
        ("typescript", "sample.ts"),
        ("tsx", "sample.tsx"),
    ] {
        let results = extract(language, path, source);

        for name in [
            "inConstructor",
            "inMethod",
            "inNestedArrow",
            "inStaticMethod",
            "inStaticField",
            "inPrototypeAssignment",
        ] {
            assert_eq!(call(&results, name).receiver_type.as_deref(), Some("Outer"));
        }
        assert_eq!(
            call(&results, "inSibling").receiver_type.as_deref(),
            Some("Sibling")
        );

        for name in [
            "inDeclaration",
            "inExpression",
            "inGeneratorDeclaration",
            "inGeneratorExpression",
            "inOrdinaryArrow",
        ] {
            assert_eq!(
                call(&results, name).receiver_type,
                None,
                "{language}: {name}"
            );
        }

        if language == "javascript" || language == "jsx" {
            assert_eq!(
                call(&results, "inSuper").receiver_type.as_deref(),
                Some("Base")
            );
        }

        results_by_language.push((language, results));
    }

    for (language, results) in results_by_language {
        let pending = results
            .structured_pending_relationships
            .iter()
            .find(|pending| pending.target.terminal_name == "missingFromDeclaration")
            .unwrap_or_else(|| panic!("{language} has no pending declaration call"));
        assert_eq!(pending.target.receiver.as_deref(), Some("this"));
        assert_eq!(pending.receiver_type, None);
        let span = pending.span.unwrap();
        assert_eq!(
            &source[span.start_byte as usize..span.end_byte as usize],
            "this.missingFromDeclaration()"
        );
    }
}
