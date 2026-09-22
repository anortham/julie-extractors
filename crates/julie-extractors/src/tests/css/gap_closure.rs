use super::{extract_symbols, extract_symbols_and_identifiers, extract_symbols_and_relationships};
use crate::base::{IdentifierKind, RelationshipKind, Symbol};
use crate::extract_canonical;
use std::path::Path;

fn find<'a>(symbols: &'a [Symbol], name: &str) -> &'a Symbol {
    symbols
        .iter()
        .find(|symbol| symbol.name == name)
        .unwrap_or_else(|| panic!("symbol {name} missing from {symbols:#?}"))
}

fn metadata_str<'a>(symbol: &'a Symbol, key: &str) -> Option<&'a str> {
    symbol.metadata.as_ref()?.get(key)?.as_str()
}

const IMPORTS: &str = "@import \"reset.css\";\n@import url(\"theme/dark.css\") screen and (prefers-color-scheme: dark);\n@import 'print.css' print;\n@import url(plain/base.css);\n\n.a { color: red; }\n";

#[test]
fn import_symbols_are_named_by_target_and_carry_media() {
    let symbols = extract_symbols(IMPORTS);
    let imports: Vec<&Symbol> = symbols
        .iter()
        .filter(|symbol| symbol.kind == crate::base::SymbolKind::Import)
        .collect();
    let names: Vec<&str> = imports.iter().map(|symbol| symbol.name.as_str()).collect();
    assert_eq!(
        names,
        ["reset.css", "theme/dark.css", "print.css", "plain/base.css"]
    );

    let dark = find(&symbols, "theme/dark.css");
    assert_eq!(metadata_str(dark, "url"), Some("theme/dark.css"));
    assert_eq!(
        metadata_str(dark, "media"),
        Some("screen and (prefers-color-scheme: dark)")
    );
    assert_eq!(
        metadata_str(find(&symbols, "print.css"), "media"),
        Some("print")
    );
    assert_eq!(metadata_str(find(&symbols, "reset.css"), "media"), None);
    assert!(imports.iter().all(|symbol| symbol.body_span.is_none()));
}

