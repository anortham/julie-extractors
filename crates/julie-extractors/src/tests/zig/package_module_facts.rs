use crate::base::ExtractionResults;
use crate::extract_canonical;
use std::path::Path;

fn extract(path: &str, source: &str) -> ExtractionResults {
    extract_canonical(path, source, Path::new("/tmp/test"))
        .expect("canonical Zig extraction must succeed")
}

fn rows(result: &ExtractionResults) -> Vec<String> {
    result
        .structural_facts
        .iter()
        .filter(|fact| {
            matches!(
                fact.pattern_id.as_str(),
                "zig.build_module.v1" | "zig.build_artifact.v1"
            )
        })
        .map(|fact| {
            let metadata = fact.metadata.clone().unwrap_or_default();
            let values: Vec<&str> = [
                "artifact_kind",
                "artifact_name",
                "module_name",
                "root_source_file",
            ]
            .iter()
            .map(|key| metadata.get(*key).and_then(|v| v.as_str()).unwrap_or("-"))
            .collect();
            format!(
                "{}@{} {}",
                fact.pattern_id,
                fact.start_line,
                values.join(" ")
            )
        })
        .collect()
}

const CURRENT: &str = r#"const std = @import("std");
pub fn build(b: *std.Build) void {
    const blog = b.addModule("blog", .{ .root_source_file = b.path("src/root.zig") });
    const util = b.createModule(.{ .root_source_file = b.path("src/util.zig") });
    const lib = b.addLibrary(.{ .name = "blog", .root_module = b.createModule(.{ .root_source_file = b.path("src/lib.zig") }) });
    const exe = b.addExecutable(.{ .name = "app", .root_source_file = b.path("src/main.zig") });
    const tests = b.addTest(.{ .root_source_file = b.path("src/tests.zig") });
    _ = .{ blog, util, lib, exe, tests };
}
"#;

#[test]
fn build_zig_records_module_and_artifact_names_with_root_sources() {
    assert_eq!(
        rows(&extract("build.zig", CURRENT)),
        vec![
            "zig.build_module.v1@3 - - blog src/root.zig",
            "zig.build_module.v1@4 - - - src/util.zig",
            "zig.build_artifact.v1@5 library blog - src/lib.zig",
            "zig.build_module.v1@5 - - - src/lib.zig",
            "zig.build_artifact.v1@6 executable app - src/main.zig",
            "zig.build_artifact.v1@7 test - - src/tests.zig",
        ]
    );
}

#[test]
fn build_zig_records_the_zig_0_11_path_struct_and_source_file_forms() {
    let result = extract(
        "build.zig",
        r#"const std = @import("std");
pub fn build(b: *std.Build) void {
    const legacy = b.addModule("legacy", .{ .source_file = .{ .path = "src/legacy.zig" } });
    const shared = b.createModule(.{ .source_file = .{ .path = "src/shared.zig" } });
    const exe = b.addExecutable(.{ .name = "app", .root_source_file = .{ .path = "src/main.zig" } });
    const lib = b.addStaticLibrary(.{ .name = "core", .root_source_file = .{ .path = "src/core.zig" } });
    _ = .{ legacy, shared, exe, lib };
}
"#,
    );

    assert_eq!(
        rows(&result),
        vec![
            "zig.build_module.v1@3 - - legacy src/legacy.zig",
            "zig.build_module.v1@4 - - - src/shared.zig",
            "zig.build_artifact.v1@5 executable app - src/main.zig",
            "zig.build_artifact.v1@6 static_library core - src/core.zig",
        ]
    );
}

#[test]
fn build_zig_records_the_zig_0_10_positional_artifact_forms() {
    let result = extract(
        "build.zig",
        r#"const std = @import("std");
pub fn build(b: *std.build.Builder) void {
    const exe = b.addExecutable("app", "src/main.zig");
    const lib = b.addSharedLibrary("core", "src/core.zig", .unversioned);
    const obj = b.addObject("obj", null);
    const tests = b.addTest("src/main.zig");
    _ = .{ exe, lib, obj, tests };
}
"#,
    );

    assert_eq!(
        rows(&result),
        vec![
            "zig.build_artifact.v1@3 executable app - src/main.zig",
            "zig.build_artifact.v1@4 shared_library core - src/core.zig",
            "zig.build_artifact.v1@5 object obj - -",
            "zig.build_artifact.v1@6 test - - src/main.zig",
        ]
    );
}

#[test]
fn build_zig_skips_computed_names_and_paths() {
    let result = extract(
        "build.zig",
        r#"const std = @import("std");
pub fn build(b: *std.Build) void {
    const name = "computed";
    const opts = .{ .name = "app" };
    const a = b.addModule(name, .{ .root_source_file = b.path(name) });
    const c = b.addExecutable(opts);
    const d = b.addExecutable(name, name);
    const e = b.addModule("cwd", .{ .root_source_file = .{ .cwd_relative = "/abs/x.zig" } });
    _ = .{ a, c, d, e };
}
"#,
    );

    assert!(rows(&result).is_empty(), "{:#?}", rows(&result));
}

#[test]
fn module_calls_outside_build_zig_publish_no_build_facts() {
    for path in [
        "src/build.zig.bak.zig",
        "tools/build_helpers.zig",
        "src/main.zig",
    ] {
        assert!(rows(&extract(path, CURRENT)).is_empty(), "{path}");
    }
    assert_eq!(rows(&extract("examples/blog/build.zig", CURRENT)).len(), 6);
}
