use std::collections::BTreeSet;
use std::path::Path;

use crate::{SymbolKind, extract_canonical};

#[test]
fn extensionless_qmldir_extracts_module_and_components() {
    let result = extract_canonical(
        "qmldir",
        "module QtQuick.Controls\nButton 2.15 Button.qml\nsingleton Theme 1.0 Theme.qml\n",
        Path::new("."),
    )
    .expect("qmldir extraction should succeed");

    assert!(
        result
            .symbols
            .iter()
            .any(|symbol| symbol.name == "QtQuick.Controls" && symbol.kind == SymbolKind::Module)
    );
    assert!(
        result
            .symbols
            .iter()
            .any(|symbol| symbol.name == "Button" && symbol.kind == SymbolKind::Class)
    );
    assert!(
        result
            .symbols
            .iter()
            .any(|symbol| symbol.name == "Theme" && symbol.kind == SymbolKind::Class)
    );
}

#[test]
fn qmldir_component_symbols_preserve_manifest_metadata_and_spans() {
    let source = "module Example\nsingleton Theme 1.0 Theme.qml\ninternal Internal Internal.qml\n";
    let result = extract_canonical("/project/qmldir", source, Path::new("/project"))
        .expect("qmldir extraction should succeed");

    let theme = result
        .symbols
        .iter()
        .find(|symbol| symbol.name == "Theme")
        .expect("singleton declaration should be a symbol");
    assert_eq!(theme.start_line, 2);
    assert_eq!(theme.start_column, 0);
    assert_eq!(theme.end_line, 3);
    assert_eq!(
        theme
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("singleton")),
        Some(&serde_json::Value::Bool(true))
    );
    assert_eq!(
        theme
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("version")),
        Some(&serde_json::Value::String("1.0".to_string()))
    );
    assert_eq!(
        theme
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("file")),
        Some(&serde_json::Value::String("Theme.qml".to_string()))
    );

    assert!(
        result
            .symbols
            .iter()
            .any(|symbol| symbol.name == "Internal")
    );
}

#[test]
fn qmldir_directive_matrix_emits_typed_facts_and_negative_controls() {
    let source = concat!(
        "module Example.Module\n",
        "Button 1.0 Button.qml\n",
        "singleton Theme 1.0 Theme.qml\n",
        "internal Private Private.qml\n",
        "MyScript 1.0 MyScript.js\n",
        "plugin examplemodule plugins\n",
        "optional plugin optionalmodule\n",
        "classname ExamplePlugin\n",
        "typeinfo plugins.qmltypes\n",
        "depends QtQuick 2.15\n",
        "import Shared auto\n",
        "designersupported\n",
        "prefer :/qt/qml/Example/Module\n",
        "linktarget ExampleModule\n",
        "not_a_directive not-a-version not-a-file\n",
    );
    let result = extract_canonical("qmldir", source, Path::new("."))
        .expect("qmldir extraction should succeed");

    let fact = |pattern_id: &str| {
        result
            .structural_facts
            .iter()
            .find(|fact| fact.pattern_id == pattern_id)
            .unwrap_or_else(|| panic!("missing structural fact {pattern_id}"))
    };
    let metadata = |pattern_id: &str, key: &str| {
        fact(pattern_id)
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get(key))
            .cloned()
            .unwrap_or_else(|| panic!("missing {key} metadata for {pattern_id}"))
    };
    let emitted_pattern_ids: BTreeSet<_> = result
        .structural_facts
        .iter()
        .map(|fact| fact.pattern_id.as_str())
        .filter(|pattern_id| pattern_id.starts_with("qmldir."))
        .collect();
    let expected_pattern_ids: BTreeSet<_> = crate::qmldir::STRUCTURAL_FACT_PATTERN_IDS
        .iter()
        .copied()
        .collect();
    assert_eq!(emitted_pattern_ids, expected_pattern_ids);

    assert_eq!(
        metadata("qmldir.module.v1", "module"),
        serde_json::json!("Example.Module")
    );
    assert_eq!(
        metadata("qmldir.object_type.v1", "type_name"),
        serde_json::json!("Button")
    );
    assert_eq!(
        metadata("qmldir.singleton_type.v1", "singleton"),
        serde_json::json!(true)
    );
    assert_eq!(
        metadata("qmldir.internal_type.v1", "file"),
        serde_json::json!("Private.qml")
    );
    assert_eq!(
        metadata("qmldir.javascript_resource.v1", "resource_name"),
        serde_json::json!("MyScript")
    );
    assert_eq!(
        metadata("qmldir.plugin.v1", "name"),
        serde_json::json!("examplemodule")
    );
    assert_eq!(
        metadata("qmldir.classname.v1", "class_name"),
        serde_json::json!("ExamplePlugin")
    );
    assert_eq!(
        metadata("qmldir.typeinfo.v1", "file"),
        serde_json::json!("plugins.qmltypes")
    );
    assert_eq!(
        metadata("qmldir.depends.v1", "version"),
        serde_json::json!("2.15")
    );
    assert_eq!(
        metadata("qmldir.import.v1", "version"),
        serde_json::json!("auto")
    );
    assert_eq!(
        metadata("qmldir.designer_supported.v1", "supported"),
        serde_json::json!(true)
    );
    assert_eq!(
        metadata("qmldir.prefer.v1", "path"),
        serde_json::json!(":/qt/qml/Example/Module")
    );
    assert_eq!(
        metadata("qmldir.linktarget.v1", "target"),
        serde_json::json!("ExampleModule")
    );
    assert_eq!(fact("qmldir.plugin.v1").start_line, 6);

    assert!(
        result
            .symbols
            .iter()
            .find(|symbol| symbol.name == "Private")
            .is_some_and(|symbol| symbol.visibility == Some(crate::base::Visibility::Internal))
    );
    assert!(!result.symbols.iter().any(|symbol| {
        [
            "ExamplePlugin",
            "plugins.qmltypes",
            "examplemodule",
            "optionalmodule",
            "Shared",
        ]
        .contains(&symbol.name.as_str())
    }));
    assert!(
        !result
            .structural_facts
            .iter()
            .any(|fact| fact.pattern_id.contains("not_a_directive"))
    );
}

