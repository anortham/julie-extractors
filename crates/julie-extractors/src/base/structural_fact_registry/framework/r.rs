//! plumber and Shiny framework SPECS for R.
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
        pattern_id: "plumber.route.v1",
        languages: &["r"],
        query_family: "framework",
        description: "A plumber route: a `#* @get /path` annotation above a top-level handler function, or a `pr_get(\"/path\", handler)` router call, with a static path.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "api_style",
                STR,
                ALWAYS,
                "\"annotation\" or \"router_call\".",
            ),
            key("route_template", STR, ALWAYS, "Raw static route path."),
            key(
                "normalized_route_template",
                STR,
                ALWAYS,
                "Cross-family normalized route template; `<id:int>` parameters lose their type.",
            ),
            key(
                "dynamic_segments",
                ARR,
                OPT,
                "Route parameter names discovered in the normalized template.",
            ),
            key("verb", STR, ALWAYS, "Uppercase HTTP method."),
            key("verb_source", STR, ALWAYS, "How the verb was attested."),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "shiny.input.v1",
        languages: &["r"],
        query_family: "framework",
        description: "A Shiny input widget (`*Input()`, `actionButton()`, `actionLink()`, `radioButtons()`) with a static input id, in a file that mentions shiny.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "input_id",
                STR,
                ALWAYS,
                "The input id (`input$<id>` on the server).",
            ),
            key("widget", STR, ALWAYS, "The widget function name."),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "shiny.output.v1",
        languages: &["r"],
        query_family: "framework",
        description: "A Shiny output: a `*Output(\"id\")` placeholder in the UI, or an `output$id <- render*()` binding on the server.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key("role", STR, ALWAYS, "\"placeholder\" or \"render\"."),
            key("output_id", STR, ALWAYS, "The output id."),
            key(
                "function",
                STR,
                ALWAYS,
                "The placeholder or render function name.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "shiny.reactive.v1",
        languages: &["r"],
        query_family: "framework",
        description: "A Shiny reactive or observer: `reactive()`, `eventReactive()`, `reactiveVal()`, `reactiveValues()`, `reactivePoll()`, `reactiveFileReader()`, `observe()`, or `observeEvent()`.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key("reactive_kind", STR, ALWAYS, "The reactive function name."),
            key("name", STR, OPT, "The name the reactive is assigned to."),
            key(
                "trigger",
                STR,
                OPT,
                "The event expression of `observeEvent()` / `eventReactive()`.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "shiny.module.v1",
        languages: &["r"],
        query_family: "framework",
        description: "A Shiny module server call: `moduleServer(id, server)` or `callModule(server, id)`.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key(
                "module_call",
                STR,
                ALWAYS,
                "\"moduleServer\" or \"callModule\".",
            ),
            key("module_id", STR, OPT, "Static module id."),
            key(
                "server_function",
                STR,
                OPT,
                "The module server function when it is named.",
            ),
        ],
    },
    StructuralFactPatternSpec {
        pattern_id: "shiny.app.v1",
        languages: &["r"],
        query_family: "framework",
        description: "A `shinyApp()` call that builds the application.",
        metadata_keys: &[
            K_PATTERN_VERSION,
            K_QUERY_FAMILY,
            K_FRAMEWORK,
            key("ui", STR, OPT, "The UI object when it is a named variable."),
            key(
                "server",
                STR,
                OPT,
                "The server function when it is a named variable.",
            ),
        ],
    },
];
