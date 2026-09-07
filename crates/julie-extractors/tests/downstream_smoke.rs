//! Integration test that proves julie-extractors is usable from a downstream
//! Rust crate via a path dependency. This is the Pillar 3 gate — see plan
//! Task 5.7 for context.
//!
//! `cargo package --list` only enumerates files; it does not prove the crate
//! is consumable. Real `cargo package -p julie-extractors --allow-dirty`
//! fails because five inherent git dependencies (tree-sitter-qmljs,
//! tree-sitter-qmldir, tree-sitter-razor, tree-sitter-powershell,
//! tree-sitter-vb-dotnet) have
//! no crates.io versions. The actual Pillar 3 contract is
//! "consumable as a Rust path/git dependency". This test spawns a tempdir
//! consumer crate, path-deps julie-extractors, and runs a program calling
//! both extract_canonical and capability_snapshot.

#![cfg(feature = "test-downstream-smoke")]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[cfg(feature = "test-downstream-smoke")]
#[test]
fn julie_extractors_works_as_path_dependency_in_downstream_crate() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let extractors_path = PathBuf::from(manifest_dir);
    assert!(
        extractors_path.join("Cargo.toml").exists(),
        "expected extractors Cargo.toml at {:?}",
        extractors_path
    );

    let tempdir = tempfile::tempdir().expect("create tempdir");
    let consumer = tempdir.path();

    let extractors_abs = extractors_path
        .canonicalize()
        .expect("canonicalize extractors path");
    let extractors_abs_str = cargo_path_dependency(&extractors_abs);

    fs::write(
        consumer.join("Cargo.toml"),
        format!(
            r#"[package]
name = "julie_extractors_downstream_smoke"
version = "0.0.0"
edition = "2021"
publish = false

[dependencies]
julie-extractors = {{ path = "{extractors_abs_str}" }}
"#
        ),
    )
    .expect("write consumer Cargo.toml");

    fs::create_dir_all(consumer.join("src")).expect("create src/");
    fs::write(
        consumer.join("src/main.rs"),
        r#"use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = "fn main() { println!(\"hi\"); }";
    let result = julie_extractors::extract_canonical("hello.rs", source, Path::new("."))?;
    assert!(!result.symbols.is_empty(), "expected at least one symbol");

    let snap = julie_extractors::capability_snapshot();
    let rust = snap.get("rust").expect("rust language row");
    let _flags: julie_extractors::CapabilityFlags = rust.capabilities;
    assert!(rust.target_capabilities.symbols);
    assert!(
        rust.kind_coverage
            .symbols
            .supported
            .iter()
            .any(|kind| kind == "function")
    );
    assert!(
        rust.kind_coverage
            .relationships
            .supported
            .iter()
            .any(|kind| kind == "calls")
    );
    assert!(
        rust.kind_coverage
            .body_spans
            .supported
            .iter()
            .any(|kind| kind == "function")
    );

    let _version: &str = julie_extractors::EXTRACTION_CONTRACT_VERSION;
    assert!(julie_extractors::EXTRACTION_CONTRACT_VERSION.contains("ecmascript-swift-shape-v3"));
    Ok(())
}
"#,
    )
    .expect("write consumer main.rs");

    let target_dir = consumer.join("target");

    let status = Command::new(env!("CARGO"))
        .args(["build", "--manifest-path"])
        .arg(consumer.join("Cargo.toml"))
        .arg("--target-dir")
        .arg(&target_dir)
        .status()
        .expect("spawn cargo build");
    assert!(
        status.success(),
        "downstream consumer crate failed to build"
    );

    let run_status = Command::new(env!("CARGO"))
        .args(["run", "--manifest-path"])
        .arg(consumer.join("Cargo.toml"))
        .arg("--target-dir")
        .arg(&target_dir)
        .status()
        .expect("spawn cargo run");
    assert!(
        run_status.success(),
        "downstream consumer crate failed to run"
    );
}

#[cfg(feature = "test-downstream-smoke")]
#[test]
fn syntax_api_is_optional_for_external_consumers() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let extractors_path = PathBuf::from(manifest_dir);
    assert!(
        extractors_path.join("Cargo.toml").exists(),
        "expected extractors Cargo.toml at {:?}",
        extractors_path
    );

    let tempdir = tempfile::tempdir().expect("create tempdir");
    let consumer = tempdir.path();

    let extractors_abs = extractors_path
        .canonicalize()
        .expect("canonicalize extractors path");
    let extractors_abs_str = cargo_path_dependency(&extractors_abs);

    let target_dir = consumer.join("target");
    fs::create_dir_all(consumer.join("src")).expect("create src/");

    let syntax_main_rs = r#"use std::path::Path;
