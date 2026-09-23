use crate::base::{
    ExtractionResults, IdentifierKind, RelationshipKind, StructuralFact, Symbol, SymbolKind,
};
use crate::extract_canonical;
use crate::language_policy::classify_literals_by_carrier;
use std::path::Path;

fn extract(code: &str) -> ExtractionResults {
    extract_canonical("styles/site.css", code, Path::new("/tmp/test"))
        .expect("canonical CSS extraction must succeed")
}

fn symbol<'a>(result: &'a ExtractionResults, name: &str) -> &'a Symbol {
    result
        .symbols
        .iter()
        .find(|symbol| symbol.name == name)
        .unwrap_or_else(|| panic!("symbol {name} missing from {:#?}", names(result)))
}

fn names(result: &ExtractionResults) -> Vec<&str> {
    result.symbols.iter().map(|s| s.name.as_str()).collect()
}

fn meta<'a>(symbol: &'a Symbol, key: &str) -> Option<&'a str> {
    symbol.metadata.as_ref()?.get(key)?.as_str()
}

fn facts<'a>(result: &'a ExtractionResults, pattern: &str) -> Vec<&'a StructuralFact> {
    result
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == pattern)
        .collect()
}

fn fact_meta<'a>(fact: &'a StructuralFact, key: &str) -> Option<&'a serde_json::Value> {
    fact.metadata.as_ref()?.get(key)
}

fn owner_name<'a>(result: &'a ExtractionResults, id: Option<&String>) -> Option<&'a str> {
    let id = id?;
    result
        .symbols
        .iter()
        .find(|symbol| &symbol.id == id)
        .map(|symbol| symbol.name.as_str())
}

