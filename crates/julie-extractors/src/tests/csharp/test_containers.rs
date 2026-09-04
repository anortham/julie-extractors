use super::*;
use std::path::PathBuf;

fn extract(code: &str) -> Vec<Symbol> {
    let mut parser = init_test_parser();
    let tree = parser.parse(code, None).unwrap();
    let mut extractor = CSharpExtractor::new(
        "csharp".to_string(),
        "ManagedTests.cs".to_string(),
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

fn container_id(symbols: &[Symbol], name: &str) -> String {
    symbols
        .iter()
        .find(|symbol| {
            symbol.name == name && matches!(symbol.kind, SymbolKind::Class | SymbolKind::Struct)
        })
        .unwrap_or_else(|| panic!("{name} must be extracted as a class or struct"))
        .id
        .clone()
}

fn member(symbols: &[Symbol], container: &str, member: &str) -> Symbol {
    let parent = container_id(symbols, container);
    symbols
        .iter()
        .find(|symbol| {
            symbol.parent_id.as_deref() == Some(parent.as_str()) && symbol.name == member
        })
        .unwrap_or_else(|| panic!("{container}.{member} must be extracted"))
        .clone()
}

fn member_role(symbols: &[Symbol], container: &str, name: &str) -> Option<String> {
    member(symbols, container, name)
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("test_role"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

fn container_role(symbols: &[Symbol], name: &str) -> Option<String> {
    let id = container_id(symbols, name);
    symbols
        .iter()
        .find(|symbol| symbol.id == id)
        .unwrap()
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("test_role"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

#[test]
fn dotnet_attributes_and_test_members_mark_classes_as_test_containers() {
    let symbols = extract(
        r#"
[TestFixture]
public class NUnitFixture { [SetUp] public void Before() {} [TearDown] public void After() {} }
[TestClass]
public class MsTestFixture {}
public class XunitByMembers { [Fact] public void FactCase() {} [Theory] public void TheoryCase() {} }
public class NUnitByMembers { [Test] public void TestCase() {} }
public class NUnitCasesByMembers { [TestCase(1)] public void ParameterizedCase(int value) {} }
public class OuterWithNested { public class NestedWithCase { [TestCase(1)] public void NestedCase(int value) {} } }
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
    for name in ["Before", "After"] {
        assert!(
            role(&symbols, name, "test_lifecycle"),
            "{name} must be classified as test_lifecycle"
        );
    }
    for name in [
        "FactCase",
        "TheoryCase",
        "TestCase",
        "ParameterizedCase",
        "NestedCase",
    ] {
        assert!(
            !role(&symbols, name, "test_lifecycle"),
            "{name} must not be classified as test_lifecycle"
        );
    }
}

#[test]
fn names_comments_strings_and_unrelated_attributes_do_not_mark_containers() {
    let symbols = extract(
        r#"
[TestFixtureFactory]
public class Ordinary { public string Text = "[TestFixture] [Fact]"; public void Fact() {} }
// [TestClass]
public class CommentOnly {}
"#,
    );
    assert!(!role(&symbols, "Ordinary", "test_container"));
    assert!(!role(&symbols, "CommentOnly", "test_container"));
}

#[test]
fn xunit_lifecycle_members_are_scoped_to_marked_test_containers() {
    let symbols = extract(
        r#"
public class XunitFixture : IDisposable, IAsyncLifetime
{
    public XunitFixture() {}
    public Task InitializeAsync() => Task.CompletedTask;
    public void Dispose() {}
    public ValueTask DisposeAsync() => default;
    [Fact] public void FactCase() {}
}
public class PlainResource : IDisposable
{
    public PlainResource() {}
    public Task InitializeAsync() => Task.CompletedTask;
    public void Dispose() {}
    public ValueTask DisposeAsync() => default;
}
"#,
    );

    for (name, expected) in [
        ("XunitFixture", "fixture_setup"),
        ("InitializeAsync", "fixture_setup"),
        ("Dispose", "fixture_teardown"),
        ("DisposeAsync", "fixture_teardown"),
    ] {
        assert_eq!(
            member_role(&symbols, "XunitFixture", name).as_deref(),
            Some(expected)
        );
    }
    assert_eq!(
        member_role(&symbols, "XunitFixture", "FactCase").as_deref(),
        Some("test_case")
    );

    for name in [
        "PlainResource",
        "InitializeAsync",
        "Dispose",
        "DisposeAsync",
    ] {
        assert_eq!(member_role(&symbols, "PlainResource", name), None);
    }
    assert!(!role(&symbols, "PlainResource", "test_container"));
}

#[test]
fn dotnet_data_driven_attributes_carry_the_parameterized_test_role() {
    let symbols = extract(
        r#"
public class DataDriven
{
    [Theory] public void XunitTheory(int value) {}
    [DataTestMethod] public void MsTestData(int value) {}
    [TestCase(1)] public void NUnitCase(int value) {}
    [TestCaseSource(nameof(Cases))] public void NUnitCaseSource(int value) {}
    [Fact] public void XunitFact() {}
    [Test] public void NUnitTest() {}
    [TestMethod] public void MsTestMethod() {}
}
"#,
    );

    for name in ["XunitTheory", "MsTestData", "NUnitCase", "NUnitCaseSource"] {
        assert_eq!(
            member_role(&symbols, "DataDriven", name).as_deref(),
            Some("parameterized_test"),
            "{name} must carry the parameterized_test role"
        );
    }
    for name in ["XunitFact", "NUnitTest", "MsTestMethod"] {
        assert_eq!(
            member_role(&symbols, "DataDriven", name).as_deref(),
            Some("test_case"),
            "{name} must stay a plain test case"
        );
    }
    assert!(role(&symbols, "DataDriven", "test_container"));
}

#[test]
fn assembly_level_dotnet_hooks_carry_fixture_roles() {
    let symbols = extract(
        r#"
[TestClass]
public class AssemblyHooks
{
    [AssemblyInitialize] public static void Boot(TestContext context) {}
    [AssemblyCleanup] public static void Shutdown() {}
}
"#,
    );

    assert_eq!(
        member_role(&symbols, "AssemblyHooks", "Boot").as_deref(),
        Some("fixture_setup")
    );
    assert_eq!(
        member_role(&symbols, "AssemblyHooks", "Shutdown").as_deref(),
        Some("fixture_teardown")
    );
}

#[test]
fn xunit_and_nunit_container_attributes_mark_test_containers() {
    let symbols = extract(
        r#"
[CollectionDefinition("db")]
public class DatabaseCollection {}
[SetUpFixture]
public class AssemblySetup { [OneTimeSetUp] public void Boot() {} }
[TestFixtureSource(nameof(Cases))]
public class ParameterizedFixture { [Test] public void Works() {} }
"#,
    );

    for name in [
        "DatabaseCollection",
        "AssemblySetup",
        "ParameterizedFixture",
    ] {
        assert_eq!(
            container_role(&symbols, name).as_deref(),
            Some("test_container"),
            "{name} must be a test container"
        );
    }
}

#[test]
fn struct_test_classes_are_marked_as_test_containers() {
    let symbols = extract(
        r#"
[TestFixture]
public struct StructFixture { [Test] public void Works() {} }
public record struct RecordStructFixture { [Fact] public void Works() {} }
public struct PlainStruct { public void Works() {} }
"#,
    );

    for name in ["StructFixture", "RecordStructFixture"] {
        assert_eq!(
            container_role(&symbols, name).as_deref(),
            Some("test_container"),
            "{name} must be a test container"
        );
    }
    assert_eq!(container_role(&symbols, "PlainStruct"), None);
}
