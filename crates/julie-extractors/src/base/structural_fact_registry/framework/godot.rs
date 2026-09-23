//! Godot engine SPECS for GDScript.
//!
//! Authored metadata for [`super::super::StructuralFactPatternSpec`] entries.
//! Public registry access remains through
//! [`super::super::structural_fact_pattern_specs`].

use super::super::{
    ALWAYS, ARR, K_FRAMEWORK, K_PATTERN_VERSION, K_QUERY_FAMILY, OPT, STR,
    StructuralFactPatternSpec, key,
};

pub(super) const SPECS: &[StructuralFactPatternSpec] = &[
    StructuralFactPatternSpec {
        pattern_id: "godot.signal_connection.v1",
        languages: &["gdscript"],
        query_family: "signals",
        description: "A Godot signal connection: `emitter.signal.connect(handler)`, or `connect(\"signal\", ...)` with a static signal name.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key("signal_name", STR, ALWAYS, "The connected signal."),
            key(
                "emitter",
                STR,
                OPT,
                "Source text of the object that emits the signal; absent for the script itself.",
            ),
            key(
                "handler",
                STR,
                OPT,
                "The handler callable (`_on_hit`, `obj._on_hit`) or the Godot 3 method-name string; absent for a lambda.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "godot.signal_emission.v1",
        languages: &["gdscript"],
        query_family: "signals",
        description: "A Godot signal emission: `signal.emit(...)` or `emit_signal(\"signal\", ...)` with a static signal name.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key("signal_name", STR, ALWAYS, "The emitted signal."),
            key(
                "emitter",
                STR,
                OPT,
                "Source text of the object that emits the signal; absent for the script itself.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "godot.resource_reference.v1",
        languages: &["gdscript"],
        query_family: "imports",
        description: "A static Godot resource path loaded by `preload`, `load`, or `ResourceLoader.load`, or a script path after `extends`.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "resource_path",
                STR,
                ALWAYS,
                "The resource path as written (for example \"res://scenes/bullet.tscn\").",
            ),
            key(
                "loader",
                STR,
                ALWAYS,
                "How the resource is reached: \"preload\", \"load\", \"ResourceLoader.load\", or \"extends\".",
            ),
            key(
                "bound_name",
                STR,
                OPT,
                "The `const` or `var` whose initializer is the load call.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "godot.node_path.v1",
        languages: &["gdscript"],
        query_family: "scene_tree",
        description: "A static scene-tree node path: `$Path`, `%UniqueName`, or `get_node(\"path\")`.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "node_path",
                STR,
                ALWAYS,
                "The node path without the `$`/`%` sigil or quotes.",
            ),
            key(
                "access",
                STR,
                ALWAYS,
                "The access form: \"dollar\", \"unique_name\", or \"get_node\".",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "godot.rpc_annotation.v1",
        languages: &["gdscript"],
        query_family: "networking",
        description: "An `@rpc` annotation on a function that peers may call over the network.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key("function_name", STR, ALWAYS, "The annotated function."),
            key(
                "rpc_arguments",
                ARR,
                ALWAYS,
                "The annotation arguments in order, strings unquoted (for example [\"any_peer\", \"call_local\", \"reliable\"]).",
            ),
        ],
    },
];
