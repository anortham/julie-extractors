//! Built-in language-local SPECS for Zig, Bash, and GDScript.
//!
//! Authored metadata for [`super::super::StructuralFactPatternSpec`] entries.
//! Public registry access remains through
//! [`super::super::structural_fact_pattern_specs`].

use super::super::{
    ALWAYS, ARR, BASE_KEYS, K_PATTERN_VERSION, K_QUERY_FAMILY, OPT, STR, StructuralFactPatternSpec,
    key,
};

pub(super) const SPECS: &[StructuralFactPatternSpec] = &[
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
        description: "A Bash declaration that exports its names (`export`, `declare -x`, `local -x`).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "variable_name",
                STR,
                OPT,
                "The first exported variable's name.",
            ),
            key(
                "variable_names",
                ARR,
                ALWAYS,
                "Every name the declaration exports, in source order.",
            ),
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
        description: "A GDScript variable export: an `@export` or `@export_*` annotation, or a Godot 3 `export var`.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "annotation_name",
                STR,
                ALWAYS,
                "The export annotation name (for example \"export\" or \"export_range\").",
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
];
