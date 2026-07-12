use super::*;
use std::path::PathBuf;

fn extract(code: &str) -> Vec<Symbol> {
    let mut parser = init_parser();
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
"#,
    );

    for name in [
        "NUnitFixture",
        "MsTestFixture",
        "XunitByMembers",
        "NUnitByMembers",
    ] {
        assert!(
            role(&symbols, name, "test_container"),
            "{name} must be a test container"
        );
    }
    for name in ["Before", "After", "FactCase", "TheoryCase", "TestCase"] {
        assert!(
            role(&symbols, name, "is_test"),
            "{name} must retain method test classification"
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
