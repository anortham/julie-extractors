//! Built-in language-local SPECS for PHP, Ruby, Elixir, and Lua.
//!
//! Authored metadata for [`super::super::StructuralFactPatternSpec`] entries.
//! Public registry access remains through
//! [`super::super::structural_fact_pattern_specs`].

use super::super::{
    ALWAYS, BASE_KEYS, K_PATTERN_VERSION, K_QUERY_FAMILY, NUM, OPT, STR, StructuralFactPatternSpec,
    key,
};

pub(super) const SPECS: &[StructuralFactPatternSpec] = &[
    StructuralFactPatternSpec {
        pattern_id: "php.attribute.v1",
        languages: &["php"],
        query_family: "metadata",
        description: "A PHP 8 attribute (`#[...]`).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("attribute_name", STR, OPT, "The PHP attribute name."),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "php.namespace_definition.v1",
        languages: &["php"],
        query_family: "module",
        description: "A PHP `namespace` declaration.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("namespace_name", STR, OPT, "The declared namespace path."),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "php.namespace_use_declaration.v1",
        languages: &["php"],
        query_family: "imports",
        description: "A PHP `use` import declaration.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "import_target",
                STR,
                OPT,
                "The qualified symbol being imported.",
            ),
            key(
                "import_alias",
                STR,
                OPT,
                "The local alias assigned via `as`, when present.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "php.trait_use_declaration.v1",
        languages: &["php"],
        query_family: "traits",
        description: "A PHP `use` trait mixin inside a class.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "trait_name",
                STR,
                OPT,
                "The trait being mixed into the class.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "php.anonymous_function.v1",
        languages: &["php"],
        query_family: "functional",
        description: "A PHP anonymous function or arrow function.",
        metadata_keys: BASE_KEYS,
    },
    StructuralFactPatternSpec {
        pattern_id: "php.match_expression.v1",
        languages: &["php"],
        query_family: "control_flow",
        description: "A PHP 8 `match` expression.",
        metadata_keys: BASE_KEYS,
    },
    StructuralFactPatternSpec {
        pattern_id: "ruby.require_call.v1",
        languages: &["ruby"],
        query_family: "imports",
        description: "A Ruby `require`/`require_relative` call.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "require_kind",
                STR,
                ALWAYS,
                "Which require form was used (`require` or `require_relative`).",
            ),
            key(
                "required_path",
                STR,
                OPT,
                "The quoted path/module string being required.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "ruby.mixin_call.v1",
        languages: &["ruby"],
        query_family: "mixins",
        description: "A Ruby `include`/`extend`/`prepend` mixin call.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "mixin_kind",
                STR,
                ALWAYS,
                "Which mixin form was used (`include`, `extend`, or `prepend`).",
            ),
            key(
                "mixin_target",
                STR,
                OPT,
                "The module/constant being mixed in.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "ruby.block.v1",
        languages: &["ruby"],
        query_family: "blocks",
        description: "A Ruby block (`do…end` or `{…}`).",
        metadata_keys: BASE_KEYS,
    },
    StructuralFactPatternSpec {
        pattern_id: "ruby.rescue_clause.v1",
        languages: &["ruby"],
        query_family: "error_handling",
        description: "A Ruby `rescue` clause.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "exception_type",
                STR,
                OPT,
                "The rescued exception class name.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "elixir.defmodule_call.v1",
        languages: &["elixir"],
        query_family: "module",
        description: "An Elixir `defmodule` definition.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "module_name",
                STR,
                OPT,
                "The module name declared by defmodule.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "elixir.module_attribute.v1",
        languages: &["elixir"],
        query_family: "metadata",
        description: "An Elixir module attribute (`@name`).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "attribute_name",
                STR,
                OPT,
                "The module attribute name (identifier after `@`).",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "elixir.directive_call.v1",
        languages: &["elixir"],
        query_family: "directives",
        description: "An Elixir `use`/`import`/`alias`/`require` directive.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "directive_kind",
                STR,
                ALWAYS,
                "Which directive was used (`use`, `import`, `alias`, or `require`).",
            ),
            key(
                "directive_target",
                STR,
                OPT,
                "The module/identifier the directive references.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "elixir.pipeline_operator.v1",
        languages: &["elixir"],
        query_family: "pipeline",
        description: "An Elixir pipe operator (`|>`) expression.",
        metadata_keys: BASE_KEYS,
    },
    StructuralFactPatternSpec {
        pattern_id: "elixir.with_expression.v1",
        languages: &["elixir"],
        query_family: "control_flow",
        description: "An Elixir `with` expression.",
        metadata_keys: BASE_KEYS,
    },
    StructuralFactPatternSpec {
        pattern_id: "lua.require_call.v1",
        languages: &["lua"],
        query_family: "imports",
        description: "A Lua `require` call.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "call_name",
                STR,
                ALWAYS,
                "The resolved function-call name (here `require`).",
            ),
            key(
                "required_module",
                STR,
                OPT,
                "The quoted module string passed to require.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "lua.setmetatable_call.v1",
        languages: &["lua"],
        query_family: "metatable",
        description: "A Lua `setmetatable` call.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "call_name",
                STR,
                ALWAYS,
                "The resolved function-call name (here `setmetatable`).",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "lua.coroutine_call.v1",
        languages: &["lua"],
        query_family: "concurrency",
        description: "A Lua `coroutine.*` call.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "call_name",
                STR,
                ALWAYS,
                "The resolved function-call name (a `coroutine.*` call).",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "lua.module_return.v1",
        languages: &["lua"],
        query_family: "module",
        description: "A Lua module `return` value.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "returned_value",
                STR,
                OPT,
                "The expression returned as the module's value.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "lua.table_constructor.v1",
        languages: &["lua"],
        query_family: "data",
        description: "A Lua table constructor literal.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "field_count",
                NUM,
                ALWAYS,
                "Number of fields in the table literal.",
            ),
        ],
    },
];
