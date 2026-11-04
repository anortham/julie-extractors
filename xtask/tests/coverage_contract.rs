use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tempfile::tempdir;
use xtask::coverage::{
    CoverageManifest, CoveragePlan, CoverageSample, FactAssertion, LicenseEvidenceFile,
    SourceProvenance, SourceSelection, run, validate_manifest_shape, verify_fact_assertions,
};

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask crate has a repository root")
        .to_path_buf()
}

fn manifest_value() -> Value {
    let path = repository_root().join("fixtures/real-world/coverage-manifest.json");
    serde_json::from_slice(&fs::read(path).expect("read coverage manifest"))
        .expect("parse coverage manifest")
}

#[test]
fn diagnostic_probes_use_registered_revisions_for_known_upstream_projects() {
    let manifest = manifest_value();
    let mut registered =
        std::collections::BTreeMap::<&str, std::collections::BTreeSet<&str>>::new();
    for sample in manifest["languages"].as_array().expect("language samples") {
        let provenance = &sample["provenance"];
        let source_url = provenance["source_url"].as_str().expect("source URL");
        if let Some((project, _)) = source_url.split_once("/blob/") {
            registered.entry(project).or_default().insert(
                provenance["revision"]
                    .as_str()
                    .expect("registered revision"),
            );
        }
    }
    let probes: Value = serde_json::from_slice(
        &fs::read(repository_root().join("fixtures/real-world/coverage/diagnostic-probes.json"))
            .expect("diagnostic probe ledger"),
    )
    .expect("valid probe ledger");
    let mut checked = 0;
    for probe in probes["probes"].as_array().expect("diagnostic probes") {
        let source_url = probe["source_url"].as_str().expect("probe URL");
        let Some((project, _)) = source_url.split_once("/blob/") else {
            continue;
        };
        let Some(revisions) = registered.get(project) else {
            continue;
        };
        let revision = probe["revision"].as_str().expect("probe revision");
        assert!(
            revisions.contains(revision),
            "{}: {project} revision {revision} is not a registered upstream snapshot",
            probe["probe"]
        );
        checked += 1;
    }
    assert!(checked > 0);
}

fn validate_value(value: Value) -> Result<(), String> {
    let manifest: CoverageManifest =
        serde_json::from_value(value).map_err(|error| error.to_string())?;
    validate_manifest_shape(&manifest).map_err(|error| error.to_string())
}

fn write_manifest(value: &Value, directory: &Path) -> PathBuf {
    let path = directory.join("coverage-manifest.json");
    fs::write(
        &path,
        serde_json::to_vec(value).expect("serialize test manifest"),
    )
    .expect("write test manifest");
    path
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn project_evidence<'a>(manifest: &'a Value, project: &str) -> &'a Value {
    manifest["languages"]
        .as_array()
        .expect("language rows")
        .iter()
        .find(|sample| sample["provenance"]["project"] == project)
        .expect("project source row")
        .get("provenance")
        .and_then(|provenance| provenance.get("license_evidence_files"))
        .expect("project evidence files")
}

fn artifact() -> Connection {
    let connection = Connection::open_in_memory().expect("open test database");
    connection
        .execute_batch(
            "CREATE TABLE symbols (path TEXT, language TEXT, name TEXT, kind TEXT, start_byte INTEGER);
             INSERT INTO symbols VALUES ('source.rs', 'rust', 'save', 'function', 6);
             INSERT INTO symbols VALUES ('other.rs', 'rust', 'save', 'function', 6);
             INSERT INTO symbols VALUES ('source.rs', 'python', 'save', 'function', 6);
             INSERT INTO symbols VALUES ('source.rs', 'javascript', 'render', 'function', 12);",
        )
        .expect("create fact rows");
    connection
}

#[test]
fn supported_facts_are_matched_against_exact_language_and_source_file() {
    let expected = [FactAssertion {
        domain: "symbols".to_string(),
        table: "symbols".to_string(),
        field: "name".to_string(),
        value: "save".to_string(),
        min_count: 1,
        max_count: Some(1),
        expected_start_byte: None,
        fact_language: None,
    }];

    let result = verify_fact_assertions(&artifact(), "source.rs", "rust", &expected)
        .expect("the source file has exactly one matching rust fact");

    assert_eq!(result[0].actual_count, 1);
}

