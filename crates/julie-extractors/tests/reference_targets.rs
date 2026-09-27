use std::path::Path;

use julie_extractors::{
    ExtractionLevel, ExtractionResults, RelationshipKind, StructuredPendingRelationship,
    extract_canonical_for_language_at,
};

const PYTHON_SOURCE: &str =
    include_str!("../../../fixtures/extraction/python/reference_targets/source.py");
const JAVASCRIPT_SOURCE: &str =
    include_str!("../../../fixtures/extraction/javascript/reference_targets/source.js");
const JAVA_SOURCE: &str =
    include_str!("../../../fixtures/extraction/java/reference_targets/source.java");
const CSHARP_SOURCE: &str =
    include_str!("../../../fixtures/extraction/csharp/reference_targets/source.cs");
const TYPESCRIPT_SOURCE: &str =
    include_str!("../../../fixtures/extraction/typescript/reference_targets/source.ts");
const RUST_SOURCE: &str =
    include_str!("../../../fixtures/extraction/rust/reference_targets/source.rs");

fn extract(language: &str, path: &str, source: &str) -> ExtractionResults {
    extract_canonical_for_language_at(
        language,
        path,
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
        .unwrap_or_else(|| panic!("missing symbol {name:?}"))
        .id
        .clone()
}

fn call_targets(results: &ExtractionResults, caller: &str) -> Vec<String> {
    let caller_id = symbol_id(results, caller);
    results
        .relationships
        .iter()
        .filter(|relationship| {
            relationship.from_symbol_id == caller_id && relationship.kind == RelationshipKind::Calls
        })
        .map(|relationship| {
            results
                .symbols
                .iter()
                .find(|symbol| symbol.id == relationship.to_symbol_id)
                .unwrap()
                .name
                .clone()
        })
        .collect()
}

fn pending_call<'a>(
    results: &'a ExtractionResults,
    caller: &str,
    target: &str,
) -> &'a StructuredPendingRelationship {
    let caller_id = symbol_id(results, caller);
    results
        .structured_pending_relationships
        .iter()
        .find(|pending| {
            pending.caller_scope_symbol_id.as_deref() == Some(caller_id.as_str())
                && pending.target.terminal_name == target
                && pending.pending.kind == RelationshipKind::Calls
        })
        .unwrap_or_else(|| panic!("missing pending call {caller:?} -> {target:?}"))
}

fn assert_pending_site(
    results: &ExtractionResults,
    source: &str,
    caller: &str,
    target: &str,
    receiver: Option<&str>,
) {
    let pending = pending_call(results, caller, target);
    let caller_symbol_id = symbol_id(results, caller);
    let caller_symbol = results
        .symbols
        .iter()
        .find(|symbol| symbol.id == caller_symbol_id)
        .unwrap();
    let caller_source = &source[caller_symbol.start_byte as usize..caller_symbol.end_byte as usize];
    let expected_start = caller_symbol.start_byte as usize
        + caller_source.find(&format!("return {target}()")).unwrap()
        + "return ".len();
    let span = pending.span.unwrap();

    assert_eq!(span.start_byte as usize, expected_start);
    assert_eq!(span.end_byte as usize, expected_start + target.len());
    assert_eq!(pending.target.receiver.as_deref(), receiver);
    assert_eq!(
        pending.caller_scope_symbol_id.as_deref(),
        Some(caller_symbol_id.as_str())
    );
    assert!(pending.reference_site_is_exact);
}

#[test]
fn python_calls_respect_local_bindings_and_function_scopes() {
    let results = extract("python", "reference_targets.py", PYTHON_SOURCE);

    assert!(call_targets(&results, "parameter_shadow").is_empty());
    assert!(call_targets(&results, "local_shadow").is_empty());
    assert!(call_targets(&results, "owner_two").is_empty());
    assert_eq!(call_targets(&results, "forward_caller"), ["forward_target"]);
    assert_eq!(call_targets(&results, "recursive"), ["recursive"]);
    assert_eq!(call_targets(&results, "nested_caller"), ["nested_target"]);
    assert_eq!(call_targets(&results, "module_caller"), ["module_target"]);
    assert_pending_site(
        &results,
        PYTHON_SOURCE,
        "parameter_shadow",
        "module_target",
        None,
    );
    assert_pending_site(
        &results,
        PYTHON_SOURCE,
        "local_shadow",
        "module_target",
        None,
    );
    assert_pending_site(&results, PYTHON_SOURCE, "owner_two", "private_target", None);
}

#[test]
fn javascript_calls_respect_lexical_visibility_and_this_binding() {
    let results = extract("javascript", "reference_targets.js", JAVASCRIPT_SOURCE);

    assert!(call_targets(&results, "parameter_shadow").is_empty());
    assert!(call_targets(&results, "local_shadow").is_empty());
    assert!(call_targets(&results, "owner_two").is_empty());
    assert!(call_targets(&results, "method_caller").is_empty());
    assert_eq!(call_targets(&results, "forward_caller"), ["forward_target"]);
    assert_eq!(call_targets(&results, "nested_caller"), ["nested_target"]);
    assert_eq!(call_targets(&results, "arrow_caller"), ["nested"]);
    assert_eq!(call_targets(&results, "function_caller"), ["nested"]);
    let arrow_id = symbol_id(&results, "arrow_caller");
    let normal_id = symbol_id(&results, "function_caller");
    let render_id = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "render")
        .unwrap()
        .id
        .as_str();
    assert!(results.relationships.iter().any(|relationship| {
        relationship.to_symbol_id == render_id
            && results.symbols.iter().any(|symbol| {
                symbol.id == relationship.from_symbol_id
                    && symbol.parent_id.as_deref() == Some(arrow_id.as_str())
            })
    }));
    assert!(!results.relationships.iter().any(|relationship| {
        relationship.to_symbol_id == render_id
            && results.symbols.iter().any(|symbol| {
                symbol.id == relationship.from_symbol_id
                    && symbol.parent_id.as_deref() == Some(normal_id.as_str())
            })
    }));
    assert_pending_call_context(
        &results,
        JAVASCRIPT_SOURCE,
        "parameter_shadow",
        "target",
        "target()",
        None,
    );
    assert_pending_call_context(
        &results,
        JAVASCRIPT_SOURCE,
        "local_shadow",
        "target",
        "target()",
        None,
    );

    let this_call = results
        .structured_pending_relationships
        .iter()
        .find(|pending| {
            pending.target.terminal_name == "render"
                && pending.target.receiver.as_deref() == Some("this")
                && pending.receiver_type.is_none()
        })
        .unwrap();
    let span = this_call.span.unwrap();
    assert_eq!(
        &JAVASCRIPT_SOURCE[span.start_byte as usize..span.end_byte as usize],
        "this.render()"
    );
}

