use std::path::Path;

use crate::base::{ExtractionResults, RelationshipKind, SourceRegionKind, Symbol};
use crate::extract_canonical;
use crate::tests::helpers::{facts_with_pattern, metadata_str};

fn extract(path: &str, source: &str) -> ExtractionResults {
    extract_canonical(path, source, Path::new("/tmp/test")).expect("JSON extraction succeeds")
}

fn symbol<'a>(result: &'a ExtractionResults, name: &str, line: u32) -> &'a Symbol {
    result
        .symbols
        .iter()
        .find(|s| s.name == name && s.start_line == line)
        .unwrap_or_else(|| panic!("missing {name} on line {line}: {:#?}", result.symbols))
}

fn references(result: &ExtractionResults, from: &Symbol, to: &Symbol) -> bool {
    result
        .relationships
        .iter()
        .any(|r| r.from_symbol_id == from.id && r.to_symbol_id == to.id)
}

fn pending_for<'a>(
    result: &'a ExtractionResults,
    from: &Symbol,
) -> Vec<&'a crate::base::StructuredPendingRelationship> {
    result
        .structured_pending_relationships
        .iter()
        .filter(|p| p.pending.from_symbol_id == from.id)
        .collect()
}

#[test]
fn local_ref_resolves_from_the_document_root() {
    let source = r##"{
  "properties": {
    "legacy": { "definitions": { "Server": { "type": "string" } } },
    "server": { "$ref": "#/definitions/Server" }
  },
  "definitions": { "Server": { "type": "object" } }
}"##;
    let result = extract("schema.json", source);
    let server = symbol(&result, "server", 4);
    assert!(references(&result, server, symbol(&result, "Server", 6)));
    assert!(!references(&result, server, symbol(&result, "Server", 3)));
}

#[test]
fn local_ref_decodes_json_escapes_and_pointer_tokens() {
    let source = r##"{
  "$defs": {
    "Config": { "type": "object" },
    "Routes": { "/users": { "type": "string" }, "a~b": { "type": "string" } }
  },
  "properties": {
    "route": { "$ref": "#/$defs/Routes/~1users" },
    "escaped": { "$ref": "#\/$defs\/Conf\u0069g" },
    "tilde": { "$ref": "#/$defs/Routes/a~0b" },
    "percent": { "$ref": "#/$defs/Routes/%7E1users" }
  }
}"##;
    let result = extract("schema.json", source);
    let users = symbol(&result, "/users", 4);
    assert!(references(&result, symbol(&result, "route", 7), users));
    assert!(references(
        &result,
        symbol(&result, "escaped", 8),
        symbol(&result, "Config", 3)
    ));
    assert!(references(
        &result,
        symbol(&result, "tilde", 9),
        symbol(&result, "a~b", 4)
    ));
    assert!(references(&result, symbol(&result, "percent", 10), users));
}

#[test]
fn anchor_ref_resolves_to_the_object_that_declares_the_anchor() {
    let source = r##"{
  "$defs": {
    "Named": { "$ref": "#person" },
    "Person": { "$anchor": "person", "type": "object" }
  }
}"##;
    let result = extract("schema.json", source);
    assert!(references(
        &result,
        symbol(&result, "Named", 3),
        symbol(&result, "Person", 4)
    ));
}

#[test]
fn whole_document_external_refs_emit_pending_rows() {
    let source = r##"{
  "paths": { "/orders": { "$ref": "./paths/orders.json" } },
  "components": { "schemas": {
    "Money": { "$ref": "common.json#" },
    "Customer": { "$ref": "https://schemas.example.org/customer.schema.json" }
  } }
}"##;
    let result = extract("openapi.json", source);
    let cases = [
        ("/orders", 2, "./paths/orders.json", "orders"),
        ("Money", 4, "common.json", "common"),
        (
            "Customer",
            5,
            "https://schemas.example.org/customer.schema.json",
            "customer.schema",
        ),
    ];
    for (name, line, context, terminal) in cases {
        let pending = pending_for(&result, symbol(&result, name, line));
        assert_eq!(pending.len(), 1, "{name}: {pending:#?}");
        assert_eq!(pending[0].target.import_context.as_deref(), Some(context));
        assert_eq!(pending[0].target.terminal_name, terminal);
        assert!(pending[0].target.namespace_path.is_empty());
    }
}

