use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::language::supported_languages;
use crate::language_policy::{classify_literals_by_carrier, literal_carrier_policies};
use crate::{Literal, LiteralKind};

fn make_literal(language: &str, carrier: Option<&str>, text: &str) -> Literal {
    Literal {
        id: format!("lit-{language}-{text}"),
        literal_text: text.to_string(),
        kind: LiteralKind::Other,
        carrier: carrier.map(str::to_string),
        arg_position: 0,
        language: language.to_string(),
        file_path: "src/app.ts".to_string(),
        start_line: 1,
        start_column: 0,
        end_line: 1,
        end_column: 10,
        start_byte: 0,
        end_byte: 10,
        containing_symbol_id: None,
        confidence: 1.0,
    }
}

#[test]
fn literal_carrier_policy_files_are_extraction_only() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crate should live under crates/julie-extractors");
    let language_dir = repo_root.join("languages");
    let supported: BTreeSet<_> = supported_languages().iter().copied().collect();
    let entries = fs::read_dir(&language_dir)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", language_dir.display()));
    let mut files = Vec::new();

    for entry in entries {
        let path = entry.expect("language dir entry should be readable").path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("toml") {
            files.push(path);
        }
    }

    assert!(
        files.len() >= 20,
        "expected moved literal carrier configs for most extractor languages, got {files:?}"
    );

    let policies = literal_carrier_policies();
    for path in &files {
        let language = assert_policy_file_is_extraction_only(path, &supported);
        assert!(
            policies.contains_key(language),
            "policy file {} is not embedded by language_policy.rs",
            path.display()
        );
    }

    for language in policies.keys() {
        if language == "jsx" || language == "tsx" {
            continue;
        }
        assert!(
            files.iter().any(|path| {
                path.file_stem().and_then(|stem| stem.to_str()) == Some(language.as_str())
            }),
            "embedded policy {language:?} has no languages/{language}.toml file"
        );
    }
}

#[test]
fn embedded_literal_carrier_policy_loads_and_aliases_jsx_tsx() {
    let policies = literal_carrier_policies();
    let typescript = policies
        .get("typescript")
        .expect("typescript policy should load");
    assert!(
        typescript.url.contains("fetch") && typescript.url.contains("axios.get"),
        "typescript URL carriers should include fetch and axios.get: {:?}",
        typescript.url
    );
    assert!(
        typescript.sql.contains("query") && typescript.sql.contains("execute"),
        "typescript SQL carriers should include local receiver methods: {:?}",
        typescript.sql
    );

    let tsx = policies.get("tsx").expect("tsx alias should load");
    assert_eq!(
        tsx.url, typescript.url,
        "tsx should share TypeScript carriers"
    );

    let javascript = policies
        .get("javascript")
        .expect("javascript policy should load");
    let jsx = policies.get("jsx").expect("jsx alias should load");
    assert_eq!(
        jsx.url, javascript.url,
        "jsx should share JavaScript carriers"
    );

    let csharp = policies.get("csharp").expect("csharp policy should load");
    assert!(
        csharp.sql.contains("query") && csharp.sql.contains("executeasync"),
        "csharp SQL carriers should be lowercased: {:?}",
        csharp.sql
    );
}

#[test]
fn classify_literals_by_carrier_sets_kind_and_drops_bloat() {
    let mut literals = vec![
        make_literal("typescript", Some("fetch"), "/api/users"),
        make_literal("typescript", Some("pool.query"), "SELECT 1"),
        make_literal("typescript", Some("console.log"), "not useful"),
        make_literal("csharp", Some("Query"), "SELECT Id FROM Users"),
        make_literal("unknown", Some("fetch"), "/api/missing-policy"),
        make_literal("typescript", None, "/api/no-carrier"),
    ];

    classify_literals_by_carrier(&mut literals);

    let surviving: Vec<_> = literals
        .iter()
        .map(|literal| {
            (
                literal.language.as_str(),
                literal.carrier.as_deref(),
                literal.kind.clone(),
            )
        })
        .collect();

    assert_eq!(
        surviving,
        vec![
            ("typescript", Some("fetch"), LiteralKind::Url),
            ("typescript", Some("pool.query"), LiteralKind::Sql),
            ("csharp", Some("Query"), LiteralKind::Sql),
        ],
        "only configured carriers should survive classification"
    );
}

#[test]
fn fsharp_policy_retains_unclassified_literals() {
    let mut literals = vec![make_literal("fsharp", None, "42")];

    classify_literals_by_carrier(&mut literals);

    assert_eq!(literals.len(), 1);
    assert_eq!(literals[0].kind, LiteralKind::Other);
    assert_eq!(literals[0].literal_text, "42");
}

fn assert_policy_file_is_extraction_only<'a>(
    path: &'a PathBuf,
    supported: &BTreeSet<&str>,
) -> &'a str {
    let language = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .expect("policy filename should be valid UTF-8");
    assert!(
        supported.contains(language),
        "policy file {} does not match a supported language",
        path.display()
    );

    let contents =
        fs::read_to_string(path).unwrap_or_else(|err| panic!("failed to read {path:?}: {err}"));
    let mut saw_literal_carriers = false;
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            assert_eq!(
                trimmed,
                "[literal_carriers]",
                "{} contains non-extraction policy section {trimmed}",
                path.display()
            );
            saw_literal_carriers = true;
        }
    }
    assert!(
        saw_literal_carriers,
        "{} should contain [literal_carriers]",
        path.display()
    );
    language
}
