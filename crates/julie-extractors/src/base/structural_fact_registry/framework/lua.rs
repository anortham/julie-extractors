//! Lapis, Neovim, LÖVE, and lazy.nvim framework SPECS for Lua.
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
        pattern_id: "lapis.route.v1",
        languages: &["lua"],
        query_family: "framework",
        description: "A Lapis `app:get/post/put/patch/delete/head/options/match` route with a static path and a handler, in a file that mentions lapis.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key("api_style", STR, ALWAYS, "Routing style (\"dsl_routing\")."),
            key(
                "route_template",
                STR,
                ALWAYS,
                "Raw static route path from the route call.",
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
                "route_name",
                STR,
                OPT,
                "Route name when the call names the route before its path.",
            ),
            key(
                "verb",
                STR,
                OPT,
                "Uppercase HTTP method; absent for `match`, which accepts every method.",
            ),
            key("verb_source", STR, OPT, "How the verb was attested."),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "neovim.user_command.v1",
        languages: &["lua"],
        query_family: "framework",
        description: "A Neovim user command created with `vim.api.nvim_create_user_command` or `nvim_buf_create_user_command` and a static name.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key("command_name", STR, ALWAYS, "The command name."),
            key(
                "scope",
                STR,
                ALWAYS,
                "\"global\" or \"buffer\" (the buffer-local API).",
            ),
            key("desc", STR, OPT, "Static `desc` option."),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "neovim.autocmd.v1",
        languages: &["lua"],
        query_family: "framework",
        description: "A Neovim autocommand created with `vim.api.nvim_create_autocmd` and static event names.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "events",
                ARR,
                ALWAYS,
                "Event names the autocommand listens to.",
            ),
            key("patterns", ARR, OPT, "Static `pattern` option values."),
            key("group", STR, OPT, "Static `group` option."),
            key("desc", STR, OPT, "Static `desc` option."),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "neovim.keymap.v1",
        languages: &["lua"],
        query_family: "framework",
        description: "A Neovim key mapping set with `vim.keymap.set`, `nvim_set_keymap`, or `nvim_buf_set_keymap` and static modes and left-hand side.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "modes",
                ARR,
                ALWAYS,
                "Mode short names (\"n\", \"v\", ...).",
            ),
            key("lhs", STR, ALWAYS, "The mapped key sequence."),
            key("desc", STR, OPT, "Static `desc` option."),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "love.callback.v1",
        languages: &["lua"],
        query_family: "framework",
        description: "A LÖVE engine callback defined as `function love.<callback>()` or assigned as `love.<callback> = function`.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "callback",
                STR,
                ALWAYS,
                "The callback name (\"load\", \"draw\", ...).",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "lazy_nvim.plugin_spec.v1",
        languages: &["lua"],
        query_family: "framework",
        description: "A lazy.nvim plugin spec: a module-level returned table (or a table in the returned list) whose first positional value is an `owner/repo` string.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key("plugin", STR, ALWAYS, "The `owner/repo` plugin id."),
            key(
                "dependencies",
                ARR,
                OPT,
                "Plugin ids named in `dependencies`.",
            ),
            key(
                "commands",
                ARR,
                OPT,
                "Static `cmd` values that lazy-load the plugin.",
            ),
            key(
                "events",
                ARR,
                OPT,
                "Static `event` values that lazy-load the plugin.",
            ),
            key(
                "filetypes",
                ARR,
                OPT,
                "Static `ft` values that lazy-load the plugin.",
            ),
        ],
    },
];
