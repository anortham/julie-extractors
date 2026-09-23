use std::path::Path;

use crate::base::{ExtractionResults, LiteralKind, SourceRegionKind, Symbol, TestRole};
use crate::extract_canonical;
use crate::tests::helpers::{facts_with_pattern, metadata_str};

fn extract(path: &str, source: &str) -> ExtractionResults {
    extract_canonical(path, source, Path::new("/tmp/test")).expect("TOML extraction succeeds")
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

fn role(symbol: &Symbol) -> Option<&str> {
    symbol
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("test_role"))
        .and_then(serde_json::Value::as_str)
}

#[test]
fn leading_comment_blocks_document_tables_and_keys() {
    let source = r#"# Retry policy for outbound HTTP calls.
[http.retry]
# Maximum attempts before the client gives up.
# Counts the first try.
max_attempts = 5

backoff_ms = 100 # trailing note

# Cache settings for the query layer.
[cache]
# Greeting shown on the landing page.
greeting = "say \"hi\""
plain = "value"
"#;
    let result = extract("config.toml", source);
    let doc = |name: &str, line: u32| symbol(&result, name, line).doc_comment.clone();
    assert_eq!(
        doc("http.retry", 2).as_deref(),
        Some("# Retry policy for outbound HTTP calls.")
    );
    assert_eq!(
        doc("max_attempts", 5).as_deref(),
        Some("# Maximum attempts before the client gives up.\n# Counts the first try.")
    );
    assert_eq!(doc("backoff_ms", 7), None);
    assert_eq!(
        doc("cache", 10).as_deref(),
        Some("# Cache settings for the query layer.")
    );
    assert_eq!(
        doc("greeting", 12).as_deref(),
        Some("# Greeting shown on the landing page.")
    );
    assert_eq!(doc("plain", 13).as_deref(), Some("value"));
    let doc_regions: Vec<u32> = result
        .source_regions
        .iter()
        .filter(|r| r.kind == SourceRegionKind::DocComment)
        .map(|r| r.start_line)
        .collect();
    assert_eq!(
        doc_regions,
        vec![1, 3, 4, 9, 11],
        "{:#?}",
        result.source_regions
    );
}

#[test]
fn string_values_decode_escapes_and_multiline_forms() {
    let source = "[build]\n\
command = \"\"\"\n\
npm ci && \\\n    npm run build\n\"\"\"\n\
path = \"C:\\\\Users\\\\app\"\n\
quote = \"say \\\"hi\\\"\"\n\
addopts = \"-m 'not integration'\"\n\
raw = '''\nline one\nline two'''\n\
lit = 'C:\\temp'\n";
    let result = extract("netlify.toml", source);
    let doc = |name: &str| {
        result
            .symbols
            .iter()
            .find(|s| s.name == name)
            .and_then(|s| s.doc_comment.clone())
    };
    assert_eq!(doc("command").as_deref(), Some("npm ci && npm run build\n"));
    assert_eq!(doc("path").as_deref(), Some("C:\\Users\\app"));
    assert_eq!(doc("quote").as_deref(), Some("say \"hi\""));
    assert_eq!(doc("addopts").as_deref(), Some("-m 'not integration'"));
    assert_eq!(doc("raw").as_deref(), Some("line one\nline two"));
    assert_eq!(doc("lit").as_deref(), Some("C:\\temp"));
    let style = |key: &str| {
        facts_with_pattern(&result, "toml.key_value.v1")
            .into_iter()
            .find(|f| metadata_str(f, "key") == Some(key))
            .and_then(|f| metadata_str(f, "string_style").map(str::to_string))
    };
    assert_eq!(style("command").as_deref(), Some("multiline_basic"));
    assert_eq!(style("path").as_deref(), Some("basic"));
    assert_eq!(style("raw").as_deref(), Some("multiline_literal"));
    assert_eq!(style("lit").as_deref(), Some("literal"));
    assert!(
        result
            .literals
            .iter()
            .any(|l| l.literal_text == "say \"hi\"")
    );
}

