use std::fs;
use std::path::PathBuf;

#[test]
fn legacy_resolution_oracle_is_feature_gated_out_of_default_suite() {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let manifest = read(&crate_root.join("Cargo.toml"));
    assert!(
        !crate_root
            .join("tests/resolution_session_contract.rs")
            .exists()
    );
    assert!(
        !crate_root
            .join("tests/store_resolution_contract.rs")
            .exists()
    );
    assert!(
        !crate_root
            .join("tests/store_resolution_adapters.rs")
            .exists()
    );
    assert!(!crate_root.join("src/resolution.rs").exists());
    assert!(!crate_root.join("src/resolution_session.rs").exists());
    assert!(!crate_root.join("tests/resolution_contract.rs").exists());
    assert!(!crate_root.join("tests/resolution_perf.rs").exists());
    assert!(!crate_root.join("tests/resolution_shadow.rs").exists());
    assert!(!manifest.contains("test-perf = []"));
}

fn read(path: &PathBuf) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()))
        .replace("\r\n", "\n")
}
