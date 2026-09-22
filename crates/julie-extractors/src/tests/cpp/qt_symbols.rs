//! Qt properties, signals, slots and QML element metadata built from the macro sites.

use crate::base::{
    ExtractionLevel, ExtractionResults, StructuralFact, Symbol, SymbolKind, Visibility,
};
use crate::pipeline::extract_canonical_at;
use std::path::Path;

const HEADER_PATH: &str = "src/layouts/columnview.h";

fn extract(source: &str) -> ExtractionResults {
    extract_at(source, ExtractionLevel::Facts)
}

fn extract_at(source: &str, level: ExtractionLevel) -> ExtractionResults {
    extract_canonical_at(HEADER_PATH, source, Path::new("/repo"), level)
        .expect("a Qt header should extract")
}

fn in_class(body: &str) -> String {
    format!("class ColumnView : public QQuickItem\n{{\n{body}}};\n")
}

fn symbol<'a>(results: &'a ExtractionResults, name: &str) -> &'a Symbol {
    results
        .symbols
        .iter()
        .find(|symbol| symbol.name == name)
        .unwrap_or_else(|| panic!("no symbol named {name}"))
}

fn text(symbol: &Symbol, key: &str) -> String {
    symbol
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|value| value.as_str())
        .unwrap_or_else(|| panic!("symbol {} is missing metadata key {key}", symbol.name))
        .to_string()
}

fn flag(symbol: &Symbol, key: &str) -> Option<bool> {
    symbol
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|value| value.as_bool())
}

fn property_facts(results: &ExtractionResults) -> Vec<&StructuralFact> {
    results
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == "cpp.qt_property.v1")
        .collect()
}

#[test]
fn a_property_with_read_write_notify_becomes_a_property_under_its_class() {
    let source = in_class(
        "    Q_OBJECT\n    Q_PROPERTY(int index READ index WRITE setIndex NOTIFY indexChanged FINAL)\n",
    );

    let results = extract(&source);
    let property = symbol(&results, "index");

    assert_eq!(property.kind, SymbolKind::Property);
    assert_eq!(property.visibility, Some(Visibility::Public));
    assert_eq!(
        property.parent_id.as_deref(),
        Some(symbol(&results, "ColumnView").id.as_str())
    );
    assert_eq!(
        property.signature.as_deref(),
        Some("Q_PROPERTY(int index READ index WRITE setIndex NOTIFY indexChanged FINAL)")
    );
    assert_eq!(text(property, "property_type"), "int");
    assert_eq!(text(property, "read"), "index");
    assert_eq!(text(property, "write"), "setIndex");
    assert_eq!(text(property, "notify"), "indexChanged");
    assert_eq!(flag(property, "final"), Some(true));
    assert_eq!(flag(property, "constant"), None);
}

#[test]
fn a_member_property_records_member_constant_and_final() {
    let source = in_class("    Q_PROPERTY(QPointF delta MEMBER delta CONSTANT FINAL)\n");

    let results = extract(&source);
    let property = symbol(&results, "delta");

    assert_eq!(property.kind, SymbolKind::Property);
    assert_eq!(text(property, "property_type"), "QPointF");
    assert_eq!(text(property, "member"), "delta");
    assert_eq!(flag(property, "constant"), Some(true));
    assert_eq!(flag(property, "final"), Some(true));
}

#[test]
fn a_pointer_typed_property_keeps_the_pointer_in_its_type() {
    let source = in_class("    Q_PROPERTY(QQuickItem *view READ view NOTIFY viewChanged FINAL)\n");

    let results = extract(&source);
    let property = symbol(&results, "view");

    assert_eq!(property.kind, SymbolKind::Property);
    assert_eq!(text(property, "property_type"), "QQuickItem *");
    assert_eq!(text(property, "read"), "view");
}

#[test]
fn a_multi_line_property_collapses_its_whitespace_into_one_signature() {
    let source = in_class(
        "    Q_PROPERTY(qreal reservedSpace\n               READ reservedSpace\n               WRITE setReservedSpace\n               NOTIFY reservedSpaceChanged)\n",
    );

    let results = extract(&source);
    let property = symbol(&results, "reservedSpace");

    assert_eq!(property.kind, SymbolKind::Property);
    assert_eq!(
        property.signature.as_deref(),
        Some(
            "Q_PROPERTY(qreal reservedSpace READ reservedSpace WRITE setReservedSpace NOTIFY reservedSpaceChanged)"
        )
    );
    assert_eq!(text(property, "property_type"), "qreal");
    assert_eq!(property.start_line, 3);
    assert_eq!(property.end_line, 6);
}

