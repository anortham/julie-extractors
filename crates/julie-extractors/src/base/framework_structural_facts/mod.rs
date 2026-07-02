mod aspnet;
mod helpers;
mod http_clients;
mod markup;
mod razor;

use tree_sitter::Tree;

use self::aspnet::{collect_aspnet_attribute_routes, collect_aspnet_minimal_api_routes};
use self::markup::{
    collect_jsx_htmx_attributes, collect_markup_framework_attributes,
    collect_vue_template_htmx_attributes,
};
use self::razor::collect_razor_structural_facts;
use super::attach_containing_symbols;
use super::structural_facts::sort_structural_facts;
use super::types::{StructuralFact, Symbol};

pub(super) const ASPNET_MINIMAL_API_ROUTE_PATTERN_ID: &str = "aspnet.minimal_api.route.v1";
pub(super) const ASPNET_MINIMAL_API_ROUTE_GROUP_PATTERN_ID: &str =
    "aspnet.minimal_api.route_group.v1";
pub(super) const ASPNET_ATTRIBUTE_ROUTE_PATTERN_ID: &str = "aspnet.attribute_route.v1";
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
];
#[cfg(all(test, feature = "test-capability-matrix"))]
const MARKUP_FRAMEWORK_PATTERN_IDS: &[&str] =
    &[HTMX_ATTRIBUTE_PATTERN_ID, ALPINE_DIRECTIVE_PATTERN_ID];
// Component markup (JSX/TSX and Vue `<template>`) carries htmx-driven requests
// too, but not the Alpine directive surface the html/razor scan claims.
#[cfg(all(test, feature = "test-capability-matrix"))]
const COMPONENT_MARKUP_FRAMEWORK_PATTERN_IDS: &[&str] = &[HTMX_ATTRIBUTE_PATTERN_ID];
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
            csharp_facts
        }
        "html" => collect_markup_framework_attributes(language, tree, file_path, content),
        "razor" => {
            let mut razor_facts = collect_razor_structural_facts(tree, file_path, content);
            razor_facts.extend(collect_markup_framework_attributes(
                language, tree, file_path, content,
            ));
            razor_facts
        }
        "javascript" | "jsx" | "tsx" => {
            collect_jsx_htmx_attributes(language, tree, file_path, content)
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
        "javascript" | "jsx" | "tsx" | "vue" => COMPONENT_MARKUP_FRAMEWORK_PATTERN_IDS,
        _ => &[],
    }
}
