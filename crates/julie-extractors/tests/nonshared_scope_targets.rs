use std::path::Path;

use julie_extractors::{
    ExtractionLevel, ExtractionResults, RelationshipKind, StructuredPendingRelationship, Symbol,
    extract_canonical_for_language_at,
};

const LUA_SOURCE: &str =
    include_str!("../../../fixtures/extraction/lua/reference_targets/source.lua");
const QML_SOURCE: &str =
    include_str!("../../../fixtures/extraction/qml/reference_targets/source.qml");
const POWERSHELL_SOURCE: &str =
    include_str!("../../../fixtures/extraction/powershell/reference_targets/source.ps1");
const GO_SOURCE: &str = include_str!("../../../fixtures/extraction/go/reference_targets/source.go");
const GO_TEST_ROLES_SOURCE: &str =
    include_str!("../../../fixtures/extraction/go/test_roles/source_test.go");
const LUA_BASIC_SOURCE: &str = include_str!("../../../fixtures/extraction/lua/basic/source.lua");

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

fn symbol<'a>(results: &'a ExtractionResults, name: &str) -> &'a Symbol {
    results
        .symbols
        .iter()
        .find(|symbol| symbol.name == name)
        .unwrap_or_else(|| panic!("missing symbol {name:?}"))
}

fn child_symbol<'a>(results: &'a ExtractionResults, name: &str, parent: &str) -> &'a Symbol {
    let parent_id = &symbol(results, parent).id;
    results
        .symbols
        .iter()
        .find(|candidate| candidate.name == name && candidate.parent_id.as_ref() == Some(parent_id))
        .unwrap_or_else(|| panic!("missing child symbol {name:?} in {parent:?}"))
}

fn function_symbol<'a>(results: &'a ExtractionResults, name: &str) -> &'a Symbol {
    results
        .symbols
        .iter()
        .find(|candidate| {
            candidate.name == name && candidate.kind == julie_extractors::SymbolKind::Function
        })
        .unwrap_or_else(|| panic!("missing function symbol {name:?}"))
}

fn call_targets<'a>(results: &'a ExtractionResults, caller: &str) -> Vec<&'a Symbol> {
    let caller_id = &symbol(results, caller).id;
    results
        .relationships
        .iter()
        .filter(|relationship| {
            relationship.from_symbol_id == *caller_id
                && relationship.kind == RelationshipKind::Calls
        })
        .map(|relationship| {
            results
                .symbols
                .iter()
                .find(|candidate| candidate.id == relationship.to_symbol_id)
                .unwrap()
        })
        .collect()
}

fn assert_calls_to(results: &ExtractionResults, caller: &str, target: &Symbol) {
    assert_eq!(
        call_targets(results, caller)
            .iter()
            .map(|target| target.id.as_str())
            .collect::<Vec<_>>(),
        vec![target.id.as_str()]
    );
}

fn assert_calls_between(results: &ExtractionResults, caller: &Symbol, target: &Symbol) {
    assert_eq!(
        results
            .relationships
            .iter()
            .filter(|relationship| {
                relationship.from_symbol_id == caller.id
                    && relationship.kind == RelationshipKind::Calls
            })
            .map(|relationship| relationship.to_symbol_id.as_str())
            .collect::<Vec<_>>(),
        vec![target.id.as_str()]
    );
}

fn assert_no_calls(results: &ExtractionResults, caller: &str) {
    assert!(
        call_targets(results, caller).is_empty(),
        "unexpected call from {caller:?}"
    );
}

fn pending_call<'a>(
    results: &'a ExtractionResults,
    caller: &str,
    target: &str,
) -> &'a StructuredPendingRelationship {
    let caller_id = &symbol(results, caller).id;
    results
        .structured_pending_relationships
        .iter()
        .find(|pending| {
            pending.caller_scope_symbol_id.as_ref() == Some(caller_id)
                && pending.target.terminal_name == target
                && pending.pending.kind == RelationshipKind::Calls
        })
        .unwrap_or_else(|| panic!("missing pending call {caller:?} -> {target:?}"))
}