#[test]
fn missing_expected_facts_fail_with_the_observed_count() {
    let expected = [FactAssertion {
        domain: "symbols".to_string(),
        table: "symbols".to_string(),
        field: "name".to_string(),
        value: "persist".to_string(),
        min_count: 1,
        max_count: None,
        expected_start_byte: None,
        fact_language: None,
    }];

    let error = verify_fact_assertions(&artifact(), "source.rs", "rust", &expected)
        .expect_err("a nonempty unrelated table must not satisfy the assertion");

    assert!(error.to_string().contains("expected 1"));
    assert!(error.to_string().contains("found 0"));
}

#[test]
fn negative_controls_fail_when_a_confusable_fact_is_present() {
    let expected = [FactAssertion {
        domain: "symbols".to_string(),
        table: "symbols".to_string(),
        field: "name".to_string(),
        value: "save".to_string(),
        min_count: 0,
        max_count: Some(0),
        expected_start_byte: None,
        fact_language: None,
    }];

    let error = verify_fact_assertions(&artifact(), "source.rs", "rust", &expected)
        .expect_err("a confusable fact in the tested language must fail a negative control");

    assert!(error.to_string().contains("found 1"));
}

#[test]
fn source_offset_assertions_check_the_exact_persisted_byte_position() {
    let expected = [FactAssertion {
        domain: "symbols".to_string(),
        table: "symbols".to_string(),
        field: "name".to_string(),
        value: "save".to_string(),
        min_count: 1,
        max_count: Some(1),
        expected_start_byte: Some(6),
        fact_language: None,
    }];

    assert!(verify_fact_assertions(&artifact(), "source.rs", "rust", &expected).is_ok());

    let wrong_offset = [FactAssertion {
        expected_start_byte: Some(5),
        ..expected[0].clone()
    }];
    assert!(
        verify_fact_assertions(&artifact(), "source.rs", "rust", &wrong_offset)
            .expect_err("an offset shifted by one byte must fail")
            .to_string()
            .contains("found 0")
    );
}

#[test]
fn embedded_facts_match_their_inner_language_and_exact_host_file() {
    let expected = [FactAssertion {
        domain: "symbols".to_string(),
        table: "symbols".to_string(),
        field: "name".to_string(),
        value: "render".to_string(),
        min_count: 1,
        max_count: Some(1),
        expected_start_byte: Some(12),
        fact_language: Some("javascript".to_string()),
    }];

    let result = verify_fact_assertions(&artifact(), "source.rs", "html", &expected)
        .expect("embedded rows retain their host path and inner language");

    assert_eq!(result[0].actual_count, 1);
}

#[test]
fn every_declared_domain_requires_a_positive_fact_proof() {
    let manifest = CoverageManifest {
        format_version: 1,
        languages: vec![CoverageSample {
            language: "rust".to_string(),
            source_path: "fixtures/real-world/coverage/rust/source.rs".to_string(),
            source_sha256: "a".repeat(64),
            provenance: SourceProvenance {
                project: "Example".to_string(),
                source_url: "https://example.com/0123456789abcdef/source.rs".to_string(),
                revision: "0123456789abcdef".to_string(),
                source_file: "source.rs".to_string(),
                license: "MIT".to_string(),
                license_evidence_url: "https://example.com/0123456789abcdef/LICENSE".to_string(),
                license_evidence_files: vec![LicenseEvidenceFile {
                    kind: "license".to_string(),
                    path: "fixtures/real-world/coverage/licenses/example/LICENSE".to_string(),
                    sha256: "b".repeat(64),
                    source_url: "https://example.com/0123456789abcdef/LICENSE".to_string(),
                }],
                source_selection: None,
            },
            expected_domains: vec!["relationships".to_string()],
            expected_facts: vec![FactAssertion {
                domain: "symbols".to_string(),
                table: "symbols".to_string(),
                field: "name".to_string(),
                value: "main".to_string(),
                min_count: 1,
                max_count: Some(1),
                expected_start_byte: None,
                fact_language: None,
            }],
            negative_controls: Vec::new(),
            expected_parse_diagnostics: 0,
        }],
        controls: Vec::new(),
    };

    assert!(
        validate_manifest_shape(&manifest)
            .expect_err("a relationship claim needs a relationship fact assertion")
            .to_string()
            .contains("domain proof")
    );
}

