//! Built-in language-local SPECS for Erlang.
//!
//! Authored metadata for [`super::super::StructuralFactPatternSpec`] entries
//! emitted by the erlang arm of `base/code_structural_facts.rs`. Public registry
//! access remains through [`super::super::structural_fact_pattern_specs`].
//!
//! The set covers the module-header shapes a `.erl` file declares before any
//! function body: module identity, OTP behaviour adoption and callback
//! contracts, the export lists, and the include chain. Every key is scalar so a
//! consumer can filter on it directly; the exported and included names
//! themselves are already reachable as symbols and pending relationship edges.

use super::super::{
    ALWAYS, K_PATTERN_VERSION, K_QUERY_FAMILY, NUM, OPT, STR, StructuralFactPatternSpec, key,
};

pub(super) const SPECS: &[StructuralFactPatternSpec] = &[
    StructuralFactPatternSpec {
        pattern_id: "erlang.module_attribute.v1",
        languages: &["erlang"],
        query_family: "module",
        description: "An Erlang `-module(...)` declaration.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("module", STR, ALWAYS, "Declared module name."),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "erlang.behaviour_declaration.v1",
        languages: &["erlang"],
        query_family: "otp",
        description: "An Erlang `-behaviour(...)`/`-behavior(...)` adoption.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "behaviour",
                STR,
                ALWAYS,
                "Name of the behaviour module the declaring module implements.",
            ),
            key(
                "attribute",
                STR,
                ALWAYS,
                "Spelling as written in source (\"behaviour\" or \"behavior\"); both are valid.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "erlang.callback_declaration.v1",
        languages: &["erlang"],
        query_family: "otp",
        description: "An Erlang `-callback` declaration in a behaviour module.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("callback_name", STR, ALWAYS, "Declared callback name."),
            key(
                "arity",
                NUM,
                ALWAYS,
                "Number of arguments in the callback signature.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "erlang.export_attribute.v1",
        languages: &["erlang"],
        query_family: "module",
        description: "An Erlang `-export(...)` or `-export_type(...)` list.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "export_kind",
                STR,
                ALWAYS,
                "What the list publishes (\"function\" or \"type\").",
            ),
            key(
                "exported_count",
                NUM,
                ALWAYS,
                "Number of name/arity entries in the list.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "erlang.include_directive.v1",
        languages: &["erlang"],
        query_family: "imports",
        description: "An Erlang `-include(...)` or `-include_lib(...)` header dependency.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "include_kind",
                STR,
                ALWAYS,
                "Whether the directive is an \"include\" or an \"include_lib\".",
            ),
            key(
                "path",
                STR,
                ALWAYS,
                "Declared header path, exactly as written in the string literal.",
            ),
            key(
                "application",
                STR,
                OPT,
                "Leading path segment of an `include_lib` path, which names the owning OTP application; absent on a plain include and on a path with no separator.",
            ),
        ],
    },
];