#[test]
fn import_statements_emit_structured_import_pending() {
    let result = extract_canonical("styles/site.css", IMPORTS, Path::new("/tmp/test"))
        .expect("canonical CSS extraction must succeed");

    let targets: Vec<(&str, Option<&str>)> = result
        .structured_pending_relationships
        .iter()
        .map(|pending| {
            (
                pending.target.display_name.as_str(),
                pending.target.import_context.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        targets,
        [
            ("reset.css", Some("css-import")),
            ("theme/dark.css", Some("css-import")),
            ("print.css", Some("css-import")),
            ("plain/base.css", Some("css-import")),
        ]
    );
    for pending in &result.structured_pending_relationships {
        assert_eq!(pending.pending.kind, RelationshipKind::Imports);
        let import = result
            .symbols
            .iter()
            .find(|symbol| symbol.id == pending.pending.from_symbol_id)
            .expect("pending source must be the import symbol");
        assert_eq!(import.name, pending.target.display_name);
    }
    assert_eq!(result.pending_relationships.len(), 4);
    assert_eq!(
        result
            .structural_facts
            .iter()
            .filter(|fact| fact.pattern_id == "css.import.v1")
            .count(),
        4
    );
}

#[test]
fn class_and_id_identifiers_use_the_name_node_with_escapes_decoded() {
    let css = r".btn.btn-large { font-size: 2rem; }
button.primary { color: white; }
div#main-content { padding: 0; }
.sm\:flex { display: flex; }
.w-1\/2 { width: 50%; }
.card > .title:hover { color: red; }
a.link:not(.disabled) { color: blue; }
.\31 0 { order: 1; }
";
    let (_, mut identifiers) = extract_symbols_and_identifiers(css);
    identifiers.sort_by_key(|identifier| identifier.start_byte);
    let names: Vec<&str> = identifiers
        .iter()
        .filter(|identifier| identifier.kind == IdentifierKind::MemberAccess)
        .map(|identifier| identifier.name.as_str())
        .collect();
    assert_eq!(
        names,
        [
            "btn",
            "btn-large",
            "primary",
            "main-content",
            "sm:flex",
            "w-1/2",
            "card",
            "title",
            "link",
            "disabled",
            "10",
        ]
    );
    let primary = identifiers
        .iter()
        .find(|identifier| identifier.name == "primary")
        .unwrap();
    assert_eq!(
        &css[primary.start_byte as usize..primary.end_byte as usize],
        "primary"
    );
}

#[test]
fn custom_property_symbols_keep_the_full_value_and_declaration_span() {
    let css = r#":root {
  --color-accent: #0f766e;
  --color-text: rgb(20 20 20);
  --font-body: "Inter", system-ui, sans-serif;
  --shadow: 0 1px 2px rgba(0, 0, 0, 0.2);
  --gap: calc(var(--space-4) * 2);
  --size: 0.25rem;
  --z: 10;
}
"#;
    let symbols = extract_symbols(css);
    for (name, value) in [
        ("--color-accent", "#0f766e"),
        ("--color-text", "rgb(20 20 20)"),
        ("--font-body", "\"Inter\", system-ui, sans-serif"),
        ("--shadow", "0 1px 2px rgba(0, 0, 0, 0.2)"),
        ("--gap", "calc(var(--space-4) * 2)"),
        ("--size", "0.25rem"),
        ("--z", "10"),
    ] {
        let symbol = find(&symbols, name);
        assert_eq!(metadata_str(symbol, "value"), Some(value), "{name}");
        assert_eq!(
            symbol.signature.as_deref(),
            Some(format!("{name}: {value}").as_str())
        );
        let declaration = format!("{name}: {value};");
        assert_eq!(
            &css[symbol.start_byte as usize..symbol.end_byte as usize],
            declaration
        );
        let body = symbol.body_span.expect("custom property needs a body span");
        assert_eq!(
            &css[body.start_byte as usize..body.end_byte as usize],
            declaration
        );
        assert!(symbol.body_hash.is_some());
    }
}

fn edge_pairs(css: &str) -> Vec<(String, String)> {
    let (symbols, relationships) = extract_symbols_and_relationships(css);
    let name_of = |id: &str| {
        let symbol = symbols.iter().find(|symbol| symbol.id == id).unwrap();
        format!("{}@{}", symbol.name, symbol.start_line)
    };
    relationships
        .iter()
        .filter(|relationship| relationship.kind == RelationshipKind::References)
        .map(|relationship| {
            (
                name_of(&relationship.from_symbol_id),
                name_of(&relationship.to_symbol_id),
            )
        })
        .collect()
}

#[test]
fn references_come_from_the_rule_that_contains_the_use_on_one_line() {
    let css = ":root{--a:1px;--b:2px}\n.x{margin:var(--a)}.y{padding:var(--a)}.z{gap:var(--b)}\n@keyframes k{to{opacity:1}}.w{animation-name:k}.v{animation:k 1s linear}\n";
    let pairs = edge_pairs(css);
    let expected = [
        (".x@2", "--a@1"),
        (".y@2", "--a@1"),
        (".z@2", "--b@1"),
        (".w@3", "@keyframes k@3"),
        (".v@3", "@keyframes k@3"),
    ];
    assert_eq!(
        pairs,
        expected
            .iter()
            .map(|(from, to)| (from.to_string(), to.to_string()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn redefined_custom_properties_get_an_edge_to_each_declaration() {
    let css = ":root {\n  --bg: white;\n}\n.page {\n  background: var(--bg);\n}\n[data-theme=\"dark\"] {\n  --bg: black;\n}\n";
    let pairs = edge_pairs(css);
    assert_eq!(
        pairs,
        [
            (".page@4".to_string(), "--bg@2".to_string()),
            (".page@4".to_string(), "--bg@8".to_string()),
        ]
    );
}

#[test]
fn custom_property_aliases_reference_from_the_declaring_property() {
    let css = ":root {\n  --space-4: 1rem;\n  --gap: calc(var(--space-4) * 2);\n}\n";
    assert_eq!(
        edge_pairs(css),
        [("--gap@3".to_string(), "--space-4@2".to_string())]
    );
}

#[test]
fn touching_rules_bind_identifiers_to_the_rule_that_starts_there() {
    let css = ".x{margin:0}.y{padding:0}\n";
    let (symbols, identifiers) = extract_symbols_and_identifiers(css);
    let y_rule = find(&symbols, ".y");
    let y = identifiers
        .iter()
        .find(|identifier| identifier.name == "y")
        .unwrap();
    assert_eq!(y.containing_symbol_id.as_deref(), Some(y_rule.id.as_str()));
}
