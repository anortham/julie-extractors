//! Structural-fact pattern SPECS for the `manifest` registry family: package
//! manifest dependencies and API description documents (OpenAPI / Swagger).
//!
//! Authored metadata for [`super::StructuralFactPatternSpec`] entries. Public
//! registry access remains through [`super::structural_fact_pattern_specs`].

use super::{
    ALWAYS, ARR, BOOL, K_FRAMEWORK, K_PATTERN_VERSION, K_QUERY_FAMILY, OPT, STR,
    StructuralFactPatternSpec, key,
};

pub(super) const SPECS: &[StructuralFactPatternSpec] = &[
    // OpenAPI / Swagger documents (JSON and YAML)
    StructuralFactPatternSpec {
        pattern_id: "openapi.route.v1",
        languages: &["json"],
        query_family: "framework",
        description: "An OpenAPI or Swagger path operation (`paths.<template>.<verb>`).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "spec_format",
                STR,
                ALWAYS,
                "Root version key that marks the document (\"openapi\" or \"swagger\").",
            ),
            key("spec_version", STR, OPT, "Value of the root version key."),
            key(
                "verb",
                STR,
                ALWAYS,
                "Upper-case HTTP method of the operation.",
            ),
            key(
                "route_template",
                STR,
                ALWAYS,
                "Path template key as written under `paths`.",
            ),
            key(
                "effective_route_template",
                STR,
                OPT,
                "Swagger 2.0 `basePath` joined with the path template.",
            ),
            key(
                "normalized_route_template",
                STR,
                ALWAYS,
                "Cross-family normalized route template.",
            ),
            key(
                "dynamic_segments",
                ARR,
                OPT,
                "Route parameter names discovered in the normalized template.",
            ),
            key(
                "operation_id",
                STR,
                OPT,
                "The operation's `operationId` value.",
            ),
        ],
    },
    // Package manifests
    StructuralFactPatternSpec {
        pattern_id: "manifest.dependency.v1",
        languages: &["toml"],
        query_family: "dependencies",
        description: "A package dependency declared in a Cargo.toml or pyproject.toml manifest.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "ecosystem",
                STR,
                ALWAYS,
                "Package ecosystem (\"cargo\" or \"pypi\").",
            ),
            key(
                "name",
                STR,
                ALWAYS,
                "Dependency name: the Cargo key, or the PEP 503-normalized distribution name.",
            ),
            key(
                "group",
                STR,
                ALWAYS,
                "Dependency group: Cargo `dependencies`/`dev-dependencies`/`build-dependencies`/`workspace`, or Python `runtime`, `optional:<extra>`, `group:<name>`, `build-system`, `poetry:<group>`.",
            ),
            key("version", STR, OPT, "Version requirement as written."),
            key(
                "package",
                STR,
                OPT,
                "Real Cargo package name when the dependency key renames it.",
            ),
            key(
                "target",
                STR,
                OPT,
                "Cargo `target.<cfg>` platform selector.",
            ),
            key(
                "workspace",
                BOOL,
                OPT,
                "True when the Cargo dependency inherits from `[workspace.dependencies]`.",
            ),
            key("extras", ARR, OPT, "PEP 508 extras."),
            key("marker", STR, OPT, "PEP 508 environment marker."),
        ],
    },
];
