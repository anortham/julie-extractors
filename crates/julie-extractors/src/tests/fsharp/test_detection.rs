use crate::base::{Symbol, SymbolKind};
use crate::pipeline::extract_canonical;
use serde_json::Value;
use std::path::Path;

fn symbols(source: &str) -> Vec<Symbol> {
    extract_canonical("tests/xunit.fs", source, Path::new("/workspace"))
        .expect("valid F# should extract")
        .symbols
}

fn named<'a>(symbols: &'a [Symbol], name: &str) -> &'a Symbol {
    symbols
        .iter()
        .find(|symbol| symbol.name == name)
        .unwrap_or_else(|| panic!("missing symbol {name}"))
}

fn metadata_value<'a>(symbol: &'a Symbol, key: &str) -> Option<&'a Value> {
    symbol.metadata.as_ref()?.get(key)
}

fn role(symbol: &Symbol, key: &str) -> bool {
    metadata_value(symbol, key) == Some(&Value::Bool(true))
}

fn test_role(symbol: &Symbol) -> Option<&str> {
    metadata_value(symbol, "test_role").and_then(Value::as_str)
}

#[test]
fn xunit_fact_and_theory_functions_emit_their_test_roles() {
    let symbols = symbols(
        r#"open Xunit

[<Fact>]
let adds_numbers() = 1 + 1

[<Theory>]
let adds_numbers_from_data(value: int) = value + 1
"#,
    );

    let fact = named(&symbols, "adds_numbers");
    assert_eq!(fact.kind, SymbolKind::Function);
    assert_eq!(fact.annotations[0].annotation_key, "fact");
    assert!(role(fact, "is_test"));
    assert!(!role(fact, "test_lifecycle"));
    assert!(!role(fact, "test_container"));
    assert_eq!(test_role(fact), Some("test_case"));

    let theory = named(&symbols, "adds_numbers_from_data");
    assert_eq!(theory.kind, SymbolKind::Function);
    assert_eq!(theory.annotations[0].annotation_key, "theory");
    assert!(role(theory, "is_test"));
    assert!(!role(theory, "test_lifecycle"));
    assert!(!role(theory, "test_container"));
    assert_eq!(test_role(theory), Some("parameterized_test"));
}

#[test]
fn qualified_xunit_attributes_keep_their_normalized_marker_and_role() {
    let symbols = symbols(
        r#"[<Xunit.Fact>]
let qualified_fact() = ()

[<Xunit.Theory>]
let qualified_theory() = ()
"#,
    );

    let fact = named(&symbols, "qualified_fact");
    assert_eq!(fact.annotations[0].annotation_key, "xunit.fact");
    assert_eq!(test_role(fact), Some("test_case"));
    assert!(role(fact, "is_test"));

    let theory = named(&symbols, "qualified_theory");
    assert_eq!(theory.annotations[0].annotation_key, "xunit.theory");
    assert_eq!(test_role(theory), Some("parameterized_test"));
    assert!(role(theory, "is_test"));
}

#[test]
fn xunit_detection_keeps_similar_names_noncallables_and_unannotated_functions_silent() {
    let symbols = symbols(
        r#"[<FactLike>]
let fact_like() = ()

[<TheoryCase>]
let theory_case() = ()

[<Fact>]
type FactContainer = { Value: int }

let test_looks_like_a_test() = ()

[<Test>]
let nunit_case() = ()

[<TestFixture>]
type NUnitContainer = class end
"#,
    );

    for name in [
        "fact_like",
        "theory_case",
        "FactContainer",
        "test_looks_like_a_test",
        "nunit_case",
        "NUnitContainer",
    ] {
        let symbol = named(&symbols, name);
        assert_eq!(test_role(symbol), None, "{name} must have no test role");
        assert!(!role(symbol, "is_test"), "{name} must not be is_test");
        assert!(
            !role(symbol, "test_lifecycle"),
            "{name} must not be lifecycle"
        );
        assert!(
            !role(symbol, "test_container"),
            "{name} must not be a container"
        );
    }
}
