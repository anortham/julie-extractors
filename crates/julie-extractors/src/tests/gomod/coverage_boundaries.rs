use super::{extract, symbol};
use crate::base::SymbolKind;

#[test]
fn structural_only_directives_remain_facts_without_symbol_bodies_or_edges() {
    let source = concat!(
        "// Café manifest\r\n",
        "// require example.com/fake v9.9.9\r\n",
        "module example.com/app\r\n",
        "replace example.com/old v1.0.0 => example.com/new v1.1.0\r\n",
        "exclude example.com/bad v1.2.0\r\n",
        "ignore ./node_modules\r\n",
        "retract v1.0.0",
    );
    let results = extract(source);

    assert!(results.parse_diagnostics.is_empty());
    assert_eq!(results.symbols.len(), 1);
    let module = symbol(&results, "example.com/app");
    assert_eq!(module.kind, SymbolKind::Module);
    let body = module.body_span.expect("module path body");
    assert_eq!(
        &source[body.start_byte as usize..body.end_byte as usize],
        "example.com/app"
    );
    assert!(results.relationships.is_empty());
    assert!(
        results.structured_pending_relationships.is_empty(),
        "{:#?}",
        results.structured_pending_relationships
    );

    let mut facts: Vec<_> = results
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id.starts_with("gomod."))
        .collect();
    facts.sort_by_key(|fact| fact.start_byte);
    let replacement = facts
        .iter()
        .find(|fact| fact.pattern_id == "gomod.replace.v1")
        .expect("remote replacement fact");
    let replacement_metadata = replacement.metadata.as_ref().expect("replacement metadata");
    assert_eq!(
        replacement_metadata
            .get("local")
            .and_then(|value| value.as_bool()),
        Some(false)
    );
    assert_eq!(
        replacement_metadata
            .get("replacement_version")
            .and_then(|value| value.as_str()),
        Some("v1.1.0")
    );
    assert_eq!(
        facts
            .iter()
            .map(|fact| fact.pattern_id.as_str())
            .collect::<Vec<_>>(),
        vec![
            "gomod.module.v1",
            "gomod.replace.v1",
            "gomod.exclude.v1",
            "gomod.ignore.v1",
            "gomod.retract.v1",
        ]
    );
    assert_eq!(
        facts
            .iter()
            .map(|fact| { source[fact.start_byte as usize..fact.end_byte as usize].to_string() })
            .collect::<Vec<_>>(),
        vec![
            "module example.com/app",
            "replace example.com/old v1.0.0 => example.com/new v1.1.0",
            "exclude example.com/bad v1.2.0",
            "ignore ./node_modules",
            "retract v1.0.0",
        ]
    );
    assert!(
        !results
            .structural_facts
            .iter()
            .any(|fact| fact.pattern_id == "manifest.dependency.v1")
    );
    assert!(results.symbols.iter().all(|symbol| {
        ![
            "example.com/old",
            "example.com/bad",
            "./node_modules",
            "v1.0.0",
        ]
        .contains(&symbol.name.as_str())
    }));
}
