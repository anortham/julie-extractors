use std::path::Path;

use julie_extractors::{
    ExtractionLevel, ExtractionResults, RelationshipKind, extract_canonical_for_language_at,
};

const EXTENSION_SOURCE: &str = r#"
class Builder {
    fun append(s: String) {}
}
class Outer {
    fun Builder.ext() {
        this.append("x")
    }
}
"#;

const UNRESOLVED_SOURCE: &str = r#"
class Container {
    fun run() {
        this.missing()
    }
}
"#;

fn extract(source: &str) -> ExtractionResults {
    extract_canonical_for_language_at(
        "kotlin",
        "ReferenceTargets.kt",
        source,
        Path::new("."),
        ExtractionLevel::Full,
    )
    .unwrap()
}

fn symbol_id(results: &ExtractionResults, name: &str) -> String {
    results
        .symbols
        .iter()
        .find(|symbol| symbol.name == name)
        .unwrap()
        .id
        .clone()
}

fn child_symbol_id(results: &ExtractionResults, parent_id: &str, name: &str) -> String {
    results
        .symbols
        .iter()
        .find(|symbol| symbol.parent_id.as_deref() == Some(parent_id) && symbol.name == name)
        .unwrap()
        .id
        .clone()
}

fn has_call_target(results: &ExtractionResults, caller_id: &str, target_id: &str) -> bool {
    results.relationships.iter().any(|relationship| {
        relationship.from_symbol_id == caller_id
            && relationship.to_symbol_id == target_id
            && relationship.kind == RelationshipKind::Calls
    })
}

#[test]
fn extension_this_calls_resolve_to_members_of_the_receiver_type() {
    let results = extract(EXTENSION_SOURCE);
    let caller_id = symbol_id(&results, "ext");
    let target_id = symbol_id(&results, "append");

    assert!(results.relationships.iter().any(|relationship| {
        relationship.from_symbol_id == caller_id
            && relationship.to_symbol_id == target_id
            && relationship.kind == RelationshipKind::Calls
    }));
    assert!(
        results
            .structured_pending_relationships
            .iter()
            .all(|pending| {
                pending.caller_scope_symbol_id.as_deref() != Some(caller_id.as_str())
                    || pending.target.terminal_name != "append"
            })
    );
}

#[test]
fn extension_this_calls_resolve_to_the_visible_nested_receiver_owner() {
    let results = extract(
        r#"
class Container {
    class Builder {
        fun append(value: String) {}
    }
    class Other {
        class Builder {
            fun append(value: String) {}
        }
    }
    fun Builder.ext() {
        this.append("x")
    }
}
"#,
    );
    let container_id = symbol_id(&results, "Container");
    let expected_builder_id = child_symbol_id(&results, &container_id, "Builder");
    let other_id = child_symbol_id(&results, &container_id, "Other");
    let unrelated_builder_id = child_symbol_id(&results, &other_id, "Builder");
    let caller_id = child_symbol_id(&results, &container_id, "ext");
    let expected_member_id = child_symbol_id(&results, &expected_builder_id, "append");
    let unrelated_member_id = child_symbol_id(&results, &unrelated_builder_id, "append");

    assert!(has_call_target(&results, &caller_id, &expected_member_id));
    assert!(!has_call_target(&results, &caller_id, &unrelated_member_id));
}

#[test]
fn extension_this_calls_prefer_receiver_members_over_same_name_bindings() {
    let results = extract(
        r#"
fun append(value: String) {}
class Builder {
    fun append(value: String) {}
}
class Outer {
    fun append(value: String) {}
    fun Builder.ext() {
        this.append("x")
    }
}
"#,
    );
    let builder_id = symbol_id(&results, "Builder");
    let builder_member_id = child_symbol_id(&results, &builder_id, "append");
    let outer_id = symbol_id(&results, "Outer");
    let outer_member_id = child_symbol_id(&results, &outer_id, "append");
    let global_member_id = results
        .symbols
        .iter()
        .find(|symbol| symbol.parent_id.is_none() && symbol.name == "append")
        .unwrap()
        .id
        .clone();
    let caller_id = child_symbol_id(&results, &outer_id, "ext");

    assert!(has_call_target(&results, &caller_id, &builder_member_id));
    assert!(!has_call_target(&results, &caller_id, &outer_member_id));
    assert!(!has_call_target(&results, &caller_id, &global_member_id));
}

#[test]
fn extension_this_calls_with_type_parameter_receivers_remain_pending() {
    let results = extract(
        r#"
class T {
    fun append(value: String) {}
}
fun <T> T.ext() {
    this.append("x")
}
"#,
    );
    let caller_id = symbol_id(&results, "ext");
    let type_id = symbol_id(&results, "T");
    let member_id = child_symbol_id(&results, &type_id, "append");
    let pending: Vec<_> = results
        .structured_pending_relationships
        .iter()
        .filter(|pending| {
            pending.caller_scope_symbol_id.as_deref() == Some(caller_id.as_str())
                && pending.target.terminal_name == "append"
                && pending.pending.kind == RelationshipKind::Calls
        })
        .collect();

    assert!(!has_call_target(&results, &caller_id, &member_id));
    assert_eq!(pending.len(), 1);
}

