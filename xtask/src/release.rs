#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleasePackageKind {
    Binary,
    Checksum,
    Doc,
    ReleaseNote,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReleasePackageItem {
    pub kind: ReleasePackageKind,
    pub path_template: &'static str,
}

pub fn release_package_items() -> Vec<ReleasePackageItem> {
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
}

pub fn render_release_package_list() -> String {
    let mut output = String::new();
    for item in release_package_items() {
        output.push_str(match item.kind {
            ReleasePackageKind::Binary => "binary",
            ReleasePackageKind::Checksum => "checksum",
            ReleasePackageKind::Doc => "doc",
            ReleasePackageKind::ReleaseNote => "release_note",
        });
        output.push('\t');
        output.push_str(item.path_template);
        output.push('\n');
    }
    output
}
