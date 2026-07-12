use super::*;
use std::path::PathBuf;

fn extract(code: &str) -> Vec<Symbol> {
    let mut parser = init_parser();
    let tree = parser.parse(code, None).unwrap();
    let mut extractor = VbNetExtractor::new(
        "vbnet".to_string(),
        "ManagedTests.vb".to_string(),
        code.to_string(),
        &PathBuf::from("/tmp/test"),
    );
    extractor.extract_symbols(&tree)
}

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
fn dotnet_attributes_and_test_members_mark_vb_classes_as_test_containers() {
    let symbols = extract(
        r#"
<TestFixture>
Public Class NUnitFixture
    <SetUp> Public Sub Before()
    End Sub
    <TearDown> Public Sub After()
    End Sub
End Class
<TestClass>
Public Class MsTestFixture
End Class
Public Class XunitByMembers
    <Fact> Public Sub FactCase()
    End Sub
    <Theory> Public Sub TheoryCase()
    End Sub
End Class
Public Class NUnitByMembers
    <Test> Public Sub TestCase()
    End Sub
End Class
Public Class NUnitCasesByMembers
    <TestCase(1)> Public Sub ParameterizedCase(value As Integer)
    End Sub
End Class
Public Class OuterWithNested
    Public Class NestedWithCase
        <TestCase(1)> Public Sub NestedCase(value As Integer)
        End Sub
    End Class
End Class
"#,
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
fn vb_names_strings_and_unrelated_attributes_do_not_mark_containers() {
    let symbols = extract(
        r#"
<TestFixtureFactory>
Public Class Ordinary
    Public Text As String = "<TestFixture> <Fact>"
    Public Sub Fact()
    End Sub
End Class
"#,
    );
    assert!(!role(&symbols, "Ordinary", "test_container"));
}
