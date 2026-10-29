use crate::ExtractionResults;
use crate::base::SymbolKind;
use crate::pipeline::extract_canonical;
use crate::tests::helpers::import_rows_with_metadata;
use serde_json::json;
use std::path::Path;

fn extract(path: &str, source: &str) -> ExtractionResults {
    extract_canonical(path, source, Path::new("/workspace")).expect("extract")
}

#[test]
fn require_and_include_record_their_source_and_form() {
    let results = extract(
        "src/boot.php",
        "<?php\nrequire_once __DIR__ . '/lib/helpers.php';\nrequire 'config.php';\ninclude_once \"views/header.php\";\ninclude(dirname(__FILE__) . \"/x.php\");\nrequire dirname(__DIR__) . '/vendor/autoload.php';\nrequire_once(__DIR__ . '/../shared/db.php');\n",
    );
    assert_eq!(
        import_rows_with_metadata(&results),
        vec![
            (
                "./lib/helpers.php".to_string(),
                json!({"form": "require_once", "source": "./lib/helpers.php", "relative_to": "file"})
            ),
            (
                "config.php".to_string(),
                json!({"form": "require", "source": "config.php"})
            ),
            (
                "views/header.php".to_string(),
                json!({"form": "include_once", "source": "views/header.php"})
            ),
            (
                "./x.php".to_string(),
                json!({"form": "include", "source": "./x.php", "relative_to": "file"})
            ),
            (
                "../vendor/autoload.php".to_string(),
                json!({"form": "require", "source": "../vendor/autoload.php", "relative_to": "file"})
            ),
            (
                "../shared/db.php".to_string(),
                json!({"form": "require_once", "source": "../shared/db.php", "relative_to": "file"})
            ),
        ]
    );
}

#[test]
fn computed_include_paths_keep_the_row_without_a_source() {
    let results = extract(
        "src/boot.php",
        "<?php\nrequire $base . '/other.php';\ninclude \"views/$page.php\";\nrequire __DIR__ . DIRECTORY_SEPARATOR . 'x.php';\n",
    );
    assert_eq!(
        import_rows_with_metadata(&results),
        vec![
            (
                "$base . '/other.php'".to_string(),
                json!({"form": "require"})
            ),
            (
                "\"views/$page.php\"".to_string(),
                json!({"form": "include"})
            ),
            (
                "__DIR__ . DIRECTORY_SEPARATOR . 'x.php'".to_string(),
                json!({"form": "require"})
            ),
        ]
    );
}

#[test]
fn include_inside_a_function_belongs_to_the_function() {
    let results = extract(
        "src/boot.php",
        "<?php\nfunction boot() {\n    require_once __DIR__ . '/lib/helpers.php';\n}\n",
    );
    let function = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "boot")
        .expect("function boot");
    let import = results
        .symbols
        .iter()
        .find(|symbol| symbol.kind == SymbolKind::Import)
        .expect("include import");
    assert_eq!(import.parent_id.as_deref(), Some(function.id.as_str()));
}

#[test]
fn includes_leave_uses_calls_and_route_facts_unchanged() {
    let code = "<?php\nnamespace App;\n\nuse Illuminate\\Support\\Facades\\Route;\nuse App\\Http\\Controllers\\UserController;\n\nfunction helpers() { return 1; }\n\nfunction boot() {\n    helpers();\n    return new UserController();\n}\n\nRoute::get('/users/{id}', [UserController::class, 'show']);\n";
    let plain = extract("routes/web.php", code);
    let with_includes = extract(
        "routes/web.php",
        &format!(
            "{code}require __DIR__ . '/helpers.php';\nrequire 'UserController.php';\nfunction later() {{ include dirname(__FILE__) . '/auth.php'; helpers(); }}\n"
        ),
    );
    let last_plain_line = code.lines().count() as u32;

    let symbols = |results: &ExtractionResults| -> Vec<serde_json::Value> {
        results
            .symbols
            .iter()
            .filter(|symbol| symbol.start_line <= last_plain_line)
            .map(|symbol| serde_json::to_value(symbol).expect("serialize"))
            .collect()
    };
    assert_eq!(symbols(&with_includes), symbols(&plain));

    let relationships = |results: &ExtractionResults| -> Vec<serde_json::Value> {
        results
            .relationships
            .iter()
            .filter(|relationship| relationship.line_number <= last_plain_line)
            .map(|relationship| serde_json::to_value(relationship).expect("serialize"))
            .collect()
    };
    assert_eq!(relationships(&with_includes), relationships(&plain));

    let pending = |results: &ExtractionResults| -> Vec<serde_json::Value> {
        results
            .pending_relationships
            .iter()
            .filter(|pending| pending.line_number <= last_plain_line)
            .map(|pending| serde_json::to_value(pending).expect("serialize"))
            .collect()
    };
    assert_eq!(pending(&with_includes), pending(&plain));

    let identifiers = |results: &ExtractionResults| -> Vec<serde_json::Value> {
        results
            .identifiers
            .iter()
            .filter(|identifier| identifier.start_line <= last_plain_line)
            .map(|identifier| serde_json::to_value(identifier).expect("serialize"))
            .collect()
    };
    assert_eq!(identifiers(&with_includes), identifiers(&plain));

    let facts = |results: &ExtractionResults| -> Vec<serde_json::Value> {
        results
            .structural_facts
            .iter()
            .filter(|fact| fact.start_line <= last_plain_line)
            .map(|fact| serde_json::to_value(fact).expect("serialize"))
            .collect()
    };
    assert!(!facts(&plain).is_empty());
    assert_eq!(facts(&with_includes), facts(&plain));

    let include_ids: Vec<_> = with_includes
        .symbols
        .iter()
        .filter(|symbol| symbol.kind == SymbolKind::Import && symbol.start_line > last_plain_line)
        .map(|symbol| symbol.id.as_str())
        .collect();
    assert_eq!(include_ids.len(), 3);
    assert!(with_includes.relationships.iter().all(|relationship| {
        !include_ids.contains(&relationship.from_symbol_id.as_str())
            && !include_ids.contains(&relationship.to_symbol_id.as_str())
    }));
    assert!(
        with_includes
            .pending_relationships
            .iter()
            .all(|pending| !include_ids.contains(&pending.from_symbol_id.as_str()))
    );
    let later = with_includes
        .symbols
        .iter()
        .find(|symbol| symbol.name == "later")
        .expect("function later");
    let helpers = with_includes
        .symbols
        .iter()
        .find(|symbol| symbol.name == "helpers" && symbol.kind == SymbolKind::Function)
        .expect("function helpers");
    assert!(with_includes.relationships.iter().any(|relationship| {
        relationship.from_symbol_id == later.id && relationship.to_symbol_id == helpers.id
    }));
}