#[test]
fn a_property_emits_a_fact_anchored_to_its_class() {
    let source = in_class("    Q_PROPERTY(int index READ index NOTIFY indexChanged)\n");

    let results = extract(&source);
    let facts = property_facts(&results);

    assert_eq!(facts.len(), 1);
    let fact = facts[0];
    assert_eq!(fact.capture_name, "qt_property");
    assert_eq!(fact.node_kind, "macro");
    assert_eq!(fact.language, "cpp");
    assert_eq!(
        fact.containing_symbol_id.as_deref(),
        Some(symbol(&results, "ColumnView").id.as_str())
    );
    let metadata = fact.metadata.as_ref().expect("the fact carries metadata");
    assert_eq!(metadata.get("name").and_then(|v| v.as_str()), Some("index"));
    assert_eq!(
        metadata.get("property_type").and_then(|v| v.as_str()),
        Some("int")
    );
    assert_eq!(
        metadata.get("query_family").and_then(|v| v.as_str()),
        Some("properties")
    );
    let property = symbol(&results, "index");
    assert_eq!(fact.start_byte, property.start_byte);
    assert_eq!(fact.end_byte, property.end_byte);
}

#[test]
fn the_symbols_level_emits_no_property_fact() {
    let source = in_class("    Q_PROPERTY(int index READ index NOTIFY indexChanged)\n");

    let results = extract_at(&source, ExtractionLevel::Symbols);

    assert!(property_facts(&results).is_empty());
    assert_eq!(symbol(&results, "index").kind, SymbolKind::Property);
}

#[test]
fn a_malformed_property_emits_nothing() {
    let source = in_class("    Q_PROPERTY(READ index NOTIFY indexChanged)\n");

    let results = extract(&source);

    assert!(
        !results
            .symbols
            .iter()
            .any(|symbol| symbol.kind == SymbolKind::Property)
    );
    assert!(property_facts(&results).is_empty());
}

#[test]
fn a_property_outside_any_class_emits_nothing() {
    let source = "Q_PROPERTY(int index READ index NOTIFY indexChanged)\n";

    let results = extract(source);

    assert!(
        !results
            .symbols
            .iter()
            .any(|symbol| symbol.kind == SymbolKind::Property)
    );
    assert!(property_facts(&results).is_empty());
}

#[test]
fn signals_after_a_bare_q_signals_label_are_public_events() {
    let source = in_class("Q_SIGNALS:\n    void indexChanged();\n");

    let results = extract(&source);
    let signal = symbol(&results, "indexChanged");

    assert_eq!(signal.kind, SymbolKind::Event);
    assert_eq!(signal.visibility, Some(Visibility::Public));
}

#[test]
fn signals_after_a_lowercase_signals_label_are_public_events() {
    let source = in_class("signals:\n    void indexChanged();\n");

    let results = extract(&source);
    let signal = symbol(&results, "indexChanged");

    assert_eq!(signal.kind, SymbolKind::Event);
    assert_eq!(signal.visibility, Some(Visibility::Public));
}

#[test]
fn a_public_slots_method_is_public_and_marked_as_a_slot() {
    let source = in_class("public Q_SLOTS:\n    void addItem(QQuickItem *item);\n");

    let results = extract(&source);
    let slot = symbol(&results, "addItem");

    assert_eq!(slot.kind, SymbolKind::Method);
    assert_eq!(slot.visibility, Some(Visibility::Public));
    assert_eq!(flag(slot, "qt_slot"), Some(true));
}

#[test]
fn a_private_slots_method_stays_private() {
    let source = in_class("private Q_SLOTS:\n    void onTimeout();\n");

    let results = extract(&source);
    let slot = symbol(&results, "onTimeout");

    assert_eq!(slot.visibility, Some(Visibility::Private));
    assert_eq!(flag(slot, "qt_slot"), Some(true));
}

