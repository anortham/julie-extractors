//! Structural-fact pattern SPECS for the `manifest` registry family: package
//! manifest dependencies, Go module manifest directives, API description
//! documents (OpenAPI / Swagger), and CI pipeline documents.
//!
//! Authored metadata for [`super::StructuralFactPatternSpec`] entries. Public
//! registry access remains through [`super::structural_fact_pattern_specs`].

use super::{
    ALWAYS, ARR, BOOL, K_FRAMEWORK, K_PATTERN_VERSION, K_QUERY_FAMILY, NUM, OPT, STR,
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
        languages: &["json", "toml", "xml", "swift", "erlang", "gomod"],
        query_family: "dependencies",
        description: "A package dependency declared in a Cargo.toml, pyproject.toml, Pipfile, package.json, composer.json, MSBuild, NuGet, Maven, SwiftPM, rebar.config, Erlang application resource, or go.mod manifest.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "ecosystem",
                STR,
                ALWAYS,
                "Package ecosystem (\"cargo\", \"pypi\", \"npm\", \"composer\", \"nuget\", \"maven\", \"swiftpm\", \"hex\" for rebar.config, \"otp\" for application resource `applications`, or \"go\" for go.mod `require`).",
            ),
            key(
                "name",
                STR,
                ALWAYS,
                "Dependency name: the Cargo key, the PEP 503-normalized distribution name, the npm or Composer package name, the NuGet package id, the Maven `groupId:artifactId`, the SwiftPM package identity (the `name:` argument, else the last URL or path component without `.git`), the Erlang application atom, or the Go module path.",
            ),
            key(
                "group",
                STR,
                ALWAYS,
                "Dependency group: Cargo `dependencies`/`dev-dependencies`/`build-dependencies`/`workspace`; Python `runtime`, `optional:<extra>`, `group:<name>`, `build-system`, `poetry:<group>`, `pipenv:packages`/`pipenv:dev-packages`; npm `dependencies`/`devDependencies`/`peerDependencies`/`optionalDependencies`; Composer `require`/`require-dev`; NuGet `PackageReference`/`PackageVersion`/`GlobalPackageReference`/`dependency`; Maven scope, `managed`, `plugin`, or `managed-plugin`; SwiftPM `dependencies`; rebar `deps`, `plugins`, `project_plugins`, or `profile:<name>`; OTP `applications`, `included_applications`, or `optional_applications`; Go `require`.",
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
                "True when the Cargo dependency inherits from `[workspace.dependencies]`, or the npm version uses the `workspace:` protocol.",
            ),
            key("extras", ARR, OPT, "PEP 508 extras."),
            key("location", STR, OPT, "SwiftPM package URL or local path."),
            key("marker", STR, OPT, "PEP 508 environment marker."),
            key(
                "indirect",
                BOOL,
                OPT,
                "Go only: true when the `require` line carries the `// indirect` comment.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "manifest.script.v1",
        languages: &["json"],
        query_family: "pipeline",
        description: "A named script in a package.json or composer.json `scripts` object.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "ecosystem",
                STR,
                ALWAYS,
                "Package ecosystem (\"npm\" or \"composer\").",
            ),
            key("name", STR, ALWAYS, "Script name."),
            key(
                "command",
                STR,
                ALWAYS,
                "Command as written; a Composer command list is joined with ` && `.",
            ),
        ],
    },
    // Go module manifests (go.mod)
    StructuralFactPatternSpec {
        pattern_id: "gomod.module.v1",
        languages: &["gomod"],
        query_family: "dependencies",
        description: "The `module` directive of a go.mod manifest.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_GOMOD_MODULE_PATH,
            key(
                "deprecated",
                STR,
                OPT,
                "The `Deprecated:` paragraph of the directive's leading or suffix comment.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "gomod.go.v1",
        languages: &["gomod"],
        query_family: "dependencies",
        description: "The `go` directive of a go.mod manifest: the minimum Go version.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("version", STR, ALWAYS, "The Go version as written."),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "gomod.toolchain.v1",
        languages: &["gomod"],
        query_family: "dependencies",
        description: "The `toolchain` directive of a go.mod manifest.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "toolchain",
                STR,
                ALWAYS,
                "The toolchain name as written (`go1.22.4` or `default`).",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "gomod.replace.v1",
        languages: &["gomod"],
        query_family: "dependencies",
        description: "A `replace` line of a go.mod manifest.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_GOMOD_MODULE_PATH,
            key(
                "version",
                STR,
                OPT,
                "The replaced version; absent when every version is replaced.",
            ),
            key(
                "replacement",
                STR,
                ALWAYS,
                "The replacement module path, or the file path of a local replacement.",
            ),
            key(
                "replacement_version",
                STR,
                OPT,
                "The replacement module version; absent for a file path.",
            ),
            key(
                "local",
                BOOL,
                ALWAYS,
                "True when the replacement is a file path.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "gomod.exclude.v1",
        languages: &["gomod"],
        query_family: "dependencies",
        description: "An `exclude` line of a go.mod manifest.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_GOMOD_MODULE_PATH,
            key("version", STR, ALWAYS, "The excluded version."),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "gomod.retract.v1",
        languages: &["gomod"],
        query_family: "dependencies",
        description: "A `retract` line of a go.mod manifest: one version or a `[low, high]` interval of this module.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "low",
                STR,
                ALWAYS,
                "The lowest retracted version; the version itself for a single version.",
            ),
            key(
                "high",
                STR,
                ALWAYS,
                "The highest retracted version; the version itself for a single version.",
            ),
            key(
                "range",
                BOOL,
                ALWAYS,
                "True when the line is written as a `[low, high]` interval.",
            ),
            key(
                "rationale",
                STR,
                OPT,
                "The line's leading and suffix comments, else its block's, without `//`.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "gomod.tool.v1",
        languages: &["gomod"],
        query_family: "dependencies",
        description: "A `tool` line of a go.mod manifest.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "package_path",
                STR,
                ALWAYS,
                "The import path of the tool's main package.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "gomod.ignore.v1",
        languages: &["gomod"],
        query_family: "dependencies",
        description: "An `ignore` line of a go.mod manifest: a directory the go command skips when it matches package patterns.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "path",
                STR,
                ALWAYS,
                "The ignored directory path as written.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "gomod.godebug.v1",
        languages: &["gomod"],
        query_family: "dependencies",
        description: "A `godebug` setting of a go.mod manifest: the default GODEBUG value for the main module's builds.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "key",
                STR,
                ALWAYS,
                "The setting name, such as `panicnil` or `default`.",
            ),
            key("value", STR, ALWAYS, "The setting value as written."),
        ],
    },
    // Go checksum files (go.sum)
    StructuralFactPatternSpec {
        pattern_id: "gosum.checksum.v1",
        languages: &["gosum"],
        query_family: "dependencies",
        description: "One line of a go.sum file: the hash the go command verified for a module version or for its go.mod file.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("module_path", STR, ALWAYS, "The module path."),
            key(
                "version",
                STR,
                ALWAYS,
                "The module version, without the `/go.mod` suffix.",
            ),
            key(
                "go_mod",
                BOOL,
                ALWAYS,
                "True when the hash covers only the module's go.mod file (`<version>/go.mod`); false when it covers the module content.",
            ),
            key(
                "hash_algorithm",
                STR,
                ALWAYS,
                "The hash algorithm prefix, such as `h1`.",
            ),
            key(
                "hash",
                STR,
                ALWAYS,
                "The base64 hash after the algorithm prefix.",
            ),
            key(
                "incompatible",
                BOOL,
                ALWAYS,
                "True when the version ends in `+incompatible`.",
            ),
            key(
                "pseudo_version",
                BOOL,
                ALWAYS,
                "True when the version is a pseudo-version, as `module.IsPseudoVersion` decides.",
            ),
            key(
                "timestamp",
                STR,
                OPT,
                "The UTC commit time of a pseudo-version, `yyyymmddhhmmss`.",
            ),
            key(
                "revision",
                STR,
                OPT,
                "The commit hash prefix of a pseudo-version.",
            ),
        ],
    },
    // Deployment and automation documents (YAML)
    StructuralFactPatternSpec {
        pattern_id: "yaml.compose_service.v1",
        languages: &["yaml"],
        query_family: "service_structure",
        description: "A service under `services` in a Docker Compose file (`compose.yaml`, `docker-compose*.yml`).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("name", STR, ALWAYS, "Service name."),
            key("image", STR, OPT, "The `image` value."),
            key(
                "build_context",
                STR,
                OPT,
                "The `build` path, or `build.context`.",
            ),
            key("ports", ARR, OPT, "The `ports` entries as written."),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "yaml.k8s_resource.v1",
        languages: &["yaml"],
        query_family: "service_structure",
        description: "A Kubernetes resource: a YAML document whose root holds `apiVersion` and `kind`.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("api_version", STR, ALWAYS, "The `apiVersion` value."),
            key("kind", STR, ALWAYS, "The `kind` value."),
            key("name", STR, OPT, "The `metadata.name` value."),
            key("namespace", STR, OPT, "The `metadata.namespace` value."),
            key(
                "document_index",
                NUM,
                OPT,
                "0-based index of the document; present only in a stream of more than one document.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "yaml.ansible_task.v1",
        languages: &["yaml"],
        query_family: "pipeline",
        description: "A task or handler in an Ansible playbook or task file.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "module",
                STR,
                ALWAYS,
                "Module key as written (`ansible.builtin.apt`): the first key that is not a task keyword.",
            ),
            key("name", STR, OPT, "The task `name`."),
            key(
                "handler",
                BOOL,
                ALWAYS,
                "True for a task under `handlers` or in a handlers file.",
            ),
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

const K_GOMOD_MODULE_PATH: super::MetadataKeySpec = key(
    "module_path",
    STR,
    ALWAYS,
    "The module path, unquoted when written as a Go string.",
);

const K_CI_PLATFORM: super::MetadataKeySpec = key(
    "platform",
    STR,
    ALWAYS,
    "CI platform (\"github_actions\" or \"gitlab_ci\").",
);
