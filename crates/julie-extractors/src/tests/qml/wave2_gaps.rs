use std::collections::BTreeSet;
use std::path::Path;

use crate::ExtractionResults;
use crate::base::{IdentifierKind, RelationshipKind, Symbol, SymbolKind, Visibility};
use crate::tests::helpers::{facts_with_pattern, metadata_str};

fn extract(file_path: &str, source: &str) -> ExtractionResults {
    crate::pipeline::extract_canonical(file_path, source, Path::new("/repo"))
        .expect("canonical qml extraction should succeed")
}

fn symbol<'a>(results: &'a ExtractionResults, name: &str, kind: SymbolKind) -> &'a Symbol {
    results
        .symbols
        .iter()
        .find(|symbol| symbol.name == name && symbol.kind == kind)
        .unwrap_or_else(|| panic!("missing {name} {kind:?}: {:#?}", summary(results)))
}

fn summary(results: &ExtractionResults) -> Vec<(String, SymbolKind)> {
    results
        .symbols
        .iter()
        .map(|s| (s.name.clone(), s.kind.clone()))
        .collect()
}

fn parent<'a>(results: &'a ExtractionResults, child: &Symbol) -> Option<&'a Symbol> {
    let parent_id = child.parent_id.as_ref()?;
    results.symbols.iter().find(|s| &s.id == parent_id)
}

fn meta<'a>(symbol: &'a Symbol, key: &str) -> Option<&'a serde_json::Value> {
    symbol.metadata.as_ref()?.get(key)
}

fn variable_refs(results: &ExtractionResults) -> Vec<(u32, String)> {
    results
        .identifiers
        .iter()
        .filter(|i| i.kind == IdentifierKind::VariableRef)
        .map(|i| (i.start_line, i.name.clone()))
        .collect()
}

fn body_text<'a>(source: &'a str, symbol: &Symbol) -> Option<&'a str> {
    let span = symbol.body_span?;
    Some(&source[span.start_byte as usize..span.end_byte as usize])
}

#[test]
fn enum_members_with_assigned_values_are_extracted() {
    let source =
        "Item {\n    enum Status { Idle = 0, Busy = 1 }\n    enum Mode { Compact, Full = 3 }\n}\n";
    let results = extract("Enums.qml", source);

    for (member, owner, value) in [
        ("Idle", "Status", Some(0)),
        ("Busy", "Status", Some(1)),
        ("Compact", "Mode", None),
        ("Full", "Mode", Some(3)),
    ] {
        let row = symbol(&results, member, SymbolKind::EnumMember);
        assert_eq!(parent(&results, row).unwrap().name, owner);
        assert_eq!(row.visibility, Some(Visibility::Public));
        assert_eq!(meta(row, "value").and_then(|v| v.as_i64()), value);
    }
    assert!(
        variable_refs(&results).is_empty(),
        "{:?}",
        variable_refs(&results)
    );
}

#[test]
fn calls_inside_signal_handlers_come_from_the_handler_function() {
    let source = r#"Item {
    id: root
    function save() {}
    Button { onClicked: root.save() }
    Component.onCompleted: {
        save()
        console.log("ready")
    }
}
"#;
    let results = extract("HandlerCaller.qml", source);
    let save = symbol(&results, "save", SymbolKind::Function);
    let on_clicked = symbol(&results, "onClicked", SymbolKind::Function);
    let completed = symbol(&results, "Component.onCompleted", SymbolKind::Function);

    let callers: BTreeSet<_> = results
        .relationships
        .iter()
        .filter(|r| r.kind == RelationshipKind::Calls && r.to_symbol_id == save.id)
        .map(|r| r.from_symbol_id.clone())
        .collect();
    assert_eq!(
        callers,
        BTreeSet::from([on_clicked.id.clone(), completed.id.clone()])
    );
    let console = results
        .structured_pending_relationships
        .iter()
        .find(|p| p.target.display_name == "console.log")
        .expect("console.log pending");
    assert_eq!(console.pending.from_symbol_id, completed.id);
}

