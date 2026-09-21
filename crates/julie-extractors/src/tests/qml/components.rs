// QML Components Tests
// Tests for custom components, loaders, repeaters, and delegates

use super::*;
use crate::base::SymbolKind;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_custom_component_definition() {
        let qml_code = r#"
import QtQuick 2.15

Item {
    Component {
        id: customButton
        Rectangle {
            width: 100
            height: 40
            color: "blue"

            Text {
                anchors.centerIn: parent
                text: "Click Me"
            }
        }
    }
}
"#;

        let symbols = extract_symbols(qml_code);

        let components: Vec<&Symbol> = symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Class)
            .collect();

        assert_eq!(
            components.len(),
            1,
            "Should extract only the root Item component"
        );
    }

    #[test]
    fn test_extract_loader_component() {
        let qml_code = r#"
import QtQuick 2.15

Item {
    Loader {
        id: dynamicLoader
        source: "CustomComponent.qml"
        asynchronous: true
        onLoaded: {
            item.initialize()
        }
    }

    Loader {
        id: inlineLoader
        sourceComponent: Rectangle {
            width: 100
            height: 100
        }
    }
}
"#;

        let symbols = extract_symbols(qml_code);

        let components: Vec<&Symbol> = symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Class)
            .collect();

        assert_eq!(
            components.len(),
            1,
            "Should extract only the root Item component"
        );
    }

    #[test]
    fn test_extract_repeater_component() {
        let qml_code = r#"
import QtQuick 2.15

Column {
    Repeater {
        model: 10
        delegate: Rectangle {
            width: 100
            height: 30
            color: index % 2 === 0 ? "lightblue" : "lightgray"

            Text {
                text: "Item " + index
            }
        }
    }
}
"#;

        let symbols = extract_symbols(qml_code);

        let components: Vec<&Symbol> = symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Class)
            .collect();

        assert_eq!(
            components.len(),
            1,
            "Should extract only the root Column component"
        );
    }

    #[test]
    fn test_extract_listview_with_delegate() {
        let qml_code = r#"
import QtQuick 2.15

ListView {
    id: listView
    model: myModel

    delegate: Item {
        width: listView.width
        height: 50

        Row {
            Text { text: model.name }
            Text { text: model.value }
        }
    }

    header: Rectangle {
        width: parent.width
        height: 40
        color: "lightgray"
    }

    footer: Rectangle {
        width: parent.width
        height: 30
        color: "darkgray"
    }
}
"#;

        let symbols = extract_symbols(qml_code);

        let components: Vec<&Symbol> = symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Class)
            .collect();

        assert_eq!(
            components.len(),
            1,
            "Should extract only the root ListView component"
        );
    }

    #[test]
    fn test_extract_gridview_component() {
        let qml_code = r#"
import QtQuick 2.15

GridView {
    cellWidth: 100
    cellHeight: 100
    model: 20

    delegate: Rectangle {
        width: GridView.view.cellWidth
        height: GridView.view.cellHeight
        color: Qt.rgba(Math.random(), Math.random(), Math.random(), 1)
    }
}
"#;

        let symbols = extract_symbols(qml_code);

        let components: Vec<&Symbol> = symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Class)
            .collect();

        assert!(
            !components.is_empty(),
            "Should extract GridView with delegate"
        );
    }

    #[test]
    fn test_extract_inline_component() {
        let qml_code = r#"
import QtQuick 2.15

Item {
    component CustomButton: Rectangle {
        width: 100
        height: 40
        radius: 5

        signal clicked()

        property alias text: label.text

        Text {
            id: label
            anchors.centerIn: parent
        }
    }

    CustomButton {
        text: "Click Me"
    }
}
"#;

        let symbols = extract_symbols(qml_code);

        // Inline components (Qt 5.15+) might have different extraction behavior
        let components: Vec<&Symbol> = symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Class)
            .collect();

        assert!(!components.is_empty(), "Should extract inline component");
    }

    #[test]
    fn test_extract_instantiator_component() {
        let qml_code = r#"
import QtQuick 2.15

Item {
    Instantiator {
        model: 5
        delegate: Rectangle {
            width: 100
            height: 100
        }
        onObjectAdded: parent.children.push(object)
    }
}
"#;

        let symbols = extract_symbols(qml_code);

        let components: Vec<&Symbol> = symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Class)
            .collect();

        assert!(
            !components.is_empty(),
            "Should extract Instantiator with delegate"
        );
    }

    #[test]
    fn test_component_name_derived_from_file_path() {
        // In QML, the file name IS the component name.
        // ScrollablePage.qml defines a component called ScrollablePage.
        // The root element (KC.Page) is the base type it extends.
        let qml_code = r#"
import QtQuick 2.15
import org.kde.kirigami as Kirigami

Kirigami.ScrollablePage {
    id: root
    title: "Settings"

    property alias model: listView.model

    ListView {
        id: listView
    }
}
"#;

        let symbols = extract_symbols_with_path(qml_code, "src/controls/SettingsPage.qml");

        let components: Vec<&Symbol> = symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Class)
            .collect();

        assert_eq!(components.len(), 1, "Should extract one class symbol");

        // The class name should be the component name from the file, not the base type
        assert_eq!(
            components[0].name, "SettingsPage",
            "Class name should be the file-derived component name, not the base type"
        );

        // The base type should be preserved in the signature
        let sig = components[0].signature.as_deref().unwrap_or("");
        assert!(
            sig.contains("Kirigami.ScrollablePage"),
            "Signature should contain the base type. Got: {:?}",
            sig
        );
    }

    #[test]
    fn test_extract_pathview_component() {
        let qml_code = r#"
import QtQuick 2.15

PathView {
    model: 10
    delegate: Rectangle {
        width: 80
        height: 80
        color: "lightblue"
        scale: PathView.iconScale
        z: PathView.z
    }

    path: Path {
        startX: 0
        startY: height / 2

        PathQuad {
            x: width
            y: height / 2
            controlX: width / 2
            controlY: 0
        }
    }
}
"#;

        let symbols = extract_symbols(qml_code);

        let components: Vec<&Symbol> = symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Class)
            .collect();

        assert_eq!(
            components.len(),
            1,
            "Should extract only the root PathView component"
        );
    }

    #[test]
    fn inline_component_is_a_class_that_parents_its_members() {
        let qml_code = r#"
import QtQuick 2.15

Item {
    component CustomButton: Rectangle {
        property int radius: 5
    }
}
"#;

        let symbols = extract_symbols(qml_code);

        let component = symbols
            .iter()
            .find(|s| s.name == "CustomButton")
            .expect("inline component symbol");
        assert_eq!(component.kind, SymbolKind::Class);
        assert_eq!(
            component.signature.as_deref(),
            Some("component CustomButton: Rectangle")
        );
        assert_eq!(
            component
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("base_types")),
            Some(&serde_json::json!(["Rectangle"]))
        );

        let radius = symbols
            .iter()
            .find(|s| s.name == "radius")
            .expect("inline component member");
        assert_eq!(radius.parent_id.as_deref(), Some(component.id.as_str()));
    }

    #[test]
    fn qualified_inline_component_base_type_keeps_the_namespace() {
        let qml_code = r#"
import QtQuick 2.15
import org.kde.kirigami as Kirigami

Item {
    component Detail: Kirigami.Page {
    }
}
"#;

        let symbols = extract_symbols(qml_code);

        let component = symbols
            .iter()
            .find(|s| s.name == "Detail")
            .expect("inline component symbol");
        assert_eq!(
            component.signature.as_deref(),
            Some("component Detail: Kirigami.Page")
        );
        assert_eq!(
            component
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("base_types")),
            Some(&serde_json::json!(["Kirigami.Page"]))
        );
    }

    fn object_row<'a>(symbols: &'a [Symbol], name: &str) -> &'a Symbol {
        symbols
            .iter()
            .find(|symbol| symbol.name == name && symbol.kind == SymbolKind::Field)
            .unwrap_or_else(|| {
                panic!(
                    "expected object row {name}, got {:?}",
                    symbols
                        .iter()
                        .map(|symbol| (&symbol.name, &symbol.kind))
                        .collect::<Vec<_>>()
                )
            })
    }

    fn object_metadata(symbol: &Symbol, key: &str) -> Option<serde_json::Value> {
        symbol
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get(key))
            .cloned()
    }

    #[test]
    fn nested_object_with_an_id_is_a_field_row_named_by_the_id() {
        let qml_code = r#"
import QtQuick 2.15

Item {
    id: root

    Timer {
        id: localPluginReloadTimer
        interval: 200
    }
}
"#;

        let symbols = extract_symbols(qml_code);
        let timer = object_row(&symbols, "localPluginReloadTimer");

        assert_eq!(
            timer.signature.as_deref(),
            Some("localPluginReloadTimer: Timer")
        );
        assert_eq!(
            object_metadata(timer, "object_type"),
            Some(serde_json::json!("Timer"))
        );
        assert_eq!(
            object_metadata(timer, "binding_kind"),
            Some(serde_json::json!("object"))
        );

        let root_class = symbols
            .iter()
            .find(|symbol| symbol.kind == SymbolKind::Class)
            .expect("root class");
        assert_eq!(timer.parent_id.as_deref(), Some(root_class.id.as_str()));
        assert!(
            !symbols
                .iter()
                .any(|symbol| symbol.name == "localPluginReloadTimer"
                    && symbol.kind == SymbolKind::Property),
            "a nested object must not also emit an id property row"
        );
    }

    #[test]
    fn anonymous_nested_object_is_a_field_row_named_by_its_type() {
        let qml_code = r#"
import QtQuick 2.15

Item {
    Rectangle {
        color: "red"
    }

    QQC2.Button {
        text: "ok"
    }
}
"#;

        let symbols = extract_symbols(qml_code);

        assert_eq!(
            object_row(&symbols, "Rectangle").signature.as_deref(),
            Some("Rectangle")
        );
        assert_eq!(
            object_row(&symbols, "QQC2.Button").signature.as_deref(),
            Some("QQC2.Button")
        );
    }

    #[test]
    fn value_source_binding_records_its_target_property() {
        let qml_code = r#"
import QtQuick 2.15

Rectangle {
    Behavior on color {
        NumberAnimation {
            duration: 100
        }
    }
}
"#;

        let symbols = extract_symbols(qml_code);
        let behavior = object_row(&symbols, "Behavior");

        assert_eq!(
            object_metadata(behavior, "value_source_property"),
            Some(serde_json::json!("color"))
        );
        assert_eq!(
            object_metadata(behavior, "object_type"),
            Some(serde_json::json!("Behavior"))
        );
        assert_eq!(
            object_metadata(behavior, "binding_kind"),
            Some(serde_json::json!("object"))
        );

        let animation = object_row(&symbols, "NumberAnimation");
        assert_eq!(
            animation.parent_id.as_deref(),
            Some(behavior.id.as_str()),
            "the value source's child object parents to the value source row"
        );
    }

    #[test]
    fn value_source_binding_signature_names_the_target_property() {
        let qml_code = r#"
import QtQuick 2.15

Rectangle {
    Behavior on color {
        NumberAnimation {
            duration: 100
        }
    }

    Behavior on width {
        id: widthBehavior

        NumberAnimation {
            duration: 50
        }
    }
}
"#;

        let symbols = extract_symbols(qml_code);

        assert_eq!(
            object_row(&symbols, "Behavior").signature.as_deref(),
            Some("Behavior on color")
        );
        assert_eq!(
            object_row(&symbols, "widthBehavior").signature.as_deref(),
            Some("widthBehavior: Behavior")
        );
    }

    #[test]
    fn nested_object_members_parent_to_the_object_row() {
        let qml_code = r#"
import QtQuick 2.15

Item {
    Timer {
        id: poll

        property int ticks: 0

        function restart() {
            ticks = 0
        }
    }
}
"#;

        let symbols = extract_symbols(qml_code);
        let poll = object_row(&symbols, "poll");

        for member in ["ticks", "restart"] {
            let symbol = symbols
                .iter()
                .find(|symbol| symbol.name == member)
                .unwrap_or_else(|| panic!("expected member {member}"));
            assert_eq!(
                symbol.parent_id.as_deref(),
                Some(poll.id.as_str()),
                "{member} should parent to the enclosing object row"
            );
        }
    }

    #[test]
    fn inline_component_body_is_not_an_object_row() {
        let qml_code = r#"
import QtQuick 2.15

Item {
    component DeviceBadge: Rectangle {
        id: badge

        property string label: "device"
    }
}
"#;

        let symbols = extract_symbols(qml_code);

        assert!(
            !symbols
                .iter()
                .any(|symbol| symbol.kind == SymbolKind::Field),
            "an inline component's body object must not emit a field row, got {:?}",
            symbols
                .iter()
                .map(|symbol| (&symbol.name, &symbol.kind))
                .collect::<Vec<_>>()
        );

        let badge = symbols
            .iter()
            .find(|symbol| symbol.name == "DeviceBadge")
            .expect("inline component class");
        let label = symbols
            .iter()
            .find(|symbol| symbol.name == "label")
            .expect("inline component property");
        assert_eq!(label.parent_id.as_deref(), Some(badge.id.as_str()));
    }
}