fn assert_pending_call_context(
    results: &ExtractionResults,
    source: &str,
    caller: &str,
    target: &str,
    expected_call: &str,
    receiver: Option<&str>,
) {
    let pending = pending_call(results, caller, target);
    let span = pending.span.unwrap();

    assert_eq!(
        &source[span.start_byte as usize..span.end_byte as usize],
        expected_call
    );
    assert_eq!(pending.target.receiver.as_deref(), receiver);
    assert_eq!(
        pending.caller_scope_symbol_id.as_deref(),
        Some(symbol_id(results, caller).as_str())
    );
}

#[test]
fn java_bare_calls_resolve_only_to_enclosing_type_methods() {
    let results = extract("java", "ReferenceTargets.java", JAVA_SOURCE);

    assert_eq!(call_targets(&results, "caller"), ["helper"]);
    assert!(call_targets(&results, "overload_caller").is_empty());
    assert_eq!(call_targets(&results, "local"), ["Hidden"]);
    assert!(call_targets(&results, "foreign").is_empty());
    let outsider_id = symbol_id(&results, "foreign");
    assert!(
        results
            .structured_pending_relationships
            .iter()
            .any(|pending| {
                pending.caller_scope_symbol_id.as_deref() == Some(outsider_id.as_str())
                    && pending.target.terminal_name == "Hidden"
                    && pending.pending.kind == RelationshipKind::Calls
            })
    );
}

#[test]
fn csharp_block_local_does_not_hide_enclosing_method_after_block() {
    let results = extract("csharp", "ReferenceTargets.cs", CSHARP_SOURCE);

    assert_eq!(call_targets(&results, "Caller"), ["Target"]);
}

#[test]
fn javascript_named_function_expression_name_is_lexically_visible() {
    let results = extract("javascript", "reference_targets.js", JAVASCRIPT_SOURCE);

    assert_eq!(
        call_targets(&results, "namedExpression"),
        ["namedExpression"]
    );
    assert!(call_targets(&results, "namedExpressionOutside").is_empty());
    assert_pending_call_context(
        &results,
        JAVASCRIPT_SOURCE,
        "namedExpressionOutside",
        "privateName",
        "privateName()",
        None,
    );
}

#[test]
fn typescript_named_function_expression_name_is_lexically_visible() {
    let results = extract("typescript", "reference_targets.ts", TYPESCRIPT_SOURCE);

    assert_eq!(
        call_targets(&results, "namedExpression"),
        ["namedExpression"]
    );
    assert!(call_targets(&results, "namedExpressionOutside").is_empty());
    assert_pending_call_context(
        &results,
        TYPESCRIPT_SOURCE,
        "namedExpressionOutside",
        "privateName",
        "privateName()",
        None,
    );
}

#[test]
fn rust_macro_bindings_do_not_leak_out_of_their_block() {
    let results = extract("rust", "reference_targets.rs", RUST_SOURCE);

    assert!(call_target_body_contains(
        &results,
        RUST_SOURCE,
        "before_inner_scope",
        6,
        "{ 1 }"
    ));
    assert!(call_target_body_contains(
        &results,
        RUST_SOURCE,
        "scope_owner",
        14,
        "{ 2 }"
    ));
    assert!(call_target_body_contains(
        &results,
        RUST_SOURCE,
        "scope_owner",
        16,
        "{ 1 }"
    ));
    let qualified_id = symbol_id(&results, "qualified");
    assert!(
        results
            .structured_pending_relationships
            .iter()
            .any(|pending| {
                pending.caller_scope_symbol_id.as_deref() == Some(qualified_id.as_str())
                    && pending.target.terminal_name == "render"
                    && pending.target.display_name == "crate::render"
                    && pending.target.namespace_path == ["crate"]
                    && pending.pending.kind == RelationshipKind::Calls
            })
    );
}

fn call_target_body_contains(
    results: &ExtractionResults,
    source: &str,
    caller: &str,
    call_line: u32,
    expected: &str,
) -> bool {
    let caller_id = symbol_id(results, caller);
    let Some(relationship) = results.relationships.iter().find(|relationship| {
        relationship.from_symbol_id == caller_id
            && relationship.kind == RelationshipKind::Calls
            && relationship.line_number == call_line
    }) else {
        return false;
    };
    let Some(target) = results
        .symbols
        .iter()
        .find(|symbol| symbol.id == relationship.to_symbol_id)
    else {
        return false;
    };
    target.body_span.is_some_and(|span| {
        source[span.start_byte as usize..span.end_byte as usize].contains(expected)
    })
}
