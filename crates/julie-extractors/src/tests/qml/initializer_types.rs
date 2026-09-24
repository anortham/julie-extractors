use crate::base::SymbolKind;
use crate::qml::QmlExtractor;
use std::path::PathBuf;

fn local_type(source: &str, local: &str) -> Option<(String, bool)> {
    local_types(source, &[local]).remove(0)
}

fn local_types(source: &str, locals: &[&str]) -> Vec<Option<(String, bool)>> {
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
    locals
        .iter()
        .map(|local| {
            let local = symbols
                .iter()
                .find(|s| s.name == *local && s.kind == SymbolKind::Variable)
                .unwrap_or_else(|| panic!("missing local {local}"));
            extractor
                .base
                .type_info
                .get(&local.id)
                .map(|fact| (fact.resolved_type.clone(), fact.is_inferred))
        })
        .collect()
}

fn inferred(name: &str) -> Option<(String, bool)> {
    Some((name.to_string(), true))
}

fn workspace_type(body: &str) -> Option<(String, bool)> {
    scoped_workspace_type(body, "")
}

fn panel_workspace_type(body: &str) -> Option<(String, bool)> {
    scoped_workspace_type("", body)
}

fn scoped_workspace_type(root_body: &str, panel_body: &str) -> Option<(String, bool)> {
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

    function run(param) {{
        {root_body}
    }}

    Item {{
        id: panel

        function panelWorkspace(): Folder {{}}

        function panelRun(param) {{
            {panel_body}
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
fn bare_call_to_a_function_of_the_scope_object_records_its_return_type() {
    assert_eq!(
        workspace_type("let workspace = loadWorkspace()"),
        inferred("Workspace")
    );
    assert_eq!(
        panel_workspace_type("var workspace = panelWorkspace()"),
        inferred("Folder")
    );
}

#[test]
fn bare_call_from_a_non_root_object_to_a_root_function_records_nothing() {
    assert_eq!(
        panel_workspace_type("let workspace = loadWorkspace()"),
        None
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
    assert_eq!(
        panel_workspace_type("const workspace = root.loadWorkspace()"),
        inferred("Workspace")
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
    assert_eq!(workspace_type("let workspace = panelWorkspace()"), None);
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
    assert_eq!(panel_workspace_type("let workspace = param()"), None);
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
fn bare_call_from_an_object_of_a_qt_type_records_nothing() {
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
    assert_eq!(local_type(source, "fromRoot"), None);
}

#[test]
fn bare_call_from_an_inline_component_instance_records_nothing() {
    let source = r#"
Item {
    id: root
    component Fancy: Item { function load(): Bar { return null } }
    function load(): Foo { return null }
    function contains(): Foo { return null }
    Fancy { function run() { let inlineInstance = load() } }
    Rectangle { function run() { let builtin = contains() } }
}
"#;
    assert_eq!(local_type(source, "inlineInstance"), None);
    assert_eq!(local_type(source, "builtin"), None);
}

#[test]
fn bare_call_inside_an_inline_component_resolves_in_its_root() {
    let source = r#"
Item {
    function load(): Foo { return null }
    component Fancy: Item {
        function load(): Bar { return null }
        function run() { let inComponent = load() }
    }
}
"#;
    assert_eq!(local_type(source, "inComponent"), inferred("Bar"));
}

#[test]
fn var_loop_binding_anywhere_in_the_function_shadows_the_callee() {
    for body in [
        "for (var loadWorkspace of [panel.panelWorkspace]) {} let workspace = loadWorkspace()",
        "for (var loadWorkspace in helpers) {} let workspace = loadWorkspace()",
        "for (var panel of [root]) {} let workspace = panel.panelWorkspace()",
    ] {
        assert_eq!(workspace_type(body), None, "{body}");
    }
}

#[test]
fn var_loop_binding_in_a_nested_function_does_not_shadow_the_callee() {
    assert_eq!(
        workspace_type(
            "const other = () => { for (var loadWorkspace of xs) {} }; let workspace = loadWorkspace()"
        ),
        inferred("Workspace")
    );
}

#[test]
fn id_receiver_inside_an_inline_component_resolves_only_its_own_ids() {
    let source = r#"
Item {
    id: root
    function load(): Foo { return null }
    Item {
        id: panel
        function load(): Baz { return null }
    }
    component Fancy: Item {
        id: root
        function load(): Bar { return null }
        function run() {
            let fancyRoot = root.load()
            let fancyPanel = panel.load()
        }
    }
}
"#;
    assert_eq!(local_type(source, "fancyRoot"), inferred("Bar"));
    assert_eq!(local_type(source, "fancyPanel"), None);
}

#[test]
fn id_declared_twice_in_one_component_records_nothing() {
    let source = r#"
Item {
    Item {
        id: panel
        function load(): Foo { return null }
    }
    Item {
        id: panel
        function load(): Foo { return null }
    }
    function run() { let twice = panel.load() }
}
"#;
    assert_eq!(local_type(source, "twice"), None);
}

#[test]
fn id_receiver_of_the_enclosing_component_records_nothing_in_an_inline_component() {
    let source = r#"
Item {
    id: root
    function load(): Foo { return null }
    component Fancy: Item {
        function run() { let outer = root.load() }
    }
}
"#;
    assert_eq!(local_type(source, "outer"), None);
}

#[test]
fn callee_reassigned_through_an_identifier_records_nothing() {
    assert_eq!(
        workspace_type("loadWorkspace = untyped; let workspace = loadWorkspace()"),
        None
    );
    assert_eq!(
        panel_workspace_type("let workspace = panel.panelWorkspace(); panelWorkspace = untyped"),
        None
    );
}

#[test]
fn callee_changed_by_compound_assignment_or_update_records_nothing() {
    for body in [
        "loadWorkspace ||= untyped; let workspace = loadWorkspace()",
        "loadWorkspace += 1; let workspace = loadWorkspace()",
        "loadWorkspace++; let workspace = loadWorkspace()",
    ] {
        assert_eq!(workspace_type(body), None, "{body}");
    }
}

#[test]
fn callee_reassigned_by_destructuring_or_a_loop_head_records_nothing() {
    for body in [
        "[loadWorkspace] = [untyped]; let workspace = loadWorkspace()",
        "({ loadWorkspace } = { loadWorkspace: untyped }); let workspace = loadWorkspace()",
        "for (loadWorkspace of [untyped]) {} let workspace = loadWorkspace()",
    ] {
        assert_eq!(workspace_type(body), None, "{body}");
    }
}

#[test]
fn assignments_to_other_names_keep_the_callee_type() {
    assert_eq!(
        workspace_type(
            "let other = 0; other = untyped; other += 1; [other] = [untyped]; ({ loadWorkspace: other } = {}); let workspace = loadWorkspace()"
        ),
        inferred("Workspace")
    );
}

#[test]
fn many_functions_with_locals_each_resolve_their_own_scope() {
    let count = 300;
    let mut source = String::from("Item {\n    function loadWorkspace(): Workspace {}\n");
    for index in 0..count {
        source.push_str(&format!(
            "    function run{index}(param) {{ let a{index} = 0; let b{index} = a{index}; let direct{index} = loadWorkspace() }}\n\
             \x20   function shadowParam{index}(loadWorkspace) {{ let param{index} = loadWorkspace() }}\n\
             \x20   function outer{index}() {{ var loadWorkspace = null; function inner{index}() {{ let nested{index} = loadWorkspace() }} }}\n"
        ));
    }
    source.push_str("}\n");
    let names: Vec<String> = (0..count)
        .flat_map(|index| {
            [
                format!("direct{index}"),
                format!("param{index}"),
                format!("nested{index}"),
            ]
        })
        .collect();
    let locals: Vec<&str> = names.iter().map(String::as_str).collect();
    let types = local_types(&source, &locals);
    for (name, found) in names.iter().zip(types) {
        let expected = name
            .starts_with("direct")
            .then(|| inferred("Workspace"))
            .flatten();
        assert_eq!(found, expected, "{name}");
    }
}