#[test]
fn source_selection_provenance_requires_a_valid_pinned_byte_range() {
    let manifest = CoverageManifest {
        format_version: 1,
        languages: vec![CoverageSample {
            language: "rust".to_string(),
            source_path: "fixtures/real-world/coverage/rust/source.rs".to_string(),
            source_sha256: "a".repeat(64),
            provenance: SourceProvenance {
                project: "Example".to_string(),
                source_url: "https://example.com/0123456789abcdef/source.yml".to_string(),
                revision: "0123456789abcdef".to_string(),
                source_file: "source.yml".to_string(),
                license: "MIT".to_string(),
                license_evidence_url: "https://example.com/0123456789abcdef/LICENSE".to_string(),
                license_evidence_files: vec![LicenseEvidenceFile {
                    kind: "license".to_string(),
                    path: "fixtures/real-world/coverage/licenses/example/LICENSE".to_string(),
                    sha256: "c".repeat(64),
                    source_url: "https://example.com/0123456789abcdef/LICENSE".to_string(),
                }],
                source_selection: Some(SourceSelection {
                    kind: "byte_range".to_string(),
                    container_path: "fixtures/real-world/coverage/container.yaml".to_string(),
                    start_byte: 17,
                    end_byte: 16,
                    line_start: 5,
                    line_end: 5,
                    container_sha256: "b".repeat(64),
                    description: "one YAML scalar".to_string(),
                }),
            },
            expected_domains: vec!["symbols".to_string()],
            expected_facts: vec![FactAssertion {
                domain: "symbols".to_string(),
                table: "symbols".to_string(),
                field: "name".to_string(),
                value: "main".to_string(),
                min_count: 1,
                max_count: Some(1),
                expected_start_byte: None,
                fact_language: None,
            }],
            negative_controls: Vec::new(),
            expected_parse_diagnostics: 0,
        }],
        controls: Vec::new(),
    };

    assert!(
        validate_manifest_shape(&manifest)
            .expect_err("a source selection must have positive byte length")
            .to_string()
            .contains("source selection")
    );
}

#[test]
fn every_sample_has_hash_pinned_offline_license_evidence() {
    let root = repository_root();
    let manifest = manifest_value();
    for sample in manifest["languages"].as_array().expect("language rows") {
        let provenance = &sample["provenance"];
        let evidence = provenance["license_evidence_files"]
            .as_array()
            .unwrap_or_else(|| panic!("{} has no vendored license evidence", sample["language"]));
        assert!(
            !evidence.is_empty(),
            "{} has no license evidence",
            sample["language"]
        );
        assert!(
            evidence.iter().any(|row| {
                row["source_url"].as_str() == provenance["license_evidence_url"].as_str()
            }),
            "{} does not map its primary license URL",
            sample["language"]
        );
        for row in evidence {
            let relative_path = row["path"].as_str().expect("license evidence path");
            let local_path = root.join(relative_path);
            let bytes = fs::read(&local_path).unwrap_or_else(|error| {
                panic!(
                    "{} license evidence is unavailable offline: {error}",
                    local_path.display()
                )
            });
            assert_eq!(
                digest(&bytes),
                row["sha256"].as_str().expect("license evidence hash"),
                "{} license evidence hash mismatch",
                sample["language"]
            );
            assert!(
                row["source_url"]
                    .as_str()
                    .is_some_and(|url| url.starts_with("https://"))
            );
        }
    }
    for control in manifest["controls"].as_array().expect("control rows") {
        let evidence = control["license_evidence_files"]
            .as_array()
            .unwrap_or_else(|| panic!("{} has no vendored license evidence", control["name"]));
        assert!(
            !evidence.is_empty(),
            "{} has no license evidence",
            control["name"]
        );
        for row in evidence {
            let path = root.join(row["path"].as_str().expect("license evidence path"));
            assert_eq!(
                digest(&fs::read(&path).expect("read control license evidence")),
                row["sha256"].as_str().expect("license evidence hash")
            );
        }
    }
}

#[test]
fn selected_regex_bytes_match_the_pinned_yaml_container() {
    let root = repository_root();
    let manifest = manifest_value();
    let sample = manifest["languages"]
        .as_array()
        .expect("language rows")
        .iter()
        .find(|sample| sample["language"] == "regex")
        .expect("regex sample");
    let selection = &sample["provenance"]["source_selection"];
    let container_path = root.join(
        selection["container_path"]
            .as_str()
            .expect("vendored regex source container path"),
    );
    let container = fs::read(&container_path).expect("read regex container offline");
    assert_eq!(
        digest(&container),
        selection["container_sha256"]
            .as_str()
            .expect("regex source container hash")
    );
    let start = selection["start_byte"].as_u64().expect("selection start") as usize;
    let end = selection["end_byte"].as_u64().expect("selection end") as usize;
    let selected_bytes = &container[start..end];
    let sample_bytes = fs::read(root.join(sample["source_path"].as_str().expect("regex path")))
        .expect("read extracted regex scalar");
    assert_eq!(sample_bytes, selected_bytes);
}

