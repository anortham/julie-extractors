use std::path::PathBuf;
use std::process::Command;

use tempfile::TempDir;
use xtask::release::render_release_package_list;
use xtask::test_tiers::tier_names;

#[test]
fn main_delegates_to_the_commands_module() {
    let main = std::fs::read_to_string(repo_root().join("xtask/src/main.rs"))
        .expect("read xtask/src/main.rs");
    let lib = std::fs::read_to_string(repo_root().join("xtask/src/lib.rs"))
        .expect("read xtask/src/lib.rs");

    assert!(
        main.contains("xtask::commands::run_from_env_args"),
        "xtask main must delegate to the commands module"
    );
    assert!(
        lib.contains("pub mod commands;"),
        "xtask library must export the commands module"
    );
}

#[test]
fn test_tiers_module_does_not_own_release_routing() {
    let source = std::fs::read_to_string(repo_root().join("xtask/src/test_tiers.rs"))
        .expect("read xtask/src/test_tiers.rs");

    assert!(
        !source.contains("package-list"),
        "test_tiers must not route release commands"
    );
    assert!(
        !source.contains("crate::release"),
        "test_tiers must not call release module commands"
    );
    assert!(
        !source.contains("crate::performance"),
        "test_tiers must not call performance baseline commands"
    );
}

#[test]
fn commands_route_test_list() {
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(["test", "list"])
        .output()
        .expect("run xtask test list");

    assert!(
        output.status.success(),
        "command failed: status {:?}, stderr {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    let expected = tier_names().join("\n") + "\n";
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is utf-8"),
        expected
    );
}

#[test]
fn commands_route_release_package_list() {
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(["release", "package-list"])
        .output()
        .expect("run xtask release package-list");

    assert!(
        output.status.success(),
        "command failed: status {:?}, stderr {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout is utf-8"),
        render_release_package_list()
    );
}

#[test]
fn commands_route_release_package_staging_before_test_tier_parser() {
    let temp = TempDir::new().expect("tempdir");
    let out_dir = temp.path().join("package");
    let binary = temp.path().join("missing-julie-extract");
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args([
            "release",
            "package",
            "--version",
            "0.1.0",
            "--target",
            "x86_64-apple-darwin",
            "--out-dir",
            out_dir.to_str().expect("utf-8 path"),
            "--binary",
            binary.to_str().expect("utf-8 path"),
        ])
        .output()
        .expect("run xtask release package");

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("missing release input"),
        "release package route must reach release staging, stderr: {stderr}"
    );
}

#[test]
fn commands_route_performance_baseline_before_test_tier_parser() {
    let temp = TempDir::new().expect("tempdir");
    let out_dir = temp.path().join("baseline");
    let binary = temp.path().join("missing-julie-extract");
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args([
            "performance",
            "baseline",
            "--root",
            temp.path().to_str().expect("utf-8 path"),
            "--out-dir",
            out_dir.to_str().expect("utf-8 path"),
            "--binary",
            binary.to_str().expect("utf-8 path"),
            "--runs",
            "3",
        ])
        .output()
        .expect("run xtask performance baseline");

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("failed to run"),
        "performance baseline route must reach baseline runner, stderr: {stderr}"
    );
}

#[test]
fn workflow_commands_keep_fast_and_specialist_gates_separate() {
    let root = repo_root();
    let ci = std::fs::read_to_string(root.join(".github/workflows/ci.yml")).expect("read ci.yml");
    let specialist = std::fs::read_to_string(root.join(".github/workflows/specialist-gates.yml"))
        .expect("read specialist-gates.yml");
    let testing_doc =
        std::fs::read_to_string(root.join("docs/testing-strategy.md")).expect("testing docs");
    let release_doc = std::fs::read_to_string(root.join("docs/release.md")).expect("release docs");

    for command in [
        "cargo fmt --check",
        "cargo metadata --format-version 1",
        "cargo test -p xtask",
        "cargo xtask test default",
        "cargo xtask test contract",
    ] {
        assert!(ci.contains(command), "ci.yml must run `{command}`");
    }

    for forbidden in [
        "cargo xtask test certification",
        "cargo xtask test real-world-smoke",
        "cargo xtask test real-world-release",
        "cargo xtask dogfood repo",
        "cargo xtask release package --version",
        "cargo xtask performance baseline",
    ] {
        assert!(
            !ci.contains(forbidden),
            "regular CI must not run slow gate `{forbidden}`"
        );
    }

    for command in [
        "cargo xtask test certification",
        "cargo xtask test real-world-smoke",
        "cargo xtask test real-world-release",
        "cargo xtask dogfood repo --root . --out-dir target/dogfood/julie-extractors",
        "cargo xtask release package --version",
    ] {
        assert!(
            specialist.contains(command),
            "specialist workflow must run `{command}`"
        );
        assert!(
            testing_doc.contains(command) || release_doc.contains(command),
            "docs must mention specialist command `{command}`"
        );
    }
}

#[test]
fn workflow_commands_keep_release_binary_workflow_explicit() {
    let root = repo_root();
    let workflow = std::fs::read_to_string(root.join(".github/workflows/release-binaries.yml"))
        .expect("read release-binaries.yml");
    let release_doc = std::fs::read_to_string(root.join("docs/release.md")).expect("release docs");

    for required in [
        "workflow_dispatch:",
        "tags:",
        "- 'v*'",
        "ubuntu-latest",
        "macos-15",
        "windows-2022",
        "x86_64-unknown-linux-gnu",
        "aarch64-apple-darwin",
        "x86_64-pc-windows-msvc",
        "cargo build --release -p julie-extract-cli --bin julie-extract",
        "cargo xtask release package",
        "actions/upload-artifact",
    ] {
        assert!(
            workflow.contains(required),
            "release-binaries.yml must contain `{required}`"
        );
    }

    for required in [
        "Release Binaries",
        "workflow_dispatch",
        "tag pushes matching `v*`",
        "GitHub Actions artifacts",
    ] {
        assert!(
            release_doc.contains(required),
            "release docs must mention `{required}`"
        );
    }
}

#[test]
fn workflow_actions_use_node24_ready_checkout() {
    let root = repo_root();

    for workflow in [
        ".github/workflows/ci.yml",
        ".github/workflows/specialist-gates.yml",
        ".github/workflows/release-binaries.yml",
    ] {
        let source = std::fs::read_to_string(root.join(workflow)).expect("read workflow");
        assert!(
            !source.contains("actions/checkout@v4"),
            "{workflow} must not use checkout@v4 because it runs on deprecated Node.js 20"
        );
        assert!(
            source.contains("actions/checkout@v5"),
            "{workflow} must use checkout@v5"
        );
    }
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask crate should live under repo root")
        .to_path_buf()
}
