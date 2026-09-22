use crate::base::{ExtractionResults, RelationshipKind, StructuralFact};
use crate::extract_canonical;
use crate::tests::helpers::{facts_with_pattern, metadata_str};
use std::collections::BTreeSet;
use std::path::Path;

fn extract(file: &str, source: &str) -> ExtractionResults {
    extract_canonical(file, source, Path::new("/tmp/test")).unwrap()
}

fn imported_names(results: &ExtractionResults) -> BTreeSet<(String, String)> {
    results
        .relationships
        .iter()
        .filter(|r| r.kind == RelationshipKind::Imports)
        .map(|r| {
            let metadata = r.metadata.as_ref().unwrap();
            (
                metadata["dependencyName"].as_str().unwrap().to_string(),
                metadata["dependencyKind"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

fn dependency_facts(results: &ExtractionResults) -> Vec<&StructuralFact> {
    facts_with_pattern(results, "manifest.dependency.v1")
}

fn pair(name: &str, kind: &str) -> (String, String) {
    (name.to_string(), kind.to_string())
}

#[test]
fn cargo_dependency_forms_emit_imports_by_crate_name() {
    let source = r#"[package]
name = "app"

[workspace.dependencies]
serde = { version = "1.0" }

[dependencies]
serde.workspace = true
serde.features = ["derive"]
json = { package = "serde_json", version = "1" }

[dependencies.reqwest]
version = "0.12"

[target.'cfg(windows)'.dev-dependencies]
windows-sys = "0.59"

[target.x86_64-pc-windows-msvc.build-dependencies]
cc = "1"
"#;
    let results = extract("Cargo.toml", source);

    assert_eq!(
        imported_names(&results),
        BTreeSet::from([
            pair("serde", "workspace"),
            pair("serde", "dependencies"),
            pair("json", "dependencies"),
            pair("reqwest", "dependencies"),
            pair("windows-sys", "dev-dependencies"),
            pair("cc", "build-dependencies"),
        ])
    );
    let facts = dependency_facts(&results);
    let fact = |name: &str, group: &str| {
        *facts
            .iter()
            .find(|f| {
                metadata_str(f, "name") == Some(name) && metadata_str(f, "group") == Some(group)
            })
            .unwrap_or_else(|| panic!("missing {name} fact: {facts:#?}"))
    };
    assert_eq!(
        metadata_str(fact("json", "dependencies"), "package"),
        Some("serde_json")
    );
    assert_eq!(
        metadata_str(fact("reqwest", "dependencies"), "version"),
        Some("0.12")
    );
    assert_eq!(
        metadata_str(fact("windows-sys", "dev-dependencies"), "target"),
        Some("cfg(windows)")
    );
    let inherited = fact("serde", "dependencies").metadata.as_ref().unwrap();
    assert_eq!(inherited["workspace"], serde_json::Value::Bool(true));
}

#[test]
fn cargo_workspace_inheritance_emits_pending_references() {
    let source = r#"[package]
name = "member"
version.workspace = true
edition = { workspace = true }

[dependencies]
anyhow = { workspace = true }

[lints]
workspace = true
"#;
    let results = extract("crates/member/Cargo.toml", source);

    let targets: BTreeSet<_> = results
        .structured_pending_relationships
        .iter()
        .map(|p| p.target.display_name.clone())
        .collect();
    assert_eq!(
        targets,
        BTreeSet::from([
            "workspace.package.version".to_string(),
            "workspace.package.edition".to_string(),
            "workspace.dependencies.anyhow".to_string(),
            "workspace.lints".to_string(),
        ])
    );
}

#[test]
fn pyproject_requirements_emit_dependency_facts() {
    let source = r#"[build-system]
requires = ["hatchling>=1.20"]

[project]
name = "shop"
dependencies = [
    "fastapi>=0.115",
    "SQLAlchemy[asyncio]>=2.0; python_version >= '3.10'",
]

[project.optional-dependencies]
redis = ["redis>=5"]

[dependency-groups]
test = ["pytest>=8", "httpx", { include-group = "lint" }]
"#;
    let results = extract("pyproject.toml", source);

    let facts = dependency_facts(&results);
    let summary: BTreeSet<_> = facts
        .iter()
        .map(|f| {
            (
                metadata_str(f, "name").unwrap().to_string(),
                metadata_str(f, "group").unwrap().to_string(),
            )
        })
        .collect();
    assert_eq!(
        summary,
        BTreeSet::from([
            pair("hatchling", "build-system"),
            pair("fastapi", "runtime"),
            pair("sqlalchemy", "runtime"),
            pair("redis", "optional:redis"),
            pair("pytest", "group:test"),
            pair("httpx", "group:test"),
        ])
    );
    let sqlalchemy = facts
        .iter()
        .find(|f| metadata_str(f, "name") == Some("sqlalchemy"))
        .unwrap();
    let metadata = sqlalchemy.metadata.as_ref().unwrap();
    assert_eq!(metadata["extras"], serde_json::json!(["asyncio"]));
    assert_eq!(metadata_str(sqlalchemy, "version"), Some(">=2.0"));
    assert_eq!(
        metadata_str(sqlalchemy, "marker"),
        Some("python_version >= '3.10'")
    );
    assert!(
        facts
            .iter()
            .all(|f| metadata_str(f, "ecosystem") == Some("pypi"))
    );
}

#[test]
fn poetry_dependency_tables_emit_imports() {
    let source = r#"[tool.poetry.dependencies]
python = "^3.11"
fastapi = "^0.115"

[tool.poetry.group.dev.dependencies]
pytest = { version = "^8" }
"#;
    let results = extract("pyproject.toml", source);

    let imports = imported_names(&results);
    assert!(imports.contains(&pair("fastapi", "poetry:main")));
    assert!(imports.contains(&pair("pytest", "poetry:dev")));
    assert!(!imports.iter().any(|(name, _)| name == "python"));
}

#[test]
fn python_entry_points_emit_pending_references() {
    let source = r#"[project.scripts]
acme = "acme.cli:main"

[project.entry-points."pytest11"]
acme = "acme.pytest_plugin"

[tool.poetry.scripts]
acme-admin = "acme.admin:App.run"
"#;
    let results = extract("pyproject.toml", source);

    let targets: BTreeSet<_> = results
        .structured_pending_relationships
        .iter()
        .map(|p| {
            (
                p.target.terminal_name.clone(),
                p.target.namespace_path.join("."),
                p.target.import_context.clone().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        targets,
        BTreeSet::from([
            (
                "main".to_string(),
                "acme.cli".to_string(),
                "acme.cli".to_string()
            ),
            (
                "pytest_plugin".to_string(),
                "acme".to_string(),
                "acme.pytest_plugin".to_string()
            ),
            (
                "run".to_string(),
                "acme.admin.App".to_string(),
                "acme.admin".to_string()
            ),
        ])
    );
}