fn pending_call_count(results: &ExtractionResults, caller: &str) -> usize {
    let caller_id = &symbol(results, caller).id;
    results
        .structured_pending_relationships
        .iter()
        .filter(|pending| {
            pending.caller_scope_symbol_id.as_ref() == Some(caller_id)
                && pending.pending.kind == RelationshipKind::Calls
        })
        .count()
}

fn assert_pending_site(
    results: &ExtractionResults,
    source: &str,
    caller: &str,
    target: &str,
    call_snippet: &str,
    receiver: Option<&str>,
) {
    let pending = pending_call(results, caller, target);
    let snippet_start = source.find(call_snippet).unwrap();
    let expected_start = snippet_start + call_snippet.rfind(target).unwrap();
    let span = pending.span.unwrap();

    assert_eq!(span.start_byte as usize, expected_start);
    assert_eq!(span.end_byte as usize, expected_start + target.len());
    assert_eq!(pending.target.receiver.as_deref(), receiver);
    assert!(pending.reference_site_is_exact);
}

#[test]
fn lua_calls_follow_block_scope_and_keep_table_ownership() {
    let results = extract("lua", "reference_targets.lua", LUA_SOURCE);
    assert!(results.parse_diagnostics.is_empty());

    assert_calls_to(
        &results,
        "nearest_caller",
        child_symbol(&results, "target", "scope_owner"),
    );
    assert_calls_to(&results, "positive_caller", symbol(&results, "target"));
    assert_no_calls(&results, "parameter_shadow");
    assert_pending_site(
        &results,
        LUA_SOURCE,
        "parameter_shadow",
        "target",
        "local function parameter_shadow(target)\n    target()",
        None,
    );
    assert_no_calls(&results, "sibling_caller");
    assert_pending_site(
        &results,
        LUA_SOURCE,
        "sibling_caller",
        "hidden",
        "local function sibling_caller()\n    hidden()",
        None,
    );
    assert_calls_to(
        &results,
        "table_caller",
        child_symbol(&results, "owned", "Table"),
    );
}

#[test]
fn lua_predeclared_local_functions_resolve_mutual_recursion() {
    let results = extract("lua", "basic.lua", LUA_BASIC_SOURCE);
    assert!(results.parse_diagnostics.is_empty());

    let is_even = function_symbol(&results, "is_even");
    let is_odd = function_symbol(&results, "is_odd");
    assert_calls_between(&results, is_even, is_odd);
    assert_calls_between(&results, is_odd, is_even);
    assert!(
        !results
            .structured_pending_relationships
            .iter()
            .any(|pending| {
                pending.pending.kind == RelationshipKind::Calls
                    && ((pending.caller_scope_symbol_id.as_deref() == Some(is_even.id.as_str())
                        && pending.target.terminal_name == "is_odd")
                        || (pending.caller_scope_symbol_id.as_deref() == Some(is_odd.id.as_str())
                            && pending.target.terminal_name == "is_even"))
            })
    );
}

#[test]
fn lua_later_local_function_does_not_bind_a_prior_call() {
    let source = "local function caller() later() end\nlocal function later() end\n";
    let results = extract("lua", "forward.lua", source);
    assert!(results.parse_diagnostics.is_empty());

    assert_no_calls(&results, "caller");
    assert_pending_site(
        &results,
        source,
        "caller",
        "later",
        "local function caller() later() end",
        None,
    );
}

#[test]
fn lua_local_function_placeholder_does_not_bind_calls_before_its_declaration() {
    let source = "local function caller() later() end\nlocal later\nfunction later() end\n";
    let results = extract("lua", "forward.lua", source);
    assert!(results.parse_diagnostics.is_empty());

    assert_no_calls(&results, "caller");
    assert_pending_site(
        &results,
        source,
        "caller",
        "later",
        "local function caller() later() end",
        None,
    );
}