#[test]
fn quoted_keys_use_unquoted_segments_everywhere() {
    let source = r#"['quoted'.table]
x = 1
[project.entry-points."pytest11"]
acme = "acme.pytest_plugin"
[ spaced . table ]
z = 3
[server]
"key=with=eq" = "x"
"" = "empty"
"a\u00e9" = 1
entry-points."pytest11".shop = "demo"
"#;
    let result = extract("config.toml", source);
    for (name, line) in [
        ("quoted.table", 1),
        ("project.entry-points.pytest11", 3),
        ("spaced.table", 5),
        ("key=with=eq", 8),
        ("", 9),
        ("aé", 10),
        ("entry-points.pytest11.shop", 11),
    ] {
        symbol(&result, name, line);
    }
    let fact = |line: u32, pattern: &str| {
        facts_with_pattern(&result, pattern)
            .into_iter()
            .find(|f| f.start_line == line)
            .unwrap_or_else(|| panic!("no {pattern} on line {line}"))
    };
    assert_eq!(
        metadata_str(fact(1, "toml.table.v1"), "table_name"),
        Some("quoted.table")
    );
    assert_eq!(
        metadata_str(fact(4, "toml.key_value.v1"), "key_path"),
        Some("project.entry-points.pytest11.acme")
    );
    assert_eq!(
        metadata_str(fact(6, "toml.key_value.v1"), "key_path"),
        Some("spaced.table.z")
    );
    assert_eq!(
        metadata_str(fact(8, "toml.key_value.v1"), "key"),
        Some("key=with=eq")
    );
}

#[test]
fn array_table_key_paths_carry_the_element_index() {
    let source = r#"[[products]]
name = "Hammer"

[[products]]
name = "Nail"

[products.dims]
width = 2

[[products.variants]]
sku = "n-1"
"#;
    let result = extract("inventory.toml", source);
    let paths: Vec<(u32, String)> = facts_with_pattern(&result, "toml.key_value.v1")
        .into_iter()
        .chain(facts_with_pattern(&result, "toml.array_table.v1"))
        .chain(facts_with_pattern(&result, "toml.table.v1"))
        .map(|f| {
            (
                f.start_line,
                metadata_str(f, "key_path").unwrap().to_string(),
            )
        })
        .collect();
    for expected in [
        (1, "products[0]"),
        (2, "products[0].name"),
        (4, "products[1]"),
        (5, "products[1].name"),
        (7, "products[1].dims"),
        (8, "products[1].dims.width"),
        (10, "products[1].variants[0]"),
        (11, "products[1].variants[0].sku"),
    ] {
        assert!(
            paths.contains(&(expected.0, expected.1.to_string())),
            "missing {expected:?} in {paths:?}"
        );
    }
}

#[test]
fn config_url_keys_classify_literals_as_urls() {
    let result = extract(
        "config.toml",
        "[worker]\napi_url = \"https://api.example.com\"\nname = \"x\"\n",
    );
    let mut literals = result.literals.clone();
    crate::language_policy::classify_literals_by_carrier(&mut literals);
    let kind = |text: &str| {
        literals
            .iter()
            .find(|l| l.literal_text == text)
            .map(|l| l.kind.clone())
    };
    assert_eq!(kind("https://api.example.com"), Some(LiteralKind::Url));
    assert_eq!(kind("x"), Some(LiteralKind::Other));
}

#[test]
fn trycmd_case_files_without_stdout_are_test_cases() {
    for source in [
        "bin.name = \"bin-fixture\"\nargs = [\"--exit\", \"42\"]\nstatus.code = 42\n",
        "bin.name = \"bin-fixture\"\nargs = \"--fail\"\nstatus = \"failed\"\n",
        "bin.name = \"bin-fixture\"\nargs = [\"--help\"]\n",
    ] {
        let result = extract("tests/cmd/exit.toml", source);
        assert_eq!(
            role(symbol(&result, "bin.name", 1)),
            Some(TestRole::TestCase.as_str()),
            "{source}"
        );
    }
    let plain = extract("tool.toml", "bin.name = \"x\"\n");
    assert_eq!(role(symbol(&plain, "bin.name", 1)), None);
}

#[test]
fn nextest_config_at_its_canonical_path_needs_no_marker() {
    let source = "[profile.ci]\nretries = 2\n[test-groups.serial-db]\nmax-threads = 1\n[scripts.setup.migrate]\ncommand = \"cargo run -p migrate\"\n";
    let result = extract(".config/nextest.toml", source);
    assert_eq!(
        role(symbol(&result, "test-groups.serial-db", 3)),
        Some(TestRole::TestContainer.as_str())
    );
    assert_eq!(
        role(symbol(&result, "scripts.setup.migrate", 5)),
        Some(TestRole::FixtureSetup.as_str())
    );
    let elsewhere = extract("other.toml", source);
    assert_eq!(role(symbol(&elsewhere, "test-groups.serial-db", 3)), None);
}

