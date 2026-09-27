use std::path::Path;

use julie_extractors::{ExtractionLevel, RelationshipKind, extract_canonical_for_language_at};

#[test]
fn elixir_calls_require_a_matching_arity_and_pending_calls_preserve_it() {
    let source = include_str!("../../../fixtures/extraction/elixir/reference_targets/source.ex");
    let result = extract_canonical_for_language_at(
        "elixir",
        "arity.ex",
        source,
        Path::new("."),
        ExtractionLevel::Full,
    )
    .unwrap();
    assert!(result.parse_diagnostics.is_empty());
    let symbol = |name: &str| {
        result
            .symbols
            .iter()
            .find(|symbol| symbol.name == name)
            .unwrap()
    };
    for (caller, target) in [
        ("run_local", "map"),
        ("good", "work"),
        ("delegated", "work"),
    ] {
        assert!(
            result
                .relationships
                .iter()
                .any(|edge| edge.kind == RelationshipKind::Calls
                    && edge.from_symbol_id == symbol(caller).id
                    && edge.to_symbol_id == symbol(target).id),
            "missing valid {caller} -> {target}"
        );
    }
    for (caller, name, arity) in [
        ("run_imported", "map", 2),
        ("run_piped", "map", 2),
        ("capture_imported", "map", 2),
        ("wrong_arity", "work", 3),
        ("wrong_delegate", "work", 3),
    ] {
        assert!(
            !result
                .relationships
                .iter()
                .any(|edge| edge.kind == RelationshipKind::Calls
                    && edge.from_symbol_id == symbol(caller).id
                    && edge.to_symbol_id == symbol(name).id),
            "{caller} bound the wrong arity of {name}"
        );
        let pending = result
            .structured_pending_relationships
            .iter()
            .find(|pending| {
                pending.pending.from_symbol_id == symbol(caller).id
                    && pending.target.terminal_name == name
            })
            .unwrap_or_else(|| panic!("missing unresolved {caller} -> {name}"));
        assert_eq!(pending.target_arity, Some(arity), "{caller}");
        assert_eq!(
            pending.caller_scope_symbol_id.as_deref(),
            Some(symbol(caller).id.as_str())
        );
    }
}
