use crate::rust::RustExtractor;
use std::path::PathBuf;

fn inferred_type(source: &str, local: &str) -> Option<(String, bool)> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let mut extractor = RustExtractor::new(
        "rust".to_string(),
        "initializer_types.rs".to_string(),
        source.to_string(),
        &PathBuf::from("/tmp/test"),
    );
    let symbols = extractor.extract_symbols(&tree);
    let local = symbols
        .iter()
        .find(|s| s.name == local)
        .unwrap_or_else(|| panic!("missing local {local}"));
    extractor
        .base
        .type_info
        .get(&local.id)
        .map(|fact| (fact.resolved_type.clone(), fact.is_inferred))
}

fn in_run(prelude: &str, body: &str) -> String {
    format!("{prelude}\nfn run() -> Result<(), Error> {{\n    {body}\n    Ok(())\n}}\n")
}

const LOADERS: &str = r#"
struct Workspace;
fn resolve_root(input: &str) -> Result<Workspace, WorkspaceError> { todo!() }
fn find_root(input: &str) -> Option<Workspace> { todo!() }
fn plain_root() -> Workspace { todo!() }
"#;

fn workspace_type(body: &str) -> Option<(String, bool)> {
    inferred_type(&in_run(LOADERS, body), "workspace")
}

#[test]
fn try_on_same_file_result_call_records_ok_type_as_inferred() {
    assert_eq!(
        workspace_type("let workspace = resolve_root(input)?;"),
        Some(("Workspace".to_string(), true))
    );
}

#[test]
fn try_on_same_file_option_call_records_some_type() {
    assert_eq!(
        workspace_type("let workspace = find_root(input)?;"),
        Some(("Workspace".to_string(), true))
    );
}

#[test]
fn map_err_then_try_records_ok_type() {
    assert_eq!(
        workspace_type("let workspace = resolve_root(input)\n        .map_err(|e| wrap(e))?;"),
        Some(("Workspace".to_string(), true))
    );
}

#[test]
fn unwrapping_methods_record_inner_type() {
    for chain in [
        "resolve_root(input).unwrap()",
        "resolve_root(input).expect(\"root\")",
        "resolve_root(input).unwrap_or_else(|_| fallback())",
        "find_root(input).unwrap_or(fallback())",
        "find_root(input).unwrap_or_default()",
    ] {
        assert_eq!(
            workspace_type(&format!("let workspace = {chain};")),
            Some(("Workspace".to_string(), true)),
            "{chain}"
        );
    }
}

#[test]
fn pass_through_methods_keep_the_wrapper_for_try() {
    for chain in [
        "resolve_root(input).map_err(wrap)?",
        "find_root(input).ok_or(Error::Missing)?",
        "find_root(input).ok_or_else(|| Error::Missing)?",
        "resolve_root(input).context(\"root\")?",
        "find_root(input).with_context(|| \"root\")?",
        "find_root(input).ok_or(Error::Missing).context(\"root\").unwrap()",
    ] {
        assert_eq!(
            workspace_type(&format!("let workspace = {chain};")),
            Some(("Workspace".to_string(), true)),
            "{chain}"
        );
    }
}

#[test]
fn unwrapped_call_records_declared_return_type() {
    assert_eq!(
        workspace_type("let workspace = plain_root();"),
        Some(("Workspace".to_string(), true))
    );
    assert_eq!(
        workspace_type("let workspace = resolve_root(input);"),
        Some(("Result".to_string(), true))
    );
}

#[test]
fn chain_ending_in_other_method_records_no_fact() {
    for chain in [
        "resolve_root(input).ok()",
        "resolve_root(input)?.clone()",
        "find_root(input).map(|w| w)?",
        "resolve_root(input).and_then(check)?",
        "plain_root().into()",
    ] {
        assert_eq!(
            workspace_type(&format!("let workspace = {chain};")),
            None,
            "{chain}"
        );
    }
}

#[test]
fn unwrapping_a_non_wrapper_records_no_fact() {
    for chain in [
        "plain_root()?",
        "plain_root().unwrap()",
        "plain_root().map_err(wrap)?",
    ] {
        assert_eq!(
            workspace_type(&format!("let workspace = {chain};")),
            None,
            "{chain}"
        );
    }
}

#[test]
fn call_to_function_outside_the_file_records_no_fact() {
    assert_eq!(
        workspace_type("let workspace = load_elsewhere(input)?;"),
        None
    );
    assert_eq!(
        workspace_type("let workspace = paths::resolve_root(input)?;"),
        None
    );
}

#[test]
fn same_named_functions_with_different_return_types_record_no_fact() {
    let source = in_run(
        r#"
fn load() -> Result<Workspace, Error> { todo!() }
mod other {
    fn load() -> Result<Project, Error> { todo!() }
}
"#,
        "let workspace = load()?;",
    );
    assert_eq!(inferred_type(&source, "workspace"), None);
}

