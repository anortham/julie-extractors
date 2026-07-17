//! Built-in language-local SPECS for R, Zig, QML, Bash, PowerShell, GDScript, and VB.NET.
//!
//! Authored metadata for [`super::super::StructuralFactPatternSpec`] entries.
//! Public registry access remains through
//! [`super::super::structural_fact_pattern_specs`].

use super::super::{
    ALWAYS, BASE_KEYS, K_PATTERN_VERSION, K_QUERY_FAMILY, OPT, STR, StructuralFactPatternSpec, key,
};

pub(super) const SPECS: &[StructuralFactPatternSpec] = &[
    StructuralFactPatternSpec {
        pattern_id: "r.library_call.v1",
        languages: &["r"],
        query_family: "imports",
        description: "An R `library()`/`require()` package load.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "load_kind",
                STR,
                ALWAYS,
                "Which load form was used (`library` or `require`).",
            ),
            key(
                "package_name",
                STR,
                OPT,
                "The package name argument (quotes stripped).",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "r.pipe_expression.v1",
        languages: &["r"],
        query_family: "pipeline",
        description: "An R pipe expression (`|>` or `%>%`).",
        metadata_keys: BASE_KEYS,
    },
    StructuralFactPatternSpec {
        pattern_id: "r.formula_expression.v1",
        languages: &["r"],
        query_family: "modeling",
        description: "An R model formula expression (`y ~ x`).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "formula_text",
                STR,
                ALWAYS,
                "The full text of the R model formula.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "zig.builtin_call.v1",
        languages: &["zig"],
        query_family: "builtin",
        description: "A Zig builtin function call (`@name(...)`).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "builtin_name",
                STR,
                OPT,
                "The builtin function name with leading `@` stripped.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "zig.threadlocal_variable.v1",
        languages: &["zig"],
        query_family: "storage",
        description: "A Zig `threadlocal` variable declaration.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "variable_name",
                STR,
                OPT,
                "The threadlocal variable's name.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "zig.inline_function.v1",
        languages: &["zig"],
        query_family: "functions",
        description: "A Zig `inline fn` function.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("function_name", STR, OPT, "The inline function's name."),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "zig.exported_function.v1",
        languages: &["zig"],
        query_family: "ffi",
        description: "A Zig `export fn` function.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("function_name", STR, OPT, "The exported function's name."),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "zig.comptime_parameter.v1",
        languages: &["zig"],
        query_family: "metaprogramming",
        description: "A Zig `comptime` function parameter.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("parameter_name", STR, OPT, "The comptime parameter's name."),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "qml.import_statement.v1",
        languages: &["qml"],
        query_family: "imports",
        description: "A QML `import` statement.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("import_module", STR, OPT, "The imported QML module source."),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "qml.property_declaration.v1",
        languages: &["qml"],
        query_family: "properties",
        description: "A QML `property` declaration.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("property_name", STR, OPT, "The declared property name."),
            key("property_type", STR, OPT, "The declared property type."),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "qml.signal_declaration.v1",
        languages: &["qml"],
        query_family: "signals",
        description: "A QML `signal` declaration.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("signal_name", STR, OPT, "The declared signal name."),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "qml.binding.v1",
        languages: &["qml"],
        query_family: "bindings",
        description: "A QML property binding.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("property_name", STR, ALWAYS, "The bound property's name."),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "bash.shebang.v1",
        languages: &["bash"],
        query_family: "script_header",
        description: "A Bash script shebang line.",
        metadata_keys: BASE_KEYS,
    },
    StructuralFactPatternSpec {
        pattern_id: "bash.command_substitution.v1",
        languages: &["bash"],
        query_family: "expansion",
        description: "A Bash command substitution (`$(...)` or backticks).",
        metadata_keys: BASE_KEYS,
    },
    StructuralFactPatternSpec {
        pattern_id: "bash.arithmetic_expansion.v1",
        languages: &["bash"],
        query_family: "expansion",
        description: "A Bash arithmetic expansion (`$((...))`).",
        metadata_keys: BASE_KEYS,
    },
    StructuralFactPatternSpec {
        pattern_id: "bash.export_declaration.v1",
        languages: &["bash"],
        query_family: "environment",
        description: "A Bash `export` declaration.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("variable_name", STR, OPT, "The exported variable's name."),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "powershell.cmdlet_binding_attribute.v1",
        languages: &["powershell"],
        query_family: "metadata",
        description: "A PowerShell `[CmdletBinding()]` attribute.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "attribute_name",
                STR,
                ALWAYS,
                "The attribute name (always \"CmdletBinding\").",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "powershell.param_block.v1",
        languages: &["powershell"],
        query_family: "parameters",
        description: "A PowerShell `param(...)` block.",
        metadata_keys: BASE_KEYS,
    },
    StructuralFactPatternSpec {
        pattern_id: "powershell.pipeline_expression.v1",
        languages: &["powershell"],
        query_family: "pipeline",
        description: "A PowerShell pipeline expression (`|`).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "pipeline_marker",
                STR,
                ALWAYS,
                "Pipeline marker token (always \"|\").",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "powershell.class_definition.v1",
        languages: &["powershell"],
        query_family: "types",
        description: "A PowerShell `class` definition.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("class_name", STR, OPT, "The PowerShell class name."),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "gdscript.class_name.v1",
        languages: &["gdscript"],
        query_family: "types",
        description: "A GDScript `class_name` registration.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("class_name", STR, OPT, "The registered class name."),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "gdscript.extends_declaration.v1",
        languages: &["gdscript"],
        query_family: "inheritance",
        description: "A GDScript `extends` declaration.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "base_type",
                STR,
                OPT,
                "The base type/scene the script extends.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "gdscript.signal_declaration.v1",
        languages: &["gdscript"],
        query_family: "signals",
        description: "A GDScript `signal` declaration.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("signal_name", STR, OPT, "The declared signal name."),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "gdscript.export_annotation.v1",
        languages: &["gdscript"],
        query_family: "metadata",
        description: "A GDScript `@export` annotation.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "annotation_name",
                STR,
                ALWAYS,
                "The annotation name (always \"export\").",
            ),
            key(
                "exported_variable",
                STR,
                OPT,
                "The variable name the export annotation applies to.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "gdscript.match_statement.v1",
        languages: &["gdscript"],
        query_family: "control_flow",
        description: "A GDScript `match` statement.",
        metadata_keys: BASE_KEYS,
    },
    StructuralFactPatternSpec {
        pattern_id: "vbnet.handles_clause.v1",
        languages: &["vbnet"],
        query_family: "events",
        description: "A VB.NET `Handles` clause on a method.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "handles_target",
                STR,
                OPT,
                "The event target named after the `Handles` keyword.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "vbnet.implements_clause.v1",
        languages: &["vbnet"],
        query_family: "interface",
        description: "A VB.NET `Implements` clause on a member.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "implements_target",
                STR,
                OPT,
                "The interface member named after the `Implements` keyword.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "vbnet.event_declaration.v1",
        languages: &["vbnet"],
        query_family: "events",
        description: "A VB.NET `Event` declaration.",
        metadata_keys: BASE_KEYS,
    },
    StructuralFactPatternSpec {
        pattern_id: "vbnet.attribute.v1",
        languages: &["vbnet"],
        query_family: "metadata",
        description: "A VB.NET attribute use.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("attribute_name", STR, OPT, "The .NET attribute name."),
        ],
    },
];
