use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};
use tempfile::TempDir;
use xtask::release::{
    ReleasePackageItem, ReleasePackageKind, StagePackagePlan, collect_staged_files,
    plan_package_from_args, release_package_items, render_release_package_list,
    stage_package_from_root, validate_package_manifest,
};

#[test]
fn release_package_list_contains_only_standalone_extract_artifacts() {
    let items = release_package_items();
    assert!(!items.is_empty(), "release package list cannot be empty");
    assert!(
        items
            .iter()
            .any(|item| item.path_template.contains("julie-extract")),
        "release must ship julie-extract binary artifacts"
    );
    assert!(
        items
            .iter()
            .any(|item| item.kind == ReleasePackageKind::Checksum),
        "release must ship checksums"
    );
    assert!(
        items
            .iter()
            .any(|item| item.kind == ReleasePackageKind::Doc),
        "release must ship contract docs"
    );
    assert!(
        items
            .iter()
            .any(|item| item.kind == ReleasePackageKind::ReleaseNote),
        "release must ship release notes"
    );

    for item in items {
        assert!(
            matches!(
                item.kind,
                ReleasePackageKind::Binary
                    | ReleasePackageKind::Checksum
                    | ReleasePackageKind::Doc
                    | ReleasePackageKind::ReleaseNote
            ),
            "unexpected release package kind: {:?}",
            item.kind
        );
        for forbidden in [
            "julie-server",
            "julie-daemon",
            "julie-adapter",
            "mcp",
            "search",
            "embedding",
            "watcher",
            "dashboard",
            "editing",
        ] {
            assert!(
                !item.path_template.contains(forbidden),
                "release package item must not include forbidden Julie artifact {forbidden}: {}",
                item.path_template
            );
        }
    }
}

#[test]
fn release_package_list_is_exact_and_ordered() {
    assert_eq!(
        release_package_items(),
        vec![
            ReleasePackageItem {
                kind: ReleasePackageKind::Binary,
                path_template: "dist/{target}/julie-extract{exe_suffix}",
            },
            ReleasePackageItem {
                kind: ReleasePackageKind::Checksum,
                path_template: "dist/{target}/julie-extract{exe_suffix}.sha256",
            },
            ReleasePackageItem {
                kind: ReleasePackageKind::Doc,
                path_template: "docs/contracts/cli.md",
            },
            ReleasePackageItem {
                kind: ReleasePackageKind::Doc,
                path_template: "docs/contracts/sqlite-schema-v1.md",
            },
            ReleasePackageItem {
                kind: ReleasePackageKind::Doc,
                path_template: "docs/contracts/jsonl-v1.md",
            },
            ReleasePackageItem {
                kind: ReleasePackageKind::Doc,
                path_template: "docs/contracts/reports.md",
            },
            ReleasePackageItem {
                kind: ReleasePackageKind::Doc,
                path_template: "docs/architecture/product-boundary.md",
            },
            ReleasePackageItem {
                kind: ReleasePackageKind::Doc,
                path_template: "docs/architecture/schema-principles.md",
            },
            ReleasePackageItem {
                kind: ReleasePackageKind::Doc,
                path_template: "docs/testing-strategy.md",
            },
            ReleasePackageItem {
                kind: ReleasePackageKind::Doc,
                path_template: "docs/release.md",
            },
            ReleasePackageItem {
                kind: ReleasePackageKind::ReleaseNote,
                path_template: "docs/release-notes/v{version}.md",
            },
        ]
    );
}

#[test]
fn release_package_list_renders_as_a_stable_xtask_manifest() {
    let rendered = render_release_package_list();

    assert!(rendered.contains("binary\tdist/{target}/julie-extract{exe_suffix}\n"));
    assert!(rendered.contains("checksum\tdist/{target}/julie-extract{exe_suffix}.sha256\n"));
    assert!(rendered.contains("doc\tdocs/contracts/cli.md\n"));
    assert!(rendered.contains("doc\tdocs/release.md\n"));
    assert!(rendered.contains("release_note\tdocs/release-notes/v{version}.md\n"));
}