#[test]
fn qml_bare_parameters_shadow_component_methods() {
    let results = extract("qml", "reference_targets.qml", QML_SOURCE);
    assert!(results.parse_diagnostics.is_empty());

    assert_calls_to(&results, "lexicalCaller", symbol(&results, "target"));
    assert_no_calls(&results, "parameterShadow");
    assert_pending_site(
        &results,
        QML_SOURCE,
        "parameterShadow",
        "target",
        "function parameterShadow(target) { target(); }",
        None,
    );
    assert_eq!(pending_call_count(&results, "parameterShadow"), 1);
    assert_no_calls(&results, "unresolvedReceiver");
    assert_pending_site(
        &results,
        QML_SOURCE,
        "unresolvedReceiver",
        "refresh",
        "function unresolvedReceiver(item) { item.refresh(); }",
        Some("item"),
    );
    assert_eq!(pending_call_count(&results, "unresolvedReceiver"), 1);
}

#[test]
fn powershell_calls_follow_nearest_function_scope_and_ignore_case() {
    let results = extract("powershell", "reference_targets.ps1", POWERSHELL_SOURCE);
    assert!(results.parse_diagnostics.is_empty());

    assert_calls_to(
        &results,
        "Outer",
        child_symbol(&results, "Find-Thing", "Outer"),
    );
    assert_calls_to(
        &results,
        "Inner",
        child_symbol(&results, "Find-Thing", "Inner"),
    );
    assert_no_calls(&results, "Sibling");
    assert_pending_site(
        &results,
        POWERSHELL_SOURCE,
        "Sibling",
        "Find-Thing",
        "function Sibling {\n    Find-Thing",
        None,
    );
    assert_calls_to(&results, "CaseCaller", symbol(&results, "Get-Visible"));
    assert_calls_to(&results, "AliasInner", symbol(&results, "Invoke-Remote"));
}

#[test]
fn go_bare_calls_select_package_functions_and_preserve_pending_receivers() {
    let results = extract("go", "reference_targets.go", GO_SOURCE);
    assert!(results.parse_diagnostics.is_empty());

    assert_no_calls(&results, "methodOnlyCaller");
    assert_pending_site(
        &results,
        GO_SOURCE,
        "methodOnlyCaller",
        "hidden",
        "func methodOnlyCaller() {\n\thidden()",
        None,
    );
    let package_function = results
        .symbols
        .iter()
        .find(|candidate| {
            candidate.name == "shared" && candidate.kind == julie_extractors::SymbolKind::Function
        })
        .unwrap();
    assert_calls_to(&results, "packageCaller", package_function);
    assert_calls_to(
        &results,
        "genericSiblingCaller",
        symbol(&results, "genericTarget"),
    );
    assert_no_calls(&results, "genericShadow");
    assert_pending_site(
        &results,
        GO_SOURCE,
        "genericShadow",
        "genericTarget",
        "func genericShadow(genericTarget int) {\n\tgenericTarget[int]()",
        None,
    );
    assert_no_calls(&results, "parameterShadow");
    assert_pending_site(
        &results,
        GO_SOURCE,
        "parameterShadow",
        "shared",
        "func parameterShadow(shared int) {\n\tshared()",
        None,
    );
    assert_no_calls(&results, "localShadow");
    assert_pending_site(
        &results,
        GO_SOURCE,
        "localShadow",
        "shared",
        "func localShadow() {\n\tshared := func() {}\n\tshared()",
        None,
    );
    assert_no_calls(&results, "receiverPending");
    assert_pending_site(
        &results,
        GO_SOURCE,
        "receiverPending",
        "shared",
        "func receiverPending(worker Worker) {\n\tworker.shared()",
        Some("worker"),
    );
}

#[test]
fn go_ginkgo_synthetic_hooks_keep_exact_pending_call_sites() {
    let results = extract("go", "test_roles/source_test.go", GO_TEST_ROLES_SOURCE);
    assert!(results.parse_diagnostics.is_empty());

    assert_pending_site(
        &results,
        GO_TEST_ROLES_SOURCE,
        "addition",
        "BeforeEach",
        "BeforeEach(func() {})",
        None,
    );
    assert_pending_site(
        &results,
        GO_TEST_ROLES_SOURCE,
        "addition",
        "AfterEach",
        "AfterEach(func() {})",
        None,
    );
}
