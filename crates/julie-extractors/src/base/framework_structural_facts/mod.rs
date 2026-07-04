mod aspnet;
mod go_http;
mod helpers;
mod http_clients;
mod kotlin_spring;
mod laravel;
mod markup;
mod nestjs;
mod node;
mod phoenix;
mod python_web;
mod rails;
mod razor;
mod scan;
mod spring;
mod static_arg;

use tree_sitter::Tree;

use self::aspnet::{collect_aspnet_attribute_routes, collect_aspnet_minimal_api_routes};
use self::go_http::collect_go_http_boundary_facts;
use self::http_clients::collect_backend_http_client_requests;
use self::kotlin_spring::collect_kotlin_spring_routes;
use self::laravel::collect_laravel_routes;
use self::markup::{
    collect_jsx_htmx_attributes, collect_markup_framework_attributes,
    collect_vue_template_htmx_attributes,
};
use self::nestjs::collect_nestjs_route_facts;
use self::node::collect_node_http_boundary_facts;
use self::phoenix::collect_phoenix_routes;
use self::python_web::collect_python_web_facts;
use self::rails::collect_rails_routes;
use self::razor::collect_razor_structural_facts;
use self::spring::collect_spring_request_mappings;
use super::attach_containing_symbols;
use super::structural_facts::sort_structural_facts;
use super::types::{StructuralFact, Symbol};

pub(super) const ASPNET_MINIMAL_API_ROUTE_PATTERN_ID: &str = "aspnet.minimal_api.route.v1";
pub(super) const ASPNET_MINIMAL_API_ROUTE_GROUP_PATTERN_ID: &str =
    "aspnet.minimal_api.route_group.v1";
pub(super) const ASPNET_ATTRIBUTE_ROUTE_PATTERN_ID: &str = "aspnet.attribute_route.v1";
pub(super) const EXPRESS_ROUTE_PATTERN_ID: &str = "express.route.v1";
pub(super) const EXPRESS_ROUTER_MOUNT_PATTERN_ID: &str = "express.router_mount.v1";
pub(super) const FASTIFY_ROUTE_PATTERN_ID: &str = "fastify.route.v1";
pub(super) const NESTJS_ROUTE_PATTERN_ID: &str = "nestjs.route.v1";
pub(super) const FASTAPI_ROUTE_PATTERN_ID: &str = "fastapi.route.v1";
pub(super) const FASTAPI_INCLUDE_ROUTER_PATTERN_ID: &str = "fastapi.include_router.v1";
pub(super) const FLASK_ROUTE_PATTERN_ID: &str = "flask.route.v1";
pub(super) const FLASK_BLUEPRINT_REGISTRATION_PATTERN_ID: &str = "flask.blueprint_registration.v1";
pub(super) const DJANGO_URL_PATTERN_ID: &str = "django.url_pattern.v1";
pub(super) const DJANGO_URL_INCLUDE_PATTERN_ID: &str = "django.url_include.v1";
pub(super) const SPRING_REQUEST_MAPPING_PATTERN_ID: &str = "spring.request_mapping.v1";
pub(super) const GO_NET_HTTP_ROUTE_PATTERN_ID: &str = "go.net_http.route.v1";
pub(super) const GIN_ROUTE_PATTERN_ID: &str = "gin.route.v1";
pub(super) const ECHO_ROUTE_PATTERN_ID: &str = "echo.route.v1";
pub(super) const RAILS_ROUTE_PATTERN_ID: &str = "rails.route.v1";
pub(super) const RAILS_RESOURCE_ROUTE_PATTERN_ID: &str = "rails.resource_route.v1";
pub(super) const RAILS_MOUNT_PATTERN_ID: &str = "rails.mount.v1";
pub(super) const LARAVEL_ROUTE_PATTERN_ID: &str = "laravel.route.v1";
pub(super) const LARAVEL_RESOURCE_ROUTE_PATTERN_ID: &str = "laravel.resource_route.v1";
pub(super) const LARAVEL_ROUTE_PREFIX_PATTERN_ID: &str = "laravel.route_prefix.v1";
pub(super) const PHOENIX_ROUTE_PATTERN_ID: &str = "phoenix.route.v1";
pub(super) const PHOENIX_RESOURCE_ROUTE_PATTERN_ID: &str = "phoenix.resource_route.v1";
pub(super) const PHOENIX_FORWARD_PATTERN_ID: &str = "phoenix.forward.v1";
pub(super) const HTTP_CLIENT_REQUEST_PATTERN_ID: &str = "http.client_request.v1";
pub(super) const HTMX_ATTRIBUTE_PATTERN_ID: &str = "htmx.attribute.v1";
pub(super) const ALPINE_DIRECTIVE_PATTERN_ID: &str = "alpine.directive.v1";
pub(super) const RAZOR_PAGE_DIRECTIVE_PATTERN_ID: &str = "razor.page_directive.v1";
pub(super) const RAZOR_CODE_BLOCK_PATTERN_ID: &str = "razor.code_block.v1";
pub(super) const RAZOR_TEMPLATE_EXPRESSION_PATTERN_ID: &str = "razor.template_expression.v1";

