use crate::base::{ExtractionResults, StructuralFact};
use crate::extract_canonical;
use std::path::Path;

fn extract(path: &str, code: &str) -> ExtractionResults {
    extract_canonical(path, code, Path::new("/tmp/test")).expect("swift extraction")
}

fn rows(result: &ExtractionResults, pattern_id: &str, keys: &[&str]) -> Vec<String> {
    result
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == pattern_id)
        .map(|fact| {
            keys.iter()
                .map(|key| value(fact, key))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect()
}

fn value(fact: &StructuralFact, key: &str) -> String {
    fact.metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .map(|value| match value {
            serde_json::Value::String(text) => text.clone(),
            other => other.to_string(),
        })
        .unwrap_or_else(|| "-".to_string())
}

const TARGET_KEYS: &[&str] = &[
    "target_kind",
    "name",
    "path",
    "effective_path",
    "path_origin",
    "package_name",
];

const MANIFEST: &str = r#"// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "App",
    products: [
        .library(name: "AppLib", targets: ["AppCore"]),
        .executable(name: "app", targets: ["cli"]),
    ],
    targets: [
        .target(name: "AppCore", path: "Sources/App"),
        .testTarget(name: "AppTests", dependencies: ["AppCore", .target(name: "Helpers")]),
        .executableTarget(name: "cli", dependencies: [.product(name: "ArgumentParser", package: "swift-argument-parser")]),
        .macro(name: "AppMacros"),
        .plugin(name: "Lint", capability: .buildTool()),
        .systemLibrary(name: "CZlib"),
        .binaryTarget(name: "Remote", url: "https://example.com/Remote.zip", checksum: "abc"),
        .binaryTarget(name: "Local", path: "Frameworks/Local.xcframework"),
    ]
)
"#;

#[test]
fn swiftpm_targets_record_the_written_or_default_path_and_the_package_name() {
    let result = extract("Package.swift", MANIFEST);

    assert_eq!(
        rows(&result, "swiftpm.target.v1", TARGET_KEYS),
        vec![
            "target AppCore Sources/App Sources/App written App",
            "testTarget AppTests - Tests/AppTests default App",
            "executableTarget cli - Sources/cli default App",
            "macro AppMacros - Sources/AppMacros default App",
            "plugin Lint - Plugins/Lint default App",
            "systemLibrary CZlib - Sources/CZlib default App",
            "binaryTarget Remote - - - App",
            "binaryTarget Local Frameworks/Local.xcframework Frameworks/Local.xcframework written App",
        ]
    );
    assert_eq!(
        rows(
            &result,
            "swiftpm.product.v1",
            &["product_kind", "name", "targets", "package_name"]
        ),
        vec![
            "library AppLib [\"AppCore\"] App",
            "executable app [\"cli\"] App",
        ]
    );
}

#[test]
fn swiftpm_targets_added_outside_the_package_call_are_targets() {
    let code = r#"// swift-tools-version:5.9
import PackageDescription

let coreTargets: [Target] = [
    .target(name: "Core", dependencies: [.target(name: "Base")]),
]
let extraProducts: [PackageDescription.Product] = [.library(name: "CoreLib", targets: ["Core"])]
let package = Package(name: "Kit", products: extraProducts, targets: coreTargets)
package.targets.append(.testTarget(name: "CoreTests", dependencies: ["Core"]))
package.targets.append(contentsOf: [.executableTarget(name: "tool", path: "Tools/tool")])
package.targets += [.plugin(name: "Gen", capability: .command(intent: .custom(verb: "gen", description: "")))]
package.products.append(.executable(name: "tool", targets: ["tool"]))
package.dependencies += [.package(url: "https://github.com/apple/swift-log.git", from: "1.5.0")]
"#;
    let result = extract("Package.swift", code);

    assert_eq!(
        rows(&result, "swiftpm.target.v1", TARGET_KEYS),
        vec![
            "target Core - Sources/Core default Kit",
            "testTarget CoreTests - Tests/CoreTests default Kit",
            "executableTarget tool Tools/tool Tools/tool written Kit",
            "plugin Gen - Plugins/Gen default Kit",
        ]
    );
    assert_eq!(
        rows(&result, "swiftpm.product.v1", &["product_kind", "name"]),
        vec!["library CoreLib", "executable tool"]
    );
    assert_eq!(
        rows(&result, "manifest.dependency.v1", &["name"]),
        vec!["swift-log"]
    );
}

