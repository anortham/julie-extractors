use tree_sitter::Tree;

use super::structural_facts::sort_structural_facts;
use super::types::{StructuralFact, Symbol};

mod css;
mod fact_builders;
mod html;
mod http_client;
mod js_imports;
mod js_object_scan;
mod jsx_scan;
mod nextjs_nuxt;
mod react;
mod vue;

use super::attach_containing_symbols;
use css::collect_css_structural_facts;
use html::collect_html_structural_facts;
use http_client::collect_http_client_requests;
use js_imports::collect_js_imports;
use nextjs_nuxt::{
    collect_nextjs_route_handlers, collect_nextjs_route_references, nextjs_file_route_fact,
    nuxt_file_route_fact, nuxt_server_route_fact,
};
use react::{collect_react_router_route_definitions, collect_react_router_route_references};
pub(crate) use vue::vue_template_section_ranges;
use vue::{
    collect_vue_router_route_definitions, collect_vue_structural_facts, vue_script_section_ranges,
};

const CSS_SELECTOR_RULE_PATTERN_ID: &str = "css.selector_rule.v1";
const CSS_CUSTOM_PROPERTY_PATTERN_ID: &str = "css.custom_property.v1";
const CSS_MEDIA_QUERY_PATTERN_ID: &str = "css.media_query.v1";
const CSS_KEYFRAMES_PATTERN_ID: &str = "css.keyframes.v1";
const HTML_LINK_PATTERN_ID: &str = "html.link.v1";
const HTML_SCRIPT_PATTERN_ID: &str = "html.script.v1";
const HTML_FORM_PATTERN_ID: &str = "html.form.v1";
const HTML_FORM_CONTROL_PATTERN_ID: &str = "html.form_control.v1";
const VUE_SFC_SECTION_PATTERN_ID: &str = "vue.sfc_section.v1";
const VUE_TEMPLATE_DIRECTIVE_PATTERN_ID: &str = "vue.template_directive.v1";
const VUE_ROUTE_REFERENCE_PATTERN_ID: &str = "vue.route_reference.v1";
const VUE_ROUTE_DEFINITION_PATTERN_ID: &str = "vue.route_definition.v1";
const REACT_ROUTE_REFERENCE_PATTERN_ID: &str = "react.route_reference.v1";
const REACT_ROUTE_DEFINITION_PATTERN_ID: &str = "react.route_definition.v1";
const NEXTJS_ROUTE_REFERENCE_PATTERN_ID: &str = "nextjs.route_reference.v1";
const NEXTJS_FILE_ROUTE_PATTERN_ID: &str = "nextjs.file_route.v1";
const NEXTJS_ROUTE_HANDLER_PATTERN_ID: &str = "nextjs.route_handler.v1";
const NUXT_ROUTE_REFERENCE_PATTERN_ID: &str = "nuxt.route_reference.v1";
const NUXT_FILE_ROUTE_PATTERN_ID: &str = "nuxt.file_route.v1";
const NUXT_SERVER_ROUTE_PATTERN_ID: &str = "nuxt.server_route.v1";
const HTTP_CLIENT_REQUEST_PATTERN_ID: &str = "http.client_request.v1";

#[cfg(all(test, feature = "test-capability-matrix"))]
const CSS_WEB_PATTERN_IDS: &[&str] = &[
    CSS_CUSTOM_PROPERTY_PATTERN_ID,
    CSS_KEYFRAMES_PATTERN_ID,
    CSS_MEDIA_QUERY_PATTERN_ID,
    CSS_SELECTOR_RULE_PATTERN_ID,
];

#[cfg(all(test, feature = "test-capability-matrix"))]
const HTML_WEB_PATTERN_IDS: &[&str] = &[
    HTML_FORM_CONTROL_PATTERN_ID,
    HTML_FORM_PATTERN_ID,
    HTML_LINK_PATTERN_ID,
    HTML_SCRIPT_PATTERN_ID,
];

#[cfg(all(test, feature = "test-capability-matrix"))]
const VUE_WEB_PATTERN_IDS: &[&str] = &[
    HTTP_CLIENT_REQUEST_PATTERN_ID,
    NUXT_FILE_ROUTE_PATTERN_ID,
    NUXT_ROUTE_REFERENCE_PATTERN_ID,
    VUE_ROUTE_DEFINITION_PATTERN_ID,
    VUE_ROUTE_REFERENCE_PATTERN_ID,
    VUE_SFC_SECTION_PATTERN_ID,
    VUE_TEMPLATE_DIRECTIVE_PATTERN_ID,
];

