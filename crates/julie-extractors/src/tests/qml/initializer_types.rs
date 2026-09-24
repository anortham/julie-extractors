use crate::base::SymbolKind;
use crate::qml::QmlExtractor;
use std::path::PathBuf;

fn local_type(source: &str, local: &str) -> Option<(String, bool)> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_qmljs::LANGUAGE.into())
        .unwrap();
    let tree = parser.parse(source, None).unwrap();
    let mut extractor = QmlExtractor::new(
        "qml".to_string(),
        "Loader.qml".to_string(),
        source.to_string(),
        &PathBuf::from("/tmp/test"),
    );
    let symbols = extractor.extract_symbols(&tree);
    let local = symbols
        .iter()
        .find(|s| s.name == local && s.kind == SymbolKind::Variable)
        .unwrap_or_else(|| panic!("missing local {local}"));
    extractor
        .base
        .type_info
        .get(&local.id)
        .map(|fact| (fact.resolved_type.clone(), fact.is_inferred))
}

fn inferred(name: &str) -> Option<(String, bool)> {
    Some((name.to_string(), true))
}

fn workspace_type(body: &str) -> Option<(String, bool)> {
    local_type(
        &format!(
            r#"
Item {{
    id: root

    function loadWorkspace(): Workspace {{}}
    function loadList(): list<Workspace> {{}}
    function loadNothing(): void {{}}
    function untyped() {{}}
    signal loaded()

    Item {{
        id: panel

        function panelWorkspace(): Folder {{}}

        function run(param) {{
            {body}
        }}
    }}

    Item {{
        function siblingWorkspace(): Folder {{}}
    }}
}}
"#
        ),
        "workspace",
    )
}

#[test]
fn bare_call_to_an_enclosing_object_function_records_its_return_type() {
    assert_eq!(
        workspace_type("let workspace = loadWorkspace()"),
        inferred("Workspace")
    );
    assert_eq!(
        workspace_type("var workspace = panelWorkspace()"),
        inferred("Folder")
    );
}

#[test]
fn call_through_a_same_file_id_records_its_return_type() {
    assert_eq!(
        workspace_type("const workspace = root.loadWorkspace()"),
        inferred("Workspace")
    );
    assert_eq!(
        workspace_type("const workspace = panel.panelWorkspace()"),
        inferred("Folder")
    );
}

#[test]
fn generic_return_type_records_its_base_name() {
    assert_eq!(
        workspace_type("let workspace = loadList()"),
        inferred("list")
    );
}

#[test]
fn written_local_type_wins_over_call_inference() {
    assert_eq!(
        workspace_type("let workspace: Folder = loadWorkspace()"),
        Some(("Folder".to_string(), false))
    );
}

#[test]
fn constructor_call_still_records_its_type() {
    assert_eq!(
        workspace_type("let workspace = new Workspace()"),
        inferred("Workspace")
    );
}

#[test]
fn calls_without_a_usable_return_type_record_nothing() {
    for call in ["untyped()", "loadNothing()", "loaded()", "elsewhere()"] {
        assert_eq!(
            workspace_type(&format!("let workspace = {call}")),
            None,
            "{call}"
        );
    }
}

#[test]
fn bare_call_to_a_function_of_an_unrelated_object_records_nothing() {
    assert_eq!(workspace_type("let workspace = siblingWorkspace()"), None);
}

#[test]
fn call_through_an_unknown_receiver_records_nothing() {
    for call in [
        "other.loadWorkspace()",
        "this.loadWorkspace()",
        "root.missing()",
        "loadWorkspace().child()",
        "loadWorkspace?.()",
        "root?.loadWorkspace()",
    ] {
        assert_eq!(
            workspace_type(&format!("let workspace = {call}")),
            None,
            "{call}"
        );
    }
}

#[test]
fn local_bindings_shadowing_the_callee_record_nothing() {
    for body in [
        "let loadWorkspace = pick(); let workspace = loadWorkspace()",
        "function loadWorkspace() {} let workspace = loadWorkspace()",
        "for (const loadWorkspace of loaders) { let workspace = loadWorkspace() }",
        "try {} catch (loadWorkspace) { let workspace = loadWorkspace() }",
        "let root = pick(); let workspace = root.loadWorkspace()",
    ] {
        assert_eq!(workspace_type(body), None, "{body}");
    }
}

#[test]
fn parameter_shadowing_the_callee_records_nothing() {
    assert_eq!(workspace_type("let workspace = param()"), None);
    let source = r#"
Item {
    function loadWorkspace(): Workspace {}
    function run(loadWorkspace) {
        let workspace = loadWorkspace()
    }
}
"#;
    assert_eq!(local_type(source, "workspace"), None);
}

#[test]
fn bare_call_does_not_resolve_in_objects_between_the_scope_object_and_the_root() {
    let source = r#"
Item {
    id: root
    function load(): Item {}

    Rectangle {
        id: mid
        function load(): Rectangle {}

        Text {
            id: leaf
            function run() {
                let shadowed = load()
            }
        }
    }
}
"#;
    assert_eq!(local_type(source, "shadowed"), None);
}

#[test]
fn bare_call_skips_intermediate_objects_that_do_not_declare_the_name() {
    let source = r#"
Item {
    id: root
    function load(): Item {}

    Rectangle {
        function other(): Rectangle {}

        Text {
            function run() {
                let fromRoot = load()
            }
        }
    }
}
"#;
    assert_eq!(local_type(source, "fromRoot"), inferred("Item"));
}
