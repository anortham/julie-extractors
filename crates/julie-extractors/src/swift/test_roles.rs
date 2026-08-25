//! Swift test containers and the member roles that only a container can grant.
//!
//! Swift has three test frameworks and none of them marks a suite the same way:
//!
//! - **XCTest** subclasses `XCTestCase`, so the container is found through the
//!   `base_types` metadata the type extractor records.
//! - **Swift Testing** annotates a `struct`, `class`, `enum`, or `actor` with
//!   the `@Suite` macro, and annotates a case with `@Test`. Both are
//!   path-independent.
//! - **Quick** declares a group as a `describe`/`context` call, which the call
//!   adapter already materialises as a container symbol.
//!
//! Everything else in the Swift test vocabulary — `func testXxx`, `init`,
//! `deinit` — is ordinary Swift, so those names earn a role only inside a
//! container. [`normalize_scoped_test_roles`] enforces that, and the passes
//! after it put back the roles that come from a macro instead of a name.

use std::collections::HashSet;

use crate::base::{AnnotationMarker, Symbol, SymbolKind, TestRole};
use crate::test_detection::{
    SWIFT_SUITE_MACRO_KEY, SWIFT_TEST_MACRO_KEY, apply_test_role, mark_base_type_test_containers,
    normalize_scoped_test_roles,
};

const XCTEST_BASE_TYPE: &str = "XCTestCase";

/// The label Swift Testing uses for the argument rows of a parameterized case.
const ARGUMENT_ROWS_LABEL: &str = "arguments:";

/// Classify Swift test containers and the members whose role depends on one.
pub(super) fn apply_swift_test_roles(symbols: &mut [Symbol]) {
    mark_suite_containers(symbols);
    mark_base_type_test_containers(symbols, XCTEST_BASE_TYPE);
    mark_container_extensions(symbols);

    let container_ids = test_container_ids(symbols);
    normalize_scoped_test_roles(symbols, &container_ids);
    restore_container_roles(symbols, &container_ids);
    apply_member_roles(symbols, &container_ids);
}

/// Swift Testing accepts a suite on any type declaration, so every type kind the
/// Swift extractor emits is eligible.
fn mark_suite_containers(symbols: &mut [Symbol]) {
    for symbol in symbols.iter_mut().filter(|symbol| {
        matches!(
            symbol.kind,
            SymbolKind::Class | SymbolKind::Struct | SymbolKind::Enum
        ) && has_macro(symbol, SWIFT_SUITE_MACRO_KEY)
    }) {
        apply_test_role(
            symbol.metadata.get_or_insert_with(Default::default),
            TestRole::TestContainer,
        );
    }
}

/// Swift lets a suite split its cases across extensions, and XCTest runs a
/// `test`-prefixed method declared in one. An extension is its own symbol with
/// its own children, so it must be a container in its own right or the scoping
/// pass would strip every case it holds.
///
/// The match is by name within the file, because an extension records the type
/// it extends by name. A container declared in another file is out of reach of
/// a per-file extractor.
fn mark_container_extensions(symbols: &mut [Symbol]) {
    let container_names: HashSet<String> = symbols
        .iter()
        .filter(|symbol| is_test_container(symbol))
        .map(|symbol| symbol.name.clone())
        .collect();

    for symbol in symbols.iter_mut().filter(|symbol| {
        symbol.kind == SymbolKind::Module && container_names.contains(&symbol.name)
    }) {
        apply_test_role(
            symbol.metadata.get_or_insert_with(Default::default),
            TestRole::TestContainer,
        );
    }
}

fn is_test_container(symbol: &Symbol) -> bool {
    symbol
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("test_container"))
        .and_then(|value| value.as_bool())
        == Some(true)
}

fn test_container_ids(symbols: &[Symbol]) -> HashSet<String> {
    symbols
        .iter()
        .filter(|symbol| is_test_container(symbol))
        .map(|symbol| symbol.id.clone())
        .collect()
}

/// Give back the container role the scoping pass took from a Quick group.
///
/// A Quick `describe` is a callable symbol that is itself a container, and the
/// outermost one has no container ancestor, so scoping clears it. Every other
/// container is a type declaration, which scoping never touches.
fn restore_container_roles(symbols: &mut [Symbol], container_ids: &HashSet<String>) {
    for symbol in symbols
        .iter_mut()
        .filter(|symbol| container_ids.contains(&symbol.id))
    {
        apply_test_role(
            symbol.metadata.get_or_insert_with(Default::default),
            TestRole::TestContainer,
        );
    }
}

/// Re-derive the roles that a macro grants and the roles that container
/// membership grants.
///
/// Scoping is a name-convention guard and cannot see where a role came from, so
/// it also strips a top-level `@Test` function, which Swift Testing collects
/// with no enclosing suite at all. Re-deriving from the macro alone puts those
/// roles back without reviving the name convention.
fn apply_member_roles(symbols: &mut [Symbol], container_ids: &HashSet<String>) {
    for symbol in symbols.iter_mut() {
        let inside_container = symbol
            .parent_id
            .as_ref()
            .is_some_and(|parent_id| container_ids.contains(parent_id));
        let Some(role) = member_role(symbol, inside_container) else {
            continue;
        };
        apply_test_role(symbol.metadata.get_or_insert_with(Default::default), role);
    }
}

/// Swift Testing runs `init` before each case and `deinit` after it, because it
/// builds one instance of the suite per case. Both names are ordinary Swift, so
/// they earn a role only inside a container — the same rule the xUnit
/// constructor takes.
fn member_role(symbol: &Symbol, inside_container: bool) -> Option<TestRole> {
    if has_macro(symbol, SWIFT_TEST_MACRO_KEY) {
        return Some(macro_case_role(symbol));
    }
    if !inside_container {
        return None;
    }
    match symbol.kind {
        SymbolKind::Constructor => Some(TestRole::FixtureSetup),
        SymbolKind::Destructor => Some(TestRole::FixtureTeardown),
        _ => None,
    }
}

/// `@Test(arguments:)` runs the function once per argument row, so the runner
/// reports one result per row instead of one result per function.
fn macro_case_role(symbol: &Symbol) -> TestRole {
    let parameterized = symbol
        .annotations
        .iter()
        .filter(|marker| marker.annotation_key == SWIFT_TEST_MACRO_KEY)
        .any(declares_argument_rows);
    if parameterized {
        TestRole::ParameterizedTest
    } else {
        TestRole::TestCase
    }
}

fn declares_argument_rows(marker: &AnnotationMarker) -> bool {
    marker
        .raw_text
        .as_deref()
        .and_then(|raw_text| raw_text.split_once('('))
        .is_some_and(|(_, arguments)| labels_argument_rows(arguments))
}

/// Whether the macro's argument list carries the `arguments:` label outside of
/// a string. A display name may spell the label as text — `@Test("no
/// arguments: here")` — and that is not a parameterized case.
fn labels_argument_rows(arguments: &str) -> bool {
    let mut inside_string = false;
    let mut previous_was_escape = false;

    for (index, character) in arguments.char_indices() {
        if inside_string {
            if previous_was_escape {
                previous_was_escape = false;
            } else if character == '\\' {
                previous_was_escape = true;
            } else if character == '"' {
                inside_string = false;
            }
            continue;
        }
        if character == '"' {
            inside_string = true;
            continue;
        }
        if arguments[index..].starts_with(ARGUMENT_ROWS_LABEL) {
            return true;
        }
    }

    false
}

fn has_macro(symbol: &Symbol, annotation_key: &str) -> bool {
    symbol
        .annotations
        .iter()
        .any(|marker| marker.annotation_key == annotation_key)
}