#[test]
fn jsonc_comments_document_the_following_key() {
    let source = r#"{
  /* Compiler settings */
  "compilerOptions": {
    // Emit ES2022 output.
    "target": "ES2022",
    /**
     * Enable every strict check.
     */
    "strict": true,
    "paths": { "@/*": ["src/*"] }, // Mirrors the Vite alias.
    "noEmit": true
  }
}"#;
    let result = extract("tsconfig.jsonc", source);
    let doc = |name: &str, line: u32| symbol(&result, name, line).doc_comment.clone();
    assert_eq!(
        doc("compilerOptions", 3).as_deref(),
        Some("/* Compiler settings */")
    );
    assert_eq!(doc("target", 5).as_deref(), Some("// Emit ES2022 output."));
    assert_eq!(
        doc("strict", 9).as_deref(),
        Some("/**\n     * Enable every strict check.\n     */")
    );
    assert_eq!(
        doc("paths", 10).as_deref(),
        Some("// Mirrors the Vite alias.")
    );
    assert_eq!(doc("noEmit", 11), None);
    let doc_regions = result
        .source_regions
        .iter()
        .filter(|r| r.kind == SourceRegionKind::DocComment)
        .count();
    assert_eq!(doc_regions, 3, "{:#?}", result.source_regions);
}

#[test]
fn schema_description_documents_the_described_object() {
    let source = r#"{
  "properties": {
    "install_id": { "description": "Random persistent identifier.", "type": "string" },
    "roles": { "title": "Granted roles", "type": "array" },
    "get": { "summary": "List pets", "description": "Longer text" }
  },
  "items": [ { "description": "First element" } ]
}"#;
    let result = extract("schema.json", source);
    let doc = |name: &str, line: u32| symbol(&result, name, line).doc_comment.clone();
    assert_eq!(
        doc("install_id", 3).as_deref(),
        Some("Random persistent identifier.")
    );
    assert_eq!(doc("roles", 4).as_deref(), Some("Granted roles"));
    assert_eq!(doc("get", 5).as_deref(), Some("Longer text"));
    assert_eq!(doc("[0]", 7).as_deref(), Some("First element"));
}

#[test]
fn keys_docs_and_literals_decode_json_escapes() {
    let source = r#"{
  "say \"hi\"": "She said \"hello\"",
  "caf\u00e9": "Caf\u00e9 menu",
  "path\\to": "C:\\temp\\file",
  "\"": "a lone quote key",
  "url": "https:\/\/example.com\/a"
}"#;
    let result = extract("escapes.json", source);
    let doc = |name: &str, line: u32| symbol(&result, name, line).doc_comment.clone();
    assert_eq!(doc("say \"hi\"", 2).as_deref(), Some("She said \"hello\""));
    assert_eq!(doc("café", 3).as_deref(), Some("Café menu"));
    assert_eq!(doc("path\\to", 4).as_deref(), Some("C:\\temp\\file"));
    assert_eq!(doc("\"", 5).as_deref(), Some("a lone quote key"));
    let keys: Vec<_> = facts_with_pattern(&result, "json.property.v1")
        .into_iter()
        .filter_map(|fact| metadata_str(fact, "key").map(str::to_string))
        .collect();
    assert!(keys.contains(&"say \"hi\"".to_string()), "{keys:?}");
    assert!(keys.contains(&"café".to_string()), "{keys:?}");
    assert!(
        result
            .literals
            .iter()
            .any(|l| l.literal_text == "https://example.com/a"),
        "{:#?}",
        result.literals
    );
}

#[test]
fn scalar_pairs_carry_a_signature() {
    let source = r#"{
  "port": 8080,
  "debug": true,
  "timeout": null,
  "name": "api",
  "limits": { "max": 1 }
}"#;
    let result = extract("config.json", source);
    let sig = |name: &str, line: u32| symbol(&result, name, line).signature.clone();
    assert_eq!(sig("port", 2).as_deref(), Some("\"port\": 8080"));
    assert_eq!(sig("debug", 3).as_deref(), Some("\"debug\": true"));
    assert_eq!(sig("timeout", 4).as_deref(), Some("\"timeout\": null"));
    assert_eq!(sig("name", 5).as_deref(), Some("\"name\": \"api\""));
    assert_eq!(sig("limits", 6), None);
}

