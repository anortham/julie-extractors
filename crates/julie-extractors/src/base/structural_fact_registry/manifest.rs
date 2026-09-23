//! Structural-fact pattern SPECS for the `manifest` registry family: package
//! manifest dependencies, API description documents (OpenAPI / Swagger), and
//! CI pipeline documents.
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
        languages: &["json", "yaml"],
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
        languages: &["toml", "xml", "erlang"],
        query_family: "dependencies",
        description: "A package dependency declared in a Cargo.toml, pyproject.toml, MSBuild, NuGet, Maven, rebar.config, or Erlang application resource manifest.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "ecosystem",
                STR,
                ALWAYS,
                "Package ecosystem (\"cargo\", \"pypi\", \"nuget\", \"maven\", \"hex\" for rebar.config, or \"otp\" for application resource `applications`).",
            ),
            key(
                "name",
                STR,
                ALWAYS,
                "Dependency name: the Cargo key, the PEP 503-normalized distribution name, the NuGet package id, the Maven `groupId:artifactId`, or the Erlang application atom.",
            ),
            key(
                "group",
                STR,
                ALWAYS,
                "Dependency group: Cargo `dependencies`/`dev-dependencies`/`build-dependencies`/`workspace`; Python `runtime`, `optional:<extra>`, `group:<name>`, `build-system`, `poetry:<group>`; NuGet `PackageReference`/`PackageVersion`/`GlobalPackageReference`/`dependency`; Maven scope, `managed`, `plugin`, or `managed-plugin`; rebar `deps`, `plugins`, `project_plugins`, or `profile:<name>`; OTP `applications`, `included_applications`, or `optional_applications`.",
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
                "Cargo `target.<cfg>` platform selector, or the NuGet dependency group's target framework.",
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
    // CI pipelines (YAML)
    StructuralFactPatternSpec {
        pattern_id: "yaml.ci_job.v1",
        languages: &["yaml"],
        query_family: "pipeline",
        description: "A GitHub Actions or GitLab CI job.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_CI_PLATFORM,
            key("job_id", STR, ALWAYS, "The job key."),
            key(
                "runs_on",
                STR,
                OPT,
                "GitHub Actions `runs-on` scalar value.",
            ),
            key("stage", STR, OPT, "GitLab CI `stage` value."),
            key("needs", ARR, OPT, "Job ids named by `needs`."),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "yaml.ci_trigger.v1",
        languages: &["yaml"],
        query_family: "pipeline",
        description: "A GitHub Actions workflow trigger event under `on`.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_CI_PLATFORM,
            key("event", STR, ALWAYS, "Trigger event name."),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "yaml.ci_uses.v1",
        languages: &["yaml"],
        query_family: "pipeline",
        description: "A GitHub Actions `uses:` action, reusable workflow, or container.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_CI_PLATFORM,
            key("uses", STR, ALWAYS, "The `uses` value as written."),
            key(
                "kind",
                STR,
                ALWAYS,
                "\"action\", \"reusable_workflow\", \"docker\", or \"local\".",
            ),
            key(
                "action",
                STR,
                OPT,
                "`owner/repo[/path]` for an action or reusable workflow.",
            ),
            key("ref", STR, OPT, "The version after `@`."),
        ],
    },
];

const K_CI_PLATFORM: super::MetadataKeySpec = key(
    "platform",
    STR,
    ALWAYS,
    "CI platform (\"github_actions\" or \"gitlab_ci\").",
);