#[test]
fn locally_authored_controls_point_to_the_workspace_license_declaration() {
    let manifest = manifest_value();
    for control in manifest["controls"].as_array().expect("control rows") {
        let evidence = control["license_evidence_files"]
            .as_array()
            .expect("control evidence files");
        assert!(evidence.iter().any(|row| {
            row["kind"] == "scope"
                && row["path"]
                    == "fixtures/real-world/coverage/licenses/controls/Cargo.toml"
                && row["source_url"]
                    == "https://github.com/anortham/julie-extractors/blob/f9a1b6cfbd6c51e9aecad39a19610695f210ad05/Cargo.toml"
        }));
    }
}

#[test]
fn locally_authored_controls_reject_duplicate_expected_domains() {
    let mut manifest = manifest_value();
    let domains = manifest["controls"][0]["expected_domains"]
        .as_array_mut()
        .expect("control domains");
    let duplicate = domains[0].clone();
    domains.push(duplicate);

    let error = validate_value(manifest).expect_err("duplicate control domains are invalid");

    assert!(error.contains("duplicate expected domain"));
}

#[test]
fn malformed_fact_shapes_reject_unknown_tables_fields_and_domain_mismatches() {
    let mut unknown_table = manifest_value();
    unknown_table["languages"][0]["expected_facts"][0]["table"] =
        Value::String("unknown_table".to_string());
    assert!(
        validate_value(unknown_table)
            .expect_err("unknown fact table")
            .contains("unsupported fact table")
    );

    let mut unknown_field = manifest_value();
    unknown_field["languages"][0]["expected_facts"][0]["field"] =
        Value::String("unknown_field".to_string());
    assert!(
        validate_value(unknown_field)
            .expect_err("unknown fact field")
            .contains("unsupported fact field")
    );

    let mut mismatched_domain = manifest_value();
    mismatched_domain["languages"][0]["expected_facts"][0]["domain"] =
        Value::String("relationships".to_string());
    assert!(
        validate_value(mismatched_domain)
            .expect_err("fact domain must match its output table")
            .contains("belongs to domain")
    );
}

#[test]
fn malformed_sample_hashes_and_paths_are_rejected() {
    let mut bad_hash = manifest_value();
    bad_hash["languages"][0]["source_sha256"] = Value::String("not-a-sha256".to_string());
    assert!(
        validate_value(bad_hash)
            .expect_err("invalid source digest")
            .contains("invalid SHA-256")
    );

    let mut traversal = manifest_value();
    traversal["languages"][0]["source_path"] = Value::String("../outside.rs".to_string());
    assert!(
        validate_value(traversal)
            .expect_err("source traversal")
            .contains("relative path")
    );

    let mut bad_license_hash = manifest_value();
    bad_license_hash["languages"][0]["provenance"]["license_evidence_files"][0]["sha256"] =
        Value::String("not-a-sha256".to_string());
    assert!(
        validate_value(bad_license_hash)
            .expect_err("invalid license evidence digest")
            .contains("invalid SHA-256")
    );

    let mut license_traversal = manifest_value();
    license_traversal["languages"][0]["provenance"]["license_evidence_files"][0]["path"] =
        Value::String("../outside/LICENSE".to_string());
    assert!(
        validate_value(license_traversal)
            .expect_err("license evidence traversal")
            .contains("relative path")
    );
}

#[test]
fn provenance_urls_must_bind_the_declared_revision() {
    let mut source_url = manifest_value();
    source_url["languages"][0]["provenance"]["source_url"] =
        Value::String("https://example.com/unpinned/source.rs".to_string());
    assert!(
        validate_value(source_url)
            .expect_err("the source URL must include its revision")
            .contains("bind revision")
    );

    let mut primary_license_url = manifest_value();
    primary_license_url["languages"][0]["provenance"]["license_evidence_url"] =
        Value::String("https://example.com/unpinned/LICENSE".to_string());
    assert!(
        validate_value(primary_license_url)
            .expect_err("the primary license URL must include its revision")
            .contains("bind revision")
    );

    let mut license_file_url = manifest_value();
    license_file_url["languages"][0]["provenance"]["license_evidence_files"][0]["source_url"] =
        Value::String("https://example.com/unpinned/LICENSE".to_string());
    assert!(
        validate_value(license_file_url)
            .expect_err("every license evidence URL must include its revision")
            .contains("bind revision")
    );
}