#[test]
fn same_named_functions_that_agree_record_the_type() {
    let source = in_run(
        r#"
#[cfg(unix)]
fn load() -> Result<Workspace, Error> { todo!() }
#[cfg(windows)]
fn load() -> Result<Workspace, Error> { todo!() }
"#,
        "let workspace = load()?;",
    );
    assert_eq!(
        inferred_type(&source, "workspace"),
        Some(("Workspace".to_string(), true))
    );
}

#[test]
fn generic_or_unit_return_records_no_fact() {
    let source = in_run(
        r#"
fn parse<T>(text: &str) -> Result<T, Error> { todo!() }
fn touch() {}
"#,
        "let workspace = parse(text)?;\n    let touched = touch();",
    );
    assert_eq!(inferred_type(&source, "workspace"), None);
    assert_eq!(inferred_type(&source, "touched"), None);
}

#[test]
fn free_call_does_not_match_a_method_of_the_same_name() {
    let source = in_run(
        r#"
impl Registry {
    fn load(&self) -> Result<Workspace, Error> { todo!() }
}
"#,
        "let workspace = load()?;",
    );
    assert_eq!(inferred_type(&source, "workspace"), None);
}

#[test]
fn associated_function_call_records_its_return_type() {
    let source = in_run(
        r#"
impl Workspace {
    fn open(path: &str) -> Result<Self, Error> { todo!() }
}
impl Project {
    fn open(path: &str) -> Result<Self, Error> { todo!() }
}
"#,
        "let workspace = Workspace::open(path)?;",
    );
    assert_eq!(
        inferred_type(&source, "workspace"),
        Some(("Workspace".to_string(), true))
    );
}

#[test]
fn new_call_uses_same_file_declaration_before_constructor_fallback() {
    let source = in_run(
        r#"
impl Workspace {
    fn new(path: &str) -> Result<Self, Error> { todo!() }
}
"#,
        "let workspace = Workspace::new(path)?;\n    let external = Project::new();",
    );
    assert_eq!(
        inferred_type(&source, "workspace"),
        Some(("Workspace".to_string(), true))
    );
    assert_eq!(
        inferred_type(&source, "external"),
        Some(("Project".to_string(), true))
    );
}

#[test]
fn try_on_constructor_fallback_records_no_fact() {
    assert_eq!(workspace_type("let workspace = Project::new()?;"), None);
}

#[test]
fn self_constructors_inside_impl_record_the_impl_type() {
    let source = r#"
impl Workspace {
    fn fork(&self) -> Self {
        let workspace = Self::new();
        let copy = Self { root: self.root.clone() };
        let loaded = Self::load(path).unwrap();
        copy
    }
    fn load(path: &str) -> Option<Self> { todo!() }
}
"#;
    for local in ["workspace", "copy", "loaded"] {
        assert_eq!(
            inferred_type(source, local),
            Some(("Workspace".to_string(), true)),
            "{local}"
        );
    }
}

#[test]
fn self_constructors_outside_impl_record_no_fact() {
    let source = r#"
trait Fork {
    fn fork(&self) {
        let workspace = Self::new();
        let copy = Self { root: 1 };
    }
}
"#;
    assert_eq!(inferred_type(source, "workspace"), None);
    assert_eq!(inferred_type(source, "copy"), None);
}

#[test]
fn self_method_call_records_the_impl_method_return_type() {
    let source = r#"
impl Server {
    fn handle(&self) -> Result<(), Error> {
        let workspace = self.workspace_for(path)?;
        let other = registry.workspace_for(path)?;
        Ok(())
    }
    fn workspace_for(&self, path: &str) -> Result<Workspace, Error> { todo!() }
}
impl Registry {
    fn workspace_for(&self, path: &str) -> Result<Project, Error> { todo!() }
}
"#;
    assert_eq!(
        inferred_type(source, "workspace"),
        Some(("Workspace".to_string(), true))
    );
    assert_eq!(inferred_type(source, "other"), None);
}

#[test]
fn trait_methods_are_not_return_type_sources() {
    let source = r#"
trait Loader {
    fn load(&self) -> Result<Workspace, Error>;
    fn fetch(&self) -> Result<Workspace, Error> { todo!() }
}
fn handle() {
    let workspace = fetch().unwrap();
}
"#;
    assert_eq!(inferred_type(source, "workspace"), None);
}

#[test]
fn written_type_wins_over_initializer() {
    let source = in_run(LOADERS, "let workspace: Project = resolve_root(input)?;");
    assert_eq!(
        inferred_type(&source, "workspace"),
        Some(("Project".to_string(), false))
    );
}
