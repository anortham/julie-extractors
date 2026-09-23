//! Vapor route and SwiftPM manifest SPECS.
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
        pattern_id: "vapor.route.v1",
        languages: &["swift"],
        query_family: "framework",
        description: "A Vapor route-builder verb call with static path components and a handler, joined to its same-file `grouped`/`group` prefix, in a file that imports Vapor.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "api_style",
                STR,
                ALWAYS,
                "Routing style (\"route_builder\").",
            ),
            key(
                "route_template",
                STR,
                ALWAYS,
                "The call's own static path components joined with `/` behind a leading slash.",
            ),
            key(
                "effective_route_template",
                STR,
                OPT,
                "The same-file `grouped(...)`/`group(...)` prefix joined with the route template, when a prefix applies.",
            ),
            key(
                "normalized_route_template",
                STR,
                ALWAYS,
                "Cross-family join key with a leading slash and Vapor `:param` segments preserved.",
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
                "Uppercase HTTP method from the verb method name or the `on(.VERB, ...)` argument.",
            ),
            key(
                "verb_source",
                STR,
                ALWAYS,
                "How the verb was attested (\"attested\").",
            ),
            key(
                "handler",
                STR,
                OPT,
                "Handler expression passed as `use:`; absent for a trailing closure.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "swiftpm.package.v1",
        languages: &["swift"],
        query_family: "dependencies",
        description: "The `Package(name:)` declaration of a SwiftPM `Package.swift` manifest.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key("name", STR, ALWAYS, "Package name."),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "swiftpm.product.v1",
        languages: &["swift"],
        query_family: "dependencies",
        description: "A product (`.library`, `.executable`, or `.plugin`) declared in a SwiftPM `Package.swift` manifest.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "product_kind",
                STR,
                ALWAYS,
                "Product factory name (\"library\", \"executable\", or \"plugin\").",
            ),
            key("name", STR, ALWAYS, "Product name."),
            key(
                "targets",
                ARR,
                OPT,
                "Static target names the product exports.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "swiftpm.target.v1",
        languages: &["swift"],
        query_family: "dependencies",
        description: "A target (`.target`, `.testTarget`, `.executableTarget`, `.macro`, `.plugin`, `.systemLibrary`, or `.binaryTarget`) declared in a SwiftPM `Package.swift` manifest.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            key(
                "target_kind",
                STR,
                ALWAYS,
                "Target factory name (for example \"target\" or \"testTarget\").",
            ),
            key("name", STR, ALWAYS, "Target name."),
            key("path", STR, OPT, "Static `path:` argument."),
            key(
                "dependencies",
                ARR,
                OPT,
                "Static dependency names: a bare string, `.target(name:)`, `.byName(name:)`, or `.product(name:)`.",
            ),
        ],
    },
];