#[test]
fn qmldir_parser_recovers_after_malformed_lines() {
    let result = extract_canonical(
        "qmldir",
        "module Example\n! malformed\nButton 1.0 Button.qml\n",
        Path::new("."),
    )
    .expect("qmldir extraction should recover from malformed lines");

    assert!(result.symbols.iter().any(|symbol| symbol.name == "Example"));
    assert!(result.symbols.iter().any(|symbol| symbol.name == "Button"));
}

#[test]
fn qmldir_import_forms_emit_optional_and_default_metadata() {
    let source = concat!(
        "module Example\n",
        "import QtQuick 2.15\n",
        "optional import QtQml.Models 2.15\n",
        "default import QtQuick.Controls 2.15\n",
        "optional import Versionless.Module\n",
        "optional import Broken nope\n",
        "optional import\n",
        "default import\n",
        "optional plugin examplemodule\n",
    );
    let result = extract_canonical("qmldir", source, Path::new("."))
        .expect("qmldir extraction should succeed");

    let imports: Vec<_> = result
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == "qmldir.import.v1")
        .collect();
    assert_eq!(imports.len(), 4);

    for (fact, expected) in imports.iter().take(3).zip([
        ("import", "QtQuick", "2.15", false, false),
        ("optional", "QtQml.Models", "2.15", true, false),
        ("default", "QtQuick.Controls", "2.15", true, true),
    ]) {
        let metadata = fact
            .metadata
            .as_ref()
            .expect("qmldir import facts should have metadata");
        assert_eq!(
            metadata.get("directive"),
            Some(&serde_json::json!(expected.0))
        );
        assert_eq!(metadata.get("module"), Some(&serde_json::json!(expected.1)));
        assert_eq!(
            metadata.get("version"),
            Some(&serde_json::json!(expected.2))
        );
        assert_eq!(
            metadata.get("optional"),
            Some(&serde_json::json!(expected.3))
        );
        assert_eq!(
            metadata.get("default"),
            Some(&serde_json::json!(expected.4))
        );
    }
    assert!(
        !imports[3]
            .metadata
            .as_ref()
            .expect("versionless qmldir import should have metadata")
            .contains_key("version")
    );
}
