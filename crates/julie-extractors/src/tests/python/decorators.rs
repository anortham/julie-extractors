// Python decorators inline tests extracted from extractors/python/decorators.rs

use crate::base::{Symbol, SymbolKind};
use crate::python::PythonExtractor;
use std::path::PathBuf;

fn extract(file_path: &str, code: &str) -> Vec<Symbol> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .expect("failed to load Python grammar");
    let tree = parser.parse(code, None).expect("failed to parse Python");
    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor =
        PythonExtractor::new(file_path.to_string(), code.to_string(), &workspace_root);
    extractor.extract_symbols(&tree)
}

fn annotation_keys(symbol: &Symbol) -> Vec<String> {
    symbol
        .annotations
        .iter()
        .map(|annotation| annotation.annotation_key.clone())
        .collect()
}

fn test_role(symbol: &Symbol) -> Option<String> {
    symbol
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("test_role"))
        .and_then(|value| value.as_str())
        .map(str::to_string)
}

#[test]
fn nested_function_does_not_inherit_the_enclosing_decorators() {
    let symbols = extract(
        "src/service.py",
        r#"
@pytest.fixture
def build_client():
    def make(url):
        return url
    return make
"#,
    );

    let inner = symbols
        .iter()
        .find(|symbol| symbol.name == "make")
        .expect("nested function should be extracted");

    assert!(
        annotation_keys(inner).is_empty(),
        "nested `make` must carry no decorator of its own, got {:?}",
        inner.annotations
    );
    assert_eq!(
        test_role(inner),
        None,
        "nested `make` must carry no test role, got {inner:?}"
    );
}

#[test]
fn nested_function_inside_a_decorated_test_is_not_itself_a_test() {
    let symbols = extract(
        "src/service.py",
        r#"
@pytest.mark.parametrize("value", [1, 2])
def test_value(value):
    def helper():
        return value
    assert helper() == value
"#,
    );

    let outer = symbols
        .iter()
        .find(|symbol| symbol.name == "test_value")
        .expect("decorated test should be extracted");
    assert_eq!(
        test_role(outer).as_deref(),
        Some("parameterized_test"),
        "the decorated function keeps its own role, got {outer:?}"
    );

    let helper = symbols
        .iter()
        .find(|symbol| symbol.name == "helper")
        .expect("nested helper should be extracted");
    assert!(
        annotation_keys(helper).is_empty(),
        "nested `helper` must not inherit the parametrize decorator, got {:?}",
        helper.annotations
    );
    assert_eq!(
        test_role(helper),
        None,
        "nested `helper` must carry no test role, got {helper:?}"
    );
}

#[test]
fn method_inside_a_decorated_class_does_not_inherit_the_class_decorator() {
    let symbols = extract(
        "src/models.py",
        r#"
@dataclass
class User:
    id: int

    def rename(self, name):
        self.name = name
"#,
    );

    let rename = symbols
        .iter()
        .find(|symbol| symbol.name == "rename")
        .expect("method should be extracted");
    assert!(
        annotation_keys(rename).is_empty(),
        "`rename` must not inherit @dataclass, got {:?}",
        rename.annotations
    );
}

#[test]
fn python_decorator_markers_persist_for_functions_and_classes() {
    let code = r#"
@app.route("/users/<id>")
def show_user(id):
    return id

@pytest.mark.parametrize("value", [1, 2])
def test_value(value):
    assert value

@dataclass
class User:
    id: int
"#;

    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .expect("failed to load Python grammar");
    let tree = parser.parse(code, None).expect("failed to parse Python");

    let workspace_root = PathBuf::from("/tmp/test");
    let mut extractor = PythonExtractor::new(
        "test_routes.py".to_string(),
        code.to_string(),
        &workspace_root,
    );
    let symbols = extractor.extract_symbols(&tree);

    let show_user = symbols
        .iter()
        .find(|s| s.name == "show_user" && s.kind == SymbolKind::Function)
        .expect("show_user function should be extracted");
    assert_eq!(show_user.annotations.len(), 1);
    assert_eq!(show_user.annotations[0].annotation, "app.route");
    assert_eq!(show_user.annotations[0].annotation_key, "app.route");
    assert_eq!(
        show_user.annotations[0].raw_text.as_deref(),
        Some("app.route(\"/users/<id>\")")
    );
    assert_eq!(show_user.annotations[0].carrier, None);
    assert!(
        show_user
            .signature
            .as_deref()
            .unwrap_or_default()
            .contains("@app.route def show_user")
    );

    let test_value = symbols
        .iter()
        .find(|s| s.name == "test_value" && s.kind == SymbolKind::Function)
        .expect("test_value function should be extracted");
    assert_eq!(test_value.annotations.len(), 1);
    assert_eq!(
        test_value.annotations[0].annotation,
        "pytest.mark.parametrize"
    );
    assert_eq!(
        test_value.annotations[0].annotation_key,
        "pytest.mark.parametrize"
    );
    assert_eq!(
        test_value.annotations[0].raw_text.as_deref(),
        Some("pytest.mark.parametrize(\"value\", [1, 2])")
    );
    assert_eq!(
        test_value
            .metadata
            .as_ref()
            .and_then(|m| m.get("is_test"))
            .and_then(|v| v.as_bool()),
        Some(true)
    );

    let user = symbols
        .iter()
        .find(|s| s.name == "User" && s.kind == SymbolKind::Class)
        .expect("User class should be extracted");
    assert_eq!(user.annotations.len(), 1);
    assert_eq!(user.annotations[0].annotation, "dataclass");
    assert_eq!(user.annotations[0].annotation_key, "dataclass");
    assert_eq!(user.annotations[0].raw_text.as_deref(), Some("dataclass"));
    assert!(
        user.signature
            .as_deref()
            .unwrap_or_default()
            .contains("@dataclass class User")
    );
}
