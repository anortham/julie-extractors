use super::extract_symbols;
use crate::base::Symbol;

fn metadata_bool(symbol: &Symbol, key: &str) -> bool {
    symbol
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}

#[test]
fn mocha_bdd_roles_require_external_loader_and_bdd_setup() {
    let source = r#"<!doctype html>
<html>
<head>
  <script src="https://unpkg.com/mocha/mocha.js"></script>
  <script>mocha.setup({ ui: "bdd" });</script>
</head>
<body>
  <script>
    describe("outer suite", () => {
      context("nested suite", () => {
        before(() => {});
        beforeEach(() => {});
        it("runs", () => {});
        afterEach(() => {});
        after(() => {});
        test("jest-shaped", () => {});
        it.skip("qualified", () => {});
      });
    });
  </script>
  <script>
    function describe() {}
    app.describe("member call", () => {});
  </script>
</body>
</html>"#;

    let symbols = extract_symbols(source);
    let outer = symbols
        .iter()
        .find(|symbol| symbol.name == "outer suite")
        .expect("Mocha describe call should be extracted");
    let nested = symbols
        .iter()
        .find(|symbol| symbol.name == "nested suite")
        .expect("Mocha context call should be extracted");
    let case_symbol = symbols
        .iter()
        .find(|symbol| symbol.name == "runs")
        .expect("Mocha it call should be extracted");

    assert!(metadata_bool(outer, "test_container"));
    assert!(metadata_bool(nested, "test_container"));
    assert!(metadata_bool(case_symbol, "is_test"));
    assert!(!metadata_bool(case_symbol, "test_lifecycle"));

    for lifecycle in ["before", "beforeEach", "afterEach", "after"] {
        let symbol = symbols
            .iter()
            .find(|symbol| symbol.name == lifecycle)
            .unwrap_or_else(|| panic!("missing Mocha lifecycle {lifecycle}"));
        assert!(metadata_bool(symbol, "is_test"));
        assert!(metadata_bool(symbol, "test_lifecycle"));
    }

    assert!(!symbols.iter().any(|symbol| symbol.name == "jest-shaped"));
    assert!(!symbols.iter().any(|symbol| symbol.name == "qualified"));
    let ordinary_describe = symbols
        .iter()
        .find(|symbol| symbol.name == "describe")
        .expect("ordinary declaration should remain a symbol");
    assert!(!metadata_bool(ordinary_describe, "test_container"));
}

#[test]
fn mocha_roles_are_suppressed_without_exact_bdd_contract() {
    for source in [
        r#"<script src="/mocha.js"></script><script>describe("suite", () => {}); it("case", () => {});</script>"#,
        r#"<script>mocha.setup("bdd"); describe("suite", () => {}); it("case", () => {});</script>"#,
        r#"<script src="/mocha.js"></script><script>mocha.setup("tdd"); describe("suite", () => {}); it("case", () => {});</script>"#,
    ] {
        let symbols = extract_symbols(source);
        assert!(!symbols.iter().any(|symbol| {
            metadata_bool(symbol, "is_test") || metadata_bool(symbol, "test_container")
        }));
    }
}
