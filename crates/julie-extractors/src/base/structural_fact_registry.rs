//! Typed registry of every structural-fact `pattern_id` the extractor emits.
//!
//! This module is the machine-readable source of truth for the metadata payload
//! carried by each structural fact: for every pattern it declares the languages
//! it fires for, its query family, and every metadata key with a value type and
//! a presence rule. Downstream consumers (Miller, the `languages --json` report,
//! contract docs) read this registry instead of hard-coding out-of-band
//! knowledge of the payloads.
//!
//! The registry describes the EXISTING v3 contract; it does not change emission.
//! It is authored directly from the collector emission sites
//! (`insert_string`/`insert_number`/`metadata.insert` call sites) across these
//! sources (with the languages each covers):
//!
//! - `base/structural_facts.rs`: built-in patterns for c, cpp, go, javascript, jsx, python, rust, tsx, typescript.
//! - `base/code_structural_facts.rs`: dart, elixir, java, kotlin, lua, php, r, ruby, scala, swift, bash, gdscript, powershell, qml, vbnet, zig.
//! - `base/data_structural_facts.rs`: markdown, json, toml, yaml, regex.
//! - `base/sql_structural_facts.rs`: sql.
//! - `base/framework_structural_facts.rs`: aspnet, htmx, alpine, razor.
//! - `base/web_structural_facts/`: css, html, vue, react, nextjs, nuxt, http client.
//!
//! Presence semantics (the conformance rule Task 2 enforces over the golden
//! corpus): an `Always` key is present on every emitted fact of its pattern; an
//! `Optional` key may be absent. When a key is derived from a value that gates
//! emission (the fact is only produced when the value exists) it is `Always`.
//!
//! Every fact also carries the two base keys `pattern_version` and
//! `query_family` (from each collector's `base_metadata`). Framework facts and
//! web route/http facts additionally carry a `framework` key.

/// JSON value type a metadata key carries. Additions to this enum are
/// lead-adjudicated contract decisions, not silent extensions: when a collector
/// emits a value shape none of these variants can express, that is a contract
/// mismatch to escalate and adjudicate, never to paper over. `ObjectArray` is
/// the one such adjudicated addition so far (Task 2, finding D1), covering
/// `route_parameters` on `razor.page_directive.v1` — a shipped v2.5.x payload
/// that cannot be flattened to a `StringArray` without losing per-parameter
/// fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetadataValueType {
    String,
    Bool,
    Number,
    StringArray,
    /// A JSON array whose every element is a JSON object. The object's fields
    /// are documented in prose on the declaring key; the registry does not carry
    /// a per-field schema for them.
    ObjectArray,
}

/// Whether a declared metadata key is guaranteed present (`Always`) on every
/// emitted fact of the pattern, or may be absent (`Optional`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyPresence {
    Always,
    Optional,
}

/// One metadata key declared for a pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MetadataKeySpec {
    /// Metadata key name, e.g. `route_path`.
    pub key: &'static str,
    /// JSON value type the key carries.
    pub value_type: MetadataValueType,
    /// Whether the key is always present or conditional.
    pub presence: KeyPresence,
    /// One-sentence, consumer-facing description of the key.
    pub description: &'static str,
}

/// The full contract for one structural-fact `pattern_id`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StructuralFactPatternSpec {
    /// Stable pattern identifier, e.g. `nextjs.file_route.v1`.
    pub pattern_id: &'static str,
    /// Languages the collectors emit this pattern for.
    pub languages: &'static [&'static str],
    /// Query family the fact belongs to (mirrors the emitted `query_family`).
    pub query_family: &'static str,
    /// One-sentence, consumer-facing description of the pattern.
    pub description: &'static str,
    /// Every metadata key the pattern can carry, with type and presence.
    pub metadata_keys: &'static [MetadataKeySpec],
}

/// The registry: one spec per emitted structural-fact `pattern_id`.
///
/// Tasks 2–4 consume this via the accessor below.
pub fn structural_fact_pattern_specs() -> &'static [StructuralFactPatternSpec] {
    SPECS
}

// ---------------------------------------------------------------------------
// Authoring helpers (compile-time only; keep the SPECS table readable).
// ---------------------------------------------------------------------------

use KeyPresence::{Always as ALWAYS, Optional as OPT};
use MetadataValueType::{
    Bool as BOOL, Number as NUM, ObjectArray as OBJARR, String as STR, StringArray as ARR,
};

const fn key(
    key: &'static str,
    value_type: MetadataValueType,
    presence: KeyPresence,
    description: &'static str,
) -> MetadataKeySpec {
    MetadataKeySpec {
        key,
        value_type,
        presence,
        description,
    }
}

/// `pattern_version` + `query_family`, inserted by every collector's
/// `base_metadata` on every fact.
const K_PATTERN_VERSION: MetadataKeySpec = key(
    "pattern_version",
    NUM,
    ALWAYS,
    "Schema version of this structural-fact pattern (currently 1).",
);
const K_QUERY_FAMILY: MetadataKeySpec = key(
    "query_family",
    STR,
    ALWAYS,
    "Coarse query family the fact belongs to; mirrors the spec's query_family.",
);
/// Explicit `framework` key: a base key for all framework-collector facts, and
/// an emitted key on web route/http facts.
const K_FRAMEWORK: MetadataKeySpec = key(
    "framework",
    STR,
    ALWAYS,
    "Owning framework or HTTP-client label for the fact.",
);

/// Base keys shared by every fact that does not add a `framework` key.
const BASE_KEYS: &[MetadataKeySpec] = &[K_PATTERN_VERSION, K_QUERY_FAMILY];

