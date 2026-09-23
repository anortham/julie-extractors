//! Built-in language-local SPECS for C-family, Go, Python, and JS/TS.
//!
//! Authored metadata for [`super::super::StructuralFactPatternSpec`] entries.
//! Public registry access remains through
//! [`super::super::structural_fact_pattern_specs`].

use super::super::{
    ALWAYS, ARR, BASE_KEYS, BOOL, K_PATTERN_VERSION, K_QUERY_FAMILY, MetadataKeySpec, OPT, STR,
    StructuralFactPatternSpec, key,
};

const RUST_DOC_TEST_KEYS: &[MetadataKeySpec] = &[
    BASE_KEYS[0],
    BASE_KEYS[1],
    key(
        "mode",
        STR,
        ALWAYS,
        "Rustdoc execution mode: run, ignore, no_run, or compile_fail.",
    ),
];

pub(super) const SPECS: &[StructuralFactPatternSpec] = &[
    StructuralFactPatternSpec {
        pattern_id: "rust.unsafe_block.v1",
        languages: &["rust"],
        query_family: "safety",
        description: "A Rust `unsafe { … }` block.",
        metadata_keys: BASE_KEYS,
    },
    StructuralFactPatternSpec {
        pattern_id: "rust.doc_test.v1",
        languages: &["rust"],
        query_family: "testing",
        description: "An executable fenced Rust code block in a Rustdoc comment.",
        metadata_keys: RUST_DOC_TEST_KEYS,
    },
    StructuralFactPatternSpec {
        pattern_id: "go.goroutine_launch.v1",
        languages: &["go"],
        query_family: "concurrency",
        description: "A Go `go call()` goroutine launch.",
        metadata_keys: BASE_KEYS,
    },
    StructuralFactPatternSpec {
        pattern_id: "go.defer_statement.v1",
        languages: &["go"],
        query_family: "lifecycle",
        description: "A Go `defer call()` statement.",
        metadata_keys: BASE_KEYS,
    },
    StructuralFactPatternSpec {
        pattern_id: "python.decorated_definition.v1",
        languages: &["python"],
        query_family: "metadata",
        description: "A Python decorated function or class definition.",
        metadata_keys: BASE_KEYS,
    },
    StructuralFactPatternSpec {
        pattern_id: "javascript.await_expression.v1",
        languages: &["javascript"],
        query_family: "async",
        description: "A JavaScript `await` expression.",
        metadata_keys: BASE_KEYS,
    },
    StructuralFactPatternSpec {
        pattern_id: "jsx.await_expression.v1",
        languages: &["jsx"],
        query_family: "async",
        description: "A JSX `await` expression.",
        metadata_keys: BASE_KEYS,
    },
    StructuralFactPatternSpec {
        pattern_id: "typescript.await_expression.v1",
        languages: &["typescript"],
        query_family: "async",
        description: "A TypeScript `await` expression.",
        metadata_keys: BASE_KEYS,
    },
    StructuralFactPatternSpec {
        pattern_id: "tsx.await_expression.v1",
        languages: &["tsx"],
        query_family: "async",
        description: "A TSX `await` expression.",
        metadata_keys: BASE_KEYS,
    },
    StructuralFactPatternSpec {
        pattern_id: "c.preprocessor_definition.v1",
        languages: &["c"],
        query_family: "preprocessor",
        description: "A C `#define` object-like or function-like macro.",
        metadata_keys: BASE_KEYS,
    },
    StructuralFactPatternSpec {
        pattern_id: "cpp.preprocessor_definition.v1",
        languages: &["cpp"],
        query_family: "preprocessor",
        description: "A C++ `#define` object-like or function-like macro.",
        metadata_keys: BASE_KEYS,
    },
    StructuralFactPatternSpec {
        pattern_id: "cpp.qt_property.v1",
        languages: &["cpp"],
        query_family: "properties",
        description: "A Qt `Q_PROPERTY` declaration on a C++ class.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("name", STR, ALWAYS, "The property name."),
            key("property_type", STR, OPT, "The declared property type."),
            key("read", STR, OPT, "The READ accessor name."),
            key("write", STR, OPT, "The WRITE accessor name."),
            key("notify", STR, OPT, "The NOTIFY signal name."),
            key("member", STR, OPT, "The MEMBER variable name."),
            key("reset", STR, OPT, "The RESET accessor name."),
            key("bindable", STR, OPT, "The BINDABLE accessor name."),
            key("designable", STR, OPT, "The DESIGNABLE condition or value."),
            key("scriptable", STR, OPT, "The SCRIPTABLE condition or value."),
            key("stored", STR, OPT, "The STORED condition or value."),
            key("user", STR, OPT, "The USER condition or value."),
            key("revision", STR, OPT, "The REVISION value."),
            key("constant", BOOL, OPT, "Whether CONSTANT was declared."),
            key("final", BOOL, OPT, "Whether FINAL was declared."),
            key("required", BOOL, OPT, "Whether REQUIRED was declared."),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "fsharp.attribute.v1",
        languages: &["fsharp"],
        query_family: "metadata",
        description: "An F# attribute applied to a declaration.",
        metadata_keys: BASE_KEYS,
    },
    StructuralFactPatternSpec {
        pattern_id: "fsharp.computation_expression.v1",
        languages: &["fsharp"],
        query_family: "computation_expression",
        description: "An F# computation expression such as `task { }` or `seq { }`.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "builder",
                STR,
                ALWAYS,
                "The builder expression before the braces (`task`, `async`, `seq`, `result`).",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "fsharp.active_pattern.v1",
        languages: &["fsharp"],
        query_family: "pattern_matching",
        description: "An F# active pattern definition such as `(|Even|Odd|)`.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("name", STR, ALWAYS, "The banana-clip name as written."),
            key("cases", ARR, ALWAYS, "The case names in declaration order."),
            key(
                "partial",
                BOOL,
                ALWAYS,
                "Whether the pattern is partial (`(|Case|_|)`).",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "fsharp.quotation.v1",
        languages: &["fsharp"],
        query_family: "metaprogramming",
        description: "An F# code quotation (`<@ ... @>` or `<@@ ... @@>`).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "quotation_kind",
                STR,
                ALWAYS,
                "\"typed\" for `<@ @>`, \"untyped\" for `<@@ @@>`.",
            ),
        ],
    },
];