#[test]
fn release_package_list_command_prints_the_manifest() {
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
fn documented_release_manifest_command_matches_the_xtask_route() {
    let release_doc =
        std::fs::read_to_string(repo_root().join("docs/release.md")).expect("read docs/release.md");
    let testing_doc = std::fs::read_to_string(repo_root().join("docs/testing-strategy.md"))
        .expect("read docs/testing-strategy.md");

    for doc in [release_doc, testing_doc] {
        assert!(
            doc.contains("cargo xtask release package-list"),
            "release docs must name the package manifest command"
        );
    }
}

#[test]
fn release_package_args_plan_host_binary_and_paths() {
    let temp = TempDir::new().expect("tempdir");
    let out_dir = temp.path().join("package");
    let binary = temp.path().join("bin/julie-extract");

    let plan = plan_package_from_args([
        "package",
        "--version",
        "0.1.0",
        "--target",
        "x86_64-apple-darwin",
        "--out-dir",
        path_str(&out_dir),
        "--binary",
        path_str(&binary),
    ])
    .expect("release package plan");

    assert_eq!(plan.version, "0.1.0");
    assert_eq!(plan.target, "x86_64-apple-darwin");
    assert_eq!(plan.out_dir, out_dir);
    assert_eq!(plan.binary, binary);

    let error = plan_package_from_args(["package", "--version", "0.1.0"])
        .expect_err("missing target and out-dir must fail");
    assert!(
        error.to_string().contains("missing --target"),
        "unexpected error: {error}"
    );
}

#[test]
fn release_package_staging_copies_only_manifest_items_and_writes_checksum() {
    let fixture = ReleaseFixture::new();
    fixture.write_manifest_inputs("0.1.0");
    fixture.write_binary(b"julie extract binary");

    let result = stage_package_from_root(&fixture.repo_root, fixture.plan("0.1.0"))
        .expect("stage release package");

    let binary_path = fixture
        .out_dir
        .join("dist/x86_64-apple-darwin/julie-extract");
    let checksum_path = fixture
        .out_dir
        .join("dist/x86_64-apple-darwin/julie-extract.sha256");
    assert_eq!(
        std::fs::read(&binary_path).expect("binary"),
        b"julie extract binary"
    );
    assert_eq!(
        std::fs::read_to_string(&checksum_path).expect("checksum"),
        format!(
            "{}  dist/x86_64-apple-darwin/julie-extract\n",
            hex_sha256(b"julie extract binary")
        )
    );
    assert_eq!(
        std::fs::read_to_string(fixture.out_dir.join("docs/contracts/cli.md")).expect("doc"),
        "docs/contracts/cli.md\n"
    );
    assert_eq!(
        std::fs::read_to_string(fixture.out_dir.join("docs/release-notes/v0.1.0.md"))
            .expect("release note"),
        "release notes\n"
    );
    assert_eq!(
        collect_staged_files(&fixture.out_dir).expect("staged files"),
        result.staged_files
    );
    assert_eq!(result.staged_files, expected_staged_files("0.1.0"));
}

#[test]
fn release_package_staging_rejects_missing_inputs_and_dirty_output() {
    let fixture = ReleaseFixture::new();
    fixture.write_manifest_inputs("0.1.0");

    let error = stage_package_from_root(&fixture.repo_root, fixture.plan("0.1.0"))
        .expect_err("missing binary must fail");
    assert!(
        error.to_string().contains("missing release input"),
        "unexpected error: {error}"
    );

    fixture.write_binary(b"binary");
    std::fs::remove_file(fixture.repo_root.join("docs/release-notes/v0.1.0.md"))
        .expect("remove release note");
    let error = stage_package_from_root(&fixture.repo_root, fixture.plan("0.1.0"))
        .expect_err("missing release note must fail");
    assert!(
        error.to_string().contains("docs/release-notes/v0.1.0.md"),
        "unexpected error: {error}"
    );

    fixture.write_manifest_inputs("0.1.0");
    std::fs::create_dir_all(&fixture.out_dir).expect("out dir");
    std::fs::write(fixture.out_dir.join("extra.txt"), "extra").expect("extra");
    let error = stage_package_from_root(&fixture.repo_root, fixture.plan("0.1.0"))
        .expect_err("dirty output dir must fail");
    assert!(
        error.to_string().contains("output directory must be empty"),
        "unexpected error: {error}"
    );
}

#[test]
fn release_package_manifest_rejects_forbidden_paths() {
    let item = ReleasePackageItem {
        kind: ReleasePackageKind::Doc,
        path_template: "../secret.txt",
    };

    let error = validate_package_manifest(&[item], "0.1.0", "x86_64-apple-darwin", "")
        .expect_err("forbidden package path must fail");

    assert!(
        error.to_string().contains("forbidden package path"),
        "unexpected error: {error}"
    );
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask crate should live under repo root")
        .to_path_buf()
}

struct ReleaseFixture {
    _temp: TempDir,
    repo_root: PathBuf,
    out_dir: PathBuf,
    binary: PathBuf,
}

impl ReleaseFixture {
    fn new() -> Self {
        let temp = TempDir::new().expect("tempdir");
        let repo_root = temp.path().join("repo");
        let out_dir = temp.path().join("package");
        let binary = temp.path().join("bin/julie-extract");
        Self {
            _temp: temp,
            repo_root,
            out_dir,
            binary,
        }
    }

    fn plan(&self, version: &str) -> StagePackagePlan {
        StagePackagePlan {
            version: version.to_string(),
            target: "x86_64-apple-darwin".to_string(),
            out_dir: self.out_dir.clone(),
            binary: self.binary.clone(),
        }
    }

    fn write_manifest_inputs(&self, version: &str) {
        for item in release_package_items() {
            if matches!(
                item.kind,
                ReleasePackageKind::Binary | ReleasePackageKind::Checksum
            ) {
                continue;
            }
            let relative = item
                .path_template
                .replace("{version}", version)
                .replace("{target}", "x86_64-apple-darwin")
                .replace("{exe_suffix}", "");
            let path = self.repo_root.join(&relative);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("parent dir");
            let content = if matches!(item.kind, ReleasePackageKind::ReleaseNote) {
                "release notes\n".to_string()
            } else {
                format!("{relative}\n")
            };
            std::fs::write(path, content).expect("manifest input");
        }
    }

    fn write_binary(&self, bytes: &[u8]) {
        std::fs::create_dir_all(self.binary.parent().expect("parent")).expect("binary dir");
        std::fs::write(&self.binary, bytes).expect("binary");
    }
}

fn expected_staged_files(version: &str) -> Vec<PathBuf> {
    let mut files = release_package_items()
        .into_iter()
        .map(|item| {
            PathBuf::from(
                item.path_template
                    .replace("{version}", version)
                    .replace("{target}", "x86_64-apple-darwin")
                    .replace("{exe_suffix}", ""),
            )
        })
        .collect::<Vec<_>>();
    files.sort();
    files
}

fn path_str(path: &Path) -> &str {
    path.to_str().expect("utf-8 path")
}

fn hex_sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