const SPECS: &[StructuralFactPatternSpec] = &[
    // -----------------------------------------------------------------------
    // Built-in patterns (base/structural_facts.rs). Metadata is base keys only.
    // -----------------------------------------------------------------------
    StructuralFactPatternSpec {
        pattern_id: "rust.unsafe_block.v1",
        languages: &["rust"],
        query_family: "safety",
        description: "A Rust `unsafe { … }` block.",
        metadata_keys: BASE_KEYS,
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
    // -----------------------------------------------------------------------
    // Code collector (base/code_structural_facts.rs).
    // -----------------------------------------------------------------------
    // Java
    StructuralFactPatternSpec {
        pattern_id: "java.synchronized_statement.v1",
        languages: &["java"],
        query_family: "concurrency",
        description: "A Java `synchronized` statement.",
        metadata_keys: BASE_KEYS,
    },
    StructuralFactPatternSpec {
        pattern_id: "java.try_with_resources_statement.v1",
        languages: &["java"],
        query_family: "resources",
        description: "A Java try-with-resources statement.",
        metadata_keys: BASE_KEYS,
    },
    StructuralFactPatternSpec {
        pattern_id: "java.lambda_expression.v1",
        languages: &["java"],
        query_family: "functional",
        description: "A Java lambda expression.",
        metadata_keys: BASE_KEYS,
    },
    StructuralFactPatternSpec {
        pattern_id: "java.marker_annotation.v1",
        languages: &["java"],
        query_family: "metadata",
        description: "A Java marker annotation (no arguments).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "annotation_name",
                STR,
                OPT,
                "The annotation's name identifier.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "java.annotation.v1",
        languages: &["java"],
        query_family: "metadata",
        description: "A Java annotation with arguments.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "annotation_name",
                STR,
                OPT,
                "The annotation's name identifier.",
            ),
        ],
    },
    // Kotlin
    StructuralFactPatternSpec {
        pattern_id: "kotlin.suspend_modifier.v1",
        languages: &["kotlin"],
        query_family: "async",
        description: "A Kotlin `suspend` modifier on a function.",
        metadata_keys: BASE_KEYS,
    },
    StructuralFactPatternSpec {
        pattern_id: "kotlin.property_delegate.v1",
        languages: &["kotlin"],
        query_family: "delegation",
        description: "A Kotlin delegated property (`by`).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "delegate_name",
                STR,
                OPT,
                "The delegate provider backing the property (e.g. lazy).",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "kotlin.annotation.v1",
        languages: &["kotlin"],
        query_family: "metadata",
        description: "A Kotlin annotation use.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "annotation_name",
                STR,
                OPT,
                "The annotation's name identifier.",
            ),
        ],
    },
    // Scala
    StructuralFactPatternSpec {
        pattern_id: "scala.extension_definition.v1",
        languages: &["scala"],
        query_family: "metaprogramming",
        description: "A Scala 3 `extension` definition.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "extended_type",
                STR,
                OPT,
                "The type the extension methods attach to.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "scala.given_definition.v1",
        languages: &["scala"],
        query_family: "typeclass",
        description: "A Scala 3 `given` instance definition.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "given_name",
                STR,
                OPT,
                "The named identifier of the given instance, when named.",
            ),
            key(
                "given_type",
                STR,
                OPT,
                "The declared type of the given instance, used when anonymous.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "scala.for_expression.v1",
        languages: &["scala"],
        query_family: "comprehension",
        description: "A Scala `for` comprehension.",
        metadata_keys: BASE_KEYS,
    },
    StructuralFactPatternSpec {
        pattern_id: "scala.annotation.v1",
        languages: &["scala"],
        query_family: "metadata",
        description: "A Scala annotation use.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "annotation_name",
                STR,
                OPT,
                "The annotation's name identifier.",
            ),
        ],
    },
    // Swift
    StructuralFactPatternSpec {
        pattern_id: "swift.await_expression.v1",
        languages: &["swift"],
        query_family: "async",
        description: "A Swift `await` expression.",
        metadata_keys: BASE_KEYS,
    },
    StructuralFactPatternSpec {
        pattern_id: "swift.actor_declaration.v1",
        languages: &["swift"],
        query_family: "concurrency",
        description: "A Swift `actor` declaration.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("actor_name", STR, OPT, "The declared actor type name."),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "swift.attribute.v1",
        languages: &["swift"],
        query_family: "metadata",
        description: "A Swift attribute (e.g. `@main`, `@objc`).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("attribute_name", STR, OPT, "The Swift attribute name."),
        ],
    },
    // Dart
    StructuralFactPatternSpec {
        pattern_id: "dart.await_expression.v1",
        languages: &["dart"],
        query_family: "async",
        description: "A Dart `await` expression.",
        metadata_keys: BASE_KEYS,
    },
    StructuralFactPatternSpec {
        pattern_id: "dart.async_modifier.v1",
        languages: &["dart"],
        query_family: "async",
        description: "A Dart `async`/`async*`/`sync*` function modifier.",
        metadata_keys: BASE_KEYS,
    },
    StructuralFactPatternSpec {
        pattern_id: "dart.annotation.v1",
        languages: &["dart"],
        query_family: "metadata",
        description: "A Dart annotation use.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "annotation_name",
                STR,
                OPT,
                "The annotation's name identifier.",
            ),
        ],
    },
    // PHP
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
    // Ruby
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
    // Elixir
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
    // Lua
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
    // R
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
    // Zig
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
    // QML
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
    // Bash
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
    // PowerShell
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
    // GDScript
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
    // VB.NET
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
    // -----------------------------------------------------------------------
    // Data collector (base/data_structural_facts.rs).
    // -----------------------------------------------------------------------
    // Markdown
    StructuralFactPatternSpec {
        pattern_id: "markdown.frontmatter.v1",
        languages: &["markdown"],
        query_family: "document_metadata",
        description: "A Markdown frontmatter block (YAML `---` or TOML `+++`).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "format",
                STR,
                ALWAYS,
                "Frontmatter serialization format (\"toml\" or \"yaml\").",
            ),
            key(
                "key_count",
                NUM,
                ALWAYS,
                "Count of non-empty, non-comment frontmatter key lines.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "markdown.heading.v1",
        languages: &["markdown"],
        query_family: "document_structure",
        description: "A Markdown ATX heading.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("level", NUM, ALWAYS, "Heading depth clamped to 1–6."),
            key(
                "text",
                STR,
                ALWAYS,
                "Heading title text with the ATX marker stripped.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "markdown.fenced_code_block.v1",
        languages: &["markdown"],
        query_family: "document_structure",
        description: "A Markdown fenced code block.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "language",
                STR,
                OPT,
                "Fence language token (first word of the info string).",
            ),
            key("info_string", STR, OPT, "Full trimmed fence info string."),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "markdown.inline_link.v1",
        languages: &["markdown"],
        query_family: "document_links",
        description: "A Markdown inline link.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("label", STR, ALWAYS, "Visible link text."),
            key("destination", STR, ALWAYS, "Link target URL/path."),
            key(
                "title",
                STR,
                OPT,
                "Optional link title (only on tree-parsed links, never the regex fallback).",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "markdown.link_definition.v1",
        languages: &["markdown"],
        query_family: "document_links",
        description: "A Markdown link-reference definition.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "label",
                STR,
                ALWAYS,
                "Reference label of the link definition.",
            ),
            key(
                "destination",
                STR,
                ALWAYS,
                "Target URL/path the label resolves to.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "markdown.table.v1",
        languages: &["markdown"],
        query_family: "document_structure",
        description: "A Markdown pipe table.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "row_count",
                NUM,
                ALWAYS,
                "Total table rows including the header row.",
            ),
            key(
                "column_count",
                NUM,
                ALWAYS,
                "Number of columns detected in the table.",
            ),
            key(
                "header_row",
                STR,
                OPT,
                "Trimmed raw text of the header row, when present.",
            ),
        ],
    },
    // JSON
    StructuralFactPatternSpec {
        pattern_id: "json.object.v1",
        languages: &["json"],
        query_family: "data_structure",
        description: "A JSON object node.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "path",
                STR,
                ALWAYS,
                "Dotted/indexed JSON path to this object from the root.",
            ),
            key("depth", NUM, ALWAYS, "Nesting depth of this object."),
            key(
                "property_count",
                NUM,
                ALWAYS,
                "Number of direct properties in the object.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "json.array.v1",
        languages: &["json"],
        query_family: "data_structure",
        description: "A JSON array node.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "path",
                STR,
                ALWAYS,
                "Dotted/indexed JSON path to this array from the root.",
            ),
            key("depth", NUM, ALWAYS, "Nesting depth of this array."),
            key(
                "element_count",
                NUM,
                ALWAYS,
                "Number of elements in the array.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "json.property.v1",
        languages: &["json"],
        query_family: "data_structure",
        description: "A JSON object property.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("key", STR, ALWAYS, "Property key name."),
            key(
                "path",
                STR,
                ALWAYS,
                "Dotted/indexed JSON path to the property's parent.",
            ),
            key(
                "value_kind",
                STR,
                ALWAYS,
                "Normalized kind of the property value.",
            ),
            key("depth", NUM, ALWAYS, "Nesting depth of the property."),
        ],
    },
    // TOML
    StructuralFactPatternSpec {
        pattern_id: "toml.table.v1",
        languages: &["toml"],
        query_family: "config_structure",
        description: "A TOML `[table]` header.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "table_name",
                STR,
                ALWAYS,
                "Declared name of the `[table]` header.",
            ),
            key(
                "key_path",
                STR,
                ALWAYS,
                "Full dotted key path to the table including ancestors.",
            ),
            key(
                "is_array_table",
                BOOL,
                ALWAYS,
                "Always false for standard tables.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "toml.array_table.v1",
        languages: &["toml"],
        query_family: "config_structure",
        description: "A TOML `[[array table]]` element.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "table_name",
                STR,
                ALWAYS,
                "Declared name of the `[[array_table]]` header.",
            ),
            key(
                "key_path",
                STR,
                ALWAYS,
                "Full dotted key path to the array table including ancestors.",
            ),
            key(
                "is_array_table",
                BOOL,
                ALWAYS,
                "Always true, marking an array-of-tables element.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "toml.key_value.v1",
        languages: &["toml"],
        query_family: "config_structure",
        description: "A TOML key/value assignment.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("key", STR, ALWAYS, "The assignment key name."),
            key(
                "key_path",
                STR,
                ALWAYS,
                "Full dotted key path including the enclosing table path.",
            ),
            key(
                "value_kind",
                STR,
                ALWAYS,
                "Normalized kind of the assigned value.",
            ),
            key(
                "is_array_table",
                BOOL,
                ALWAYS,
                "Always false for key/value pairs.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "toml.inline_table.v1",
        languages: &["toml"],
        query_family: "config_structure",
        description: "A TOML inline table value.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "key_path",
                STR,
                ALWAYS,
                "Full dotted key path of the key holding the inline table.",
            ),
            key(
                "entry_count",
                NUM,
                ALWAYS,
                "Number of direct entries in the inline table.",
            ),
            key(
                "is_array_table",
                BOOL,
                ALWAYS,
                "Always false for inline tables.",
            ),
        ],
    },
    // YAML
    StructuralFactPatternSpec {
        pattern_id: "yaml.document.v1",
        languages: &["yaml"],
        query_family: "config_structure",
        description: "A YAML document node.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "has_directives",
                BOOL,
                ALWAYS,
                "Whether the document contains any YAML directive.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "yaml.mapping.v1",
        languages: &["yaml"],
        query_family: "config_structure",
        description: "A YAML block or flow mapping.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "key_path",
                STR,
                ALWAYS,
                "Dotted key path to this mapping from the document root.",
            ),
            key(
                "pair_count",
                NUM,
                ALWAYS,
                "Number of direct key/value pairs in the mapping.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "yaml.sequence.v1",
        languages: &["yaml"],
        query_family: "config_structure",
        description: "A YAML block or flow sequence.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "key_path",
                STR,
                ALWAYS,
                "Dotted key path to this sequence from the document root.",
            ),
            key(
                "sequence_length",
                NUM,
                ALWAYS,
                "Number of items in the sequence.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "yaml.anchor.v1",
        languages: &["yaml"],
        query_family: "config_structure",
        description: "A YAML anchor definition (`&name`).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "anchor_name",
                STR,
                ALWAYS,
                "Declared anchor name (the `&name` token).",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "yaml.alias.v1",
        languages: &["yaml"],
        query_family: "config_structure",
        description: "A YAML alias reference (`*name`).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "alias_target",
                STR,
                ALWAYS,
                "Target anchor name the alias references (the `*name` token).",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "yaml.key_value.v1",
        languages: &["yaml"],
        query_family: "config_structure",
        description: "A YAML mapping key/value pair.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("key", STR, ALWAYS, "The mapping key name."),
            key(
                "key_path",
                STR,
                ALWAYS,
                "Dotted key path including ancestor keys.",
            ),
            key(
                "value_kind",
                STR,
                ALWAYS,
                "Normalized kind of the mapped value.",
            ),
        ],
    },
    // Regex
    StructuralFactPatternSpec {
        pattern_id: "regex.capture_group.v1",
        languages: &["regex"],
        query_family: "pattern_structure",
        description: "A regex anonymous capturing group.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "capture_index",
                NUM,
                ALWAYS,
                "1-based ordinal index of this capturing group.",
            ),
            key(
                "named",
                BOOL,
                ALWAYS,
                "Always false, distinguishing anonymous groups from named ones.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "regex.named_capture.v1",
        languages: &["regex"],
        query_family: "pattern_structure",
        description: "A regex named capturing group.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "capture_name",
                STR,
                ALWAYS,
                "The declared name of the named capture group.",
            ),
            key(
                "capture_index",
                NUM,
                ALWAYS,
                "1-based ordinal index of this capturing group.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "regex.lookaround.v1",
        languages: &["regex"],
        query_family: "pattern_structure",
        description: "A regex lookahead or lookbehind assertion.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("direction", STR, ALWAYS, "\"lookahead\" or \"lookbehind\"."),
            key("polarity", STR, ALWAYS, "\"positive\" or \"negative\"."),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "regex.character_class.v1",
        languages: &["regex"],
        query_family: "pattern_structure",
        description: "A regex character class (`[...]`).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "negated",
                BOOL,
                ALWAYS,
                "Whether the class is negated (starts with `[^`).",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "regex.quantifier.v1",
        languages: &["regex"],
        query_family: "pattern_structure",
        description: "A regex quantifier.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "quantifier",
                STR,
                ALWAYS,
                "Trimmed raw text of the quantifier (e.g. \"*\", \"+\", \"{2,4}\").",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "regex.alternation.v1",
        languages: &["regex"],
        query_family: "pattern_structure",
        description: "A regex alternation (`a|b`).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "branch_count",
                NUM,
                ALWAYS,
                "Number of alternation branches.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "regex.anchor.v1",
        languages: &["regex"],
        query_family: "pattern_structure",
        description: "A regex anchor assertion.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "anchor_kind",
                STR,
                ALWAYS,
                "Classified anchor kind (start/end/word_boundary/…).",
            ),
        ],
    },
    // -----------------------------------------------------------------------
    // SQL collector (base/sql_structural_facts.rs).
    // -----------------------------------------------------------------------
    StructuralFactPatternSpec {
        pattern_id: "sql.table_definition.v1",
        languages: &["sql"],
        query_family: "schema_structure",
        description: "A SQL `CREATE TABLE` definition.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "table_name",
                STR,
                ALWAYS,
                "Name of the table being created.",
            ),
            key(
                "schema_name",
                STR,
                OPT,
                "Schema/namespace qualifier when the table name is qualified.",
            ),
            key(
                "column_count",
                NUM,
                ALWAYS,
                "Count of column definitions in the table body.",
            ),
            key(
                "constraint_count",
                NUM,
                ALWAYS,
                "Count of table-level constraints in the table body.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "sql.view_definition.v1",
        languages: &["sql"],
        query_family: "schema_structure",
        description: "A SQL `CREATE VIEW` definition.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("view_name", STR, ALWAYS, "Name of the view being created."),
            key(
                "schema_name",
                STR,
                OPT,
                "Schema/namespace qualifier when the view name is qualified.",
            ),
            key(
                "source_table_count",
                NUM,
                ALWAYS,
                "Number of distinct source tables in the view query.",
            ),
            key(
                "source_tables",
                ARR,
                ALWAYS,
                "Sorted, deduped list of source table names (may be empty).",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "sql.trigger_definition.v1",
        languages: &["sql"],
        query_family: "schema_structure",
        description: "A SQL `CREATE TRIGGER` definition.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("trigger_name", STR, ALWAYS, "Name of the trigger."),
            key(
                "schema_name",
                STR,
                OPT,
                "Schema qualifier for the trigger (normal parse path only).",
            ),
            key(
                "timing",
                STR,
                OPT,
                "Trigger timing (before/after) when detected.",
            ),
            key(
                "event",
                STR,
                OPT,
                "Trigger event (insert/update/delete/truncate) when detected.",
            ),
            key(
                "target_table",
                STR,
                OPT,
                "Table the trigger fires on when resolvable.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "sql.index_definition.v1",
        languages: &["sql"],
        query_family: "schema_structure",
        description: "A SQL `CREATE INDEX` definition.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("index_name", STR, ALWAYS, "Name of the index."),
            key(
                "table_name",
                STR,
                OPT,
                "Table the index is defined on when resolvable.",
            ),
            key(
                "unique",
                BOOL,
                ALWAYS,
                "Whether the index carries the UNIQUE keyword.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "sql.column_definition.v1",
        languages: &["sql"],
        query_family: "schema_structure",
        description: "A SQL column definition inside a CREATE TABLE.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("column_name", STR, ALWAYS, "Name of the column."),
            key(
                "type_name",
                STR,
                OPT,
                "Declared column data type when resolvable.",
            ),
            key(
                "table_name",
                STR,
                OPT,
                "Enclosing CREATE TABLE name when an ancestor table is found.",
            ),
            key("nullable", BOOL, ALWAYS, "Whether the column allows NULL."),
            key(
                "has_default",
                BOOL,
                ALWAYS,
                "Whether the column has a DEFAULT clause.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "sql.constraint.v1",
        languages: &["sql"],
        query_family: "schema_structure",
        description: "A SQL table or column constraint.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "constraint_type",
                STR,
                ALWAYS,
                "Constraint category (primary_key/foreign_key/unique/check/index).",
            ),
            key(
                "constraint_name",
                STR,
                OPT,
                "Explicit constraint name when present.",
            ),
            key(
                "table_name",
                STR,
                OPT,
                "Enclosing CREATE TABLE name when resolvable.",
            ),
            key(
                "column_names",
                ARR,
                OPT,
                "Constrained column names; omitted when empty.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "sql.foreign_key.v1",
        languages: &["sql"],
        query_family: "schema_structure",
        description: "A SQL foreign-key constraint.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "table_name",
                STR,
                OPT,
                "Local table declaring the foreign key when resolvable.",
            ),
            key(
                "column_names",
                ARR,
                OPT,
                "Local column names participating in the FK; omitted when empty.",
            ),
            key(
                "referenced_table",
                STR,
                ALWAYS,
                "Target table the FK references.",
            ),
            key(
                "referenced_schema",
                STR,
                OPT,
                "Schema qualifier of the referenced table when present.",
            ),
            key(
                "referenced_columns",
                ARR,
                OPT,
                "Referenced column names on the target table; omitted when empty.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "sql.select_query.v1",
        languages: &["sql"],
        query_family: "query_structure",
        description: "A SQL `SELECT` query.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "projection_count",
                NUM,
                ALWAYS,
                "Number of selected projections/terms.",
            ),
            key(
                "source_count",
                NUM,
                ALWAYS,
                "Number of FROM relations plus joins.",
            ),
            key(
                "has_where",
                BOOL,
                ALWAYS,
                "Whether the query has a WHERE clause.",
            ),
            key(
                "has_group_by",
                BOOL,
                ALWAYS,
                "Whether the query has a GROUP BY clause.",
            ),
            key(
                "has_order_by",
                BOOL,
                ALWAYS,
                "Whether the query has an ORDER BY clause.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "sql.cte.v1",
        languages: &["sql"],
        query_family: "query_structure",
        description: "A SQL common table expression (`WITH`).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "cte_name",
                STR,
                ALWAYS,
                "Name of the common table expression.",
            ),
            key(
                "recursive",
                BOOL,
                ALWAYS,
                "Whether the enclosing WITH clause is RECURSIVE.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "sql.join.v1",
        languages: &["sql"],
        query_family: "query_structure",
        description: "A SQL join clause.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "join_type",
                STR,
                ALWAYS,
                "Join kind (inner/left/right/full/cross); defaults to \"inner\".",
            ),
            key(
                "left_table",
                STR,
                OPT,
                "Left-side table from the enclosing FROM when resolvable.",
            ),
            key(
                "right_table",
                STR,
                OPT,
                "Right-side (joined) table when resolvable.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "sql.transaction.v1",
        languages: &["sql"],
        query_family: "transaction_structure",
        description: "A SQL transaction control statement.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "transaction_kind",
                STR,
                ALWAYS,
                "Transaction verb (begin/commit/rollback, else \"transaction\").",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "sql.update_statement.v1",
        languages: &["sql"],
        query_family: "mutation_structure",
        description: "A SQL `UPDATE` statement.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "table_name",
                STR,
                OPT,
                "Target table of the UPDATE when resolvable.",
            ),
            key(
                "has_where",
                BOOL,
                ALWAYS,
                "Whether the UPDATE has a WHERE clause.",
            ),
        ],
    },
    // -----------------------------------------------------------------------
    // Framework collector (base/framework_structural_facts.rs).
    // These facts additionally carry a `framework` base key.
    // -----------------------------------------------------------------------
    StructuralFactPatternSpec {
        pattern_id: "aspnet.minimal_api.route.v1",
        languages: &["csharp"],
        query_family: "framework",
        description: "An ASP.NET Core minimal-API endpoint route (MapGet/MapPost/…).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "api_style",
                STR,
                ALWAYS,
                "Routing style; \"minimal_api\" for Map* endpoint calls.",
            ),
            key(
                "verb",
                STR,
                ALWAYS,
                "HTTP method derived from the Map* call (GET/POST/PUT/PATCH/DELETE).",
            ),
            key(
                "route_template",
                STR,
                ALWAYS,
                "Raw route string passed as the first Map* argument.",
            ),
            key(
                "normalized_route_template",
                STR,
                ALWAYS,
                "Cross-family join key with a leading slash and ASP.NET route parameters converted to :param form.",
            ),
            key(
                "route_source",
                STR,
                ALWAYS,
                "Origin of the parsed route literal (\"string_literal\").",
            ),
            key(
                "route_group_prefix",
                STR,
                OPT,
                "Prefix contributed by an enclosing MapGroup, when found.",
            ),
            key(
                "effective_route_template",
                STR,
                OPT,
                "Group prefix joined with the route template, when a prefix is found.",
            ),
            key(
                "route_group_source",
                STR,
                OPT,
                "How the group prefix was resolved (\"map_group\"), when found.",
            ),
            key(
                "handler_kind",
                STR,
                OPT,
                "Handler expression shape (\"lambda\" or \"method_group\"), when parsed.",
            ),
            key(
                "handler_name",
                STR,
                OPT,
                "Dotted identifier path of a method-group handler.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "aspnet.minimal_api.route_group.v1",
        languages: &["csharp"],
        query_family: "framework",
        description: "An ASP.NET Core minimal-API `MapGroup` route group.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key("api_style", STR, ALWAYS, "Routing style (\"minimal_api\")."),
            key(
                "route_prefix",
                STR,
                ALWAYS,
                "Route prefix string passed to MapGroup(...).",
            ),
            key(
                "normalized_route_template",
                STR,
                ALWAYS,
                "Cross-family join key for the group prefix with ASP.NET route parameters converted to :param form.",
            ),
            key(
                "route_source",
                STR,
                ALWAYS,
                "Origin of the parsed prefix literal (\"string_literal\").",
            ),
            key(
                "source_kind",
                STR,
                ALWAYS,
                "Construct that produced the group (\"map_group\").",
            ),
            key(
                "group_variable",
                STR,
                OPT,
                "Variable the MapGroup result is assigned to, for linking child routes.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "aspnet.attribute_route.v1",
        languages: &["csharp"],
        query_family: "framework",
        description: "An ASP.NET Core attribute-routing fact ([Route]/[Http*] on controllers or actions).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "api_style",
                STR,
                ALWAYS,
                "Routing style (\"attribute_routing\").",
            ),
            key(
                "attribute_kind",
                STR,
                ALWAYS,
                "Fact kind (\"controller_route\", \"http_method\", or \"route\").",
            ),
            key(
                "route_template",
                STR,
                OPT,
                "Raw template from the attribute's string-literal argument.",
            ),
            key(
                "effective_route_template",
                STR,
                OPT,
                "Controller+method template joined and token-substituted.",
            ),
            key(
                "normalized_route_template",
                STR,
                OPT,
                "Cross-family join key computed from the effective route template when present, else the raw route template.",
            ),
            key(
                "route_tokens",
                ARR,
                OPT,
                "Tokens actually substituted (\"controller\"/\"action\").",
            ),
            key(
                "verb",
                STR,
                OPT,
                "HTTP method for Http* verb attributes; absent on plain [Route] facts.",
            ),
            key(
                "controller_route_template",
                STR,
                OPT,
                "Owning controller's [Route] template attached to a method fact.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "express.route.v1",
        languages: &["javascript", "jsx", "typescript", "tsx"],
        query_family: "framework",
        description: "A static Express route registration on an in-file traced app/router receiver.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "api_style",
                STR,
                ALWAYS,
                "Routing style (\"call_routing\").",
            ),
            key(
                "route_template",
                STR,
                ALWAYS,
                "Raw static route path passed to the Express registration call.",
            ),
            key(
                "normalized_route_template",
                STR,
                ALWAYS,
                "Cross-family join key with a leading slash and Express :param segments preserved.",
            ),
            key(
                "dynamic_segments",
                ARR,
                OPT,
                "Route parameter names discovered in the normalized template.",
            ),
            key(
                "verb",
                STR,
                OPT,
                "HTTP method for verb-restricted registrations; omitted for app.all.",
            ),
            key(
                "verb_source",
                STR,
                OPT,
                "How the verb was attested (\"attested\" for Express verb methods).",
            ),
            key(
                "route_group_prefix",
                STR,
                OPT,
                "Same-file app.use mount prefix resolved onto a router route.",
            ),
            key(
                "effective_route_template",
                STR,
                OPT,
                "Mount prefix joined with the route template when same-file resolution applies.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "express.router_mount.v1",
        languages: &["javascript", "jsx", "typescript", "tsx"],
        query_family: "framework",
        description: "A static Express app.use/router.use mount point with a literal mount path.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "mount_path",
                STR,
                ALWAYS,
                "Raw static mount path passed as the first app.use argument.",
            ),
            key(
                "normalized_mount_path",
                STR,
                ALWAYS,
                "Cross-family normalized mount path.",
            ),
            key(
                "mount_target",
                STR,
                ALWAYS,
                "Source text of the mounted router or middleware expression.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "fastify.route.v1",
        languages: &["javascript", "jsx", "typescript", "tsx"],
        query_family: "framework",
        description: "A static Fastify shorthand or object-form route registration.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "api_style",
                STR,
                ALWAYS,
                "Routing style (\"call_routing\").",
            ),
            key(
                "route_template",
                STR,
                ALWAYS,
                "Raw static route path passed to the Fastify registration.",
            ),
            key(
                "normalized_route_template",
                STR,
                ALWAYS,
                "Cross-family join key with Fastify :param segments preserved.",
            ),
            key(
                "dynamic_segments",
                ARR,
                OPT,
                "Route parameter names discovered in the normalized template.",
            ),
            key(
                "verb",
                STR,
                OPT,
                "HTTP method for verb-restricted registrations; omitted for all-method registrations.",
            ),
            key(
                "verb_source",
                STR,
                OPT,
                "How the verb was attested (\"attested\" for Fastify method names or method properties).",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "nestjs.route.v1",
        languages: &["javascript", "typescript"],
        query_family: "framework",
        description: "A static NestJS HTTP-method decorator route joined to its @Controller class prefix.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "api_style",
                STR,
                ALWAYS,
                "Routing style (\"decorator_routing\").",
            ),
            key(
                "route_template",
                STR,
                ALWAYS,
                "Raw static path from the method HTTP decorator; empty for a bare @Get().",
            ),
            key(
                "normalized_route_template",
                STR,
                ALWAYS,
                "Cross-family join key with a leading slash and NestJS :param segments preserved.",
            ),
            key(
                "dynamic_segments",
                ARR,
                OPT,
                "Route parameter names discovered in the normalized template.",
            ),
            key(
                "class_route_template",
                STR,
                OPT,
                "Static @Controller class prefix (string, { path }, or array element).",
            ),
            key(
                "effective_route_template",
                STR,
                OPT,
                "Class @Controller prefix joined with the method sub-path when a static prefix applies.",
            ),
            key(
                "verb",
                STR,
                OPT,
                "Uppercase HTTP method; omitted for @All (accepts any method).",
            ),
            key(
                "verb_source",
                STR,
                OPT,
                "How the verb was attested (\"attested\" from the decorator name).",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "fastapi.route.v1",
        languages: &["python"],
        query_family: "framework",
        description: "A static FastAPI path-operation decorator on a traced FastAPI/APIRouter receiver.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "api_style",
                STR,
                ALWAYS,
                "Routing style (\"decorator_routing\").",
            ),
            key(
                "route_template",
                STR,
                ALWAYS,
                "Raw static route path from the decorator.",
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
            key("verb", STR, ALWAYS, "Uppercase HTTP method."),
            key("verb_source", STR, ALWAYS, "How the verb was attested."),
            key("router_prefix", STR, OPT, "Same-file APIRouter prefix."),
            key(
                "effective_route_template",
                STR,
                OPT,
                "Router prefix joined with route template.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "fastapi.include_router.v1",
        languages: &["python"],
        query_family: "framework",
        description: "A FastAPI include_router mount call.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "mount_target",
                STR,
                ALWAYS,
                "Source text of the included router argument.",
            ),
            key(
                "mount_path",
                STR,
                OPT,
                "Literal prefix mount path, when present.",
            ),
            key(
                "normalized_mount_path",
                STR,
                OPT,
                "Normalized mount path, when a literal prefix is present.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "flask.route.v1",
        languages: &["python"],
        query_family: "framework",
        description: "A static Flask route decorator on a traced Flask/Blueprint receiver.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "api_style",
                STR,
                ALWAYS,
                "Routing style (\"decorator_routing\").",
            ),
            key(
                "route_template",
                STR,
                ALWAYS,
                "Raw static route path from the decorator.",
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
            key("verb", STR, ALWAYS, "Uppercase HTTP method."),
            key(
                "verb_source",
                STR,
                ALWAYS,
                "Default or attested verb source.",
            ),
            key(
                "blueprint",
                STR,
                OPT,
                "Blueprint name literal for blueprint-owned routes.",
            ),
            key("url_prefix", STR, OPT, "Same-file Blueprint url_prefix."),
            key(
                "effective_route_template",
                STR,
                OPT,
                "Blueprint prefix joined with route template.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "flask.blueprint_registration.v1",
        languages: &["python"],
        query_family: "framework",
        description: "A Flask register_blueprint mount call.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "mount_target",
                STR,
                ALWAYS,
                "Source text of the blueprint argument.",
            ),
            key(
                "mount_path",
                STR,
                OPT,
                "Literal url_prefix mount path, when present.",
            ),
            key(
                "normalized_mount_path",
                STR,
                OPT,
                "Normalized mount path, when a literal url_prefix is present.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "django.url_pattern.v1",
        languages: &["python"],
        query_family: "framework",
        description: "A Django path/re_path URL pattern with a static route argument.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key("api_style", STR, ALWAYS, "Routing style (\"dsl_routing\")."),
            key("route_template", STR, ALWAYS, "Raw static route string."),
            key("route_syntax", STR, ALWAYS, "\"path\" or \"regex\"."),
            key(
                "normalized_route_template",
                STR,
                OPT,
                "Normalized route template for path syntax.",
            ),
            key(
                "dynamic_segments",
                ARR,
                OPT,
                "Route parameter names discovered in path syntax.",
            ),
            key("route_name", STR, OPT, "Literal name= value."),
            key(
                "view_target",
                STR,
                ALWAYS,
                "Source text of the view argument.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "django.url_include.v1",
        languages: &["python"],
        query_family: "framework",
        description: "A Django include mount inside a path() URL pattern.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key("mount_path", STR, ALWAYS, "Raw path() prefix string."),
            key(
                "normalized_mount_path",
                STR,
                ALWAYS,
                "Normalized mount path.",
            ),
            key(
                "included_module",
                STR,
                ALWAYS,
                "Included module literal or source text.",
            ),
            key("namespace", STR, OPT, "Literal namespace= value."),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "spring.request_mapping.v1",
        languages: &["java", "kotlin"],
        query_family: "framework",
        description: "A Spring MVC request-mapping annotation on a class or method.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "api_style",
                STR,
                ALWAYS,
                "Routing style (\"annotation_routing\").",
            ),
            key(
                "attribute_kind",
                STR,
                ALWAYS,
                "class_route/http_method/request_mapping shape.",
            ),
            key("route_template", STR, ALWAYS, "Raw static route template."),
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
                "class_route_template",
                STR,
                OPT,
                "Nearest class-level route template.",
            ),
            key(
                "effective_route_template",
                STR,
                OPT,
                "Class and method templates joined.",
            ),
            key(
                "verb",
                STR,
                OPT,
                "Uppercase HTTP method when verb-restricted.",
            ),
            key("verb_source", STR, OPT, "How the verb was attested."),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "go.net_http.route.v1",
        languages: &["go"],
        query_family: "framework",
        description: "A Go net/http Handle or HandleFunc route registration.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key("api_style", STR, ALWAYS, "Routing style (\"mux_routing\")."),
            key(
                "route_template",
                STR,
                ALWAYS,
                "Route path portion of the pattern.",
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
                "host",
                STR,
                OPT,
                "Host portion when the Go 1.22+ pattern is host-scoped.",
            ),
            key(
                "verb",
                STR,
                OPT,
                "Method prefix when the Go pattern names one.",
            ),
            key("verb_source", STR, OPT, "How the verb was attested."),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "gin.route.v1",
        languages: &["go"],
        query_family: "framework",
        description: "A gin route registration on a traced router or group receiver.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "api_style",
                STR,
                ALWAYS,
                "Routing style (\"call_routing\").",
            ),
            key("route_template", STR, ALWAYS, "Raw static route path."),
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
            key("route_group_prefix", STR, OPT, "Same-file group prefix."),
            key(
                "effective_route_template",
                STR,
                OPT,
                "Group prefix joined with route template.",
            ),
            key("verb", STR, OPT, "Uppercase HTTP method."),
            key("verb_source", STR, OPT, "How the verb was attested."),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "echo.route.v1",
        languages: &["go"],
        query_family: "framework",
        description: "An echo route registration on a traced Echo or group receiver.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "api_style",
                STR,
                ALWAYS,
                "Routing style (\"call_routing\").",
            ),
            key("route_template", STR, ALWAYS, "Raw static route path."),
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
            key("route_group_prefix", STR, OPT, "Same-file group prefix."),
            key(
                "effective_route_template",
                STR,
                OPT,
                "Group prefix joined with route template.",
            ),
            key("verb", STR, OPT, "Uppercase HTTP method."),
            key("verb_source", STR, OPT, "How the verb was attested."),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "rails.route.v1",
        languages: &["ruby"],
        query_family: "framework",
        description: "A Rails routes DSL handler route.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key("api_style", STR, ALWAYS, "Routing style (\"dsl_routing\")."),
            key(
                "route_template",
                STR,
                ALWAYS,
                "Raw route path from the DSL call.",
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
            key("scope_path", STR, OPT, "Enclosing namespace/scope path."),
            key(
                "effective_route_template",
                STR,
                OPT,
                "Scope path joined with route template.",
            ),
            key(
                "verb",
                STR,
                OPT,
                "Uppercase HTTP method when verb-restricted.",
            ),
            key("verb_source", STR, OPT, "How the verb was attested."),
            key(
                "controller_action",
                STR,
                OPT,
                "Literal controller#action target.",
            ),
            key("route_name", STR, OPT, "Literal/as-symbol route name."),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "rails.resource_route.v1",
        languages: &["ruby"],
        query_family: "framework",
        description: "A Rails resources/resource declaration.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key("api_style", STR, ALWAYS, "Routing style (\"dsl_routing\")."),
            key("resource_name", STR, ALWAYS, "Declared resource name."),
            key("resource_kind", STR, ALWAYS, "collection or singular."),
            key("only", ARR, OPT, "Literal only: action list."),
            key("except", ARR, OPT, "Literal except: action list."),
            key("scope_path", STR, OPT, "Enclosing namespace/scope path."),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "rails.mount.v1",
        languages: &["ruby"],
        query_family: "framework",
        description: "A Rails mount route for a Rack app or engine.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key("mount_path", STR, ALWAYS, "Raw mount path literal."),
            key(
                "normalized_mount_path",
                STR,
                ALWAYS,
                "Normalized mount path including same-file scope.",
            ),
            key(
                "mount_target",
                STR,
                ALWAYS,
                "Source text of mounted app/engine.",
            ),
            key("scope_path", STR, OPT, "Enclosing namespace/scope path."),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "laravel.route.v1",
        languages: &["php"],
        query_family: "framework",
        description: "A static Laravel Route facade route joined to its same-file group prefix.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "api_style",
                STR,
                ALWAYS,
                "Routing style (\"call_routing\").",
            ),
            key(
                "route_template",
                STR,
                ALWAYS,
                "Raw static route path from the Route facade call.",
            ),
            key(
                "normalized_route_template",
                STR,
                ALWAYS,
                "Cross-family join key with a leading slash and Laravel {param}/{param?} segments preserved as :param.",
            ),
            key(
                "dynamic_segments",
                ARR,
                OPT,
                "Route parameter names discovered in the normalized template.",
            ),
            key(
                "route_group_prefix",
                STR,
                OPT,
                "Same-file Route::prefix()/group(['prefix'=>...]) prefix governing the route.",
            ),
            key(
                "effective_route_template",
                STR,
                OPT,
                "Group prefix joined with the route template when a static prefix applies.",
            ),
            key(
                "verb",
                STR,
                OPT,
                "Uppercase HTTP method; omitted for Route::any (accepts any method).",
            ),
            key(
                "verb_source",
                STR,
                OPT,
                "How the verb was attested (\"attested\").",
            ),
            key(
                "controller_action",
                STR,
                OPT,
                "Controller action target (\"Ctrl@method\" or the literal action string) when statically resolvable.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "laravel.resource_route.v1",
        languages: &["php"],
        query_family: "framework",
        description: "A Laravel Route::resource/apiResource declaration.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "api_style",
                STR,
                ALWAYS,
                "Routing style (\"resource_routing\").",
            ),
            key(
                "resource_name",
                STR,
                ALWAYS,
                "Raw static resource URI literal.",
            ),
            key(
                "resource_kind",
                STR,
                ALWAYS,
                "resource (7 RESTful actions) or api_resource (5, no create/edit).",
            ),
            key(
                "controller",
                STR,
                OPT,
                "Controller class name when a static Ctrl::class reference is given.",
            ),
            key(
                "route_group_prefix",
                STR,
                OPT,
                "Same-file group prefix governing the resource.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "laravel.route_prefix.v1",
        languages: &["php"],
        query_family: "framework",
        description: "A static Laravel Route::prefix()/group(['prefix'=>...]) prefix at its definition site.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "mount_path",
                STR,
                ALWAYS,
                "Raw static prefix literal at this site.",
            ),
            key(
                "normalized_mount_path",
                STR,
                ALWAYS,
                "Normalized prefix path including enclosing same-file group scope.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "phoenix.route.v1",
        languages: &["elixir"],
        query_family: "framework",
        description: "A static Phoenix router verb-macro route joined to its same-file scope prefix.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key("api_style", STR, ALWAYS, "Routing style (\"dsl_routing\")."),
            key(
                "route_template",
                STR,
                ALWAYS,
                "Raw static route path from the router verb macro.",
            ),
            key(
                "normalized_route_template",
                STR,
                ALWAYS,
                "Cross-family join key with a leading slash and Phoenix :param segments preserved.",
            ),
            key(
                "dynamic_segments",
                ARR,
                OPT,
                "Route parameter names discovered in the normalized template.",
            ),
            key(
                "route_group_prefix",
                STR,
                OPT,
                "Same-file enclosing scope prefix governing the route.",
            ),
            key(
                "effective_route_template",
                STR,
                OPT,
                "Scope prefix joined with the route template when a static prefix applies.",
            ),
            key(
                "verb",
                STR,
                OPT,
                "Uppercase HTTP method (every Phoenix verb macro is verb-restricted).",
            ),
            key(
                "verb_source",
                STR,
                OPT,
                "How the verb was attested (\"attested\").",
            ),
            key(
                "controller",
                STR,
                OPT,
                "Controller/plug module alias as written at the route.",
            ),
            key(
                "action",
                STR,
                OPT,
                "Controller action atom name (`:show` recorded as show).",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "phoenix.resource_route.v1",
        languages: &["elixir"],
        query_family: "framework",
        description: "A Phoenix router `resources \"/x\", Ctrl` RESTful resource declaration.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "api_style",
                STR,
                ALWAYS,
                "Routing style (\"resource_routing\").",
            ),
            key(
                "resource_path",
                STR,
                ALWAYS,
                "Raw static resource URI literal.",
            ),
            key(
                "normalized_resource_path",
                STR,
                ALWAYS,
                "Normalized resource path including same-file scope prefix.",
            ),
            key(
                "controller",
                STR,
                OPT,
                "Controller module alias when statically resolvable.",
            ),
            key(
                "route_group_prefix",
                STR,
                OPT,
                "Same-file enclosing scope prefix governing the resource.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "phoenix.forward.v1",
        languages: &["elixir"],
        query_family: "framework",
        description: "A static Phoenix router `forward \"/lit\", Plug` prefix registration at its definition site.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "mount_path",
                STR,
                ALWAYS,
                "Raw static forward path literal at this site.",
            ),
            key(
                "normalized_mount_path",
                STR,
                ALWAYS,
                "Normalized forward path including enclosing same-file scope prefix.",
            ),
            key("mount_target", STR, ALWAYS, "Forwarded plug module alias."),
            key(
                "route_group_prefix",
                STR,
                OPT,
                "Same-file enclosing scope prefix governing the forward.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "axum.route.v1",
        languages: &["rust"],
        query_family: "framework",
        description: "A static axum `Router::new().route(\"/x\", get(h))` route, one fact per method-router verb.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "api_style",
                STR,
                ALWAYS,
                "Routing style (\"call_routing\").",
            ),
            key(
                "route_template",
                STR,
                ALWAYS,
                "Raw static route path passed to `.route`.",
            ),
            key(
                "normalized_route_template",
                STR,
                ALWAYS,
                "Cross-family join key; axum 0.8 `{id}` brace captures normalize to `:id` segments.",
            ),
            key(
                "dynamic_segments",
                ARR,
                OPT,
                "Route parameter names discovered in the normalized template (axum 0.8 brace captures; a 0.7 `:id` template is an honest under-report and yields none).",
            ),
            key(
                "verb",
                STR,
                OPT,
                "Uppercase HTTP method for a verb-restricted method router; omitted for `any`/`any_service`.",
            ),
            key(
                "verb_source",
                STR,
                OPT,
                "How the verb was attested (\"attested\"); omitted with the verb for `any`/`any_service`.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "axum.nest.v1",
        languages: &["rust"],
        query_family: "framework",
        description: "A static axum `Router::new().nest(\"/lit\", sub_router)` prefix registration at its definition site.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "mount_path",
                STR,
                ALWAYS,
                "Raw static nest path literal at this site.",
            ),
            key(
                "normalized_mount_path",
                STR,
                ALWAYS,
                "Normalized nest path (axum 0.8 brace captures preserved as `:param`).",
            ),
            key(
                "mount_target",
                STR,
                ALWAYS,
                "Source text of the nested sub-router expression (a cross-file target; no route join is guessed).",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "actix.attribute_route.v1",
        languages: &["rust"],
        query_family: "framework",
        description: "A static actix-web attribute-macro route (`#[get(\"/x\")]` / `#[route(\"/x\", method = \"GET\")]`) on a handler fn, one fact per verb.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key("api_style", STR, ALWAYS, "Routing style (\"attribute\")."),
            key(
                "route_template",
                STR,
                ALWAYS,
                "Raw static route path from the attribute macro's first argument.",
            ),
            key(
                "normalized_route_template",
                STR,
                ALWAYS,
                "Cross-family join key; actix `{id}` brace captures normalize to `:id` segments.",
            ),
            key(
                "dynamic_segments",
                ARR,
                OPT,
                "Route parameter names discovered in the normalized template.",
            ),
            key(
                "verb",
                STR,
                ALWAYS,
                "Uppercase HTTP method from the macro name (`#[get]`→GET) or a `method = \"VERB\"` argument (`#[route]`).",
            ),
            key(
                "verb_source",
                STR,
                ALWAYS,
                "How the verb was attested (\"attested\"; attribute-macro verbs are always explicit).",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "actix.scope_route.v1",
        languages: &["rust"],
        query_family: "framework",
        description: "A static actix-web scope-chained route (`web::scope(\"/api\").route(\"/x\", web::post().to(h))`) with a same-file scope prefix.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "api_style",
                STR,
                ALWAYS,
                "Routing style (\"call_routing\").",
            ),
            key(
                "route_template",
                STR,
                ALWAYS,
                "Raw static route path passed to `.route` (without the scope prefix).",
            ),
            key(
                "normalized_route_template",
                STR,
                ALWAYS,
                "Cross-family join key computed from the effective (scope prefix + route) template; actix `{id}` brace captures normalize to `:id`.",
            ),
            key(
                "dynamic_segments",
                ARR,
                OPT,
                "Route parameter names discovered in the normalized template.",
            ),
            key(
                "verb",
                STR,
                OPT,
                "Uppercase HTTP method from the `web::<verb>()` method router; omitted for the method-agnostic `web::route()`.",
            ),
            key(
                "verb_source",
                STR,
                OPT,
                "How the verb was attested (\"attested\"); omitted with the verb for `web::route()`.",
            ),
            key(
                "route_group_prefix",
                STR,
                ALWAYS,
                "Same-file `web::scope(\"/lit\")` prefix the route chains off (scope routes are always scoped).",
            ),
            key(
                "effective_route_template",
                STR,
                ALWAYS,
                "Scope prefix joined with the route template.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "actix.mount.v1",
        languages: &["rust"],
        query_family: "framework",
        description: "A static actix-web `web::scope(\"/lit\").configure(fn)` / `.service(sub)` mount, the scope prefix recorded at its registration site.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "mount_path",
                STR,
                ALWAYS,
                "Raw static scope path literal at this site.",
            ),
            key(
                "normalized_mount_path",
                STR,
                ALWAYS,
                "Normalized scope path (actix brace captures preserved as `:param`).",
            ),
            key(
                "mount_target",
                STR,
                ALWAYS,
                "Source text of the `configure`/`service` target (a cross-file target; no route join is guessed).",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "htmx.attribute.v1",
        languages: &["html", "razor", "javascript", "jsx", "tsx", "vue"],
        query_family: "frontend_interaction",
        description: "An htmx attribute (hx-* or data-hx-*).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "attribute_name",
                STR,
                ALWAYS,
                "Canonical htmx attribute name (normalized to hx-* form).",
            ),
            key(
                "data_prefix",
                BOOL,
                OPT,
                "Present and true only when the data-hx-* form was used.",
            ),
            key(
                "attribute_value",
                STR,
                OPT,
                "Raw attribute value, when the attribute has a value.",
            ),
            key(
                "verb",
                STR,
                OPT,
                "HTTP method for request attributes (hx-get/post/…); absent otherwise.",
            ),
            key(
                "target_path",
                STR,
                OPT,
                "Static request path from the attribute value, when applicable.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "alpine.directive.v1",
        languages: &["html", "razor"],
        query_family: "frontend_interaction",
        description: "An Alpine.js directive (x-*, @, or :).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "directive",
                STR,
                ALWAYS,
                "Canonical Alpine directive name (e.g. \"x-on\", \"x-bind\").",
            ),
            key(
                "argument",
                STR,
                OPT,
                "Directive argument after the colon (e.g. event name), when present.",
            ),
            key(
                "modifiers",
                ARR,
                OPT,
                "Dot-modifiers (e.g. [\"prevent\", \"stop\"]); omitted when empty.",
            ),
            key(
                "expression",
                STR,
                OPT,
                "The directive's value/expression, when present.",
            ),
            key(
                "shorthand",
                BOOL,
                ALWAYS,
                "True when the shorthand form (@… or :…) was used.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "razor.page_directive.v1",
        languages: &["razor"],
        query_family: "component_routing",
        description: "A Razor `@page` directive.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key("directive", STR, ALWAYS, "Directive kind (\"page\")."),
            key(
                "route",
                STR,
                ALWAYS,
                "Route string from the @page directive.",
            ),
            key(
                "route_template",
                STR,
                ALWAYS,
                "Same route value under the template key, for aspnet consistency.",
            ),
            key(
                "normalized_route_template",
                STR,
                ALWAYS,
                "Route template normalized for HTTP-boundary joins.",
            ),
            key(
                "route_parameter_count",
                NUM,
                ALWAYS,
                "Count of {param} segments parsed from the route.",
            ),
            key(
                "has_route_constraints",
                BOOL,
                ALWAYS,
                "True when any route parameter carries a :constraint.",
            ),
            key(
                "route_parameters",
                OBJARR,
                ALWAYS,
                "Parsed route parameters as a JSON array of objects (empty when the \
                 route has none). Each object carries `name` (String), `optional` \
                 (Bool), and `catch_all` (Bool) always, plus `constraint` (String) \
                 only when the {param:constraint} form is used.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "razor.code_block.v1",
        languages: &["razor"],
        query_family: "component_code",
        description: "A Razor `@code`/`@functions` block.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "block_type",
                STR,
                ALWAYS,
                "Razor block type (\"code\" or \"functions\").",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "razor.template_expression.v1",
        languages: &["razor"],
        query_family: "component_template",
        description: "A Razor template expression (`@expr` or `@(expr)`).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "expression",
                STR,
                ALWAYS,
                "The Razor expression text with the leading @ stripped.",
            ),
            key(
                "implicit",
                BOOL,
                ALWAYS,
                "True for implicit expressions (vs explicit @(...)).",
            ),
        ],
    },
    // -----------------------------------------------------------------------
    // Web collector (base/web_structural_facts/).
    // Route/http facts additionally carry a `framework` key (emitted directly,
    // not via base_metadata).
    // -----------------------------------------------------------------------
    // CSS
    StructuralFactPatternSpec {
        pattern_id: "css.selector_rule.v1",
        languages: &["css"],
        query_family: "stylesheet_structure",
        description: "A CSS selector rule set.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "selector",
                STR,
                ALWAYS,
                "Raw CSS selector text of the rule set.",
            ),
            key(
                "selector_kind",
                STR,
                ALWAYS,
                "Coarse selector classification (class/id/pseudo/selector_list/compound).",
            ),
            key(
                "declaration_count",
                NUM,
                ALWAYS,
                "Count of declarations inside the rule block.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "css.custom_property.v1",
        languages: &["css"],
        query_family: "stylesheet_structure",
        description: "A CSS custom property declaration (`--name`).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "property_name",
                STR,
                ALWAYS,
                "The `--*` CSS custom property name.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "css.media_query.v1",
        languages: &["css"],
        query_family: "responsive_design",
        description: "A CSS `@media` query.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "query",
                STR,
                OPT,
                "The `@media` prelude/condition text, when present.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "css.keyframes.v1",
        languages: &["css"],
        query_family: "animation",
        description: "A CSS `@keyframes` animation.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "animation_name",
                STR,
                OPT,
                "The `@keyframes` animation name, when present.",
            ),
        ],
    },
    // HTML
    StructuralFactPatternSpec {
        pattern_id: "html.link.v1",
        languages: &["html"],
        query_family: "document_navigation",
        description: "An HTML anchor (`<a href>`) link.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("tag_name", STR, ALWAYS, "The element tag name (\"a\")."),
            key("href", STR, ALWAYS, "Link target from the href attribute."),
            key("id", STR, OPT, "Element id attribute, when present."),
            key("class", STR, OPT, "Element class attribute, when present."),
            key("rel", STR, OPT, "Link rel attribute, when present."),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "html.script.v1",
        languages: &["html"],
        query_family: "document_assets",
        description: "An HTML `<script>` element.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "tag_name",
                STR,
                ALWAYS,
                "The element tag name (\"script\").",
            ),
            key(
                "inline",
                BOOL,
                ALWAYS,
                "True when the script has no src (inline body).",
            ),
            key("src", STR, OPT, "External script src, when present."),
            key("type", STR, OPT, "Script type attribute, when present."),
            key("id", STR, OPT, "Element id attribute, when present."),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "html.form.v1",
        languages: &["html"],
        query_family: "document_forms",
        description: "An HTML `<form>` element.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("tag_name", STR, ALWAYS, "The element tag name (\"form\")."),
            key("action", STR, OPT, "Raw action attribute, when present."),
            key(
                "method",
                STR,
                ALWAYS,
                "Normalized lowercase HTTP method, defaulting to \"get\".",
            ),
            key(
                "method_source",
                STR,
                ALWAYS,
                "Whether method was \"explicit\" on the tag or \"default\".",
            ),
            key(
                "action_kind",
                STR,
                ALWAYS,
                "Action classification (static_path/other/same_document).",
            ),
            key(
                "target_path",
                STR,
                OPT,
                "The action value when it is a static path.",
            ),
            key("id", STR, OPT, "Form id attribute, when present."),
            key("name", STR, OPT, "Form name attribute, when present."),
            key("enctype", STR, OPT, "Form enctype attribute, when present."),
            key("target", STR, OPT, "Form target attribute, when present."),
            key(
                "autocomplete",
                STR,
                OPT,
                "Form autocomplete attribute, when present.",
            ),
            key(
                "novalidate",
                BOOL,
                ALWAYS,
                "True when the novalidate attribute is present.",
            ),
            key(
                "control_count",
                NUM,
                ALWAYS,
                "Number of descendant form controls.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "html.form_control.v1",
        languages: &["html"],
        query_family: "document_forms",
        description: "An HTML form control (input/button/select/textarea).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "tag_name",
                STR,
                ALWAYS,
                "Control tag name (input/button/select/textarea).",
            ),
            key("type", STR, OPT, "Control type attribute, when present."),
            key("name", STR, OPT, "Control name attribute, when present."),
            key("id", STR, OPT, "Control id attribute, when present."),
            key("value", STR, OPT, "Control value attribute, when present."),
            key(
                "required",
                BOOL,
                ALWAYS,
                "True when the required attribute is present.",
            ),
            key(
                "disabled",
                BOOL,
                OPT,
                "Present (true) only when the disabled attribute exists.",
            ),
            key(
                "readonly",
                BOOL,
                OPT,
                "Present (true) only when the readonly attribute exists.",
            ),
            key(
                "checked",
                BOOL,
                OPT,
                "Present (true) only when the checked attribute exists.",
            ),
            key(
                "multiple",
                BOOL,
                OPT,
                "Present (true) only when the multiple attribute exists.",
            ),
            key(
                "form_id",
                STR,
                OPT,
                "Owning form's id, when an owner form is resolved.",
            ),
            key(
                "form_name",
                STR,
                OPT,
                "Owning form's name, when an owner form is resolved.",
            ),
            key(
                "form_action",
                STR,
                OPT,
                "Owning form's action, when an owner form is resolved.",
            ),
            key(
                "form_method",
                STR,
                OPT,
                "Owning form's normalized method, when an owner form is resolved.",
            ),
        ],
    },
    // Vue
    StructuralFactPatternSpec {
        pattern_id: "vue.sfc_section.v1",
        languages: &["vue"],
        query_family: "component_structure",
        description: "A Vue single-file-component section (template/script/style).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "section_type",
                STR,
                ALWAYS,
                "SFC section kind (template/script/style).",
            ),
            key(
                "lang",
                STR,
                OPT,
                "The block's lang attribute (e.g. ts, scss), when present.",
            ),
            key(
                "setup",
                BOOL,
                ALWAYS,
                "True when the block carries the setup attribute.",
            ),
            key(
                "scoped",
                BOOL,
                ALWAYS,
                "True when the block carries the scoped attribute.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "vue.template_directive.v1",
        languages: &["vue"],
        query_family: "component_template",
        description: "A Vue template directive (v-bind/v-on/v-if/…).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "directive",
                STR,
                ALWAYS,
                "Canonical directive name (v-bind, v-on, v-if, v-model, …).",
            ),
            key(
                "attribute_name",
                STR,
                ALWAYS,
                "Raw attribute name as written (e.g. `:to`, `@click`).",
            ),
            key(
                "shorthand",
                BOOL,
                ALWAYS,
                "True when written in shorthand (`:`/`@`).",
            ),
            key(
                "argument",
                STR,
                OPT,
                "Directive argument (e.g. the event name for v-on), when present.",
            ),
            key(
                "modifiers",
                ARR,
                OPT,
                "Directive modifiers list; omitted when empty.",
            ),
            key(
                "expression",
                STR,
                OPT,
                "The attribute's bound expression/value, when present.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "vue.route_reference.v1",
        languages: &["vue"],
        query_family: "frontend_navigation",
        description: "A Vue Router navigation reference (router-link or navigation call).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "source_kind",
                STR,
                ALWAYS,
                "Reference origin (router_link or router_navigation_expression).",
            ),
            key(
                "target_path",
                STR,
                ALWAYS,
                "Static route path being navigated to.",
            ),
            key(
                "verb",
                STR,
                ALWAYS,
                "HTTP method for the navigation (always \"GET\").",
            ),
            key(
                "attribute_name",
                STR,
                ALWAYS,
                "The source attribute name (e.g. `to`, `@click`).",
            ),
            key(
                "expression",
                STR,
                OPT,
                "Original bound expression when the path came from a binding.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "vue.route_definition.v1",
        languages: &["vue", "javascript", "jsx", "tsx", "typescript"],
        query_family: "frontend_navigation",
        description: "A Vue Router route definition object.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "target_path",
                STR,
                ALWAYS,
                "The route's own static path string.",
            ),
            key(
                "source_kind",
                STR,
                ALWAYS,
                "Definition origin (\"vue_router_route\").",
            ),
            key(
                "route_source",
                STR,
                ALWAYS,
                "Origin of the parsed path literal (\"string_literal\").",
            ),
            key(
                "parent_route_path",
                STR,
                OPT,
                "Parent route path when nested under another route object.",
            ),
            key(
                "effective_route_template",
                STR,
                OPT,
                "Parent+own path joined into the full route template.",
            ),
            key(
                "route_name",
                STR,
                OPT,
                "Named-route `name` property, when present.",
            ),
            key(
                "component_name",
                STR,
                OPT,
                "Identifier of the mapped component, when present.",
            ),
            key(
                "component_path",
                STR,
                OPT,
                "Import path resolved for the component identifier, when present.",
            ),
        ],
    },
    // React
    StructuralFactPatternSpec {
        pattern_id: "react.route_reference.v1",
        languages: &["javascript", "jsx", "tsx"],
        query_family: "frontend_navigation",
        description: "A React Router link reference (`<Link to>`).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "library",
                STR,
                ALWAYS,
                "Routing library (\"react_router\").",
            ),
            key(
                "target_path",
                STR,
                ALWAYS,
                "Static route path from the `to` attribute.",
            ),
            key(
                "attribute_name",
                STR,
                ALWAYS,
                "The source attribute name (\"to\").",
            ),
            key(
                "component_name",
                STR,
                ALWAYS,
                "The JSX component/tag name (e.g. Link).",
            ),
            key(
                "import_source",
                STR,
                ALWAYS,
                "Module the link component was imported from.",
            ),
            key(
                "route_source",
                STR,
                ALWAYS,
                "Origin of the parsed path literal (\"string_literal\").",
            ),
            key(
                "source_kind",
                STR,
                ALWAYS,
                "Reference origin (\"react_router_link\").",
            ),
            key(
                "verb",
                STR,
                ALWAYS,
                "HTTP method for the navigation (always \"GET\").",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "react.route_definition.v1",
        languages: &["javascript", "jsx", "tsx", "typescript"],
        query_family: "frontend_navigation",
        description: "A React Router route definition (JSX <Route> or route object).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "library",
                STR,
                ALWAYS,
                "Routing library (\"react_router\").",
            ),
            key(
                "source_kind",
                STR,
                ALWAYS,
                "Definition form (jsx_route or route_object).",
            ),
            key(
                "route_path",
                STR,
                OPT,
                "The route's own static path (absent for index routes).",
            ),
            key(
                "route_source",
                STR,
                ALWAYS,
                "\"string_literal\" when a path exists, else \"index_route\".",
            ),
            key(
                "index_route",
                BOOL,
                OPT,
                "Present (true) only for index routes.",
            ),
            key(
                "route_component",
                STR,
                OPT,
                "Mapped component identifier, when present.",
            ),
            key(
                "route_id",
                STR,
                OPT,
                "Route object `id` property, when present.",
            ),
            key(
                "parent_route_path",
                STR,
                OPT,
                "Parent route path when nested.",
            ),
            key(
                "effective_route_template",
                STR,
                OPT,
                "Full joined route template from parent + own path.",
            ),
        ],
    },
    // Next.js
    StructuralFactPatternSpec {
        pattern_id: "nextjs.route_reference.v1",
        languages: &["javascript", "jsx", "tsx"],
        query_family: "frontend_navigation",
        description: "A Next.js `<Link href>` navigation reference.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "target_path",
                STR,
                ALWAYS,
                "Static route path from the href.",
            ),
            key(
                "attribute_name",
                STR,
                ALWAYS,
                "The source attribute name (\"href\").",
            ),
            key(
                "component_name",
                STR,
                ALWAYS,
                "The Link component/tag name.",
            ),
            key(
                "import_source",
                STR,
                ALWAYS,
                "Module the link component was imported from.",
            ),
            key(
                "route_source",
                STR,
                ALWAYS,
                "Origin of the parsed path (\"string_literal\" or \"object_pathname_literal\").",
            ),
            key(
                "source_kind",
                STR,
                ALWAYS,
                "Reference origin (\"next_link\").",
            ),
            key(
                "verb",
                STR,
                ALWAYS,
                "HTTP method for the navigation (always \"GET\").",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "nextjs.file_route.v1",
        languages: &["javascript", "jsx", "tsx", "typescript"],
        query_family: "frontend_navigation",
        description: "A Next.js file-based page route.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "router",
                STR,
                ALWAYS,
                "Router family (\"app\" or \"pages\").",
            ),
            key(
                "file_convention",
                STR,
                ALWAYS,
                "File convention (\"page\").",
            ),
            key(
                "route_path",
                STR,
                ALWAYS,
                "Route path derived from the file path.",
            ),
            key(
                "source_kind",
                STR,
                ALWAYS,
                "Route origin (\"nextjs_file_route\").",
            ),
            key(
                "normalized_route_template",
                STR,
                OPT,
                "Path with dynamic segments normalized, when dynamic.",
            ),
            key(
                "dynamic_segments",
                ARR,
                OPT,
                "Dynamic segment names; omitted when empty.",
            ),
            key(
                "route_group_segments",
                ARR,
                OPT,
                "App Router `(group)` segment names; omitted when empty.",
            ),
            key(
                "parallel_route_segments",
                ARR,
                OPT,
                "App Router `@slot` parallel-route names; omitted when empty.",
            ),
            key(
                "intercepting_route_markers",
                ARR,
                OPT,
                "Intercepting-route markers ((.)/(..)/…); omitted when empty.",
            ),
            key(
                "intercepted_route_segments",
                ARR,
                OPT,
                "Segments targeted by intercepting routes; omitted when empty.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "nextjs.route_handler.v1",
        languages: &["javascript", "typescript"],
        query_family: "framework",
        description: "A Next.js App Router route handler export (one fact per HTTP verb).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key("router", STR, ALWAYS, "Router family (App Router \"app\")."),
            key(
                "file_convention",
                STR,
                ALWAYS,
                "File convention (\"route\").",
            ),
            key(
                "route_path",
                STR,
                ALWAYS,
                "Route path derived from the file path.",
            ),
            key(
                "source_kind",
                STR,
                ALWAYS,
                "Route origin (\"nextjs_route_handler\").",
            ),
            key("verb", STR, ALWAYS, "Exported HTTP verb (GET/POST/…)."),
            key(
                "verb_source",
                STR,
                ALWAYS,
                "How the verb was attested (\"attested\").",
            ),
            key(
                "normalized_route_template",
                STR,
                OPT,
                "Dynamic-normalized route template, when dynamic.",
            ),
            key(
                "dynamic_segments",
                ARR,
                OPT,
                "Dynamic segment names; omitted when empty.",
            ),
            key(
                "route_group_segments",
                ARR,
                OPT,
                "App Router group segment names; omitted when empty.",
            ),
            key(
                "parallel_route_segments",
                ARR,
                OPT,
                "App Router parallel-route names; omitted when empty.",
            ),
            key(
                "intercepting_route_markers",
                ARR,
                OPT,
                "Intercepting-route markers; omitted when empty.",
            ),
            key(
                "intercepted_route_segments",
                ARR,
                OPT,
                "Intercepted segment names; omitted when empty.",
            ),
        ],
    },
    // Nuxt
    StructuralFactPatternSpec {
        pattern_id: "nuxt.route_reference.v1",
        languages: &["vue"],
        query_family: "frontend_navigation",
        description: "A Nuxt `<NuxtLink to>` navigation reference.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "target_path",
                STR,
                ALWAYS,
                "Static route path from the `to` attribute.",
            ),
            key(
                "verb",
                STR,
                ALWAYS,
                "HTTP method for the navigation (always \"GET\").",
            ),
            key(
                "attribute_name",
                STR,
                ALWAYS,
                "The source attribute name (\"to\").",
            ),
            key("component_name", STR, ALWAYS, "The NuxtLink tag name."),
            key(
                "route_source",
                STR,
                ALWAYS,
                "Origin of the parsed path literal (\"string_literal\").",
            ),
            key(
                "source_kind",
                STR,
                ALWAYS,
                "Reference origin (\"nuxt_link\").",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "nuxt.file_route.v1",
        languages: &["vue", "javascript", "jsx", "tsx", "typescript"],
        query_family: "frontend_navigation",
        description: "A Nuxt file-based page route.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key("router", STR, ALWAYS, "Router family (\"pages\")."),
            key(
                "file_convention",
                STR,
                ALWAYS,
                "File convention (\"page\").",
            ),
            key(
                "route_path",
                STR,
                ALWAYS,
                "Route path derived from the file path.",
            ),
            key(
                "source_kind",
                STR,
                ALWAYS,
                "Route origin (\"nuxt_file_route\").",
            ),
            key(
                "normalized_route_template",
                STR,
                OPT,
                "Dynamic-normalized route template, when dynamic.",
            ),
            key(
                "dynamic_segments",
                ARR,
                OPT,
                "Dynamic segment names; omitted when empty.",
            ),
            key(
                "route_group_segments",
                ARR,
                OPT,
                "Group segment names; omitted when empty.",
            ),
            key(
                "parallel_route_segments",
                ARR,
                OPT,
                "Parallel-route slot names; omitted when empty.",
            ),
            key(
                "intercepting_route_markers",
                ARR,
                OPT,
                "Intercepting-route markers; omitted when empty.",
            ),
            key(
                "intercepted_route_segments",
                ARR,
                OPT,
                "Intercepted segment names; omitted when empty.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "nuxt.server_route.v1",
        languages: &["javascript", "typescript"],
        query_family: "framework",
        description: "A Nuxt server route (server/api handler).",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key("router", STR, ALWAYS, "Router family (\"server\")."),
            key(
                "route_path",
                STR,
                ALWAYS,
                "Server route path derived from the file path.",
            ),
            key(
                "source_kind",
                STR,
                ALWAYS,
                "Route origin (\"nuxt_server_route\").",
            ),
            key(
                "normalized_route_template",
                STR,
                OPT,
                "Dynamic-normalized route template, when dynamic.",
            ),
            key(
                "dynamic_segments",
                ARR,
                OPT,
                "Dynamic segment names; omitted when empty.",
            ),
            key(
                "verb",
                STR,
                OPT,
                "HTTP method from a filename suffix (.get/.post/…), when present.",
            ),
            key(
                "verb_source",
                STR,
                OPT,
                "How the verb was attested (\"attested\"); inserted only alongside verb.",
            ),
        ],
    },
    // HTTP client
    StructuralFactPatternSpec {
        pattern_id: "http.client_request.v1",
        languages: &[
            "vue",
            "javascript",
            "jsx",
            "tsx",
            "typescript",
            "python",
            "csharp",
            "go",
            "java",
            "kotlin",
            "php",
            "ruby",
            "elixir",
            "rust",
        ],
        query_family: "web.http_client",
        description: "An outbound HTTP client request with a static URL literal.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "client",
                STR,
                ALWAYS,
                "HTTP client label (for example fetch, axios, requests, httpx, httpclient, net/http, java.net.http, ktor, net::http).",
            ),
            key(
                "target_path",
                STR,
                ALWAYS,
                "Static string URL/path of the request.",
            ),
            key(
                "url_kind",
                STR,
                ALWAYS,
                "URL classification (path/absolute/relative).",
            ),
            key(
                "verb",
                STR,
                ALWAYS,
                "Uppercase HTTP method for the request.",
            ),
            key(
                "verb_source",
                STR,
                ALWAYS,
                "\"attested\" (explicit) or \"default\" (spec GET).",
            ),
            key(
                "import_source",
                STR,
                OPT,
                "Import/module source when the collector has one.",
            ),
        ],
    },
];