#[test]
fn declaration_names_are_not_variable_refs() {
    let source = r#"import QtQuick
import QtQuick.Controls as QQC2
import "helpers.js" as Helpers
Item {
    id: root
    signal saved(string name, int count)
    enum Mode { Compact, Full }
    width: 100
    function format(x) { let r = String(x); return r }
    function recordRun(id, count) {
        let total = count + 1
        var d = new Date()
        return observe(id, total, d)
    }
    MouseArea { id: area; onClicked: root.saved("x", 1) }
}
"#;
    let results = extract("Decls.qml", source);
    let refs: Vec<_> = variable_refs(&results);
    assert_eq!(
        refs,
        vec![
            (9, "x".to_string()),
            (9, "r".to_string()),
            (11, "count".to_string()),
            (13, "id".to_string()),
            (13, "total".to_string()),
            (13, "d".to_string()),
        ]
    );
    assert!(
        results
            .identifiers
            .iter()
            .any(|i| i.kind == IdentifierKind::Call && i.name == "Date")
    );
}

#[test]
fn qdoc_and_signal_doc_comments_are_extracted() {
    let source = r#"/*!
    \qmltype Docs
*/
Item {
    /*! The title. */
    property string title
    /**
     * Emitted when activated.
     */
    signal activated(int index)
    /// Fired on close.
    signal closed()
    /*! Resets the view. */
    function reset() {}
}
"#;
    let results = extract("Docs.qml", source);
    let docs = |name: &str, kind| {
        symbol(&results, name, kind)
            .doc_comment
            .clone()
            .unwrap_or_default()
    };
    assert!(docs("Docs", SymbolKind::Class).contains("\\qmltype Docs"));
    assert!(docs("title", SymbolKind::Property).contains("The title."));
    assert!(docs("activated", SymbolKind::Event).contains("Emitted when activated."));
    assert!(docs("closed", SymbolKind::Event).contains("Fired on close."));
    assert!(docs("reset", SymbolKind::Function).contains("Resets the view."));
    assert_eq!(
        symbol(&results, "closed", SymbolKind::Event).visibility,
        Some(Visibility::Public)
    );
}

#[test]
fn calls_through_import_aliases_carry_the_import_source() {
    let source = r#"import QtQuick.LocalStorage as Sql
import "utils.js" as Utils
Item {
    function run() {
        Utils.clamp(1, 2, 3)
        Sql.LocalStorage.openDatabaseSync("notes", "1.0", "Notes", 1000)
    }
}
"#;
    let results = extract("Alias.qml", source);
    let context = |display: &str| {
        results
            .structured_pending_relationships
            .iter()
            .find(|p| p.target.display_name == display)
            .unwrap_or_else(|| panic!("missing pending {display}"))
            .target
            .import_context
            .clone()
    };
    assert_eq!(context("Utils.clamp").as_deref(), Some("utils.js"));
    assert_eq!(
        context("Sql.LocalStorage.openDatabaseSync").as_deref(),
        Some("QtQuick.LocalStorage")
    );
}

#[test]
fn grouped_property_blocks_give_qualified_binding_facts_only() {
    let source = r#"Item {
    Text {
        font { bold: true; pixelSize: 12 }
        border.width: 1
    }
    Rectangle {
        anchors { left: parent.left; leftMargin: 4 }
    }
}
"#;
    let results = extract("Grouped.qml", source);
    let types: Vec<_> = facts_with_pattern(&results, "qml.object_instantiation.v1")
        .iter()
        .map(|f| metadata_str(f, "type_name").unwrap().to_string())
        .collect();
    assert_eq!(types, vec!["Item", "Text", "Rectangle"]);
    let bindings: Vec<_> = facts_with_pattern(&results, "qml.binding.v1")
        .iter()
        .map(|f| metadata_str(f, "property_name").unwrap().to_string())
        .collect();
    assert_eq!(
        bindings,
        vec![
            "font.bold",
            "font.pixelSize",
            "border.width",
            "anchors.left",
            "anchors.leftMargin"
        ]
    );
}

