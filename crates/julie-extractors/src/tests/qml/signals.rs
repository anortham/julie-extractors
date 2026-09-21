// QML Signals Tests
// Tests for signal declarations and signal handlers

use super::*;
use crate::base::SymbolKind;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_qml_signal_handlers() {
        let qml_code = r#"
import QtQuick 2.15

Rectangle {
    id: button

    signal clicked()

    MouseArea {
        anchors.fill: parent
        onClicked: button.clicked()
        onPressed: console.log("Pressed")
    }
}
"#;

        let symbols = extract_symbols(qml_code);

        // Should extract the signal
        let signals: Vec<&Symbol> = symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Event)
            .collect();
        assert_eq!(signals.len(), 1, "Should extract clicked signal");
        assert_eq!(signals[0].name, "clicked");
    }

    #[test]
    fn test_extract_signal_with_parameters() {
        let qml_code = r#"
import QtQuick 2.15

Item {
    signal activated(string name, int index)
    signal dataChanged(var oldData, var newData)
    signal positionChanged(real x, real y, real z)
}
"#;

        let symbols = extract_symbols(qml_code);

        let signals: Vec<&Symbol> = symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Event)
            .collect();

        assert_eq!(signals.len(), 3, "Should extract all three signals");

        // Verify signal names
        let signal_names: Vec<&str> = signals.iter().map(|s| s.name.as_str()).collect();
        assert!(signal_names.contains(&"activated"));
        assert!(signal_names.contains(&"dataChanged"));
        assert!(signal_names.contains(&"positionChanged"));
    }

    #[test]
    fn test_extract_signal_handlers_with_javascript() {
        let qml_code = r#"
import QtQuick 2.15

Rectangle {
    signal customSignal(int value)

    onCustomSignal: {
        console.log("Value:", value)
        if (value > 10) {
            width = value * 2
        }
    }
}
"#;

        let symbols = extract_symbols(qml_code);

        let signals: Vec<&Symbol> = symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Event)
            .collect();

        assert_eq!(signals.len(), 1, "Should extract customSignal");
    }

    #[test]
    fn test_extract_connection_blocks() {
        let qml_code = r#"
import QtQuick 2.15

Item {
    id: root
    signal mySignal()

    Connections {
        target: root
        function onMySignal() {
            console.log("Signal received")
        }
    }
}
"#;

        let symbols = extract_symbols(qml_code);

        // Should extract the signal declaration
        let signals: Vec<&Symbol> = symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Event)
            .collect();

        assert!(
            !signals.is_empty(),
            "Should extract signal from Connections block"
        );
    }

    #[test]
    fn test_extract_component_completed_signal() {
        let qml_code = r#"
import QtQuick 2.15

Item {
    Component.onCompleted: {
        console.log("Component is ready")
        initialize()
    }

    Component.onDestruction: {
        cleanup()
    }
}
"#;

        let _symbols = extract_symbols(qml_code);

        // Note: Component.onCompleted might be extracted as a function or not extracted
        // This documents expected behavior
    }

    #[test]
    fn test_extract_property_change_signals() {
        let qml_code = r#"
import QtQuick 2.15

Rectangle {
    property int counter: 0

    onCounterChanged: {
        console.log("Counter changed to:", counter)
    }

    width: 100
    onWidthChanged: console.log("Width:", width)
}
"#;

        let _symbols = extract_symbols(qml_code);

        // Property change signals might be implicit
        // Document expected extraction behavior
    }

    #[test]
    fn test_extract_attached_signal_handlers() {
        let qml_code = r#"
import QtQuick 2.15

Item {
    Keys.onPressed: {
        if (event.key === Qt.Key_Escape) {
            Qt.quit()
        }
    }

    Keys.onReleased: {
        console.log("Key released")
    }
}
"#;

        let _symbols = extract_symbols(qml_code);

        // Attached signal handlers (Keys.onPressed)
        // Document extraction behavior
    }

    #[test]
    fn test_extract_multiple_signals_same_component() {
        let qml_code = r#"
import QtQuick 2.15

Item {
    signal started()
    signal stopped()
    signal paused()
    signal resumed()
    signal error(string message)
}
"#;

        let symbols = extract_symbols(qml_code);

        let signals: Vec<&Symbol> = symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Event)
            .collect();

        assert_eq!(signals.len(), 5, "Should extract all five signals");
    }

    #[test]
    fn signal_signature_is_the_declaration_with_typed_parameters() {
        let qml_code = r#"
import QtQuick 2.15

Item {
    signal closeRequested(string reason)
}
"#;

        let symbols = extract_symbols(qml_code);

        let signal = symbols
            .iter()
            .find(|s| s.name == "closeRequested")
            .expect("signal symbol");
        assert_eq!(
            signal.signature.as_deref(),
            Some("signal closeRequested(string reason)")
        );
        assert_eq!(
            signal
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("parameters")),
            Some(&serde_json::json!([{"name": "reason", "type": "string"}]))
        );
    }

    #[test]
    fn signal_without_parameters_records_no_parameter_metadata() {
        let qml_code = r#"
import QtQuick 2.15

Item {
    signal clicked()
}
"#;

        let symbols = extract_symbols(qml_code);

        let signal = symbols
            .iter()
            .find(|s| s.name == "clicked")
            .expect("signal symbol");
        assert_eq!(signal.signature.as_deref(), Some("signal clicked()"));
        assert!(
            signal
                .metadata
                .as_ref()
                .is_none_or(|metadata| !metadata.contains_key("parameters"))
        );
    }

    #[test]
    fn connections_handler_function_records_the_handled_signal() {
        let qml_code = r#"
import QtQuick 2.15

Item {
    Connections {
        target: backend
        function onReloaded() {}
    }
}
"#;

        let symbols = extract_symbols(qml_code);

        let handler = symbols
            .iter()
            .find(|symbol| symbol.name == "onReloaded")
            .expect("handler function symbol");
        assert_eq!(
            handler
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("handled_signal")),
            Some(&serde_json::json!("reloaded"))
        );
    }

    #[test]
    fn an_ordinary_function_records_no_handled_signal() {
        let qml_code = r#"
import QtQuick 2.15

Item {
    function onlyHelper() {}
}
"#;

        let symbols = extract_symbols(qml_code);

        let helper = symbols
            .iter()
            .find(|symbol| symbol.name == "onlyHelper")
            .expect("helper function symbol");
        assert!(
            helper
                .metadata
                .as_ref()
                .is_none_or(|metadata| !metadata.contains_key("handled_signal"))
        );
    }

    #[test]
    fn a_qualified_connections_object_keeps_its_handler_semantics() {
        let qml_code = r#"
import QtQml as Qml

Item {
    Qml.Connections {
        target: backend
        function onReloaded() {}
    }
}
"#;

        let symbols = extract_symbols(qml_code);
        let identifiers = extract_identifiers(qml_code);

        let handler = symbols
            .iter()
            .find(|symbol| symbol.name == "onReloaded")
            .expect("handler function symbol");
        assert_eq!(
            handler
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("handled_signal")),
            Some(&serde_json::json!("reloaded"))
        );

        let signal_handler = identifiers
            .iter()
            .find(|identifier| identifier.name == "reloaded")
            .expect("signal handler identifier");
        assert_eq!(
            signal_handler
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("receiver")),
            Some(&serde_json::json!("backend"))
        );
    }
}
