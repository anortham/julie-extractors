// QML Identifiers Tests
// Tests for identifier extraction: function calls, member access, variable references

use super::*;
use crate::base::IdentifierKind;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_function_call_identifiers() {
        let qml_code = r#"
import QtQuick 2.15

Item {
    function processData(items) {
        let result = calculateSum(items)
        return formatResult(result)
    }

    function calculateSum(arr) { return 0 }
    function formatResult(val) { return val }
}
"#;

        let identifiers = extract_identifiers(qml_code);

        // Should extract identifiers for calculateSum and formatResult calls
        let call_identifiers: Vec<&Identifier> = identifiers
            .iter()
            .filter(|id| id.kind == IdentifierKind::Call)
            .collect();

        assert!(
            call_identifiers.len() >= 2,
            "Should extract at least 2 function call identifiers"
        );

        let call_names: Vec<&str> = call_identifiers.iter().map(|id| id.name.as_str()).collect();
        assert!(
            call_names.contains(&"calculateSum"),
            "Should extract calculateSum call identifier"
        );
        assert!(
            call_names.contains(&"formatResult"),
            "Should extract formatResult call identifier"
        );
    }

    #[test]
    fn test_builtin_property_types_are_not_recorded_as_type_usage() {
        // QML property/signal annotations use builtin primitives (`string`, `int`,
        // `real`, `bool`, `var`, ...). These are not resolvable type references and
        // must be filtered out of identifier extraction, matching the C#/Python/Razor
        // convention (`is_*_builtin_type`). A user-defined type (`UserModel`) in the
        // same position MUST still be recorded so cross-file type refs keep working.
        let qml_code = r#"
import QtQuick 2.15

Item {
    property string title: "Worker"
    property int count: 0
    signal activated(string value)
    property UserModel model
}
"#;

        let identifiers = extract_identifiers(qml_code);
        let type_usages: Vec<&str> = identifiers
            .iter()
            .filter(|id| id.kind == IdentifierKind::TypeUsage)
            .map(|id| id.name.as_str())
            .collect();

        for builtin in ["string", "int", "real", "bool", "var"] {
            assert!(
                !type_usages.contains(&builtin),
                "QML builtin `{builtin}` must not be recorded as a TypeUsage identifier, got {type_usages:?}"
            );
        }
        assert!(
            type_usages.contains(&"UserModel"),
            "user-defined type `UserModel` must be recorded as a TypeUsage identifier, got {type_usages:?}"
        );
    }

    #[test]
    fn test_extract_member_access_identifiers() {
        let qml_code = r#"
import QtQuick 2.15

Rectangle {
    id: container

    Rectangle {
        id: child
        width: parent.width
        height: container.height
        anchors.fill: parent
    }
}
"#;

        let identifiers = extract_identifiers(qml_code);

        // Should extract member access identifiers for parent.width, container.height, etc.
        let member_access_identifiers: Vec<&Identifier> = identifiers
            .iter()
            .filter(|id| id.kind == IdentifierKind::MemberAccess)
            .collect();

        assert!(
            member_access_identifiers.len() >= 2,
            "Should extract member access identifiers for property access"
        );

        let member_names: Vec<&str> = member_access_identifiers
            .iter()
            .map(|id| id.name.as_str())
            .collect();

        // Should find property access patterns
        assert!(
            member_names
                .iter()
                .any(|&name| name == "width" || name == "height"),
            "Should extract property access identifiers"
        );
    }

    #[test]
    fn test_extract_variable_reference_identifiers() {
        let qml_code = r#"
import QtQuick 2.15

Item {
    function processItems(items) {
        let count = items.length
        let result = []

        for (let i = 0; i < count; i++) {
            result.push(items[i])
        }

        return result
    }
}
"#;

        let identifiers = extract_identifiers(qml_code);

        // Should extract variable references for items, count, result, i
        let var_ref_identifiers: Vec<&Identifier> = identifiers
            .iter()
            .filter(|id| id.kind == IdentifierKind::VariableRef)
            .collect();

        assert!(
            var_ref_identifiers.len() >= 2,
            "Should extract variable reference identifiers"
        );
    }

    #[test]
    fn test_extract_signal_handler_identifiers() {
        let qml_code = r#"
import QtQuick 2.15

Rectangle {
    signal customSignal()

    function myHandler() {
        console.log("Handler called")
    }

    MouseArea {
        onClicked: myHandler()
        onPressed: customSignal()
    }
}
"#;

        let identifiers = extract_identifiers(qml_code);

        // Should extract identifiers for myHandler and customSignal calls
        let call_identifiers: Vec<&Identifier> = identifiers
            .iter()
            .filter(|id| id.kind == IdentifierKind::Call)
            .collect();

        assert!(
            call_identifiers.len() >= 2,
            "Should extract call identifiers from signal handlers"
        );

        let call_names: Vec<&str> = call_identifiers.iter().map(|id| id.name.as_str()).collect();
        assert!(
            call_names.contains(&"myHandler"),
            "Should extract myHandler call"
        );
        assert!(
            call_names.contains(&"customSignal"),
            "Should extract customSignal call"
        );
    }

    #[test]
    fn test_extract_console_log_identifiers() {
        let qml_code = r#"
import QtQuick 2.15

Item {
    function debugInfo(message) {
        console.log(message)
        console.error("Error:", message)
        console.warn("Warning")
    }
}
"#;

        let identifiers = extract_identifiers(qml_code);

        // Should extract member access for console.log, console.error, console.warn
        let member_identifiers: Vec<&Identifier> = identifiers
            .iter()
            .filter(|id| id.kind == IdentifierKind::MemberAccess || id.kind == IdentifierKind::Call)
            .collect();

        assert!(
            member_identifiers.len() >= 3,
            "Should extract console method identifiers"
        );
    }

    #[test]
    fn test_identifier_location_accuracy() {
        let qml_code = r#"
import QtQuick 2.15

Item {
    function test() {
        calculateSum(10, 20)
    }
}
"#;

        let identifiers = extract_identifiers(qml_code);

        let calc_sum_id = identifiers
            .iter()
            .find(|id| id.name == "calculateSum")
            .expect("Should find calculateSum identifier");

        // Verify position information is captured
        assert!(calc_sum_id.start_line > 0, "Should have valid start_line");
        assert!(calc_sum_id.end_line > 0, "Should have valid end_line");
        assert!(
            calc_sum_id.start_line <= calc_sum_id.end_line,
            "start_line should be <= end_line"
        );
    }

    #[test]
    fn test_extract_component_instantiation_as_type_usage() {
        // Nested QML components (Rectangle {}, Button {}, etc.) are type references.
        // The type name used in a ui_object_definition is analogous to a constructor call
        // or type annotation — it should produce a TypeUsage identifier.
        let qml_code = r#"
import QtQuick 2.15

Item {
    Rectangle {
        id: header
        width: parent.width

        Text {
            text: "Hello"
        }
    }

    ListView {
        id: mainList
        model: 10
    }

    MouseArea {
        anchors.fill: parent
    }
}
"#;

        let identifiers = extract_identifiers(qml_code);

        let type_usages: Vec<&Identifier> = identifiers
            .iter()
            .filter(|id| id.kind == IdentifierKind::TypeUsage)
            .collect();

        let type_names: Vec<&str> = type_usages.iter().map(|id| id.name.as_str()).collect();

        // Each nested component instantiation should produce a TypeUsage identifier
        assert!(
            type_names.contains(&"Rectangle"),
            "Rectangle instantiation should be TypeUsage. Got: {:?}",
            type_names
        );
        assert!(
            type_names.contains(&"Text"),
            "Text instantiation should be TypeUsage. Got: {:?}",
            type_names
        );
        assert!(
            type_names.contains(&"ListView"),
            "ListView instantiation should be TypeUsage. Got: {:?}",
            type_names
        );
        assert!(
            type_names.contains(&"MouseArea"),
            "MouseArea instantiation should be TypeUsage. Got: {:?}",
            type_names
        );
    }

    fn role_of(identifier: &Identifier) -> Option<&str> {
        identifier.metadata.as_ref()?.get("role")?.as_str()
    }

    fn receiver_of(identifier: &Identifier) -> Option<&str> {
        identifier.metadata.as_ref()?.get("receiver")?.as_str()
    }

    #[test]
    fn qualified_nested_type_name_records_the_terminal_segment_with_its_receiver() {
        let qml_code = r#"
import org.kde.kirigami as Kirigami

Item {
    Kirigami.Page {}
}
"#;

        let identifiers = extract_identifiers(qml_code);

        let page = identifiers
            .iter()
            .find(|id| id.kind == IdentifierKind::TypeUsage && id.name == "Page")
            .expect("terminal segment type usage");
        assert_eq!(receiver_of(page), Some("Kirigami"));
        assert!(
            !identifiers
                .iter()
                .any(|id| id.kind == IdentifierKind::TypeUsage && id.name.contains('.')),
            "no dotted type usage names may remain"
        );
    }

    #[test]
    fn root_base_type_is_a_type_usage_identifier_with_the_base_type_role() {
        let qml_code = r#"
import QtQuick 2.15

Rectangle {
    id: root
}
"#;

        let identifiers = extract_identifiers(qml_code);

        let base = identifiers
            .iter()
            .find(|id| id.kind == IdentifierKind::TypeUsage && id.name == "Rectangle")
            .expect("root base type usage");
        assert_eq!(role_of(base), Some("base_type"));
        assert_eq!(receiver_of(base), None);
    }

    #[test]
    fn qualified_root_base_type_records_the_terminal_segment_with_its_receiver() {
        let qml_code = r#"
import org.kde.kirigami as Kirigami

Kirigami.Page {
    id: root
}
"#;

        let identifiers = extract_identifiers(qml_code);

        let base = identifiers
            .iter()
            .find(|id| id.kind == IdentifierKind::TypeUsage && id.name == "Page")
            .expect("root base type usage");
        assert_eq!(role_of(base), Some("base_type"));
        assert_eq!(receiver_of(base), Some("Kirigami"));
    }

    #[test]
    fn attached_binding_name_records_the_attached_type() {
        let qml_code = r#"
import QtQuick 2.15

Item {
    Rectangle {
        Layout.fillWidth: true
    }
}
"#;

        let identifiers = extract_identifiers(qml_code);

        let attached = identifiers
            .iter()
            .find(|id| id.kind == IdentifierKind::TypeUsage && id.name == "Layout")
            .expect("attached type usage");
        assert_eq!(role_of(attached), Some("attached_type"));
        assert_eq!(receiver_of(attached), None);
    }

    #[test]
    fn three_segment_attached_binding_name_records_the_inner_type_with_its_receiver() {
        let qml_code = r#"
import org.kde.kirigami as Kirigami

Item {
    Rectangle {
        Kirigami.FormData.label: "Name"
    }
}
"#;

        let identifiers = extract_identifiers(qml_code);

        let attached = identifiers
            .iter()
            .find(|id| id.kind == IdentifierKind::TypeUsage && id.name == "FormData")
            .expect("attached type usage");
        assert_eq!(role_of(attached), Some("attached_type"));
        assert_eq!(receiver_of(attached), Some("Kirigami"));
    }

    #[test]
    fn lowercase_grouped_binding_name_records_no_attached_type() {
        let qml_code = r#"
import QtQuick 2.15

Item {
    Rectangle {
        anchors.fill: parent
    }
}
"#;

        let identifiers = extract_identifiers(qml_code);

        assert!(
            !identifiers
                .iter()
                .any(|id| role_of(id) == Some("attached_type")),
            "a lowercase grouped property is not an attached type"
        );
    }

    #[test]
    fn signal_handler_binding_records_a_member_access_for_the_signal() {
        let qml_code = r#"
import QtQuick 2.15

Item {
    Button {
        onClicked: doThing()
    }
}
"#;

        let identifiers = extract_identifiers(qml_code);

        let handler = identifiers
            .iter()
            .find(|id| id.kind == IdentifierKind::MemberAccess && id.name == "clicked")
            .expect("signal handler member access");
        assert_eq!(role_of(handler), Some("signal_handler"));
        assert_eq!(receiver_of(handler), Some("Button"));
    }

    #[test]
    fn root_signal_handler_takes_the_root_base_type_as_its_receiver() {
        let qml_code = r#"
import QtQuick 2.15

Item {
    onFocusRequested: doThing()
}
"#;

        let identifiers = extract_identifiers(qml_code);

        let handler = identifiers
            .iter()
            .find(|id| id.kind == IdentifierKind::MemberAccess && id.name == "focusRequested")
            .expect("signal handler member access");
        assert_eq!(receiver_of(handler), Some("Item"));
    }

    #[test]
    fn connections_handler_function_records_the_signal_with_the_target_id_receiver() {
        let qml_code = r#"
import QtQuick 2.15

Item {
    Connections {
        target: backend
        function onReloaded() {}
    }
}
"#;

        let identifiers = extract_identifiers(qml_code);

        let handler = identifiers
            .iter()
            .find(|id| id.kind == IdentifierKind::MemberAccess && id.name == "reloaded")
            .expect("connections handler member access");
        assert_eq!(role_of(handler), Some("signal_handler"));
        assert_eq!(receiver_of(handler), Some("backend"));
    }

    #[test]
    fn connections_handler_binding_records_the_signal_with_the_target_id_receiver() {
        let qml_code = r#"
import QtQuick 2.15

Item {
    Connections {
        target: backend
        onReloaded: refresh()
    }
}
"#;

        let identifiers = extract_identifiers(qml_code);

        let handler = identifiers
            .iter()
            .find(|id| id.kind == IdentifierKind::MemberAccess && id.name == "reloaded")
            .expect("connections handler member access");
        assert_eq!(receiver_of(handler), Some("backend"));
    }

    #[test]
    fn connections_handler_without_an_id_target_records_no_receiver() {
        let qml_code = r#"
import QtQuick 2.15

Item {
    Connections {
        target: backend.model
        function onReloaded() {}
    }
}
"#;

        let identifiers = extract_identifiers(qml_code);

        let handler = identifiers
            .iter()
            .find(|id| id.kind == IdentifierKind::MemberAccess && id.name == "reloaded")
            .expect("connections handler member access");
        assert_eq!(receiver_of(handler), None);
    }

    #[test]
    fn property_change_handler_points_at_the_property() {
        let qml_code = r#"
import QtQuick 2.15

Item {
    Rectangle {
        onColorChanged: repaint()
    }
}
"#;

        let identifiers = extract_identifiers(qml_code);

        let handler = identifiers
            .iter()
            .find(|id| id.kind == IdentifierKind::MemberAccess && id.name == "color")
            .expect("change handler member access");
        assert_eq!(role_of(handler), Some("signal_handler"));
        assert_eq!(
            handler
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("change_handler")),
            Some(&serde_json::json!(true))
        );
        assert_eq!(receiver_of(handler), Some("Rectangle"));
    }

    #[test]
    fn attached_signal_handler_records_both_the_attached_type_and_the_signal() {
        let qml_code = r#"
import QtQuick 2.15

Item {
    Rectangle {
        Keys.onPressed: handle(event)
    }
}
"#;

        let identifiers = extract_identifiers(qml_code);

        assert!(
            identifiers
                .iter()
                .any(|id| id.name == "Keys" && role_of(id) == Some("attached_type")),
            "attached type usage"
        );
        assert!(
            identifiers
                .iter()
                .any(|id| id.name == "pressed" && role_of(id) == Some("signal_handler")),
            "signal handler member access"
        );
    }

    #[test]
    fn the_root_base_type_identifier_is_contained_by_the_root_component() {
        let qml_code = r#"
import QtQuick 2.15

Rectangle {
    id: root
}
"#;

        let tree = crate::tests::helpers::init_parser(qml_code, "qml");
        let mut extractor = crate::qml::QmlExtractor::new(
            "qml".to_string(),
            "test.qml".to_string(),
            qml_code.to_string(),
            std::path::Path::new("/tmp/test"),
        );
        let symbols = extractor.extract_symbols(&tree);
        let identifiers = extractor.extract_identifiers(&tree, &symbols);

        let component = symbols
            .iter()
            .find(|symbol| symbol.name == "test")
            .expect("root component");
        let base = identifiers
            .iter()
            .find(|id| id.kind == IdentifierKind::TypeUsage && id.name == "Rectangle")
            .expect("root base type usage");
        assert_eq!(
            base.containing_symbol_id.as_deref(),
            Some(component.id.as_str())
        );
    }

    #[test]
    fn grouped_property_blocks_emit_no_type_usage() {
        let qml_code = r#"
import QtQuick 2.15

Rectangle {
    Text {
        anchors { fill: parent }
    }
}
"#;

        let identifiers = extract_identifiers(qml_code);

        assert!(
            !identifiers
                .iter()
                .any(|identifier| identifier.name == "anchors"
                    && identifier.kind == IdentifierKind::TypeUsage),
            "a grouped property block is not a type usage: {:?}",
            identifiers
                .iter()
                .map(|identifier| (&identifier.name, &identifier.kind))
                .collect::<Vec<_>>()
        );
    }
}