#[test]
fn value_source_objects_are_instantiated_and_typed() {
    let source = r#"Rectangle {
    Behavior on opacity { NumberAnimation { duration: 200 } }
    Controls.FancyBehavior on y { }
}
"#;
    let results = extract("ValueSrc.qml", source);
    let instantiated: BTreeSet<_> = results
        .structured_pending_relationships
        .iter()
        .filter(|p| p.pending.kind == RelationshipKind::Instantiates)
        .map(|p| p.target.display_name.clone())
        .collect();
    assert_eq!(
        instantiated,
        BTreeSet::from([
            "Behavior".to_string(),
            "NumberAnimation".to_string(),
            "Controls.FancyBehavior".to_string()
        ])
    );
    let fancy = results
        .identifiers
        .iter()
        .find(|i| i.name == "FancyBehavior")
        .expect("type usage");
    assert_eq!(fancy.kind, IdentifierKind::TypeUsage);
    assert_eq!(
        fancy.metadata.as_ref().unwrap()["receiver"],
        serde_json::json!("Controls")
    );
    let opacity = results
        .identifiers
        .iter()
        .find(|i| i.name == "opacity")
        .expect("on target");
    assert_eq!(opacity.kind, IdentifierKind::MemberAccess);
    assert!(
        variable_refs(&results).is_empty(),
        "{:?}",
        variable_refs(&results)
    );
    let facts: Vec<_> = facts_with_pattern(&results, "qml.object_instantiation.v1")
        .iter()
        .map(|f| metadata_str(f, "type_name").unwrap().to_string())
        .collect();
    assert_eq!(
        facts,
        vec![
            "Rectangle",
            "Behavior",
            "NumberAnimation",
            "Controls.FancyBehavior"
        ]
    );
}

#[test]
fn signal_handler_signature_is_the_header_and_body_is_the_value() {
    let source = r#"Item {
    Component.onCompleted: {
        save()
    }
    onWidthChanged: relayout()
    Keys.onReturnPressed: if (enabled) activate()
    onPressed: (mouse) => { track(mouse) }
    property bool hot: area.containsMouse || focus
    readonly property color tint: Qt.rgba(0.1, 0.2, 0.3, 1)
    property int count: 3
}
"#;
    let results = extract("Handlers.qml", source);
    let completed = symbol(&results, "Component.onCompleted", SymbolKind::Function);
    assert_eq!(
        completed.signature.as_deref(),
        Some("Component.onCompleted")
    );
    assert_eq!(
        body_text(source, completed),
        Some("{\n        save()\n    }")
    );
    let width = symbol(&results, "onWidthChanged", SymbolKind::Function);
    assert_eq!(width.signature.as_deref(), Some("onWidthChanged"));
    assert_eq!(body_text(source, width), Some("relayout()"));
    let keys = symbol(&results, "Keys.onReturnPressed", SymbolKind::Function);
    assert_eq!(body_text(source, keys), Some("if (enabled) activate()"));
    let pressed = symbol(&results, "onPressed", SymbolKind::Function);
    assert_eq!(pressed.signature.as_deref(), Some("onPressed: (mouse) =>"));

    let hot = symbol(&results, "hot", SymbolKind::Property);
    assert_eq!(body_text(source, hot), Some("area.containsMouse || focus"));
    let tint = symbol(&results, "tint", SymbolKind::Property);
    assert_eq!(body_text(source, tint), Some("Qt.rgba(0.1, 0.2, 0.3, 1)"));
    let count = symbol(&results, "count", SymbolKind::Property);
    assert_eq!(count.body_span, None);
    assert_eq!(count.body_hash, None);
}

#[test]
fn component_url_references_emit_facts() {
    let source = r#"ApplicationWindow {
    StackView { id: stack; initialItem: "pages/Home.qml" }
    Loader { source: Qt.resolvedUrl("pages/Settings.qml") }
    function openDetails() {
        stack.push(Qt.resolvedUrl("pages/Details.qml"), { itemId: 3 })
        const c = Qt.createComponent("Dialog.qml")
        console.log("not a component")
    }
}
"#;
    let results = extract("Urls.qml", source);
    let facts: Vec<_> = facts_with_pattern(&results, "qml.component_url.v1")
        .iter()
        .map(|f| {
            (
                metadata_str(f, "url").unwrap().to_string(),
                metadata_str(f, "carrier").unwrap().to_string(),
            )
        })
        .collect();
    assert_eq!(
        facts,
        vec![
            ("pages/Home.qml".to_string(), "initialItem".to_string()),
            (
                "pages/Settings.qml".to_string(),
                "Qt.resolvedUrl".to_string()
            ),
            (
                "pages/Details.qml".to_string(),
                "Qt.resolvedUrl".to_string()
            ),
            ("Dialog.qml".to_string(), "Qt.createComponent".to_string()),
        ]
    );
}