#[test]
fn fact_paths_quote_keys_that_are_not_plain_names() {
    let source = r#"{
  "files.exclude": { "**/.git": true },
  "files": { "exclude": { "dist": true } }
}"#;
    let result = extract("settings.json", source);
    let path_of = |key: &str| {
        facts_with_pattern(&result, "json.property.v1")
            .into_iter()
            .find(|fact| metadata_str(fact, "key") == Some(key))
            .and_then(|fact| metadata_str(fact, "path").map(str::to_string))
    };
    assert_eq!(path_of("**/.git").as_deref(), Some("$['files.exclude']"));
    assert_eq!(path_of("dist").as_deref(), Some("$.files.exclude"));
}

#[test]
fn single_line_top_level_property_facts_bind_to_no_sibling() {
    let source = r#"{"name":"app","scripts":{"test":"jest"},"private":true}"#;
    let result = extract("package.json", source);
    let scripts = symbol(&result, "scripts", 1);
    for fact in facts_with_pattern(&result, "json.property.v1") {
        if matches!(metadata_str(fact, "key"), Some("name" | "private")) {
            assert_ne!(
                fact.containing_symbol_id.as_deref(),
                Some(scripts.id.as_str())
            );
        }
    }
}

#[test]
fn jsonl_body_spans_follow_their_record() {
    let source = concat!(
        "{\"id\": 1, \"user\": {\"name\": \"ada\"}}\n",
        "{\"id\": 2, \"user\": {\"name\": \"bob\"}}\n"
    );
    let result = extract("events.jsonl", source);
    let user = symbol(&result, "user", 2);
    let body = user.body_span.expect("object value has a body span");
    assert_eq!(body.start_line, 2);
    assert_eq!(
        &source[body.start_byte as usize..body.end_byte as usize],
        "{\"name\": \"bob\"}"
    );
}

#[test]
fn ndjson_and_json_family_extensions_extract() {
    let records = extract("events.ndjson", "{\"a\": 1}\n{\"a\": 2}\n");
    assert_eq!(
        records.symbols.iter().filter(|s| s.name == "a").count(),
        2,
        "{:#?}",
        records.symbols
    );
    assert_eq!(records.symbols[1].start_line, 2);
    for path in [
        "app.code-workspace",
        "person.jsonld",
        "map.geojson",
        "site.webmanifest",
    ] {
        let result = extract(path, "{ \"folders\": [] }");
        assert_eq!(result.symbols.len(), 1, "{path}");
        assert_eq!(result.symbols[0].language, "json", "{path}");
    }
}

#[test]
fn npm_and_composer_dependencies_emit_imports_and_facts() {
    let source = r#"{
  "name": "@acme/web",
  "scripts": { "test": "vitest run" },
  "dependencies": { "express": "^4.19.2", "@acme/core": "workspace:*" },
  "devDependencies": { "vitest": "^1.6.0" }
}"#;
    let result = extract("web/package.json", source);
    let deps = symbol(&result, "dependencies", 4);
    let express = symbol(&result, "express", 4);
    let edge = result
        .relationships
        .iter()
        .find(|r| r.from_symbol_id == deps.id && r.to_symbol_id == express.id)
        .expect("dependency table imports its dependency");
    assert_eq!(edge.kind, RelationshipKind::Imports);
    assert!(references_kind(
        &result,
        symbol(&result, "devDependencies", 5),
        symbol(&result, "vitest", 5)
    ));
    let facts = facts_with_pattern(&result, "manifest.dependency.v1");
    assert_eq!(facts.len(), 3, "{facts:#?}");
    let core = facts
        .iter()
        .find(|f| metadata_str(f, "name") == Some("@acme/core"))
        .unwrap();
    assert_eq!(metadata_str(core, "ecosystem"), Some("npm"));
    assert_eq!(metadata_str(core, "group"), Some("dependencies"));
    assert_eq!(metadata_str(core, "version"), Some("workspace:*"));
    assert_eq!(
        core.metadata.as_ref().unwrap().get("workspace"),
        Some(&serde_json::Value::Bool(true))
    );
    let scripts = facts_with_pattern(&result, "manifest.script.v1");
    assert_eq!(scripts.len(), 1);
    assert_eq!(metadata_str(scripts[0], "name"), Some("test"));
    assert_eq!(metadata_str(scripts[0], "command"), Some("vitest run"));

    let composer = extract(
        "composer.json",
        r#"{
  "require": { "php": ">=8.2", "laravel/framework": "^11.0" },
  "require-dev": { "phpunit/phpunit": "^11.0" },
  "scripts": { "test": ["@php artisan test", "phpstan"] }
}"#,
    );
    let facts = facts_with_pattern(&composer, "manifest.dependency.v1");
    assert_eq!(facts.len(), 3, "{facts:#?}");
    assert!(
        facts
            .iter()
            .all(|f| metadata_str(f, "ecosystem") == Some("composer"))
    );
    assert!(references_kind(
        &composer,
        symbol(&composer, "require-dev", 3),
        symbol(&composer, "phpunit/phpunit", 3)
    ));
    let scripts = facts_with_pattern(&composer, "manifest.script.v1");
    assert_eq!(
        metadata_str(scripts[0], "command"),
        Some("@php artisan test && phpstan")
    );
}