#[cfg(all(test, feature = "test-capability-matrix"))]
const CSHARP_FRAMEWORK_PATTERN_IDS: &[&str] = &[
    ASPNET_ATTRIBUTE_ROUTE_PATTERN_ID,
    ASPNET_MINIMAL_API_ROUTE_GROUP_PATTERN_ID,
    ASPNET_MINIMAL_API_ROUTE_PATTERN_ID,
    HTTP_CLIENT_REQUEST_PATTERN_ID,
];
#[cfg(all(test, feature = "test-capability-matrix"))]
const MARKUP_FRAMEWORK_PATTERN_IDS: &[&str] =
    &[HTMX_ATTRIBUTE_PATTERN_ID, ALPINE_DIRECTIVE_PATTERN_ID];
// Component markup (JSX/TSX and Vue `<template>`) carries htmx-driven requests
// too, but not the Alpine directive surface the html/razor scan claims.
#[cfg(all(test, feature = "test-capability-matrix"))]
const COMPONENT_MARKUP_FRAMEWORK_PATTERN_IDS: &[&str] = &[HTMX_ATTRIBUTE_PATTERN_ID];
// jsx/tsx are React component files; NestJS controllers never live there, so
// nestjs.route.v1 is javascript/typescript only (see JAVASCRIPT_FRAMEWORK_PATTERN_IDS).
#[cfg(all(test, feature = "test-capability-matrix"))]
const NODE_FRAMEWORK_PATTERN_IDS: &[&str] = &[
    EXPRESS_ROUTE_PATTERN_ID,
    EXPRESS_ROUTER_MOUNT_PATTERN_ID,
    FASTIFY_ROUTE_PATTERN_ID,
    HTMX_ATTRIBUTE_PATTERN_ID,
];
#[cfg(all(test, feature = "test-capability-matrix"))]
const JAVASCRIPT_FRAMEWORK_PATTERN_IDS: &[&str] = &[
    EXPRESS_ROUTE_PATTERN_ID,
    EXPRESS_ROUTER_MOUNT_PATTERN_ID,
    FASTIFY_ROUTE_PATTERN_ID,
    NESTJS_ROUTE_PATTERN_ID,
    HTMX_ATTRIBUTE_PATTERN_ID,
];
#[cfg(all(test, feature = "test-capability-matrix"))]
const PYTHON_WEB_PATTERN_IDS: &[&str] = &[
    FASTAPI_ROUTE_PATTERN_ID,
    FASTAPI_INCLUDE_ROUTER_PATTERN_ID,
    FLASK_ROUTE_PATTERN_ID,
    FLASK_BLUEPRINT_REGISTRATION_PATTERN_ID,
    DJANGO_URL_PATTERN_ID,
    DJANGO_URL_INCLUDE_PATTERN_ID,
    HTTP_CLIENT_REQUEST_PATTERN_ID,
];
#[cfg(all(test, feature = "test-capability-matrix"))]
const GO_HTTP_PATTERN_IDS: &[&str] = &[
    GO_NET_HTTP_ROUTE_PATTERN_ID,
    GIN_ROUTE_PATTERN_ID,
    ECHO_ROUTE_PATTERN_ID,
    HTTP_CLIENT_REQUEST_PATTERN_ID,
];
#[cfg(all(test, feature = "test-capability-matrix"))]
const RAILS_PATTERN_IDS: &[&str] = &[
    RAILS_ROUTE_PATTERN_ID,
    RAILS_RESOURCE_ROUTE_PATTERN_ID,
    RAILS_MOUNT_PATTERN_ID,
    HTTP_CLIENT_REQUEST_PATTERN_ID,
];
#[cfg(all(test, feature = "test-capability-matrix"))]
const LARAVEL_PATTERN_IDS: &[&str] = &[
    LARAVEL_ROUTE_PATTERN_ID,
    LARAVEL_RESOURCE_ROUTE_PATTERN_ID,
    LARAVEL_ROUTE_PREFIX_PATTERN_ID,
    HTTP_CLIENT_REQUEST_PATTERN_ID,
];
#[cfg(all(test, feature = "test-capability-matrix"))]
const ELIXIR_PATTERN_IDS: &[&str] = &[
    PHOENIX_ROUTE_PATTERN_ID,
    PHOENIX_RESOURCE_ROUTE_PATTERN_ID,
    PHOENIX_FORWARD_PATTERN_ID,
    HTTP_CLIENT_REQUEST_PATTERN_ID,
];
#[cfg(all(test, feature = "test-capability-matrix"))]
const RAZOR_FRAMEWORK_PATTERN_IDS: &[&str] = &[
    ALPINE_DIRECTIVE_PATTERN_ID,
    HTMX_ATTRIBUTE_PATTERN_ID,
    RAZOR_CODE_BLOCK_PATTERN_ID,
    RAZOR_PAGE_DIRECTIVE_PATTERN_ID,
    RAZOR_TEMPLATE_EXPRESSION_PATTERN_ID,
];