#[test]
fn qmltypes_components_link_prototypes_types_enum_values_and_exports() {
    let source = r#"import QtQuick.tooling 1.2
Module {
    Component {
        name: "Plasma::Svg"
        prototype: "QObject"
        exports: ["org.kde.plasma.core/Svg 2.0", "org.kde.plasma.core/Svg 2.1"]
        Enum {
            name: "Status"
            values: { "Normal": 0, "Selected": 1 }
        }
        Property { name: "imagePath"; type: "string" }
        Method { name: "elementSize"; type: "QSizeF"
            Parameter { name: "elementId"; type: "string" } }
    }
}
"#;
    let results = extract("plugins.qmltypes", source);
    let svg = symbol(&results, "Plasma::Svg", SymbolKind::Class);
    let extends = results
        .structured_pending_relationships
        .iter()
        .find(|p| p.pending.kind == RelationshipKind::Extends)
        .expect("prototype edge");
    assert_eq!(extends.pending.from_symbol_id, svg.id);
    assert_eq!(extends.target.display_name, "QObject");

    let declared = |name: &str, kind| {
        let row = symbol(&results, name, kind);
        results.types.get(&row.id).map(|t| t.resolved_type.clone())
    };
    assert_eq!(
        declared("imagePath", SymbolKind::Property).as_deref(),
        Some("string")
    );
    assert_eq!(
        declared("elementSize", SymbolKind::Method).as_deref(),
        Some("QSizeF")
    );
    assert_eq!(
        declared("elementId", SymbolKind::Variable).as_deref(),
        Some("string")
    );

    let selected = symbol(&results, "Selected", SymbolKind::EnumMember);
    assert_eq!(meta(selected, "value").and_then(|v| v.as_i64()), Some(1));
    assert_eq!(parent(&results, selected).unwrap().name, "Status");

    let exports: Vec<_> = results
        .symbols
        .iter()
        .filter(|s| s.kind == SymbolKind::Export)
        .collect();
    assert_eq!(exports.len(), 1, "{:#?}", summary(&results));
    assert_eq!(exports[0].name, "Svg");
    assert_eq!(exports[0].parent_id.as_deref(), Some(svg.id.as_str()));
}

#[test]
fn property_modifiers_are_recorded_on_symbols_and_facts() {
    let source = r#"Item {
    required property var model
    readonly property int count: 3
    default property list<Item> content
    property string plain
}
"#;
    let results = extract("Mods.qml", source);
    let flag = |name: &str, key: &str| {
        meta(symbol(&results, name, SymbolKind::Property), key).and_then(|v| v.as_bool())
    };
    assert_eq!(flag("model", "required"), Some(true));
    assert_eq!(flag("count", "readonly"), Some(true));
    assert_eq!(flag("content", "default"), Some(true));
    assert_eq!(flag("plain", "required"), None);

    let facts = facts_with_pattern(&results, "qml.property_declaration.v1");
    let model = facts
        .iter()
        .find(|f| metadata_str(f, "property_name") == Some("model"))
        .unwrap();
    assert_eq!(model.metadata.as_ref().unwrap()["required"], true);
}

#[test]
fn xhr_open_method_argument_is_not_a_url() {
    let source = r#"Item {
    function load() {
        var xhr = new XMLHttpRequest()
        xhr.open("GET", "https://api.example.com/items")
    }
}
"#;
    let mut literals = extract("Net.qml", source).literals;
    crate::classify_literals_by_carrier(&mut literals);
    let urls: Vec<_> = literals
        .iter()
        .filter(|l| l.kind == crate::LiteralKind::Url)
        .map(|l| l.literal_text.as_str())
        .collect();
    assert_eq!(urls, vec!["https://api.example.com/items"]);
}