#[test]
fn extension_this_calls_with_qualified_receivers_remain_pending() {
    let results = extract(
        r#"
class Builder {
    fun append(value: String) {}
}
fun unresolved.package.Builder.ext() {
    this.append("x")
}
"#,
    );
    let caller_id = symbol_id(&results, "ext");
    let builder_id = symbol_id(&results, "Builder");
    let member_id = child_symbol_id(&results, &builder_id, "append");
    let pending: Vec<_> = results
        .structured_pending_relationships
        .iter()
        .filter(|pending| {
            pending.caller_scope_symbol_id.as_deref() == Some(caller_id.as_str())
                && pending.target.terminal_name == "append"
                && pending.pending.kind == RelationshipKind::Calls
        })
        .collect();

    assert!(!has_call_target(&results, &caller_id, &member_id));
    assert_eq!(pending.len(), 1);
}

#[test]
fn extension_this_infix_calls_and_function_references_resolve_to_receiver_members() {
    let results = extract(
        r#"
class Builder {
    infix fun append(value: String) {}
    fun remove(value: String) {}
}
fun Builder.ext() {
    this append "x"
    val callback = this::remove
}
"#,
    );
    let builder_id = symbol_id(&results, "Builder");
    let append_id = child_symbol_id(&results, &builder_id, "append");
    let remove_id = child_symbol_id(&results, &builder_id, "remove");
    let caller_id = symbol_id(&results, "ext");

    assert!(has_call_target(&results, &caller_id, &append_id));
    assert!(has_call_target(&results, &caller_id, &remove_id));
}

#[test]
fn nested_extension_this_calls_do_not_resolve_to_same_name_outer_bindings() {
    let results = extract(
        r#"
fun append(value: String) {}
class Builder {
    fun append(value: String) {}
}
class Outer {
    fun append(value: String) {}
    fun Builder.ext() {
        fun nested() {
            this.append("x")
        }
        nested()
    }
}
"#,
    );
    let builder_id = symbol_id(&results, "Builder");
    let builder_member_id = child_symbol_id(&results, &builder_id, "append");
    let outer_id = symbol_id(&results, "Outer");
    let outer_member_id = child_symbol_id(&results, &outer_id, "append");
    let global_member_id = results
        .symbols
        .iter()
        .find(|symbol| symbol.parent_id.is_none() && symbol.name == "append")
        .unwrap()
        .id
        .clone();
    let extension_id = child_symbol_id(&results, &outer_id, "ext");
    let nested_id = child_symbol_id(&results, &extension_id, "nested");
    let pending: Vec<_> = results
        .structured_pending_relationships
        .iter()
        .filter(|pending| {
            pending.caller_scope_symbol_id.as_deref() == Some(nested_id.as_str())
                && pending.target.terminal_name == "append"
                && pending.pending.kind == RelationshipKind::Calls
        })
        .collect();

    assert!(has_call_target(&results, &nested_id, &builder_member_id) || pending.len() == 1);
    assert!(!has_call_target(&results, &nested_id, &outer_member_id));
    assert!(!has_call_target(&results, &nested_id, &global_member_id));
}

#[test]
fn class_this_calls_prefer_members_over_same_name_local_functions() {
    let results = extract(
        r#"
class Container {
    fun append(value: String) {}
    fun run() {
        fun append(value: String) {}
        this.append("x")
    }
}
"#,
    );
    let container_id = symbol_id(&results, "Container");
    let member_id = child_symbol_id(&results, &container_id, "append");
    let caller_id = child_symbol_id(&results, &container_id, "run");
    let local_id = child_symbol_id(&results, &caller_id, "append");

    assert!(has_call_target(&results, &caller_id, &member_id));
    assert!(!has_call_target(&results, &caller_id, &local_id));
}

#[test]
fn unresolved_call_sites_emit_one_pending_row() {
    let results = extract(UNRESOLVED_SOURCE);
    let caller_id = symbol_id(&results, "run");
    let pending: Vec<_> = results
        .structured_pending_relationships
        .iter()
        .filter(|pending| {
            pending.caller_scope_symbol_id.as_deref() == Some(caller_id.as_str())
                && pending.target.terminal_name == "missing"
                && pending.pending.kind == RelationshipKind::Calls
        })
        .collect();

    assert_eq!(pending.len(), 1);
    let span = pending[0].span.unwrap();
    assert_eq!(
        &UNRESOLVED_SOURCE[span.start_byte as usize..span.end_byte as usize],
        "this.missing()"
    );
}
