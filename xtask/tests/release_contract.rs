use std::path::PathBuf;
use std::process::Command;
use xtask::release::{
    ReleasePackageItem, ReleasePackageKind, release_package_items, render_release_package_list,
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

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask crate should live under repo root")
        .to_path_buf()
}