#[test]
fn annotations_attach_to_their_declarations() {
    let source = r#"@Deprecated { reason: "Use NewThing" }
Item {
    @Designer { importance: 100 }
    property color accent: "red"
    @Deprecated { reason: "old" }
    function legacy() {}
}
"#;
    let results = extract("Annot.qml", source);
    let names = |name: &str, kind| {
        symbol(&results, name, kind)
            .annotations
            .iter()
            .map(|a| a.annotation.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(names("Annot", SymbolKind::Class), vec!["Deprecated"]);
    assert_eq!(names("accent", SymbolKind::Property), vec!["Designer"]);
    assert_eq!(names("legacy", SymbolKind::Function), vec!["Deprecated"]);
    assert!(facts_with_pattern(&results, "qml.binding.v1").is_empty());
    let annotation_types: Vec<_> = results
        .identifiers
        .iter()
        .filter(|i| i.kind == IdentifierKind::TypeUsage && i.name != "Item")
        .map(|i| i.name.as_str())
        .collect();
    assert_eq!(
        annotation_types,
        vec!["Deprecated", "Designer", "Deprecated"]
    );
    assert!(
        variable_refs(&results).is_empty(),
        "{:?}",
        variable_refs(&results)
    );
}

#[test]
fn object_values_record_the_property_they_are_bound_to() {
    let source = r#"Item {
    property QtObject wifi: QtObject { property bool connected: true }
    ToolTip {
        background: Rectangle { color: "black" }
    }
}
"#;
    let results = extract("ObjVal.qml", source);
    let wifi = symbol(&results, "wifi", SymbolKind::Property);
    let object = symbol(&results, "QtObject", SymbolKind::Field);
    assert_eq!(object.parent_id.as_deref(), Some(wifi.id.as_str()));
    let rectangle = symbol(&results, "Rectangle", SymbolKind::Field);
    assert_eq!(
        meta(rectangle, "bound_property").and_then(|v| v.as_str()),
        Some("background")
    );
}

#[test]
fn qualified_property_types_are_one_type_usage_with_a_receiver() {
    let source = r#"import org.kde.kirigami as Kirigami
Item {
    property Kirigami.Action action
    property alias label: body.text
    Text { id: body }
}
"#;
    let results = extract("Typed.qml", source);
    let line3: Vec<_> = results
        .identifiers
        .iter()
        .filter(|i| i.start_line == 3)
        .collect();
    assert_eq!(line3.len(), 1, "{line3:#?}");
    assert_eq!(line3[0].name, "Action");
    assert_eq!(line3[0].kind, IdentifierKind::TypeUsage);
    assert_eq!(
        line3[0].metadata.as_ref().unwrap()["receiver"],
        serde_json::json!("Kirigami")
    );
    assert!(!results.identifiers.iter().any(|i| i.name == "alias"));
}

#[test]
fn nested_functions_are_children_of_their_function() {
    let source = r#"Item {
    function outer(items) {
        function inner(x) { return x }
        return items.map(i => inner(i))
    }
}
"#;
    let results = extract("Funcs.qml", source);
    let outer = symbol(&results, "outer", SymbolKind::Function);
    let inner = symbol(&results, "inner", SymbolKind::Function);
    assert_eq!(inner.parent_id.as_deref(), Some(outer.id.as_str()));
    assert_eq!(inner.visibility, Some(Visibility::Private));
}

#[test]
fn signal_connect_marks_the_signal_and_its_handler() {
    let source = r#"Item {
    id: root
    signal refreshed()
    function onRefreshedHandler() { }
    Component.onCompleted: root.refreshed.connect(onRefreshedHandler)
}
"#;
    let results = extract("Connect.qml", source);
    let signal = results
        .identifiers
        .iter()
        .find(|i| i.name == "refreshed" && i.kind == IdentifierKind::MemberAccess)
        .expect("signal segment");
    let metadata = signal.metadata.as_ref().unwrap();
    assert_eq!(metadata["role"], "signal_handler");
    assert_eq!(metadata["receiver"], "root");
    let handler = symbol(&results, "onRefreshedHandler", SymbolKind::Function);
    assert_eq!(
        meta(handler, "handled_signal").and_then(|v| v.as_str()),
        Some("refreshed")
    );
}

#[test]
fn mjs_imports_are_javascript_imports() {
    let results = extract("Mjs.qml", "import \"math.mjs\" as MathLib\nItem {}\n");
    let import = symbol(&results, "math.mjs", SymbolKind::Import);
    assert_eq!(
        meta(import, "import_kind").and_then(|v| v.as_str()),
        Some("javascript")
    );
}
