use crate::base::SymbolKind;
use crate::tests::qml::extract_symbols_with_path;
use serde_json::Value;

fn metadata<'a>(symbol: &'a crate::base::Symbol, key: &str) -> Option<&'a Value> {
    symbol
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
}

#[test]
fn qmltypes_publish_module_type_and_member_evidence() {
    let symbols = extract_symbols_with_path(
        r#"
import QtQuick.tool 1.2
Module {
        Component {
            name: "QQuickThing"
            prototype: "QObject"
            exports: ["QtQuick/Thing 1.0"]
            AttachedType {
                name: "ThingAttached"
                prototype: "QObject"
                revision: 1
            }
            Extension {
                name: "ThingExtension"
                prototype: "QObject"
                version: "1.2"
            }
            Property {
                name: "value"
                type: "int"
                revision: 2
            }
            Signal {
                name: "changed"
                Parameter {
                    name: "changedValue"
                    type: "int"
                }
            }
            Method {
                name: "reset"
                returnType: "void"
                Parameter {
                    name: "count"
                    type: "int"
                    revision: 3
                }
            }
            Enum {
                name: "Mode"
                revision: 4
                values: ["Off", "On"]
                EnumValue {
                    name: "Explicit"
                    value: 5
                }
            }
        }
}
"#,
        "QtQuick.qmltypes",
    );

    let module = symbols
        .iter()
        .find(|symbol| symbol.kind == SymbolKind::Module && symbol.name == "Module")
        .expect("qmltypes module symbol");
    assert_eq!(
        metadata(module, "typeinfo_kind").and_then(Value::as_str),
        Some("module")
    );

    let component = symbols
        .iter()
        .find(|symbol| symbol.kind == SymbolKind::Class && symbol.name == "QQuickThing")
        .expect("qmltypes component type symbol");
    assert_eq!(
        metadata(component, "prototype").and_then(Value::as_str),
        Some("QObject")
    );
    assert_eq!(
        metadata(component, "exports")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(1)
    );
    assert_eq!(component.parent_id.as_deref(), Some(module.id.as_str()));

    let attached = symbols
        .iter()
        .find(|symbol| symbol.kind == SymbolKind::Class && symbol.name == "ThingAttached")
        .expect("qmltypes attached type symbol");
    assert_eq!(
        metadata(attached, "typeinfo_kind").and_then(Value::as_str),
        Some("attached_type")
    );
    assert_eq!(
        metadata(attached, "revision").and_then(Value::as_u64),
        Some(1)
    );
    assert_eq!(attached.parent_id.as_deref(), Some(component.id.as_str()));

    let extension = symbols
        .iter()
        .find(|symbol| symbol.kind == SymbolKind::Class && symbol.name == "ThingExtension")
        .expect("qmltypes extension type symbol");
    assert_eq!(
        metadata(extension, "typeinfo_kind").and_then(Value::as_str),
        Some("extension")
    );
    assert_eq!(
        metadata(extension, "version").and_then(Value::as_str),
        Some("1.2")
    );
    assert_eq!(extension.parent_id.as_deref(), Some(component.id.as_str()));

    let property = symbols
        .iter()
        .find(|symbol| symbol.kind == SymbolKind::Property && symbol.name == "value")
        .expect("qmltypes property symbol");
    assert_eq!(
        metadata(property, "type").and_then(Value::as_str),
        Some("int")
    );
    assert_eq!(
        metadata(property, "revision").and_then(Value::as_u64),
        Some(2)
    );
    assert_eq!(property.parent_id.as_deref(), Some(component.id.as_str()));

    let signal = symbols
        .iter()
        .find(|symbol| symbol.kind == SymbolKind::Event && symbol.name == "changed")
        .expect("qmltypes signal symbol");
    let signal_parameter = symbols
        .iter()
        .find(|symbol| symbol.kind == SymbolKind::Variable && symbol.name == "changedValue")
        .expect("qmltypes signal parameter symbol");
    assert_eq!(
        signal_parameter.parent_id.as_deref(),
        Some(signal.id.as_str())
    );
    assert_eq!(
        metadata(signal_parameter, "type").and_then(Value::as_str),
        Some("int")
    );

    let method = symbols
        .iter()
        .find(|symbol| symbol.kind == SymbolKind::Method && symbol.name == "reset")
        .expect("qmltypes method symbol");
    let method_parameter = symbols
        .iter()
        .find(|symbol| symbol.kind == SymbolKind::Variable && symbol.name == "count")
        .expect("qmltypes method parameter symbol");
    assert_eq!(
        metadata(method, "returnType").and_then(Value::as_str),
        Some("void")
    );
    assert_eq!(
        method_parameter.parent_id.as_deref(),
        Some(method.id.as_str())
    );
    assert_eq!(
        metadata(method_parameter, "revision").and_then(Value::as_u64),
        Some(3)
    );

    let enum_symbol = symbols
        .iter()
        .find(|symbol| symbol.kind == SymbolKind::Enum && symbol.name == "Mode")
        .expect("qmltypes enum symbol");
    assert_eq!(
        metadata(enum_symbol, "revision").and_then(Value::as_u64),
        Some(4)
    );
    for name in ["Off", "On"] {
        let member = symbols
            .iter()
            .find(|symbol| symbol.kind == SymbolKind::EnumMember && symbol.name == name)
            .unwrap_or_else(|| panic!("qmltypes enum member {name}"));
        assert_eq!(member.parent_id.as_deref(), Some(enum_symbol.id.as_str()));
    }
    let explicit_member = symbols
        .iter()
        .find(|symbol| symbol.kind == SymbolKind::EnumMember && symbol.name == "Explicit")
        .expect("qmltypes explicit enum member");
    assert_eq!(
        explicit_member.parent_id.as_deref(),
        Some(enum_symbol.id.as_str())
    );
    assert_eq!(
        metadata(explicit_member, "value").and_then(Value::as_u64),
        Some(5)
    );
}
