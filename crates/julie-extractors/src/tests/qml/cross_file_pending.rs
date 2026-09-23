//! Phase 4b.qml — QML emits `StructuredPendingRelationship` for
//! cross-module calls (`import "OtherModule"; external_helper()`).

use crate::extract_canonical;
use std::path::Path;

#[test]
fn test_qml_emits_structured_pending_for_cross_module_call() {
    let source = include_str!("../../../../../fixtures/extraction/qml/cross_file/source.qml");
    let workspace_root = Path::new("/tmp/test");
    let result = extract_canonical("source.qml", source, workspace_root)
        .expect("canonical QML extraction must succeed");

    let external = result
        .structured_pending_relationships
        .iter()
        .find(|p| p.target.terminal_name == "external_helper")
        .unwrap_or_else(|| {
            panic!(
                "expected structured pending for cross-module external_helper; got {} entries: {:#?}",
                result.structured_pending_relationships.len(),
                result.structured_pending_relationships
            )
        });

    assert!(external.pending.line_number > 0);
    assert_eq!(external.pending.file_path, "source.qml");

    assert!(
        result
            .structured_pending_relationships
            .iter()
            .all(|p| p.target.terminal_name != "local_helper"),
        "intra-file local_helper must not appear as structured pending"
    );
}

#[test]
fn inaccessible_component_calls_emit_structured_pending_relationships() {
    let source = r#"
import QtQuick 2.15

Item {
    id: root
    function helper() {}

    function parameterShadow(root) {
        root.helper()
    }

    component First: Item {
        function run() {
            secondOnly()
        }
        Component.onCompleted: secondOnly()
    }

    component Second: Item {
        function secondOnly() {}
    }
}
"#;
    let result = extract_canonical("scope.qml", source, Path::new("/tmp/test"))
        .expect("canonical QML extraction must succeed");

    for name in ["helper", "secondOnly"] {
        assert!(
            result
                .structured_pending_relationships
                .iter()
                .any(|pending| pending.target.terminal_name == name),
            "expected pending {name}, got {:#?}",
            result.structured_pending_relationships
        );
    }

    let first = result
        .symbols
        .iter()
        .find(|symbol| symbol.name == "First")
        .expect("first inline component");
    let first_handler = result
        .symbols
        .iter()
        .find(|symbol| {
            symbol.name == "Component.onCompleted"
                && symbol.parent_id.as_deref() == Some(first.id.as_str())
        })
        .expect("first inline component handler");
    assert!(
        result
            .structured_pending_relationships
            .iter()
            .any(|pending| {
                pending.target.terminal_name == "secondOnly"
                    && pending.pending.from_symbol_id == first_handler.id
            })
    );
}

#[test]
fn test_qml_negative_local_helper_not_emitted_as_pending() {
    let source = include_str!("../../../../../fixtures/extraction/qml/cross_file/source.qml");
    let workspace_root = Path::new("/tmp/test");
    let result = extract_canonical("source.qml", source, workspace_root)
        .expect("canonical QML extraction must succeed");

    assert!(
        result
            .structured_pending_relationships
            .iter()
            .all(|p| p.pending.callee_name != "local_helper"
                && p.target.terminal_name != "local_helper"),
        "intra-file local_helper leaked into pending"
    );
}