// ---------------------------------------------------------------------------
// JSON serialization: the checked-in contract artifact and the
// `languages --json` report section share this one serializer, so the file and
// the report stay byte-equivalent in content.
// ---------------------------------------------------------------------------

/// Stable lower_snake token a `MetadataValueType` serializes to in the JSON
/// contract. This mapping is itself a contract: renames are lead-adjudicated.
fn value_type_token(value_type: MetadataValueType) -> &'static str {
    match value_type {
        MetadataValueType::String => "string",
        MetadataValueType::Bool => "bool",
        MetadataValueType::Number => "number",
        MetadataValueType::StringArray => "string_array",
        MetadataValueType::ObjectArray => "object_array",
    }
}

/// Stable lower_snake token a `KeyPresence` serializes to in the JSON contract.
fn presence_token(presence: KeyPresence) -> &'static str {
    match presence {
        KeyPresence::Always => "always",
        KeyPresence::Optional => "optional",
    }
}

/// The structural-fact pattern registry serialized as a deterministic JSON
/// array — the machine-readable, source-of-truth metadata-payload contract.
///
/// Determinism: specs are sorted by `pattern_id` (unique, so a total order),
/// and every object emits its keys in a fixed order matching the Rust struct
/// fields. Spec objects emit `pattern_id`, `languages`, `query_family`,
/// `description`, `metadata_keys`; each metadata-key object emits `key`,
/// `value_type`, `presence`, `description`. A pattern's `languages` and
/// `metadata_keys` keep their authored order (both already fixed and unique in
/// the registry). Insertion order survives because serde_json's
/// `preserve_order` feature is active in this workspace's build graph; the
/// checked-in-file sync test (`tests/structural_fact_registry.rs`) is the
/// tripwire if that ever regresses.
///
/// This is the single serializer behind both
/// `docs/contracts/structural-fact-patterns.json` (Task 3) and the
/// `structural_fact_patterns` section of `languages --json` (Task 4).
pub fn structural_fact_patterns_json() -> serde_json::Value {
    let mut specs: Vec<&StructuralFactPatternSpec> =
        structural_fact_pattern_specs().iter().collect();
    specs.sort_by(|a, b| a.pattern_id.cmp(b.pattern_id));

    let specs_json: Vec<serde_json::Value> = specs
        .into_iter()
        .map(|spec| {
            let metadata_keys: Vec<serde_json::Value> = spec
                .metadata_keys
                .iter()
                .map(|meta| {
                    let mut obj = serde_json::Map::new();
                    obj.insert("key".to_string(), meta.key.into());
                    obj.insert(
                        "value_type".to_string(),
                        value_type_token(meta.value_type).into(),
                    );
                    obj.insert("presence".to_string(), presence_token(meta.presence).into());
                    obj.insert("description".to_string(), meta.description.into());
                    serde_json::Value::Object(obj)
                })
                .collect();

            let languages: Vec<serde_json::Value> =
                spec.languages.iter().map(|lang| (*lang).into()).collect();

            let mut obj = serde_json::Map::new();
            obj.insert("pattern_id".to_string(), spec.pattern_id.into());
            obj.insert("languages".to_string(), serde_json::Value::Array(languages));
            obj.insert("query_family".to_string(), spec.query_family.into());
            obj.insert("description".to_string(), spec.description.into());
            obj.insert(
                "metadata_keys".to_string(),
                serde_json::Value::Array(metadata_keys),
            );
            serde_json::Value::Object(obj)
        })
        .collect();

    serde_json::Value::Array(specs_json)
}