#[test]
fn all_language_runs_reject_a_stale_registry_manifest() {
    let directory = tempdir().expect("temporary directory");
    let mut manifest = manifest_value();
    manifest["languages"]
        .as_array_mut()
        .expect("language rows")
        .pop();
    let path = write_manifest(&manifest, directory.path());
    let plan = CoveragePlan {
        language: None,
        all_languages: true,
        binary: std::env::current_exe().expect("test binary path"),
        manifest: Some(path),
    };

    let error = run(plan).expect_err("all-language runs require a registry-complete manifest");

    assert!(
        error
            .to_string()
            .contains("coverage manifest is incomplete")
    );
    assert!(error.to_string().contains("missing 1 language sample"));
}

#[test]
fn selected_runs_reject_missing_or_hash_mismatched_source_files() {
    let directory = tempdir().expect("temporary directory");
    let mut manifest = manifest_value();
    let rust = manifest["languages"]
        .as_array_mut()
        .expect("language rows")
        .iter_mut()
        .find(|sample| sample["language"] == "rust")
        .expect("rust sample");
    rust["source_path"] = Value::String("fixtures/real-world/coverage/rust/missing.rs".to_string());
    let path = write_manifest(&manifest, directory.path());
    let plan = CoveragePlan {
        language: Some("rust".to_string()),
        all_languages: false,
        binary: std::env::current_exe().expect("test binary path"),
        manifest: Some(path.clone()),
    };
    let missing_error = run(plan).expect_err("missing selected source file must fail");
    assert!(missing_error.to_string().contains("missing source"));

    let mut manifest = manifest_value();
    let rust = manifest["languages"]
        .as_array_mut()
        .expect("language rows")
        .iter_mut()
        .find(|sample| sample["language"] == "rust")
        .expect("rust sample");
    rust["source_sha256"] = Value::String("0".repeat(64));
    let path = write_manifest(&manifest, directory.path());
    let plan = CoveragePlan {
        language: Some("rust".to_string()),
        all_languages: false,
        binary: std::env::current_exe().expect("test binary path"),
        manifest: Some(path),
    };
    let hash_error = run(plan).expect_err("selected source hash mismatch must fail");
    assert!(hash_error.to_string().contains("content hash mismatch"));
}

#[test]
fn selected_runs_reject_missing_or_hash_mismatched_license_evidence() {
    let directory = tempdir().expect("temporary directory");
    let mut manifest = manifest_value();
    let rust = manifest["languages"]
        .as_array_mut()
        .expect("language rows")
        .iter_mut()
        .find(|sample| sample["language"] == "rust")
        .expect("rust sample");
    rust["provenance"]["license_evidence_files"][0]["path"] = Value::String(
        "fixtures/real-world/coverage/licenses/tree-sitter/missing-LICENSE".to_string(),
    );
    let path = write_manifest(&manifest, directory.path());
    let plan = CoveragePlan {
        language: Some("rust".to_string()),
        all_languages: false,
        binary: std::env::current_exe().expect("test binary path"),
        manifest: Some(path.clone()),
    };
    let missing_error = run(plan).expect_err("missing selected license text must fail");
    assert!(
        missing_error
            .to_string()
            .contains("missing offline license evidence")
    );

    let mut manifest = manifest_value();
    let rust = manifest["languages"]
        .as_array_mut()
        .expect("language rows")
        .iter_mut()
        .find(|sample| sample["language"] == "rust")
        .expect("rust sample");
    rust["provenance"]["license_evidence_files"][0]["sha256"] = Value::String("0".repeat(64));
    let path = write_manifest(&manifest, directory.path());
    let plan = CoveragePlan {
        language: Some("rust".to_string()),
        all_languages: false,
        binary: std::env::current_exe().expect("test binary path"),
        manifest: Some(path),
    };
    let hash_error = run(plan).expect_err("selected license evidence hash mismatch must fail");
    assert!(hash_error.to_string().contains("content hash mismatch"));
}

#[test]
fn project_scoped_license_and_notice_files_remain_mapped_offline() {
    let manifest = manifest_value();
    let has_path = |project: &str, suffix: &str| {
        project_evidence(&manifest, project)
            .as_array()
            .expect("project evidence files")
            .iter()
            .any(|file| {
                file["path"]
                    .as_str()
                    .is_some_and(|path| path.ends_with(suffix))
            })
    };

    assert!(has_path("Blazor Samples", "blazor-samples/LICENSE-CODE.md"));
    assert!(has_path(
        "Godot Demo Projects",
        "2d/dodge_the_creeps/LICENSE"
    ));
    assert!(has_path("ggplot2", "ggplot2/DESCRIPTION"));
    assert!(has_path("ggplot2", "ggplot2/LICENSE"));
    assert!(has_path(
        "Apache Commons Lang",
        "apache-commons-lang/NOTICE.txt"
    ));
    assert!(has_path("Scala", "scala/NOTICE"));
}
