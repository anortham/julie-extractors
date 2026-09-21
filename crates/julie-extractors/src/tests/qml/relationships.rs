// QML Relationships Tests
// Tests for relationship extraction: function calls, signal connections, component instantiation

use super::*;
use crate::base::{RelationshipKind, SymbolKind};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_function_call_relationship() {
        let qml_code = r#"
import QtQuick 2.15

Item {
    function calculateTotal(items) {
        return sumValues(items)
    }

    function sumValues(arr) {
        let total = 0
        for (let i = 0; i < arr.length; i++) {
            total += arr[i]
        }
        return total
    }
}
"#;

        let (symbols, relationships) = extract_symbols_and_relationships(qml_code);

        // Verify we have both functions
        let functions: Vec<&Symbol> = symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Function)
            .collect();
        assert_eq!(functions.len(), 2, "Should extract both functions");

        // Verify call relationship: calculateTotal calls sumValues
        let call_relationships: Vec<&Relationship> = relationships
            .iter()
            .filter(|r| r.kind == RelationshipKind::Calls)
            .collect();

        assert!(
            !call_relationships.is_empty(),
            "Should extract at least one call relationship"
        );

        // Find the specific call from calculateTotal to sumValues
        let calculate_total = functions
            .iter()
            .find(|f| f.name == "calculateTotal")
            .expect("Should find calculateTotal function");
        let sum_values = functions
            .iter()
            .find(|f| f.name == "sumValues")
            .expect("Should find sumValues function");

        let call_rel = call_relationships
            .iter()
            .find(|r| r.from_symbol_id == calculate_total.id && r.to_symbol_id == sum_values.id)
            .expect("Should find call relationship from calculateTotal to sumValues");

        assert_eq!(call_rel.kind, RelationshipKind::Calls);
    }

    #[test]
    fn test_extract_signal_handler_call_relationship() {
        let qml_code = r#"
import QtQuick 2.15

Rectangle {
    id: button

    signal clicked()

    function handleClick() {
        console.log("Button clicked")
    }

    MouseArea {
        anchors.fill: parent
        onClicked: button.handleClick()
    }
}
"#;

        let tree = crate::tests::helpers::init_parser(qml_code, "qml");
        let workspace_root = std::path::PathBuf::from("/tmp/test");
        let mut extractor = crate::qml::QmlExtractor::new(
            "qml".to_string(),
            "test.qml".to_string(),
            qml_code.to_string(),
            &workspace_root,
        );
        let symbols = extractor.extract_symbols(&tree);
        let relationships = extractor.extract_relationships(&tree, &symbols);

        let button = symbols
            .iter()
            .find(|symbol| symbol.name == "button" && symbol.kind == SymbolKind::Property)
            .expect("Should extract button id");
        let component_id = button
            .parent_id
            .as_deref()
            .expect("button id should belong to the component");
        let handle_click = symbols
            .iter()
            .find(|symbol| symbol.name == "handleClick" && symbol.kind == SymbolKind::Function)
            .expect("Should extract handleClick function");

        let call_relationships: Vec<&Relationship> = relationships
            .iter()
            .filter(|r| {
                r.kind == RelationshipKind::Calls
                    && r.from_symbol_id == component_id
                    && r.to_symbol_id == handle_click.id
            })
            .collect();
        assert_eq!(
            call_relationships.len(),
            1,
            "Receiver-qualified call through the component id should resolve locally"
        );
    }

    #[test]
    fn test_component_id_receiver_call_resolves_to_local_function() {
        let qml_code = r#"
import QtQuick 2.15

Item {
    id: root

    function format(value) {
        return value
    }

    Text {
        text: root.format("ok")
    }
}
"#;

        let (symbols, relationships) = extract_symbols_and_relationships(qml_code);
        let root_id = symbols
            .iter()
            .find(|symbol| symbol.name == "root" && symbol.kind == SymbolKind::Property)
            .expect("Should extract root id");
        let component_id = root_id
            .parent_id
            .as_deref()
            .expect("root id should belong to the component");
        let format = symbols
            .iter()
            .find(|symbol| symbol.name == "format" && symbol.kind == SymbolKind::Function)
            .expect("Should extract format function");

        let resolved_call_count = relationships
            .iter()
            .filter(|relationship| {
                relationship.kind == RelationshipKind::Calls
                    && relationship.from_symbol_id == component_id
                    && relationship.to_symbol_id == format.id
            })
            .count();

        assert_eq!(
            resolved_call_count, 1,
            "root.format() should resolve to the current component's local function"
        );
    }

    #[test]
    fn test_extract_component_instantiation_relationship() {
        let qml_code = r#"
import QtQuick 2.15

Item {
    Rectangle {
        id: rect1
        width: 100
        height: 100
    }

    Text {
        id: label
        text: "Hello"
    }
}
"#;

        let (symbols, relationships) = extract_symbols_and_relationships(qml_code);

        // Only the root component (Item) is extracted as a Class symbol.
        // Nested components (Rectangle, Text) are no longer extracted,
        // so there are no instantiation relationships for them.
        let components: Vec<&Symbol> = symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Class)
            .collect();

        assert_eq!(
            components.len(),
            1,
            "Should extract only the root Item component"
        );
        // File-derived name from default "test.qml"
        assert_eq!(components[0].name, "test");
        assert!(
            relationships
                .iter()
                .all(|relationship| relationship.kind != RelationshipKind::Instantiates),
            "built-in components without local targets must not create a resolved edge"
        );
    }

    #[test]
    fn local_component_use_emits_one_resolved_instantiation_edge() {
        let qml_code = r#"
import QtQuick 2.15

Item {
    Card {}
}
"#;

        let tree = crate::tests::helpers::init_parser(qml_code, "qml");
        let workspace_root = std::path::PathBuf::from("/tmp/test");
        let mut extractor = crate::qml::QmlExtractor::new(
            "qml".to_string(),
            "test.qml".to_string(),
            qml_code.to_string(),
            &workspace_root,
        );
        let mut symbols = extractor.extract_symbols(&tree);
        let root = symbols
            .iter()
            .find(|symbol| symbol.kind == SymbolKind::Class)
            .expect("expected a root component")
            .clone();
        let mut local_card = root.clone();
        local_card.id = "local-card".to_string();
        local_card.name = "Card".to_string();
        symbols.push(local_card.clone());

        let relationships = extractor.extract_relationships(&tree, &symbols);
        let instantiations: Vec<_> = relationships
            .iter()
            .filter(|relationship| relationship.kind == RelationshipKind::Instantiates)
            .collect();

        assert_eq!(instantiations.len(), 1);
        assert_eq!(instantiations[0].from_symbol_id, root.id);
        assert_eq!(instantiations[0].to_symbol_id, local_card.id);
        assert!(
            extractor
                .get_structured_pending_relationships()
                .iter()
                .all(|pending| pending.pending.kind != RelationshipKind::Instantiates)
        );
    }

    #[test]
    fn duplicate_local_component_candidates_stay_pending() {
        let qml_code = r#"
import QtQuick 2.15

Item {
    Card {}
}
"#;

        let tree = crate::tests::helpers::init_parser(qml_code, "qml");
        let workspace_root = std::path::PathBuf::from("/tmp/test");
        let mut extractor = crate::qml::QmlExtractor::new(
            "qml".to_string(),
            "test.qml".to_string(),
            qml_code.to_string(),
            &workspace_root,
        );
        let mut symbols = extractor.extract_symbols(&tree);
        let root = symbols
            .iter()
            .find(|symbol| symbol.kind == SymbolKind::Class)
            .expect("expected a root component")
            .clone();
        for id in ["local-card-1", "local-card-2"] {
            let mut local_card = root.clone();
            local_card.id = id.to_string();
            local_card.name = "Card".to_string();
            symbols.push(local_card);
        }

        let relationships = extractor.extract_relationships(&tree, &symbols);
        assert!(
            relationships
                .iter()
                .all(|relationship| relationship.kind != RelationshipKind::Instantiates)
        );
        assert_eq!(
            extractor
                .get_structured_pending_relationships()
                .iter()
                .filter(|pending| pending.pending.kind == RelationshipKind::Instantiates)
                .count(),
            1
        );
    }

    #[test]
    fn external_component_use_emits_one_structured_pending_instantiation() {
        let qml_code = r#"
import QtQuick 2.15
import "widgets" as Widgets

Item {
    Widgets.Card {}
}
"#;

        let tree = crate::tests::helpers::init_parser(qml_code, "qml");
        let workspace_root = std::path::PathBuf::from("/tmp/test");
        let mut extractor = crate::qml::QmlExtractor::new(
            "qml".to_string(),
            "autotests/tst_cards.qml".to_string(),
            qml_code.to_string(),
            &workspace_root,
        );
        let symbols = extractor.extract_symbols(&tree);
        let relationships = extractor.extract_relationships(&tree, &symbols);

        assert!(
            relationships
                .iter()
                .all(|relationship| relationship.kind != RelationshipKind::Instantiates)
        );
        let pending: Vec<_> = extractor
            .get_structured_pending_relationships()
            .into_iter()
            .filter(|pending| pending.pending.kind == RelationshipKind::Instantiates)
            .collect();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].target.display_name, "Widgets.Card");
        assert_eq!(pending[0].target.terminal_name, "Card");
        assert_eq!(pending[0].target.receiver.as_deref(), Some("Widgets"));
        assert_eq!(pending[0].target.import_context.as_deref(), Some("widgets"));
    }

    #[test]
    fn javascript_import_alias_is_not_used_as_component_import_context() {
        let qml_code = r#"
import QtQuick 2.15
import "./js/helpers.js" as Widgets

Item {
    Widgets.Card {}
}
"#;

        let (symbols, relationships, pending) =
            extract_symbols_and_relationships_with_path(qml_code, "test.qml");
        assert!(
            relationships
                .iter()
                .all(|relationship| relationship.kind != RelationshipKind::Instantiates)
        );
        let pending = pending
            .into_iter()
            .filter(|pending| pending.pending.kind == RelationshipKind::Instantiates)
            .collect::<Vec<_>>();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].target.display_name, "Widgets.Card");
        assert_eq!(pending[0].target.import_context, None);
        let javascript_import = symbols
            .iter()
            .find(|symbol| symbol.name == "./js/helpers.js")
            .expect("javascript import symbol");
        assert_eq!(
            javascript_import
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("import_kind"))
                .and_then(serde_json::Value::as_str),
            Some("javascript")
        );
    }

    #[test]
    fn qmltypes_do_not_emit_runtime_instantiation_relationships() {
        let qmltypes = r#"
Module {
    Component {
        name: "Widget"
    }
}
"#;
        let (_symbols, relationships, pending) =
            extract_symbols_and_relationships_with_path(qmltypes, "module.QMLTYPES");
        assert!(
            relationships
                .iter()
                .all(|relationship| relationship.kind != RelationshipKind::Instantiates)
        );
        assert!(
            pending
                .iter()
                .all(|pending| pending.pending.kind != RelationshipKind::Instantiates)
        );
    }

    #[test]
    fn test_extract_nested_function_calls() {
        let qml_code = r#"
import QtQuick 2.15

Item {
    function processData(data) {
        let cleaned = cleanData(data)
        let validated = validateData(cleaned)
        return saveData(validated)
    }

    function cleanData(data) { return data }
    function validateData(data) { return data }
    function saveData(data) { return true }
}
"#;

        let (symbols, relationships) = extract_symbols_and_relationships(qml_code);

        let functions: Vec<&Symbol> = symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Function)
            .collect();
        assert_eq!(functions.len(), 4, "Should extract all four functions");

        // processData should call cleanData, validateData, and saveData
        let call_relationships: Vec<&Relationship> = relationships
            .iter()
            .filter(|r| r.kind == RelationshipKind::Calls)
            .collect();

        assert!(
            call_relationships.len() >= 3,
            "Should extract at least 3 call relationships from processData"
        );

        let process_data = functions
            .iter()
            .find(|f| f.name == "processData")
            .expect("Should find processData function");

        // Verify calls from processData
        let calls_from_process = call_relationships
            .iter()
            .filter(|r| r.from_symbol_id == process_data.id)
            .count();

        assert_eq!(
            calls_from_process, 3,
            "processData should make 3 function calls"
        );
    }

    #[test]
    fn property_use_resolves_inside_the_enclosing_object() {
        let qml_code = r#"
import QtQuick 2.15

Item {
    id: root

    property int margin: 4

    Rectangle {
        id: box

        property int margin: 8

        function shrink() {
            return box.margin - 1
        }
    }
}
"#;

        let (symbols, relationships) = extract_symbols_and_relationships(qml_code);
        let box_row = symbols
            .iter()
            .find(|symbol| symbol.name == "box" && symbol.kind == SymbolKind::Field)
            .expect("nested object row");
        let nested_margin = symbols
            .iter()
            .find(|symbol| {
                symbol.name == "margin" && symbol.parent_id.as_deref() == Some(box_row.id.as_str())
            })
            .expect("nested margin property");

        assert!(
            relationships.iter().any(|relationship| {
                relationship.kind == RelationshipKind::Uses
                    && relationship.to_symbol_id == nested_margin.id
            }),
            "box.margin should target the property declared in the same object, got: {:?}",
            relationships
                .iter()
                .map(|relationship| (&relationship.kind, &relationship.to_symbol_id))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_ambiguous_duplicate_function_names_do_not_create_resolved_calls() {
        let qml_code = r#"
import QtQuick 2.15

Item {
    function duplicate() { return 1 }

    Rectangle {
        function duplicate() { return 2 }
    }

    function caller() {
        return duplicate()
    }
}
"#;

        let tree = crate::tests::helpers::init_parser(qml_code, "qml");
        let workspace_root = std::path::PathBuf::from("/tmp/test");
        let mut extractor = crate::qml::QmlExtractor::new(
            "qml".to_string(),
            "test.qml".to_string(),
            qml_code.to_string(),
            &workspace_root,
        );

        let symbols = extractor.extract_symbols(&tree);
        let relationships = extractor.extract_relationships(&tree, &symbols);

        let caller = symbols
            .iter()
            .find(|s| s.name == "caller" && s.kind == SymbolKind::Function)
            .expect("Should find caller function");

        let resolved_calls_from_caller: Vec<&Relationship> = relationships
            .iter()
            .filter(|r| r.kind == RelationshipKind::Calls && r.from_symbol_id == caller.id)
            .collect();

        assert!(
            resolved_calls_from_caller.is_empty(),
            "Ambiguous duplicate targets should not produce resolved call edges, found: {:?}",
            resolved_calls_from_caller
                .iter()
                .map(|r| &r.to_symbol_id)
                .collect::<Vec<_>>()
        );

        let pending = extractor.get_structured_pending_relationships();
        assert!(
            pending.iter().any(|p| {
                p.pending.kind == RelationshipKind::Calls
                    && p.pending.from_symbol_id == caller.id
                    && p.target.terminal_name == "duplicate"
            }),
            "Ambiguous duplicate call should be recorded as a pending relationship"
        );
    }

    #[test]
    fn call_inside_a_nested_object_keeps_the_class_as_from_symbol() {
        let qml_code = r#"
import QtQuick 2.15

Item {
    id: root

    function refresh() {
        return 1
    }

    Timer {
        id: poll
        onTriggered: root.refresh()
    }
}
"#;

        let (symbols, relationships) = extract_symbols_and_relationships(qml_code);
        let root_class = symbols
            .iter()
            .find(|symbol| symbol.kind == SymbolKind::Class)
            .expect("root class");
        let poll = symbols
            .iter()
            .find(|symbol| symbol.name == "poll" && symbol.kind == SymbolKind::Field)
            .expect("nested object row");
        let refresh = symbols
            .iter()
            .find(|symbol| symbol.name == "refresh" && symbol.kind == SymbolKind::Function)
            .expect("refresh function");

        assert!(
            relationships.iter().any(|relationship| {
                relationship.kind == RelationshipKind::Calls
                    && relationship.from_symbol_id == root_class.id
                    && relationship.to_symbol_id == refresh.id
            }),
            "the call should be anchored to the class, got: {:?}",
            relationships
                .iter()
                .map(|relationship| (&relationship.kind, &relationship.from_symbol_id))
                .collect::<Vec<_>>()
        );
        assert!(
            relationships
                .iter()
                .all(|relationship| relationship.from_symbol_id != poll.id),
            "no relationship may start at an object row"
        );
    }

    #[test]
    fn call_through_a_nested_object_id_resolves_to_its_member_function() {
        let qml_code = r#"
import QtQuick 2.15

Item {
    id: root

    Timer {
        id: poll

        function restart() {
            return 1
        }
    }

    function run() {
        poll.restart()
    }
}
"#;

        let (symbols, relationships) = extract_symbols_and_relationships(qml_code);
        let run = symbols
            .iter()
            .find(|symbol| symbol.name == "run" && symbol.kind == SymbolKind::Function)
            .expect("run function");
        let restart = symbols
            .iter()
            .find(|symbol| symbol.name == "restart" && symbol.kind == SymbolKind::Function)
            .expect("restart function");

        assert_eq!(
            relationships
                .iter()
                .filter(|relationship| {
                    relationship.kind == RelationshipKind::Calls
                        && relationship.from_symbol_id == run.id
                        && relationship.to_symbol_id == restart.id
                })
                .count(),
            1,
            "poll.restart() should resolve through the nested object row"
        );
    }

    #[test]
    fn root_component_emits_a_pending_extends_to_its_base_type() {
        let qml_code = r#"
import org.kde.kirigami as Kirigami

Kirigami.Page {
    id: root
}
"#;

        let (symbols, _, pending) =
            extract_symbols_and_relationships_with_path(qml_code, "SettingsPage.qml");
        let component = symbols
            .iter()
            .find(|symbol| symbol.name == "SettingsPage" && symbol.kind == SymbolKind::Class)
            .expect("root component");

        let extends = pending
            .iter()
            .find(|entry| entry.pending.kind == RelationshipKind::Extends)
            .expect("pending extends");
        assert_eq!(extends.pending.from_symbol_id, component.id);
        assert_eq!(extends.target.terminal_name, "Page");
        assert_eq!(extends.target.receiver.as_deref(), Some("Kirigami"));
        assert_eq!(
            extends.target.import_context.as_deref(),
            Some("org.kde.kirigami")
        );
    }

    #[test]
    fn root_component_extending_a_same_file_inline_component_resolves_concretely() {
        let qml_code = r#"
import QtQuick 2.15

Shell {
    component Shell: Item {}
}
"#;

        let (symbols, relationships, pending) =
            extract_symbols_and_relationships_with_path(qml_code, "Main.qml");
        let component = symbols
            .iter()
            .find(|symbol| symbol.name == "Main" && symbol.kind == SymbolKind::Class)
            .expect("root component");
        let inline = symbols
            .iter()
            .find(|symbol| symbol.name == "Shell" && symbol.kind == SymbolKind::Class)
            .expect("inline component");

        assert!(
            relationships.iter().any(|relationship| {
                relationship.kind == RelationshipKind::Extends
                    && relationship.from_symbol_id == component.id
                    && relationship.to_symbol_id == inline.id
            }),
            "root extends the same-file inline component"
        );
        assert!(
            pending
                .iter()
                .all(|entry| entry.pending.kind != RelationshipKind::Extends
                    || entry.pending.from_symbol_id != component.id),
            "a resolved base type emits no pending extends"
        );
        assert!(
            pending
                .iter()
                .any(|entry| entry.pending.kind == RelationshipKind::Extends
                    && entry.pending.from_symbol_id == inline.id
                    && entry.target.terminal_name == "Item"),
            "the inline component body still extends its own base"
        );
    }

    #[test]
    fn root_component_emits_no_instantiates_for_its_own_base_type() {
        let qml_code = r#"
import QtQuick 2.15

Rectangle {
    id: root
}
"#;

        let (_, _, pending) = extract_symbols_and_relationships_with_path(qml_code, "Card.qml");

        assert!(
            pending
                .iter()
                .all(|entry| entry.pending.kind != RelationshipKind::Instantiates),
            "the root base type is an extends, not an instantiates"
        );
    }

    #[test]
    fn grouped_property_blocks_emit_no_instantiation() {
        let qml_code = r#"
import QtQuick 2.15

Rectangle {
    id: root

    Text {
        anchors { fill: parent }
    }
}
"#;

        let (_, relationships, pending) =
            extract_symbols_and_relationships_with_path(qml_code, "Panel.qml");

        assert!(
            !relationships.iter().any(|relationship| relationship.kind
                == RelationshipKind::Instantiates
                && relationship.line_number == 8),
            "a grouped property block instantiates nothing"
        );
        assert!(
            !pending
                .iter()
                .any(|entry| entry.target.terminal_name == "anchors"),
            "a grouped property block emits no pending row: {:?}",
            pending
                .iter()
                .map(|entry| entry.target.terminal_name.clone())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn an_inline_component_body_extends_its_base() {
        let qml_code = r#"
import QtQuick 2.15

Item {
    component DeviceBadge: Rectangle { }
}
"#;

        let (symbols, relationships, pending) =
            extract_symbols_and_relationships_with_path(qml_code, "Panel.qml");

        let badge = symbols
            .iter()
            .find(|symbol| symbol.name == "DeviceBadge" && symbol.kind == SymbolKind::Class)
            .expect("inline component class row");
        let base = pending
            .iter()
            .find(|entry| entry.target.terminal_name == "Rectangle")
            .expect("the inline component base type is a pending row");

        assert_eq!(base.pending.kind, RelationshipKind::Extends);
        assert_eq!(base.pending.from_symbol_id, badge.id);
        assert!(
            !relationships.iter().any(|relationship| relationship.kind
                == RelationshipKind::Instantiates
                && relationship.line_number == 5),
            "the inline component body instantiates nothing"
        );
    }

    #[test]
    fn a_qualified_type_never_resolves_to_a_same_file_class() {
        let qml_code = r#"
import QtQuick.Controls as QQC2

QQC2.Button {
    component Button: Item { }
}
"#;

        let (symbols, relationships, pending) =
            extract_symbols_and_relationships_with_path(qml_code, "Main.qml");
        let inline = symbols
            .iter()
            .find(|symbol| symbol.name == "Button" && symbol.kind == SymbolKind::Class)
            .expect("inline component class row");

        assert!(
            !relationships
                .iter()
                .any(|relationship| relationship.to_symbol_id == inline.id),
            "a qualified type name resolves to no same-file class"
        );
        let qualified = pending
            .iter()
            .filter(|entry| entry.target.terminal_name == "Button")
            .collect::<Vec<_>>();
        assert_eq!(qualified.len(), 1, "one pending row for the qualified base");
        assert_eq!(qualified[0].target.receiver.as_deref(), Some("QQC2"));
    }
}
