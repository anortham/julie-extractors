use crate::ExtractionResults;
use crate::base::{RelationshipKind, Symbol, SymbolKind};
use std::path::Path;

fn extract(file_path: &str, source: &str) -> ExtractionResults {
    crate::pipeline::extract_canonical(file_path, source, Path::new("/repo"))
        .expect("canonical qml extraction should succeed")
}

fn symbol<'a>(results: &'a ExtractionResults, name: &str, kind: SymbolKind) -> &'a Symbol {
    results
        .symbols
        .iter()
        .find(|symbol| symbol.name == name && symbol.kind == kind)
        .unwrap_or_else(|| panic!("missing {name} {kind:?}: {:#?}", results.symbols))
}

fn flag(symbol: &Symbol, key: &str) -> bool {
    symbol
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

fn declared_type(results: &ExtractionResults, symbol: &Symbol) -> Option<(String, bool)> {
    results
        .types
        .get(&symbol.id)
        .map(|info| (info.resolved_type.clone(), info.is_inferred))
}

fn has_calls(results: &ExtractionResults, from: &Symbol, to: &Symbol) -> bool {
    results.relationships.iter().any(|relationship| {
        relationship.kind == RelationshipKind::Calls
            && relationship.from_symbol_id == from.id
            && relationship.to_symbol_id == to.id
    })
}

#[test]
fn nested_testcase_object_is_a_test_container_with_test_roles() {
    let source = r#"import QtTest
Item {
    width: 200
    TestCase {
        id: tc
        name: "CalculatorTests"
        when: windowShown
        function initTestCase() {}
        function cleanup() {}
        function test_add_data() { return [] }
        function test_add(data) {}
        function benchmark_add() {}
    }
    QtTest.TestCase { function test_space() {} }
}
"#;
    let results = extract("tests/tst_calculator.qml", source);
    let container = symbol(&results, "tc", SymbolKind::Field);
    assert!(flag(container, "test_container"));
    let qualified = symbol(&results, "QtTest.TestCase", SymbolKind::Field);
    assert!(flag(qualified, "test_container"));
    for name in ["test_add", "benchmark_add", "test_space"] {
        assert!(
            flag(symbol(&results, name, SymbolKind::Function), "is_test"),
            "{name}"
        );
    }
    for name in ["initTestCase", "cleanup"] {
        let lifecycle = symbol(&results, name, SymbolKind::Function);
        assert!(flag(lifecycle, "test_lifecycle"), "{name}");
    }
    assert!(!flag(
        symbol(&results, "test_add_data", SymbolKind::Function),
        "is_test"
    ));
}

#[test]
fn function_return_type_fact_comes_only_from_the_return_annotation() {
    let source = r#"Item {
    function select(index: int) { console.log(index) }
    function open(item: Item, animated: bool) {}
    function build(): Item { return null }
}
"#;
    let results = extract("ReturnType.qml", source);
    assert_eq!(
        declared_type(&results, symbol(&results, "select", SymbolKind::Function)),
        None
    );
    assert_eq!(
        declared_type(&results, symbol(&results, "open", SymbolKind::Function)),
        None
    );
    assert_eq!(
        declared_type(&results, symbol(&results, "build", SymbolKind::Function)),
        Some(("Item".to_string(), false))
    );
}

#[test]
fn qmltypes_one_line_descriptors_give_clean_names_and_types() {
    let source = r#"Module {
    Component {
        name: "Plasma::Svg"
        Property { name: "imagePath"; type: "string" }
        Property { name: "size"; type: "QSizeF"; isReadonly: true }
        Method {
            name: "elementSize"
            Parameter { name: "elementId"; type: "string" }
        }
        Method { name: "reset"; returnType: "bool" }
    }
}
"#;
    let results = extract("plugins.qmltypes", source);
    let names: Vec<_> = results
        .symbols
        .iter()
        .map(|symbol| symbol.name.as_str())
        .collect();
    for name in ["imagePath", "size", "elementId", "reset", "elementSize"] {
        assert!(names.contains(&name), "{name} missing from {names:?}");
    }
    assert!(
        !names
            .iter()
            .any(|name| name.contains('"') || name.contains(';')),
        "{names:?}"
    );
    let size = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "size")
        .unwrap();
    assert_eq!(
        size.metadata
            .as_ref()
            .and_then(|metadata| metadata.get("type"))
            .and_then(serde_json::Value::as_str),
        Some("QSizeF")
    );
}

#[test]
fn ui_qml_form_component_is_named_without_the_ui_suffix() {
    let source = "Rectangle { id: rectangle; function go() { rectangle.reset() } }\n";
    let results = extract("Screen01.ui.qml", source);
    let root = results
        .symbols
        .iter()
        .find(|symbol| symbol.kind == SymbolKind::Class)
        .unwrap();
    assert_eq!(root.name, "Screen01");
    let pending = results
        .structured_pending_relationships
        .iter()
        .find(|pending| pending.target.terminal_name == "reset")
        .expect("pending reset call");
    assert_eq!(pending.receiver_type.as_deref(), Some("Screen01"));
}

