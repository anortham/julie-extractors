use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn test_tier_convention_keeps_slow_gates_out_of_default_suite() {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    assert_feature_exists(&crate_root, "test-golden");
    assert_feature_exists(&crate_root, "test-capability-matrix");
    assert_feature_exists(&crate_root, "test-certification");
    assert_feature_exists(&crate_root, "test-downstream-smoke");
    assert_feature_exists(&crate_root, "test-real-world");

    let tests_mod = read_to_string(crate_root.join("src/tests/mod.rs"));
    assert_module_is_feature_gated(&tests_mod, "golden", "test-golden");
    assert_module_is_feature_gated(&tests_mod, "capability_matrix", "test-capability-matrix");
    assert_module_is_feature_gated(&tests_mod, "parser_upgrade", "test-certification");
    assert_module_is_feature_gated(
        &tests_mod,
        "pending_shape_contract",
        "test-capability-matrix",
    );

    let qml_mod = read_to_string(crate_root.join("src/tests/qml/mod.rs"));
    assert_module_is_feature_gated(&qml_mod, "real_world", "test-real-world");

    let r_mod = read_to_string(crate_root.join("src/tests/r/mod.rs"));
    assert_module_is_feature_gated(&r_mod, "file_integration_bug", "test-real-world");
    assert_module_is_feature_gated(&r_mod, "real_world", "test-real-world");

    let json_mod = read_to_string(crate_root.join("src/tests/json/mod.rs"));
    assert!(
        json_mod.contains(
            "#[cfg(feature = \"test-real-world\")]\n    #[test]\n    fn test_real_world_jsonl_memories_fixture()"
        ),
        "JSON real-world fixture test must be gated behind feature `test-real-world`"
    );

    let downstream_smoke = read_to_string(crate_root.join("tests/downstream_smoke.rs"));
    assert!(
        downstream_smoke.contains(
            "#[cfg(feature = \"test-downstream-smoke\")]\n#[test]\nfn julie_extractors_works_as_path_dependency_in_downstream_crate()"
        ),
        "downstream smoke integration test must be gated behind feature `test-downstream-smoke`"
    );
}

fn assert_feature_exists(crate_root: &Path, feature: &str) {
    let manifest = read_to_string(crate_root.join("Cargo.toml"));
    assert!(
        manifest.contains(&format!("{feature} = []")),
        "Cargo feature `{feature}` must exist so slow gates can be selected by name"
    );
}

fn assert_module_is_feature_gated(content: &str, module: &str, feature: &str) {
    let expected = format!("#[cfg(feature = \"{feature}\")]");
    let module_line = format!("pub mod {module};");
    let lines = content.lines().collect::<Vec<_>>();

    for (index, line) in lines.iter().enumerate() {
        let declaration = line.trim().split("//").next().unwrap_or("").trim();
        if declaration == module_line {
            let previous_non_empty = lines
                .iter()
                .take(index)
                .rev()
                .find(|line| !line.trim().is_empty())
                .copied()
                .unwrap_or("");
            assert_eq!(
                previous_non_empty.trim(),
                expected,
                "module `{module}` must be gated by `{expected}`"
            );
            return;
        }
    }

    panic!("module `{module}` is missing from test module declarations");
}

fn read_to_string(path: PathBuf) -> String {
    fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()))
}
