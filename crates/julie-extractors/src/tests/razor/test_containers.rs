use super::*;

fn role(symbols: &[Symbol], name: &str, key: &str) -> bool {
    symbols
        .iter()
        .find(|symbol| symbol.name == name)
        .unwrap()
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(serde_json::Value::as_bool)
        == Some(true)
}

#[test]
fn embedded_csharp_attributes_and_test_members_mark_classes_as_containers() {
    let symbols = extract_symbols(
        r#"@code {
    [TestFixture]
    public class NUnitFixture { [SetUp] public void Before() {} [TearDown] public void After() {} }
    [TestClass]
    public class MsTestFixture {}
    public class XunitByMembers { [Fact] public void FactCase() {} [Theory] public void TheoryCase() {} }
    public class NUnitByMembers { [Test] public void TestCase() {} }
    public class NUnitCasesByMembers { [TestCase(1)] public void ParameterizedCase(int value) {} }
    public class OuterWithNested { public class NestedWithCase { [TestCase(1)] public void NestedCase(int value) {} } }
}"#,
    );

    for name in [
        "NUnitFixture",
        "MsTestFixture",
        "XunitByMembers",
        "NUnitByMembers",
        "NUnitCasesByMembers",
        "NestedWithCase",
    ] {
        assert!(
            role(&symbols, name, "test_container"),
            "{name} must be a test container"
        );
    }
    assert!(!role(&symbols, "OuterWithNested", "test_container"));
    for name in [
        "Before",
        "After",
        "FactCase",
        "TheoryCase",
        "TestCase",
        "ParameterizedCase",
        "NestedCase",
    ] {
        assert!(
            role(&symbols, name, "is_test"),
            "{name} must retain method test classification"
        );
    }
}

#[test]
fn embedded_csharp_names_strings_and_unrelated_attributes_do_not_mark_containers() {
    let symbols = extract_symbols(
        r#"@code {
    [TestFixtureFactory]
    public class Ordinary { public string Text = "[TestFixture] [Fact]"; public void Fact() {} }
}"#,
    );
    assert!(!role(&symbols, "Ordinary", "test_container"));
}