#[test]
fn cargo_features_reference_features_and_dependencies() {
    let source = r#"[features]
default = ["std", "json"]
std = ["serde?/std"]
json = ["dep:serde_json", "serde"]
full = ["default", "tokio/full"]

[dependencies]
serde = { version = "1", optional = true }
serde_json = { version = "1", optional = true }

[dependencies.tokio]
version = "1"

[[bin]]
name = "mylib-cli"
required-features = ["full"]
"#;
    let result = extract("Cargo.toml", source);
    let default = symbol(&result, "default", 2);
    let std = symbol(&result, "std", 3);
    let json = symbol(&result, "json", 4);
    let full = symbol(&result, "full", 5);
    let serde = symbol(&result, "serde", 8);
    let serde_json = symbol(&result, "serde_json", 9);
    let tokio = symbol(&result, "dependencies.tokio", 11);
    for (from, to) in [
        (default, std),
        (default, json),
        (std, serde),
        (json, serde_json),
        (json, serde),
        (full, default),
        (full, tokio),
        (symbol(&result, "required-features", 16), full),
    ] {
        assert!(
            references(&result, from, to),
            "{} -> {}: {:#?}",
            from.name,
            to.name,
            result.relationships
        );
    }
}

#[test]
fn gradle_version_catalog_links_version_refs_and_bundles() {
    let source = r#"[versions]
coreKtx = "1.13.1"
kotlin = "2.0.0"

[libraries]
androidx-core-ktx = { group = "androidx.core", name = "core-ktx", version.ref = "coreKtx" }
junit = { module = "junit:junit", version = "4.13.2" }

[bundles]
testing = ["junit", "androidx-core-ktx"]

[plugins]
kotlin-android = { id = "org.jetbrains.kotlin.android", version.ref = "kotlin" }
"#;
    let result = extract("gradle/libs.versions.toml", source);
    let pairs = [
        (
            symbol(&result, "version.ref", 6),
            symbol(&result, "coreKtx", 2),
        ),
        (symbol(&result, "testing", 10), symbol(&result, "junit", 7)),
        (
            symbol(&result, "testing", 10),
            symbol(&result, "androidx-core-ktx", 6),
        ),
        (
            symbol(&result, "version.ref", 13),
            symbol(&result, "kotlin", 3),
        ),
    ];
    for (from, to) in pairs {
        assert!(
            references(&result, from, to),
            "{} -> {}: {:#?}",
            from.name,
            to.name,
            result.relationships
        );
    }
}

#[test]
fn pyproject_tool_tables_never_reference_themselves() {
    let source = "[tool.poetry]\nname = \"shop-api\"\n\n[tool.pytest.ini_options]\ntestpaths = [\"tests\"]\n";
    let result = extract("pyproject.toml", source);
    assert!(
        result
            .relationships
            .iter()
            .all(|r| r.from_symbol_id != r.to_symbol_id),
        "{:#?}",
        result.relationships
    );
    let poetry = symbol(&result, "tool.poetry", 1);
    let pytest = symbol(&result, "tool.pytest.ini_options", 4);
    assert!(references(&result, poetry, pytest));
}

#[test]
fn pipfile_is_toml_with_dependency_facts() {
    let source = "[packages]\nrequests = \"*\"\n\n[dev-packages]\npytest = \">=8\"\n\n[scripts]\ntest = \"pytest -q\"\n";
    let result = extract("Pipfile", source);
    assert!(result.symbols.iter().all(|s| s.language == "toml"));
    symbol(&result, "requests", 2);
    let facts = facts_with_pattern(&result, "manifest.dependency.v1");
    let groups: Vec<_> = facts
        .iter()
        .map(|f| (metadata_str(f, "name"), metadata_str(f, "group")))
        .collect();
    assert_eq!(
        groups,
        vec![
            (Some("requests"), Some("pipenv:packages")),
            (Some("pytest"), Some("pipenv:dev-packages"))
        ]
    );
    assert!(references(
        &result,
        symbol(&result, "packages", 1),
        symbol(&result, "requests", 2)
    ));
}
