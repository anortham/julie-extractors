//! Structural-fact pattern SPECS for the `http_client` registry family.
//!
//! Authored metadata for [`super::StructuralFactPatternSpec`] entries. Public
//! registry access remains through [`super::structural_fact_pattern_specs`].

use super::{
    ALWAYS, K_FRAMEWORK, K_PATTERN_VERSION, K_QUERY_FAMILY, OPT, STR, StructuralFactPatternSpec,
    key,
};

pub(super) const SPECS: &[StructuralFactPatternSpec] = &[
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
            "razor",
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
                "HTTP client label (for example fetch, axios, requests, httpx, httpclient, net/http, java.net.http, net::http, reqwest, hyper, ureq, guzzle, laravel_http, symfony_http_client, curl, ktor, okhttp, retrofit, spring_webclient, spring_resttemplate, req, tesla, httpoison, finch, httpc).",
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