#[test]
fn url_arguments_become_url_literals_owned_by_the_rule() {
    let result = extract(
        ".logo { background-image: url(\"/assets/logo.svg\"); }\n.hero { background-image: url(images/hero.png); }\n@font-face { font-family: \"Brand\"; src: url(\"/fonts/brand.woff2\") format(\"woff2\"); }\n",
    );
    let mut literals = result.literals.clone();
    classify_literals_by_carrier(&mut literals);
    let rows: Vec<(&str, &str, Option<&str>)> = literals
        .iter()
        .map(|literal| {
            (
                literal.literal_text.as_str(),
                literal.kind.as_str(),
                owner_name(&result, literal.containing_symbol_id.as_ref()),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            ("/assets/logo.svg", "url", Some(".logo")),
            ("images/hero.png", "url", Some(".hero")),
            ("/fonts/brand.woff2", "url", Some("@font-face Brand")),
        ]
    );
}

#[test]
fn at_rule_names_hold_the_whole_prelude() {
    let result = extract(
        "@layer base;\n@layer reset, base, components;\n@container (min-width: 20rem) and (max-width: 40rem) {\n  .y { color: red; }\n}\n@font-face { font-family: \"Brand Sans\"; }\n@counter-style thumbs { system: cyclic; }\n@position-try --below { top: anchor(bottom); }\n@utility tab-4 { tab-size: 4; }\n@tailwind base;\n",
    );
    for name in [
        "@layer base",
        "@layer reset, base, components",
        "@container (min-width: 20rem) and (max-width: 40rem)",
        "@font-face Brand Sans",
        "@counter-style thumbs",
        "@position-try --below",
        "@utility tab-4",
        "@tailwind base",
    ] {
        symbol(&result, name);
    }
    assert_eq!(
        meta(symbol(&result, "@layer base"), "atRuleType"),
        Some("layer")
    );
    assert_eq!(
        meta(symbol(&result, "@font-face Brand Sans"), "fontFamily"),
        Some("Brand Sans")
    );
    let font_face = facts(&result, "css.font_face.v1");
    assert_eq!(
        fact_meta(font_face[0], "font_family").and_then(|v| v.as_str()),
        Some("Brand Sans")
    );
    let y = symbol(&result, ".y");
    assert_eq!(
        owner_name(&result, y.parent_id.as_ref()),
        Some("@container (min-width: 20rem) and (max-width: 40rem)")
    );
}

#[test]
fn var_reference_links_to_the_property_registration() {
    let result = extract(
        "@property --brand-hue { syntax: \"<number>\"; inherits: false; initial-value: 200; }\n.swatch { color: hsl(var(--brand-hue) 50% 50%); }\n",
    );
    let registration = symbol(&result, "@property --brand-hue");
    assert_eq!(registration.kind, SymbolKind::Property);
    assert!(result.relationships.iter().any(|relationship| {
        relationship.kind == RelationshipKind::References
            && relationship.to_symbol_id == registration.id
            && owner_name(&result, Some(&relationship.from_symbol_id)) == Some(".swatch")
    }));
}

#[test]
fn trailing_comments_are_not_docs_for_the_next_symbol() {
    let result = extract(
        ":root {\n  --space-1: 4px; /* xs */\n  --space-2: 8px; /* sm */\n  --space-3: 16px; /* md */\n}\n.a { color: blue; } /* trailing note for a */\n.b { color: green; }\n/* Real doc for c */\n.c { color: red; }\n",
    );
    assert_eq!(symbol(&result, "--space-2").doc_comment, None);
    assert_eq!(symbol(&result, "--space-3").doc_comment, None);
    assert_eq!(symbol(&result, ".b").doc_comment, None);
    assert_eq!(
        symbol(&result, ".c").doc_comment.as_deref(),
        Some("/* Real doc for c */")
    );
    let b = symbol(&result, ".b");
    assert!(
        result
            .source_regions
            .iter()
            .all(|region| region.containing_symbol_id.as_ref() != Some(&b.id))
    );
}

#[test]
fn tailwind_apply_classes_are_member_access_and_facts() {
    let result = extract(
        "@tailwind base;\n@layer components {\n  .btn-primary {\n    @apply px-4 py-2 rounded;\n  }\n  .card {\n    @apply btn-primary shadow-md;\n  }\n}\n",
    );
    let card = symbol(&result, ".card");
    let applied: Vec<(&str, u32)> = result
        .identifiers
        .iter()
        .filter(|identifier| identifier.kind == IdentifierKind::MemberAccess)
        .filter(|identifier| identifier.containing_symbol_id.as_ref() == Some(&card.id))
        .map(|identifier| (identifier.name.as_str(), identifier.start_line))
        .collect();
    assert_eq!(applied, [("card", 6), ("btn-primary", 7), ("shadow-md", 7)]);
    let apply = facts(&result, "css.tailwind_apply.v1");
    assert_eq!(apply.len(), 2);
    assert_eq!(
        fact_meta(apply[1], "classes"),
        Some(&serde_json::json!(["btn-primary", "shadow-md"]))
    );
    assert_eq!(
        owner_name(&result, apply[1].containing_symbol_id.as_ref()),
        Some(".card")
    );
    let directive = facts(&result, "css.tailwind_directive.v1");
    assert_eq!(directive.len(), 1);
    assert_eq!(
        fact_meta(directive[0], "directive").and_then(|v| v.as_str()),
        Some("tailwind")
    );
    assert_eq!(
        fact_meta(directive[0], "argument").and_then(|v| v.as_str()),
        Some("base")
    );
}

#[test]
fn every_rule_and_block_at_rule_owns_its_facts() {
    let result = extract(
        "body { margin: 0; }\n#app { display: grid; }\n.card {\n  padding: 1rem;\n  .title { font-weight: 600; }\n  &:hover { color: red; }\n}\n@media (min-width: 40rem) { .card { padding: 2rem; } }\n@supports (display: grid) { .g { display: grid; } }\n",
    );
    for name in ["body", "#app", ".card", "&:hover"] {
        assert_eq!(symbol(&result, name).kind, SymbolKind::Property, "{name}");
    }
    let bound: Vec<(Option<&str>, Option<&serde_json::Value>)> = result
        .structural_facts
        .iter()
        .map(|fact| {
            (
                owner_name(&result, fact.containing_symbol_id.as_ref()),
                fact_meta(fact, "declaration_count"),
            )
        })
        .collect();
    let n = |count: u64| Some(serde_json::Value::from(count));
    assert_eq!(
        bound,
        [
            (Some("body"), n(1).as_ref()),
            (Some("#app"), n(1).as_ref()),
            (Some(".card"), n(1).as_ref()),
            (Some(".title"), n(1).as_ref()),
            (Some("&:hover"), n(1).as_ref()),
            (Some("@media (min-width: 40rem)"), None),
            (Some(".card"), n(1).as_ref()),
            (Some("@supports (display: grid)"), None),
            (Some(".g"), n(1).as_ref()),
        ]
    );
    let body = facts(&result, "css.selector_rule.v1")[0];
    assert_eq!(
        fact_meta(body, "selector_kind").and_then(|v| v.as_str()),
        Some("type")
    );
}

#[test]
fn scope_rule_is_a_parent_symbol_with_a_fact() {
    let result = extract(
        "/* Card-local styles */\n@scope (.card) to (.card-content) {\n  :scope { border: 1px solid; }\n  img { border-radius: 4px; }\n}\n",
    );
    let scope = symbol(&result, "@scope (.card) to (.card-content)");
    assert_eq!(
        scope.doc_comment.as_deref(),
        Some("/* Card-local styles */")
    );
    assert_eq!(
        symbol(&result, ":scope").parent_id.as_ref(),
        Some(&scope.id)
    );
    assert_eq!(symbol(&result, "img").parent_id.as_ref(), Some(&scope.id));
    let prelude_owner: Vec<Option<&String>> = result
        .identifiers
        .iter()
        .filter(|identifier| identifier.name.starts_with("card"))
        .map(|identifier| identifier.containing_symbol_id.as_ref())
        .collect();
    assert_eq!(prelude_owner, [Some(&scope.id), Some(&scope.id)]);
    let fact = facts(&result, "css.scope.v1");
    assert_eq!(fact.len(), 1);
    assert_eq!(fact[0].containing_symbol_id.as_ref(), Some(&scope.id));
    assert_eq!(
        fact_meta(fact[0], "root").and_then(|v| v.as_str()),
        Some(".card")
    );
    assert_eq!(
        fact_meta(fact[0], "limit").and_then(|v| v.as_str()),
        Some(".card-content")
    );
}

#[test]
fn css_modules_composes_links_local_and_imports_external() {
    let result = extract(
        ".base { padding: 4px; }\n.primary {\n  composes: base;\n  composes: shadow from \"./shared.module.css\";\n  color: blue;\n}\n",
    );
    let base = symbol(&result, ".base");
    let primary = symbol(&result, ".primary");
    assert!(result.relationships.iter().any(|relationship| {
        relationship.kind == RelationshipKind::References
            && relationship.from_symbol_id == primary.id
            && relationship.to_symbol_id == base.id
            && relationship.line_number == 3
    }));
    let composed: Vec<(&str, u32)> = result
        .identifiers
        .iter()
        .filter(|identifier| identifier.start_line >= 3)
        .map(|identifier| (identifier.name.as_str(), identifier.start_line))
        .collect();
    assert_eq!(composed, [("base", 3), ("shadow", 4)]);
    let pending: Vec<(&str, Option<&str>, u32)> = result
        .structured_pending_relationships
        .iter()
        .map(|pending| {
            (
                pending.target.display_name.as_str(),
                pending.target.import_context.as_deref(),
                pending.pending.line_number,
            )
        })
        .collect();
    assert_eq!(
        pending,
        [("./shared.module.css", Some("css-modules-composes"), 4)]
    );
}

#[test]
fn functional_pseudo_classes_get_one_name_identifier_each() {
    let result = extract(
        ":is(h1, h2) .title { margin: 0; }\n:where(.btn, .link):hover { color: blue; }\nbody:not(:has(.prevent-scroll)) { overflow: visible; }\n",
    );
    let calls: Vec<(&str, u32, u32, u32)> = result
        .identifiers
        .iter()
        .filter(|identifier| identifier.kind == IdentifierKind::Call)
        .map(|identifier| {
            (
                identifier.name.as_str(),
                identifier.start_line,
                identifier.start_column,
                identifier.end_column,
            )
        })
        .collect();
    assert_eq!(
        calls,
        [
            ("is", 1, 1, 3),
            ("where", 2, 1, 6),
            ("not", 3, 5, 8),
            ("has", 3, 10, 13)
        ]
    );
}

#[test]
fn vendor_prefixed_keyframes_are_symbols_with_named_facts() {
    let result = extract(
        "@-webkit-keyframes pulse {\n  50% { opacity: .5; }\n}\n@keyframes pulse {\n  50% { opacity: .5; }\n}\n",
    );
    let webkit = symbol(&result, "@-webkit-keyframes pulse");
    assert_eq!(webkit.kind, SymbolKind::Function);
    assert_eq!(meta(webkit, "animationName"), Some("pulse"));
    let keyframes = facts(&result, "css.keyframes.v1");
    assert_eq!(keyframes.len(), 2);
    assert_eq!(
        fact_meta(keyframes[0], "animation_name").and_then(|v| v.as_str()),
        Some("pulse")
    );
    assert_eq!(keyframes[0].containing_symbol_id.as_ref(), Some(&webkit.id));
}
