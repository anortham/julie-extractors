//! Zig framework specs: the `build.zig` build graph and http.zig routes.

use super::super::{
    ALWAYS, ARR, K_FRAMEWORK, K_PATTERN_VERSION, K_QUERY_FAMILY, OPT, STR,
    StructuralFactPatternSpec, key,
};

pub(super) const SPECS: &[StructuralFactPatternSpec] = &[
    StructuralFactPatternSpec {
        pattern_id: "httpz.route.v1",
        languages: &["zig"],
        query_family: "framework",
        description: "An http.zig route registration (`router.get(\"/p\", handler, .{})`) on a router from `server.router(..)` or a `group(\"/prefix\", ..)` of one.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "api_style",
                STR,
                ALWAYS,
                "Routing style (\"call_routing\").",
            ),
            key("route_template", STR, ALWAYS, "Raw static route path."),
            key(
                "normalized_route_template",
                STR,
                ALWAYS,
                "Cross-family normalized route template; `:id` params stay `:id`.",
            ),
            key(
                "dynamic_segments",
                ARR,
                OPT,
                "Route parameter names discovered in the normalized template.",
            ),
            key("route_group_prefix", STR, OPT, "Same-file `group` prefix."),
            key(
                "effective_route_template",
                STR,
                OPT,
                "Group prefix joined with route template.",
            ),
            key(
                "verb",
                STR,
                OPT,
                "Uppercase HTTP method; omitted for `all` registrations.",
            ),
            key("verb_source", STR, OPT, "How the verb was attested."),
            key(
                "handler_name",
                STR,
                ALWAYS,
                "Source text of the handler argument.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "zig.build_artifact.v1",
        languages: &["zig"],
        query_family: "build",
        description: "A `build.zig` compile artifact (`b.addExecutable`, `addLibrary`, `addStaticLibrary`, `addSharedLibrary`, `addObject`, `addTest`).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "artifact_kind",
                STR,
                ALWAYS,
                "executable, library, static_library, shared_library, object, or test.",
            ),
            key("artifact_name", STR, OPT, "The static `.name` option."),
            key(
                "root_source_file",
                STR,
                OPT,
                "The static root source path, direct or through `.root_module = b.createModule(..)`.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "zig.build_dependency.v1",
        languages: &["zig"],
        query_family: "build",
        description: "A `build.zig` package dependency (`b.dependency(\"name\", ..)`).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "dependency_name",
                STR,
                ALWAYS,
                "The dependency name from `build.zig.zon`.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "zig.build_module.v1",
        languages: &["zig"],
        query_family: "build",
        description: "A `build.zig` module definition (`b.addModule(\"name\", .{ .root_source_file = .. })` or `b.createModule(..)`).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "module_name",
                STR,
                OPT,
                "The public module name (`addModule` only).",
            ),
            key(
                "root_source_file",
                STR,
                ALWAYS,
                "The static root source path.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "zig.build_module_import.v1",
        languages: &["zig"],
        query_family: "build",
        description: "A `build.zig` module import (`module.addImport(\"name\", dep.module(\"name\"))`).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "import_name",
                STR,
                ALWAYS,
                "The name the module is imported under (`@import(\"name\")`).",
            ),
            key(
                "module_source",
                STR,
                OPT,
                "Source text of the imported module expression.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "zig.build_step.v1",
        languages: &["zig"],
        query_family: "build",
        description: "A named `build.zig` step (`b.step(\"run\", \"description\")`).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "step_name",
                STR,
                ALWAYS,
                "The step name (`zig build <name>`).",
            ),
            key("step_description", STR, ALWAYS, "The step description."),
        ],
    },
];