#[test]
fn swiftpm_target_references_and_computed_values_are_not_targets() {
    let code = r#"// swift-tools-version:5.9
import PackageDescription

let shared: [Target.Dependency] = [.target(name: "Base"), .product(name: "Logging", package: "swift-log")]
let name = "Dynamic"
let package = Package(
    name: "Kit",
    targets: [
        .target(name: name),
        .target(name: "Core", dependencies: shared, path: computedPath(), plugins: [.plugin(name: "Gen", package: "gen")]),
    ]
)
func helper() -> Target { .target(name: "Hidden") }
"#;
    let result = extract("Package.swift", code);

    assert_eq!(
        rows(&result, "swiftpm.target.v1", TARGET_KEYS),
        vec!["target Core - - - Kit"]
    );
}

#[test]
fn swiftpm_facts_need_a_file_named_package_swift() {
    for path in ["Sources/App/Config.swift", "Sources/App/MyPackage.swift"] {
        let result = extract(path, MANIFEST);
        assert!(
            rows(&result, "swiftpm.target.v1", TARGET_KEYS).is_empty(),
            "{path}"
        );
    }
    let nested = extract("Packages/Kit/Package.swift", MANIFEST);
    assert_eq!(rows(&nested, "swiftpm.target.v1", &["name"]).len(), 8);
    assert_eq!(
        rows(
            &nested,
            "swiftpm.package.v1",
            &["name", "manifest_swift_version"]
        ),
        vec!["App -"]
    );
}

#[test]
fn version_specific_manifests_give_the_same_facts_with_their_tools_version() {
    for (path, version) in [
        ("Package@swift-5.9.swift", "5.9"),
        ("Packages/Kit/Package@swift-6.swift", "6"),
        ("Package@swift-5.10.1.swift", "5.10.1"),
    ] {
        let result = extract(path, MANIFEST);
        let targets = rows(
            &result,
            "swiftpm.target.v1",
            &["name", "manifest_swift_version"],
        );
        assert_eq!(targets.len(), 8, "{path}");
        assert_eq!(targets[0], format!("AppCore {version}"), "{path}");
        assert_eq!(
            rows(
                &result,
                "swiftpm.product.v1",
                &["name", "manifest_swift_version"]
            ),
            vec![format!("AppLib {version}"), format!("app {version}")],
            "{path}"
        );
        assert_eq!(
            rows(
                &result,
                "swiftpm.package.v1",
                &["name", "manifest_swift_version"]
            ),
            vec![format!("App {version}")],
            "{path}"
        );
    }
}

#[test]
fn names_that_only_resemble_a_manifest_give_no_facts() {
    for path in [
        "MyPackage.swift",
        "Package@swift-.swift",
        "Package@swift-5.x.swift",
        "Package@swift-5.9.1.2.swift",
        "Package@5.9.swift",
        "Package@swift-5.9.swift.swift",
    ] {
        let result = extract(path, MANIFEST);
        assert!(
            result
                .structural_facts
                .iter()
                .all(|fact| !fact.pattern_id.starts_with("swiftpm.")),
            "{path}"
        );
    }
    let backup = extract_canonical("Package.swift.bak", MANIFEST, Path::new("/tmp/test"))
        .map(|result| result.structural_facts)
        .unwrap_or_default();
    assert!(
        backup
            .iter()
            .all(|fact| !fact.pattern_id.starts_with("swiftpm."))
    );
}
