use crate::base::{ExtractionResults, Identifier, IdentifierKind};
use crate::extract_canonical;
use std::path::Path;

fn extract(source: &str) -> ExtractionResults {
    extract_canonical("styles/access.css", source, Path::new("/repo")).expect("CSS extraction")
}

fn identifier_at(results: &ExtractionResults, start: usize, kind: IdentifierKind) -> &Identifier {
    results
        .identifiers
        .iter()
        .find(|identifier| identifier.start_byte as usize == start && identifier.kind == kind)
        .unwrap_or_else(|| panic!("missing {kind:?} identifier at byte {start}"))
}

fn access(identifier: &Identifier) -> Option<&str> {
    identifier
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("access"))
        .and_then(serde_json::Value::as_str)
}

#[test]
fn stylesheet_selectors_custom_properties_and_function_names_have_no_access_mode() {
    let source = ":root { --main-color: #333; }\n.card { color: var(--main-color); width: calc(100% - 2px); }\n";
    let results = extract(source);

    assert!(results.parse_diagnostics.is_empty());

    let selector = identifier_at(
        &results,
        source.find(".card").unwrap() + 1,
        IdentifierKind::MemberAccess,
    );
    let property = identifier_at(
        &results,
        source.find("var(--main-color)").unwrap() + "var(".len(),
        IdentifierKind::VariableRef,
    );
    let var_call = identifier_at(&results, source.find("var").unwrap(), IdentifierKind::Call);
    let calc_call = identifier_at(&results, source.find("calc").unwrap(), IdentifierKind::Call);

    for identifier in [selector, property, var_call, calc_call] {
        assert_eq!(access(identifier), None, "{identifier:?}");
        assert_eq!(
            &source[identifier.start_byte as usize..identifier.end_byte as usize],
            identifier.name
        );
    }
}