#[test]
fn a_method_after_a_plain_access_label_is_no_longer_in_the_section() {
    let source =
        in_class("Q_SIGNALS:\n    void indexChanged();\n\npublic:\n    int index() const;\n");

    let results = extract(&source);

    assert_eq!(symbol(&results, "indexChanged").kind, SymbolKind::Event);
    let getter = symbol(&results, "index");
    assert_eq!(getter.kind, SymbolKind::Method);
    assert_eq!(getter.visibility, Some(Visibility::Public));
}

#[test]
fn a_q_invokable_method_is_marked_invokable() {
    let source = in_class("public:\n    Q_INVOKABLE QQuickItem *get(int index);\n");

    let results = extract(&source);
    let method = symbol(&results, "get");

    assert_eq!(flag(method, "qt_invokable"), Some(true));
    assert_eq!(flag(method, "qt_slot"), None);
}

#[test]
fn a_q_signal_prefixed_method_is_an_event() {
    let source = in_class("public:\n    Q_SIGNAL void indexChanged();\n");

    let results = extract(&source);

    assert_eq!(symbol(&results, "indexChanged").kind, SymbolKind::Event);
}

#[test]
fn a_q_slot_prefixed_method_is_marked_as_a_slot() {
    let source = in_class("public:\n    Q_SLOT void refresh();\n");

    let results = extract(&source);

    assert_eq!(flag(symbol(&results, "refresh"), "qt_slot"), Some(true));
}

#[test]
fn qml_element_names_the_class_it_sits_in() {
    let source = in_class("    Q_OBJECT\n    QML_ELEMENT\n");

    let results = extract(&source);
    let class = symbol(&results, "ColumnView");

    assert_eq!(text(class, "qml_element"), "ColumnView");
    assert_eq!(flag(class, "qt_object"), Some(true));
}

#[test]
fn qml_named_element_overrides_the_qml_element_name() {
    let source = in_class("    QML_NAMED_ELEMENT(Column)\n");

    let results = extract(&source);

    assert_eq!(
        text(symbol(&results, "ColumnView"), "qml_element"),
        "Column"
    );
}

#[test]
fn qml_singleton_and_uncreatable_record_their_values() {
    let source =
        in_class("    QML_ELEMENT\n    QML_SINGLETON\n    QML_UNCREATABLE(\"Use the factory\")\n");

    let results = extract(&source);
    let class = symbol(&results, "ColumnView");

    assert_eq!(flag(class, "qml_singleton"), Some(true));
    assert_eq!(text(class, "qml_uncreatable"), "Use the factory");
}

#[test]
fn qml_attached_and_anonymous_record_their_values() {
    let source = in_class("    QML_ANONYMOUS\n    QML_ATTACHED(ColumnViewAttached)\n");

    let results = extract(&source);
    let class = symbol(&results, "ColumnView");

    assert_eq!(flag(class, "qml_anonymous"), Some(true));
    assert_eq!(text(class, "qml_attached"), "ColumnViewAttached");
}

#[test]
fn q_gadget_marks_the_struct_it_sits_in() {
    let source = "struct Point\n{\n    Q_GADGET\npublic:\n    int x = 0;\n};\n";

    let results = extract(source);

    assert_eq!(flag(symbol(&results, "Point"), "qt_gadget"), Some(true));
}

#[test]
fn a_qml_macro_outside_any_class_sets_no_metadata() {
    let source = "QML_ELEMENT\n\nclass ColumnView : public QQuickItem\n{\n};\n";

    let results = extract(source);

    assert_eq!(flag(symbol(&results, "ColumnView"), "qml_element"), None);
    assert!(
        symbol(&results, "ColumnView")
            .metadata
            .as_ref()
            .is_none_or(|metadata| !metadata.contains_key("qml_element"))
    );
}

#[test]
fn two_runs_give_the_same_property_symbol_and_fact_ids() {
    let source = in_class("    Q_PROPERTY(int index READ index NOTIFY indexChanged)\n");

    let first = extract(&source);
    let second = extract(&source);

    assert_eq!(symbol(&first, "index").id, symbol(&second, "index").id);
    assert_eq!(property_facts(&first)[0].id, property_facts(&second)[0].id);
}
