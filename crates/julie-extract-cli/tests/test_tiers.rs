use std::fs;
use std::path::PathBuf;

#[test]
fn legacy_resolution_oracle_is_feature_gated_out_of_default_suite() {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let manifest = read(&crate_root.join("Cargo.toml"));
    assert!(manifest.contains("test-store-resolution-contract = ["));
    assert!(manifest.contains("\"julie-extract-artifact/test-store-crash\""));
    assert!(manifest.contains("\"julie-extract-artifact/test-store-resolution\""));

    let harness = read(&crate_root.join("tests/resolution_session_contract.rs"));
    assert!(harness.starts_with("#![cfg(feature = \"test-store-resolution-contract\")]"));
}

#[test]
fn legacy_resolution_fixture_and_oracle_are_checked_in_together() {
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/store-resolution/legacy-v3");
    assert!(fixture.join("expected.semantic.json").is_file());
    assert!(fixture.join("typescript/caller.ts").is_file());
    assert!(fixture.join("javascript/caller.js").is_file());
    assert!(fixture.join("rust/caller.rs").is_file());
    assert!(fixture.join("css/style.css").is_file());
}

fn read(path: &PathBuf) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()))
        .replace("\r\n", "\n")
}