use julie_extractors::{PendingSpan, UnresolvedTarget};
use julie_extractors::syntax::{
    parse_source, parse_source_with_options, ParsedSource, SyntaxError, SyntaxOptions,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = "fn update() { let item = 1; }";
    let parsed: ParsedSource = parse_source(Path::new("src/lib.rs"), source)?;
    assert!(parsed.diagnostics.is_empty());
    let function = parsed.tree.root_node().named_child(0).unwrap();
    let body = function.child_by_field_name("body").unwrap();
    assert_eq!(&source[body.byte_range()], "{ let item = 1; }");

    let _target: UnresolvedTarget = UnresolvedTarget::simple("update");
    let _span: Option<PendingSpan> = None;

    assert!(matches!(
        parse_source(Path::new("data.jsonl"), "{}\n{}"),
        Err(SyntaxError::UnsupportedContainer { .. })
    ));

    let options = SyntaxOptions {
        cancelled: None,
        deadline: None,
        max_source_bytes: 1024,
    };
    let parsed_opts = parse_source_with_options(Path::new("src/lib.rs"), source, &options)?;
    assert_eq!(parsed_opts.language, "rust");

    let dummy_err: Option<SyntaxError> = None;
    if let Some(err) = dummy_err {
        match err {
            SyntaxError::Cancelled => {}
            SyntaxError::DeadlineExceeded => {}
            SyntaxError::UnsupportedLanguage { .. } => {}
            SyntaxError::UnsupportedContainer { .. } => {}
            SyntaxError::InputTooLarge { .. } => {}
            SyntaxError::ParseFailed { .. } => {}
        }
    }

    Ok(())
}
"#;

    // Phase 1: Negative test - compile with default-features = false (syntax-api disabled)
    fs::write(
        consumer.join("Cargo.toml"),
        format!(
            r#"[package]
name = "julie_extractors_downstream_syntax_smoke"
version = "0.0.0"
edition = "2021"
publish = false

[dependencies]
julie-extractors = {{ path = "{extractors_abs_str}", default-features = false }}
"#
        ),
    )
    .expect("write consumer Cargo.toml for negative test");

    fs::write(consumer.join("src/main.rs"), syntax_main_rs)
        .expect("write consumer main.rs for negative test");

    let neg_output = Command::new(env!("CARGO"))
        .args(["build", "--manifest-path"])
        .arg(consumer.join("Cargo.toml"))
        .arg("--target-dir")
        .arg(&target_dir)
        .output()
        .expect("spawn cargo build for negative test");

    assert!(
        !neg_output.status.success(),
        "expected downstream build to fail when syntax-api feature is absent"
    );
    let stderr = String::from_utf8_lossy(&neg_output.stderr);
    assert!(
        stderr.contains("could not find `syntax` in `julie_extractors`")
            || stderr.contains("unresolved import `julie_extractors::syntax`")
            || stderr.contains("failed to resolve: could not find `syntax` in `julie_extractors`"),
        "expected stderr to report missing syntax module, got:\n{stderr}"
    );

    // Phase 2: Positive test - compile and run with features = ["syntax-api"]
    fs::write(
        consumer.join("Cargo.toml"),
        format!(
            r#"[package]
name = "julie_extractors_downstream_syntax_smoke"
version = "0.0.0"
edition = "2021"
publish = false

[dependencies]
julie-extractors = {{ path = "{extractors_abs_str}", default-features = false, features = ["syntax-api"] }}
"#
        ),
    )
    .expect("write consumer Cargo.toml for positive test");

    let pos_build_status = Command::new(env!("CARGO"))
        .args(["build", "--manifest-path"])
        .arg(consumer.join("Cargo.toml"))
        .arg("--target-dir")
        .arg(&target_dir)
        .status()
        .expect("spawn cargo build for positive test");
    assert!(
        pos_build_status.success(),
        "expected downstream build to succeed when syntax-api feature is enabled"
    );

    let pos_run_status = Command::new(env!("CARGO"))
        .args(["run", "--manifest-path"])
        .arg(consumer.join("Cargo.toml"))
        .arg("--target-dir")
        .arg(&target_dir)
        .status()
        .expect("spawn cargo run for positive test");
    assert!(
        pos_run_status.success(),
        "expected downstream run to succeed when syntax-api feature is enabled"
    );

    // Phase 3: Canonical extraction test - compile and run with default features
    fs::write(
        consumer.join("Cargo.toml"),
        format!(
            r#"[package]
name = "julie_extractors_downstream_syntax_smoke"
version = "0.0.0"
edition = "2021"
publish = false

[dependencies]
julie-extractors = {{ path = "{extractors_abs_str}" }}
"#
        ),
    )
    .expect("write consumer Cargo.toml for canonical test");

    let canonical_main_rs = r#"use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = "fn main() { println!(\"hi\"); }";
    let result = julie_extractors::extract_canonical("hello.rs", source, Path::new("."))?;
    assert!(!result.symbols.is_empty(), "expected at least one symbol");

    let snap = julie_extractors::capability_snapshot();
    let rust = snap.get("rust").expect("rust language row");
    assert!(rust.target_capabilities.symbols);
    Ok(())
}
"#;

    fs::write(consumer.join("src/main.rs"), canonical_main_rs)
        .expect("write consumer main.rs for canonical test");

    let canon_build_status = Command::new(env!("CARGO"))
        .args(["build", "--manifest-path"])
        .arg(consumer.join("Cargo.toml"))
        .arg("--target-dir")
        .arg(&target_dir)
        .status()
        .expect("spawn cargo build for canonical extraction test");
    assert!(
        canon_build_status.success(),
        "expected downstream build for canonical extraction to succeed"
    );

    let canon_run_status = Command::new(env!("CARGO"))
        .args(["run", "--manifest-path"])
        .arg(consumer.join("Cargo.toml"))
        .arg("--target-dir")
        .arg(&target_dir)
        .status()
        .expect("spawn cargo run for canonical extraction test");
    assert!(
        canon_run_status.success(),
        "expected downstream run for canonical extraction to succeed"
    );
}

fn cargo_path_dependency(path: &Path) -> String {
    let path = path.to_string_lossy();
    let path = strip_windows_verbatim_prefix(&path);
    path.replace('\\', "/")
}

#[cfg(windows)]
fn strip_windows_verbatim_prefix(path: &str) -> &str {
    path.strip_prefix(r"\\?\").unwrap_or(path)
}

#[cfg(not(windows))]
fn strip_windows_verbatim_prefix(path: &str) -> &str {
    path
}