#[test]
fn id_objects_id_properties_and_typed_parameters_get_declared_type_facts() {
    let source = r#"import org.example.backend as Backend
Item {
    id: root
    Backend.DocumentModel { id: docModel }
    Timer { id: saveTimer }
    function save(target: Item, doc: Backend.DocumentModel) {
        docModel.flush()
    }
}
"#;
    let results = extract("IdTypes.qml", source);
    let declared =
        |name: &str, kind: SymbolKind| declared_type(&results, symbol(&results, name, kind));
    assert_eq!(
        declared("docModel", SymbolKind::Field),
        Some(("Backend.DocumentModel".to_string(), false))
    );
    assert_eq!(
        declared("saveTimer", SymbolKind::Field),
        Some(("Timer".to_string(), false))
    );
    assert_eq!(
        declared("root", SymbolKind::Property),
        Some(("IdTypes".to_string(), false))
    );
    assert_eq!(
        declared("target", SymbolKind::Variable),
        Some(("Item".to_string(), false))
    );
    assert_eq!(
        declared("doc", SymbolKind::Variable),
        Some(("Backend.DocumentModel".to_string(), false))
    );
}

#[test]
fn calls_in_property_initializers_emit_edges() {
    let source = r#"Item {
    id: root
    function localHelper(x) { return x * 2 }
    readonly property real size: Math.max(Style.font.body, localHelper(4))
    property string label: Util.shellQuote("a b")
}
"#;
    let results = extract("PropInitCalls.qml", source);
    let root = symbol(&results, "PropInitCalls", SymbolKind::Class);
    let helper = symbol(&results, "localHelper", SymbolKind::Function);
    assert!(
        has_calls(&results, root, helper),
        "{:#?}",
        results.relationships
    );
    let targets: Vec<_> = results
        .structured_pending_relationships
        .iter()
        .filter(|pending| pending.pending.kind == RelationshipKind::Calls)
        .map(|pending| pending.target.display_name.as_str())
        .collect();
    assert!(targets.contains(&"Util.shellQuote"), "{targets:?}");
    assert!(targets.contains(&"Math.max"), "{targets:?}");
}

#[test]
fn chained_or_foreign_member_reads_emit_no_uses_edges() {
    let source = r#"Item {
    id: root
    property color background: "transparent"
    property string text: ""
    property color tooltipBackground: Color.tooltip.background
    property string label: Style.font.text
    Text { color: model.item.background }
    ListView { id: view }
    readonly property alias count: view.count
    property color own: root.background
}
"#;
    let results = extract("FalseUses.qml", source);
    let uses: Vec<_> = results
        .relationships
        .iter()
        .filter(|relationship| relationship.kind == RelationshipKind::Uses)
        .collect();
    let background = symbol(&results, "background", SymbolKind::Property);
    assert_eq!(uses.len(), 1, "{uses:#?}");
    assert_eq!(uses[0].to_symbol_id, background.id);
    assert_eq!(uses[0].line_number, 10);
}

#[test]
fn id_receiver_calls_resolve_by_component_scope() {
    let source = r#"Item {
    id: root
    function refresh() { }
    function apply() { }
    Timer { id: timer; onTriggered: root.apply() }
    IpcHandler {
        target: "svc"
        function refresh(): void { root.refresh() }
    }
    Component.onCompleted: refresh()
    function go() { timer.start() }
}
"#;
    let results = extract("IdReceiver.qml", source);
    let root_refresh = results
        .symbols
        .iter()
        .find(|symbol| {
            symbol.name == "refresh"
                && symbol.kind == SymbolKind::Function
                && symbol.start_line == 3
        })
        .unwrap();
    let handler_refresh = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "refresh" && symbol.start_line == 8)
        .unwrap();
    let component = symbol(&results, "IdReceiver", SymbolKind::Class);
    assert!(
        has_calls(&results, handler_refresh, root_refresh),
        "{:#?}",
        results.relationships
    );
    assert!(
        has_calls(&results, component, root_refresh),
        "{:#?}",
        results.relationships
    );
    assert!(
        !results
            .structured_pending_relationships
            .iter()
            .any(|pending| pending.target.terminal_name == "refresh"),
        "{:#?}",
        results.structured_pending_relationships
    );
    let apply_call = results
        .identifiers
        .iter()
        .find(|identifier| identifier.name == "apply" && identifier.start_line == 5)
        .expect("apply call identifier");
    assert_eq!(apply_call.receiver_type.as_deref(), Some("IdReceiver"));
}