// `javascript` and `typescript` additionally claim `nextjs.route_handler.v1`
// because App Router route handler files are `.js`/`.ts` only. `jsx`/`tsx`
// route files are nonstandard, so their arrays omit the handler pattern.
#[cfg(all(test, feature = "test-capability-matrix"))]
const JS_FRAMEWORK_WEB_PATTERN_IDS: &[&str] = &[
    HTTP_CLIENT_REQUEST_PATTERN_ID,
    NEXTJS_FILE_ROUTE_PATTERN_ID,
    NEXTJS_ROUTE_HANDLER_PATTERN_ID,
    NEXTJS_ROUTE_REFERENCE_PATTERN_ID,
    NUXT_FILE_ROUTE_PATTERN_ID,
    NUXT_SERVER_ROUTE_PATTERN_ID,
    REACT_ROUTE_DEFINITION_PATTERN_ID,
    REACT_ROUTE_REFERENCE_PATTERN_ID,
    VUE_ROUTE_DEFINITION_PATTERN_ID,
];
#[cfg(all(test, feature = "test-capability-matrix"))]
const JSX_TSX_FRAMEWORK_WEB_PATTERN_IDS: &[&str] = &[
    HTTP_CLIENT_REQUEST_PATTERN_ID,
    NEXTJS_FILE_ROUTE_PATTERN_ID,
    NEXTJS_ROUTE_REFERENCE_PATTERN_ID,
    NUXT_FILE_ROUTE_PATTERN_ID,
    REACT_ROUTE_DEFINITION_PATTERN_ID,
    REACT_ROUTE_REFERENCE_PATTERN_ID,
    VUE_ROUTE_DEFINITION_PATTERN_ID,
];
#[cfg(all(test, feature = "test-capability-matrix"))]
const TS_FRAMEWORK_WEB_PATTERN_IDS: &[&str] = &[
    HTTP_CLIENT_REQUEST_PATTERN_ID,
    NEXTJS_FILE_ROUTE_PATTERN_ID,
    NEXTJS_ROUTE_HANDLER_PATTERN_ID,
    NUXT_FILE_ROUTE_PATTERN_ID,
    NUXT_SERVER_ROUTE_PATTERN_ID,
    REACT_ROUTE_DEFINITION_PATTERN_ID,
    VUE_ROUTE_DEFINITION_PATTERN_ID,
];

pub fn collect_web_structural_facts(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    symbols: &[Symbol],
) -> Vec<StructuralFact> {
    let mut facts = match language {
        "css" => collect_css_structural_facts(tree, file_path, content),
        "html" => collect_html_structural_facts(tree, file_path, content),
        "vue" => {
            let mut facts = collect_vue_structural_facts(tree, file_path, content);
            // Client-request scanning runs over script sections only; the
            // axios import gate is local to the section that declares it.
            for (section_start, section_end) in vue_script_section_ranges(content) {
                let section_imports = collect_js_imports(&content[section_start..section_end]);
                facts.extend(collect_http_client_requests(
                    language,
                    tree,
                    file_path,
                    content,
                    &section_imports,
                    section_start,
                    section_end,
                ));
            }
            facts
        }
        "javascript" | "jsx" | "typescript" | "tsx" => {
            collect_react_nextjs_structural_facts(language, tree, file_path, content)
        }
        _ => Vec::new(),
    };

    attach_containing_symbols(&mut facts, symbols);
    sort_structural_facts(&mut facts);
    facts
}

#[cfg(all(test, feature = "test-capability-matrix"))]
pub(crate) fn web_structural_fact_pattern_ids_for_language(
    language: &str,
) -> &'static [&'static str] {
    match language {
        "css" => CSS_WEB_PATTERN_IDS,
        "html" => HTML_WEB_PATTERN_IDS,
        "vue" => VUE_WEB_PATTERN_IDS,
        "javascript" => JS_FRAMEWORK_WEB_PATTERN_IDS,
        "jsx" | "tsx" => JSX_TSX_FRAMEWORK_WEB_PATTERN_IDS,
        "typescript" => TS_FRAMEWORK_WEB_PATTERN_IDS,
        _ => &[],
    }
}

fn collect_react_nextjs_structural_facts(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let imports = collect_js_imports(content);
    let mut facts = Vec::new();
    facts.extend(collect_http_client_requests(
        language,
        tree,
        file_path,
        content,
        &imports,
        0,
        content.len(),
    ));
    facts.extend(collect_react_router_route_references(
        language, tree, file_path, content, &imports,
    ));
    facts.extend(collect_react_router_route_definitions(
        language, tree, file_path, content, &imports,
    ));
    facts.extend(collect_nextjs_route_references(
        language, tree, file_path, content, &imports,
    ));
    facts.extend(collect_nextjs_route_handlers(
        language, tree, file_path, content,
    ));
    facts.extend(collect_vue_router_route_definitions(
        language, tree, file_path, content,
    ));
    if let Some(fact) = nextjs_file_route_fact(language, tree, file_path, content) {
        facts.push(fact);
    }
    if let Some(fact) = nuxt_file_route_fact(language, tree, file_path, content) {
        facts.push(fact);
    }
    if let Some(fact) = nuxt_server_route_fact(language, tree, file_path, content) {
        facts.push(fact);
    }
    facts
}