/// Exact byte contents of `docs/contracts/structural-fact-patterns.json`:
/// [`structural_fact_patterns_json`] pretty-printed with 2-space indent and a
/// trailing newline (repo JSON convention). Both the sync test's comparison and
/// its regeneration path use this one function, so they can never diverge on
/// formatting.
pub fn structural_fact_patterns_contract_json() -> String {
    let mut rendered = serde_json::to_string_pretty(&structural_fact_patterns_json())
        .expect("structural-fact registry is always JSON-serializable");
    rendered.push('\n');
    rendered
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn pattern_ids_are_unique() {
        let mut seen = HashSet::new();
        for spec in structural_fact_pattern_specs() {
            assert!(
                seen.insert(spec.pattern_id),
                "duplicate pattern_id in registry: {}",
                spec.pattern_id
            );
        }
    }

    #[test]
    fn every_spec_has_nonempty_languages() {
        for spec in structural_fact_pattern_specs() {
            assert!(
                !spec.languages.is_empty(),
                "{} declares no languages",
                spec.pattern_id
            );
            for language in spec.languages {
                assert!(
                    !language.is_empty(),
                    "{} declares an empty language string",
                    spec.pattern_id
                );
            }
        }
    }

    #[test]
    fn every_spec_has_nonempty_description_and_query_family() {
        for spec in structural_fact_pattern_specs() {
            assert!(
                !spec.description.trim().is_empty(),
                "{} has an empty description",
                spec.pattern_id
            );
            assert!(
                !spec.query_family.trim().is_empty(),
                "{} has an empty query_family",
                spec.pattern_id
            );
        }
    }

    #[test]
    fn languages_within_a_spec_are_unique() {
        for spec in structural_fact_pattern_specs() {
            let mut seen = HashSet::new();
            for language in spec.languages {
                assert!(
                    seen.insert(*language),
                    "{} lists language {} more than once",
                    spec.pattern_id,
                    language
                );
            }
        }
    }

    #[test]
    fn metadata_keys_are_well_formed_and_unique() {
        for spec in structural_fact_pattern_specs() {
            let mut seen = HashSet::new();
            for meta in spec.metadata_keys {
                assert!(
                    !meta.key.trim().is_empty(),
                    "{} has a metadata key with an empty name",
                    spec.pattern_id
                );
                assert!(
                    !meta.description.trim().is_empty(),
                    "{} metadata key {} has an empty description",
                    spec.pattern_id,
                    meta.key
                );
                assert!(
                    seen.insert(meta.key),
                    "{} declares metadata key {} more than once",
                    spec.pattern_id,
                    meta.key
                );
            }
        }
    }

    #[test]
    fn every_spec_declares_base_metadata_keys() {
        // `pattern_version` and `query_family` are inserted by every collector's
        // `base_metadata`, so they must be declared (as Always) on every spec.
        for spec in structural_fact_pattern_specs() {
            for base in ["pattern_version", "query_family"] {
                let declared = spec
                    .metadata_keys
                    .iter()
                    .find(|meta| meta.key == base)
                    .unwrap_or_else(|| {
                        panic!("{} is missing base metadata key {}", spec.pattern_id, base)
                    });
                assert_eq!(
                    declared.presence,
                    KeyPresence::Always,
                    "{} declares base key {} as non-Always",
                    spec.pattern_id,
                    base
                );
            }
        }
    }

    #[test]
    fn framework_key_type_is_string_when_present() {
        for spec in structural_fact_pattern_specs() {
            if let Some(meta) = spec
                .metadata_keys
                .iter()
                .find(|meta| meta.key == "framework")
            {
                assert_eq!(
                    meta.value_type,
                    MetadataValueType::String,
                    "{} declares framework with a non-String type",
                    spec.pattern_id
                );
                assert_eq!(
                    meta.presence,
                    KeyPresence::Always,
                    "{} declares framework as non-Always",
                    spec.pattern_id
                );
            }
        }
    }

    /// Primary invariant: the registry's per-language pattern-id set must equal
    /// the authoritative union the extractor actually emits for that language
    /// (`structural_fact_pattern_ids_for_language`, which unions the built-in
    /// patterns and all five base collectors).
    ///
    /// That authority — like the collectors' own `*_pattern_ids_for_language`
    /// helpers — is compiled only under the `test-capability-matrix` feature, so
    /// this invariant is gated to match. Run it with:
    ///   `cargo test -p julie-extractors --features test-capability-matrix \
    ///        structural_fact_registry`.
    #[cfg(feature = "test-capability-matrix")]
    #[test]
    fn registry_pattern_ids_match_emitted_union_per_language() {
        use crate::base::structural_facts::structural_fact_pattern_ids_for_language;
        use std::collections::BTreeSet;

        // Every language any source emits for. Kept in sync with the collector
        // match arms; unioned with the registry's own languages so a spec that
        // introduces a new language is still checked.
        const KNOWN_LANGUAGES: &[&str] = &[
            // built-in patterns (base/structural_facts.rs)
            "c",
            "cpp",
            "go",
            "javascript",
            "jsx",
            "python",
            "rust",
            "tsx",
            "typescript",
            // code collector
            "dart",
            "elixir",
            "java",
            "kotlin",
            "lua",
            "php",
            "r",
            "ruby",
            "scala",
            "swift",
            "bash",
            "gdscript",
            "powershell",
            "qml",
            "vbnet",
            "zig",
            // data collector
            "markdown",
            "json",
            "toml",
            "yaml",
            "regex", //
            // sql collector
            "sql", //
            // framework + web collectors
            "csharp",
            "html",
            "razor",
            "vue",
            "css",
        ];

        let mut languages: BTreeSet<&str> = KNOWN_LANGUAGES.iter().copied().collect();
        for spec in structural_fact_pattern_specs() {
            languages.extend(spec.languages.iter().copied());
        }

        let mut errors = Vec::new();
        let mut union_from_emission: BTreeSet<String> = BTreeSet::new();
        for language in &languages {
            let registry: BTreeSet<String> = structural_fact_pattern_specs()
                .iter()
                .filter(|spec| spec.languages.contains(language))
                .map(|spec| spec.pattern_id.to_string())
                .collect();
            let emitted: BTreeSet<String> = structural_fact_pattern_ids_for_language(language)
                .into_iter()
                .map(str::to_string)
                .collect();
            union_from_emission.extend(emitted.iter().cloned());
            if registry != emitted {
                let missing: Vec<&String> = emitted.difference(&registry).collect();
                let extra: Vec<&String> = registry.difference(&emitted).collect();
                errors.push(format!(
                    "language `{language}` mismatch: missing_from_registry={missing:?} not_emitted={extra:?}"
                ));
            }
        }

        // Global completeness: no registry pattern is dead (never emitted for any
        // known language), and no emitted pattern is unregistered.
        let all_registry: BTreeSet<String> = structural_fact_pattern_specs()
            .iter()
            .map(|spec| spec.pattern_id.to_string())
            .collect();
        for dead in all_registry.difference(&union_from_emission) {
            errors.push(format!(
                "registry pattern `{dead}` is not emitted for any known language"
            ));
        }

        assert!(errors.is_empty(), "{}", errors.join("\n"));
    }
}
