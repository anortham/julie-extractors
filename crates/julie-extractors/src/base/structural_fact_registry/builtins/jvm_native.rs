//! Built-in language-local SPECS for Java, Kotlin, Scala, Swift, and Dart.
//!
//! Authored metadata for [`super::super::StructuralFactPatternSpec`] entries.
//! Public registry access remains through
//! [`super::super::structural_fact_pattern_specs`].

use super::super::{
    BASE_KEYS, K_PATTERN_VERSION, K_QUERY_FAMILY, OPT, STR, StructuralFactPatternSpec, key,
};

pub(super) const SPECS: &[StructuralFactPatternSpec] = &[
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
];
