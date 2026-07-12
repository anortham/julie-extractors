use crate::base::{IdentifierKind, RelationshipKind};
use crate::pipeline::extract_canonical;
use serde::Deserialize;
use std::path::Path;

#[derive(Deserialize)]
struct ExpectedEvidence {
    #[serde(default)]
    symbols: Vec<String>,
    #[serde(default)]
    variable_refs: Vec<String>,
    #[serde(default)]
    calls: Vec<String>,
    #[serde(default)]
    type_usages: Vec<String>,
    #[serde(default)]
    member_accesses: Vec<String>,
    #[serde(default)]
    literals: Vec<String>,
    #[serde(default)]
    relationships: Vec<ExpectedRelationship>,
}

#[derive(Deserialize)]
struct ExpectedRelationship {
    from: String,
    to: String,
    kind: String,
}

struct Fixture {
    name: &'static str,
    path: &'static str,
    source: &'static str,
    expected: &'static str,
}

const FIXTURES: &[Fixture] = &[
    Fixture {
        name: "implicit expressions",
        path: "fixtures/extraction/razor/attribute-expressions/implicit/source.razor",
        source: include_str!(
            "../../../../../fixtures/extraction/razor/attribute-expressions/implicit/source.razor"
        ),
        expected: include_str!(
            "../../../../../fixtures/extraction/razor/attribute-expressions/implicit/expected.json"
        ),
    },
    Fixture {
        name: "explicit expressions",
        path: "fixtures/extraction/razor/attribute-expressions/explicit/source.razor",
        source: include_str!(
            "../../../../../fixtures/extraction/razor/attribute-expressions/explicit/source.razor"
        ),
        expected: include_str!(
            "../../../../../fixtures/extraction/razor/attribute-expressions/explicit/expected.json"
        ),
    },
    Fixture {
        name: "directive modifiers",
        path: "fixtures/extraction/razor/attribute-expressions/modifiers/source.razor",
        source: include_str!(
            "../../../../../fixtures/extraction/razor/attribute-expressions/modifiers/source.razor"
        ),
        expected: include_str!(
            "../../../../../fixtures/extraction/razor/attribute-expressions/modifiers/expected.json"
        ),
    },
    Fixture {
        name: "directives and render fragment",
        path: "fixtures/extraction/razor/attribute-expressions/directives/source.razor",
        source: include_str!(
            "../../../../../fixtures/extraction/razor/attribute-expressions/directives/source.razor"
        ),
        expected: include_str!(
            "../../../../../fixtures/extraction/razor/attribute-expressions/directives/expected.json"
        ),
    },
    Fixture {
        name: "explicit render mode",
        path: "fixtures/extraction/razor/attribute-expressions/rendermode-explicit/source.razor",
        source: include_str!(
            "../../../../../fixtures/extraction/razor/attribute-expressions/rendermode-explicit/source.razor"
        ),
        expected: include_str!(
            "../../../../../fixtures/extraction/razor/attribute-expressions/rendermode-explicit/expected.json"
        ),
    },
];

#[test]
fn implicit_expressions_are_clean_and_semantically_visible() {
    assert_fixture(&FIXTURES[0]);
}

#[test]
fn explicit_expressions_are_clean_and_semantically_visible() {
    assert_fixture(&FIXTURES[1]);
}

#[test]
fn directive_modifiers_are_clean_and_semantically_visible() {
    assert_fixture(&FIXTURES[2]);
}

#[test]
fn directives_and_render_fragment_are_clean_and_semantically_visible() {
    assert_fixture(&FIXTURES[3]);
}

#[test]
fn explicit_render_mode_is_clean_and_semantically_visible() {
    assert_fixture(&FIXTURES[4]);
}

fn assert_fixture(fixture: &Fixture) {
    let expected: ExpectedEvidence = serde_json::from_str(fixture.expected)
        .unwrap_or_else(|error| panic!("{} has invalid expected evidence: {error}", fixture.name));
    let results = extract_canonical(fixture.path, fixture.source, Path::new("/repo"))
        .unwrap_or_else(|error| panic!("{} extraction failed: {error}", fixture.name));

    assert!(
        results.parse_diagnostics.is_empty(),
        "{} must have zero error/missing parse diagnostics: {:?}",
        fixture.name,
        results.parse_diagnostics
    );

    for name in &expected.symbols {
        assert!(
            results.symbols.iter().any(|symbol| &symbol.name == name),
            "{} missing symbol {name:?}; got {:?}",
            fixture.name,
            results
                .symbols
                .iter()
                .map(|symbol| &symbol.name)
                .collect::<Vec<_>>()
        );
    }
    assert_identifiers(
        fixture.name,
        &results.identifiers,
        IdentifierKind::VariableRef,
        &expected.variable_refs,
    );
    assert_identifiers(
        fixture.name,
        &results.identifiers,
        IdentifierKind::Call,
        &expected.calls,
    );
    assert_identifiers(
        fixture.name,
        &results.identifiers,
        IdentifierKind::TypeUsage,
        &expected.type_usages,
    );
    assert_identifiers(
        fixture.name,
        &results.identifiers,
        IdentifierKind::MemberAccess,
        &expected.member_accesses,
    );

    for literal in &expected.literals {
        assert!(
            results
                .literals
                .iter()
                .any(|row| &row.literal_text == literal),
            "{} missing literal {literal:?}; got {:?}",
            fixture.name,
            results
                .literals
                .iter()
                .map(|row| &row.literal_text)
                .collect::<Vec<_>>()
        );
    }

    for relationship in &expected.relationships {
        let from_ids: Vec<_> = results
            .symbols
            .iter()
            .filter(|symbol| symbol.name == relationship.from)
            .map(|symbol| &symbol.id)
            .collect();
        let to_ids: Vec<_> = results
            .symbols
            .iter()
            .filter(|symbol| symbol.name == relationship.to)
            .map(|symbol| &symbol.id)
            .collect();
        let kind = RelationshipKind::try_from_string(&relationship.kind)
            .unwrap_or_else(|| panic!("unknown relationship kind {:?}", relationship.kind));
        assert!(
            results.relationships.iter().any(|row| row.kind == kind
                && from_ids.contains(&&row.from_symbol_id)
                && to_ids.contains(&&row.to_symbol_id)),
            "{} missing {} relationship {} -> {}; got {:?}",
            fixture.name,
            relationship.kind,
            relationship.from,
            relationship.to,
            results.relationships
        );
    }
}

fn assert_identifiers(
    fixture_name: &str,
    actual: &[crate::base::Identifier],
    kind: IdentifierKind,
    expected: &[String],
) {
    for name in expected {
        assert!(
            actual
                .iter()
                .any(|identifier| identifier.kind == kind && identifier.name == *name),
            "{fixture_name} missing {kind} identifier {name:?}; got {:?}",
            actual
                .iter()
                .map(|identifier| (&identifier.name, &identifier.kind))
                .collect::<Vec<_>>()
        );
    }
}