fn references_kind(result: &ExtractionResults, from: &Symbol, to: &Symbol) -> bool {
    result.relationships.iter().any(|r| {
        r.from_symbol_id == from.id
            && r.to_symbol_id == to.id
            && r.kind == RelationshipKind::Imports
    })
}

#[test]
fn tsconfig_extends_and_references_emit_pending_file_rows() {
    let source = r#"{
  "extends": "./tsconfig.base.json",
  "references": [ { "path": "../core" } ]
}"#;
    let result = extract("tsconfig.json", source);
    let extends = pending_for(&result, symbol(&result, "extends", 2));
    assert_eq!(extends.len(), 1);
    assert_eq!(
        extends[0].target.import_context.as_deref(),
        Some("./tsconfig.base.json")
    );
    assert_eq!(extends[0].target.terminal_name, "tsconfig.base");
    let path = pending_for(&result, symbol(&result, "path", 3));
    assert_eq!(path.len(), 1);
    assert_eq!(path[0].target.import_context.as_deref(), Some("../core"));
    assert!(
        extract("config.json", source)
            .structured_pending_relationships
            .is_empty()
    );
}

#[test]
fn schema_definitions_emit_definition_facts() {
    let source = r##"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "$defs": {
    "NewPet": { "type": "object" },
    "Pet": { "allOf": [ { "$ref": "#/$defs/NewPet" } ] },
    "Tags": { "type": ["array", "null"] }
  },
  "components": { "schemas": { "Wolf": { "oneOf": [] } } }
}"##;
    let result = extract("pet.schema.json", source);
    let facts = facts_with_pattern(&result, "json.schema_definition.v1");
    let row = |name: &str| {
        facts
            .iter()
            .find(|f| metadata_str(f, "name") == Some(name))
            .unwrap_or_else(|| panic!("missing {name}: {facts:#?}"))
    };
    assert_eq!(metadata_str(row("NewPet"), "container"), Some("$defs"));
    assert_eq!(metadata_str(row("NewPet"), "declared_type"), Some("object"));
    assert_eq!(metadata_str(row("Pet"), "composition"), Some("allOf"));
    assert_eq!(
        metadata_str(row("Tags"), "declared_type"),
        Some("array|null")
    );
    assert_eq!(
        metadata_str(row("Wolf"), "container"),
        Some("components.schemas")
    );
    assert_eq!(
        row("Pet").containing_symbol_id.as_deref(),
        Some(symbol(&result, "Pet", 5).id.as_str())
    );
    assert!(
        facts_with_pattern(
            &extract("config.json", r#"{ "definitions": { "a": { "type": "x" } }, "x": { "definitions": { "b": {} } } }"#),
            "json.schema_definition.v1"
        )
        .len()
            == 1
    );
}

#[test]
fn local_schema_file_emits_a_pending_row_and_urls_do_not() {
    let result = extract(
        "biome.json",
        r#"{ "$schema": "./node_modules/@biomejs/biome/configuration_schema.json" }"#,
    );
    let pending = &result.structured_pending_relationships;
    assert_eq!(pending.len(), 1, "{pending:#?}");
    assert_eq!(pending[0].target.terminal_name, "configuration_schema");
    let remote = extract(
        "schema.json",
        r#"{ "$schema": "https://json-schema.org/draft/2020-12/schema" }"#,
    );
    assert!(remote.structured_pending_relationships.is_empty());
}

#[test]
fn json_lines_paths_match_by_extension_in_any_case() {
    use crate::pipeline::is_json_lines_path;
    assert!(is_json_lines_path("logs/EVENTS.NDJSON"));
    assert!(is_json_lines_path("a\\b.jsonl"));
    assert!(is_json_lines_path(".jsonl"));
    assert!(!is_json_lines_path("jsonl"));
    assert!(!is_json_lines_path("events.json"));
}