pub fn collect_framework_structural_facts(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    symbols: &[Symbol],
) -> Vec<StructuralFact> {
    let mut facts = match language {
        "csharp" => {
            let mut csharp_facts =
                collect_aspnet_minimal_api_routes(language, tree, file_path, content);
            csharp_facts.extend(collect_aspnet_attribute_routes(
                language, tree, file_path, content,
            ));
            csharp_facts.extend(collect_backend_http_client_requests(
                language, tree, file_path, content,
            ));
            csharp_facts
        }
        "python" => {
            let mut python_facts = collect_python_web_facts(language, tree, file_path, content);
            python_facts.extend(collect_backend_http_client_requests(
                language, tree, file_path, content,
            ));
            python_facts
        }
        "html" => collect_markup_framework_attributes(language, tree, file_path, content),
        "razor" => {
            let mut razor_facts = collect_razor_structural_facts(tree, file_path, content);
            razor_facts.extend(collect_markup_framework_attributes(
                language, tree, file_path, content,
            ));
            razor_facts
        }
        "javascript" => {
            let mut js_facts = collect_jsx_htmx_attributes(language, tree, file_path, content);
            js_facts.extend(collect_node_http_boundary_facts(
                language, tree, file_path, content,
            ));
            js_facts.extend(collect_nestjs_route_facts(language, tree, file_path, content));
            js_facts
        }
        // jsx/tsx are React component files; NestJS controllers never live there.
        "jsx" | "tsx" => {
            let mut js_facts = collect_jsx_htmx_attributes(language, tree, file_path, content);
            js_facts.extend(collect_node_http_boundary_facts(
                language, tree, file_path, content,
            ));
            js_facts
        }
        "typescript" => {
            let mut ts_facts =
                collect_node_http_boundary_facts(language, tree, file_path, content);
            ts_facts.extend(collect_nestjs_route_facts(language, tree, file_path, content));
            ts_facts
        }
        "java" => {
            let mut java_facts =
                collect_spring_request_mappings(language, tree, file_path, content);
            java_facts.extend(collect_backend_http_client_requests(
                language, tree, file_path, content,
            ));
            java_facts
        }
        "kotlin" => {
            let mut kotlin_facts =
                collect_kotlin_spring_routes(language, tree, file_path, content);
            kotlin_facts.extend(collect_backend_http_client_requests(
                language, tree, file_path, content,
            ));
            kotlin_facts
        }
        "go" => {
            let mut go_facts = collect_go_http_boundary_facts(language, tree, file_path, content);
            go_facts.extend(collect_backend_http_client_requests(
                language, tree, file_path, content,
            ));
            go_facts
        }
        "ruby" => {
            let mut ruby_facts = collect_rails_routes(language, tree, file_path, content);
            ruby_facts.extend(collect_backend_http_client_requests(
                language, tree, file_path, content,
            ));
            ruby_facts
        }
        "php" => {
            let mut php_facts = collect_laravel_routes(language, tree, file_path, content);
            php_facts.extend(collect_backend_http_client_requests(
                language, tree, file_path, content,
            ));
            php_facts
        }
        "elixir" => {
            let mut elixir_facts = collect_phoenix_routes(language, tree, file_path, content);
            elixir_facts.extend(collect_backend_http_client_requests(
                language, tree, file_path, content,
            ));
            elixir_facts
        }
        "vue" => collect_vue_template_htmx_attributes(language, tree, file_path, content),
        _ => Vec::new(),
    };

    attach_containing_symbols(&mut facts, symbols);
    sort_structural_facts(&mut facts);
    facts
}

#[cfg(all(test, feature = "test-capability-matrix"))]
pub(crate) fn framework_structural_fact_pattern_ids_for_language(
    language: &str,
) -> &'static [&'static str] {
    match language {
        "csharp" => CSHARP_FRAMEWORK_PATTERN_IDS,
        "html" => MARKUP_FRAMEWORK_PATTERN_IDS,
        "razor" => RAZOR_FRAMEWORK_PATTERN_IDS,
        "javascript" => JAVASCRIPT_FRAMEWORK_PATTERN_IDS,
        "jsx" | "tsx" => NODE_FRAMEWORK_PATTERN_IDS,
        "typescript" => &[
            EXPRESS_ROUTE_PATTERN_ID,
            EXPRESS_ROUTER_MOUNT_PATTERN_ID,
            FASTIFY_ROUTE_PATTERN_ID,
            NESTJS_ROUTE_PATTERN_ID,
        ],
        "python" => PYTHON_WEB_PATTERN_IDS,
        "java" | "kotlin" => &[
            SPRING_REQUEST_MAPPING_PATTERN_ID,
            HTTP_CLIENT_REQUEST_PATTERN_ID,
        ],
        "go" => GO_HTTP_PATTERN_IDS,
        "ruby" => RAILS_PATTERN_IDS,
        "php" => LARAVEL_PATTERN_IDS,
        "elixir" => ELIXIR_PATTERN_IDS,
        "vue" => COMPONENT_MARKUP_FRAMEWORK_PATTERN_IDS,
        _ => &[],
    }
}
