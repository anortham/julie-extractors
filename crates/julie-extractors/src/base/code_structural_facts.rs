use std::collections::HashMap;

use serde_json::{Number, Value};
use tree_sitter::{Node, Tree};

use super::span::NormalizedSpan;
use super::structural_facts::sort_structural_facts;
use super::types::{StructuralFact, Symbol, stable_location_id};

#[derive(Debug, Clone, Copy)]
struct CodeStructuralPattern {
    pattern_id: &'static str,
    capture_name: &'static str,
    node_kinds: &'static [&'static str],
    query_family: &'static str,
}

const JAVA_PATTERNS: &[CodeStructuralPattern] = &[
    CodeStructuralPattern {
        pattern_id: "java.synchronized_statement.v1",
        capture_name: "synchronized_statement",
        node_kinds: &["synchronized_statement"],
        query_family: "concurrency",
    },
    CodeStructuralPattern {
        pattern_id: "java.try_with_resources_statement.v1",
        capture_name: "try_with_resources_statement",
        node_kinds: &["try_with_resources_statement"],
        query_family: "resources",
    },
    CodeStructuralPattern {
        pattern_id: "java.lambda_expression.v1",
        capture_name: "lambda_expression",
        node_kinds: &["lambda_expression"],
        query_family: "functional",
    },
    CodeStructuralPattern {
        pattern_id: "java.marker_annotation.v1",
        capture_name: "marker_annotation",
        node_kinds: &["marker_annotation"],
        query_family: "metadata",
    },
    CodeStructuralPattern {
        pattern_id: "java.annotation.v1",
        capture_name: "annotation",
        node_kinds: &["annotation"],
        query_family: "metadata",
    },
];

const KOTLIN_PATTERNS: &[CodeStructuralPattern] = &[
    CodeStructuralPattern {
        pattern_id: "kotlin.suspend_modifier.v1",
        capture_name: "suspend_modifier",
        node_kinds: &["suspend"],
        query_family: "async",
    },
    CodeStructuralPattern {
        pattern_id: "kotlin.property_delegate.v1",
        capture_name: "property_delegate",
        node_kinds: &["property_delegate"],
        query_family: "delegation",
    },
    CodeStructuralPattern {
        pattern_id: "kotlin.annotation.v1",
        capture_name: "annotation",
        node_kinds: &["annotation"],
        query_family: "metadata",
    },
];

const SCALA_PATTERNS: &[CodeStructuralPattern] = &[
    CodeStructuralPattern {
        pattern_id: "scala.extension_definition.v1",
        capture_name: "extension_definition",
        node_kinds: &["extension_definition"],
        query_family: "metaprogramming",
    },
    CodeStructuralPattern {
        pattern_id: "scala.given_definition.v1",
        capture_name: "given_definition",
        node_kinds: &["given_definition"],
        query_family: "typeclass",
    },
    CodeStructuralPattern {
        pattern_id: "scala.for_expression.v1",
        capture_name: "for_expression",
        node_kinds: &["for_expression"],
        query_family: "comprehension",
    },
    CodeStructuralPattern {
        pattern_id: "scala.annotation.v1",
        capture_name: "annotation",
        node_kinds: &["annotation"],
        query_family: "metadata",
    },
];

const SWIFT_PATTERNS: &[CodeStructuralPattern] = &[
    CodeStructuralPattern {
        pattern_id: "swift.await_expression.v1",
        capture_name: "await_expression",
        node_kinds: &["await_expression"],
        query_family: "async",
    },
    CodeStructuralPattern {
        pattern_id: "swift.actor_declaration.v1",
        capture_name: "actor_declaration",
        node_kinds: &["class_declaration"],
        query_family: "concurrency",
    },
    CodeStructuralPattern {
        pattern_id: "swift.attribute.v1",
        capture_name: "attribute",
        node_kinds: &["attribute"],
        query_family: "metadata",
    },
];

const DART_PATTERNS: &[CodeStructuralPattern] = &[
    CodeStructuralPattern {
        pattern_id: "dart.await_expression.v1",
        capture_name: "await_expression",
        node_kinds: &["await_expression"],
        query_family: "async",
    },
    CodeStructuralPattern {
        pattern_id: "dart.async_modifier.v1",
        capture_name: "async_modifier",
        node_kinds: &["async"],
        query_family: "async",
    },
    CodeStructuralPattern {
        pattern_id: "dart.annotation.v1",
        capture_name: "annotation",
        node_kinds: &["annotation", "marker_annotation"],
        query_family: "metadata",
    },
];

const PHP_PATTERNS: &[CodeStructuralPattern] = &[
    CodeStructuralPattern {
        pattern_id: "php.attribute.v1",
        capture_name: "attribute",
        node_kinds: &["attribute"],
        query_family: "metadata",
    },
    CodeStructuralPattern {
        pattern_id: "php.namespace_definition.v1",
        capture_name: "namespace_definition",
        node_kinds: &["namespace_definition"],
        query_family: "module",
    },
    CodeStructuralPattern {
        pattern_id: "php.namespace_use_declaration.v1",
        capture_name: "namespace_use_declaration",
        node_kinds: &["namespace_use_declaration"],
        query_family: "imports",
    },
    CodeStructuralPattern {
        pattern_id: "php.trait_use_declaration.v1",
        capture_name: "use_declaration",
        node_kinds: &["use_declaration"],
        query_family: "traits",
    },
    CodeStructuralPattern {
        pattern_id: "php.anonymous_function.v1",
        capture_name: "anonymous_function",
        node_kinds: &["anonymous_function"],
        query_family: "functional",
    },
    CodeStructuralPattern {
        pattern_id: "php.match_expression.v1",
        capture_name: "match_expression",
        node_kinds: &["match_expression"],
        query_family: "control_flow",
    },
];

const RUBY_PATTERNS: &[CodeStructuralPattern] = &[
    CodeStructuralPattern {
        pattern_id: "ruby.require_call.v1",
        capture_name: "require_call",
        node_kinds: &["call"],
        query_family: "imports",
    },
    CodeStructuralPattern {
        pattern_id: "ruby.mixin_call.v1",
        capture_name: "mixin_call",
        node_kinds: &["call"],
        query_family: "mixins",
    },
    CodeStructuralPattern {
        pattern_id: "ruby.block.v1",
        capture_name: "block",
        node_kinds: &["block", "do_block"],
        query_family: "blocks",
    },
    CodeStructuralPattern {
        pattern_id: "ruby.rescue_clause.v1",
        capture_name: "rescue",
        node_kinds: &["rescue"],
        query_family: "error_handling",
    },
];

const ELIXIR_PATTERNS: &[CodeStructuralPattern] = &[
    CodeStructuralPattern {
        pattern_id: "elixir.defmodule_call.v1",
        capture_name: "defmodule_call",
        node_kinds: &["call"],
        query_family: "module",
    },
    CodeStructuralPattern {
        pattern_id: "elixir.module_attribute.v1",
        capture_name: "module_attribute",
        node_kinds: &["unary_operator"],
        query_family: "metadata",
    },
    CodeStructuralPattern {
        pattern_id: "elixir.directive_call.v1",
        capture_name: "directive_call",
        node_kinds: &["call"],
        query_family: "directives",
    },
    CodeStructuralPattern {
        pattern_id: "elixir.pipeline_operator.v1",
        capture_name: "pipeline_operator",
        node_kinds: &["binary_operator"],
        query_family: "pipeline",
    },
    CodeStructuralPattern {
        pattern_id: "elixir.with_expression.v1",
        capture_name: "with_expression",
        node_kinds: &["call"],
        query_family: "control_flow",
    },
];

const LUA_PATTERNS: &[CodeStructuralPattern] = &[
    CodeStructuralPattern {
        pattern_id: "lua.require_call.v1",
        capture_name: "require_call",
        node_kinds: &["function_call"],
        query_family: "imports",
    },
    CodeStructuralPattern {
        pattern_id: "lua.setmetatable_call.v1",
        capture_name: "setmetatable_call",
        node_kinds: &["function_call"],
        query_family: "metatable",
    },
    CodeStructuralPattern {
        pattern_id: "lua.coroutine_call.v1",
        capture_name: "coroutine_call",
        node_kinds: &["function_call"],
        query_family: "concurrency",
    },
    CodeStructuralPattern {
        pattern_id: "lua.module_return.v1",
        capture_name: "module_return",
        node_kinds: &["return_statement"],
        query_family: "module",
    },
    CodeStructuralPattern {
        pattern_id: "lua.table_constructor.v1",
        capture_name: "table_constructor",
        node_kinds: &["table_constructor"],
        query_family: "data",
    },
];

const R_PATTERNS: &[CodeStructuralPattern] = &[
    CodeStructuralPattern {
        pattern_id: "r.library_call.v1",
        capture_name: "library_call",
        node_kinds: &["call"],
        query_family: "imports",
    },
    CodeStructuralPattern {
        pattern_id: "r.pipe_expression.v1",
        capture_name: "pipe_expression",
        node_kinds: &["binary_operator"],
        query_family: "pipeline",
    },
    CodeStructuralPattern {
        pattern_id: "r.formula_expression.v1",
        capture_name: "formula_expression",
        node_kinds: &["binary_operator"],
        query_family: "modeling",
    },
];

const VBNET_PATTERNS: &[CodeStructuralPattern] = &[
    CodeStructuralPattern {
        pattern_id: "vbnet.handles_clause.v1",
        capture_name: "handles_clause",
        node_kinds: &["handles_clause"],
        query_family: "events",
    },
    CodeStructuralPattern {
        pattern_id: "vbnet.implements_clause.v1",
        capture_name: "implements_clause",
        node_kinds: &["implements_clause"],
        query_family: "interface",
    },
    CodeStructuralPattern {
        pattern_id: "vbnet.event_declaration.v1",
        capture_name: "event_declaration",
        node_kinds: &["event_declaration"],
        query_family: "events",
    },
    CodeStructuralPattern {
        pattern_id: "vbnet.attribute.v1",
        capture_name: "attribute",
        node_kinds: &["attribute"],
        query_family: "metadata",
    },
];

#[cfg(all(test, feature = "test-capability-matrix"))]
const JAVA_PATTERN_IDS: &[&str] = &[
    "java.synchronized_statement.v1",
    "java.try_with_resources_statement.v1",
    "java.lambda_expression.v1",
    "java.marker_annotation.v1",
    "java.annotation.v1",
];
#[cfg(all(test, feature = "test-capability-matrix"))]
const KOTLIN_PATTERN_IDS: &[&str] = &[
    "kotlin.suspend_modifier.v1",
    "kotlin.property_delegate.v1",
    "kotlin.annotation.v1",
];
#[cfg(all(test, feature = "test-capability-matrix"))]
const SCALA_PATTERN_IDS: &[&str] = &[
    "scala.extension_definition.v1",
    "scala.given_definition.v1",
    "scala.for_expression.v1",
    "scala.annotation.v1",
];
#[cfg(all(test, feature = "test-capability-matrix"))]
const SWIFT_PATTERN_IDS: &[&str] = &[
    "swift.await_expression.v1",
    "swift.actor_declaration.v1",
    "swift.attribute.v1",
];
#[cfg(all(test, feature = "test-capability-matrix"))]
const DART_PATTERN_IDS: &[&str] = &[
    "dart.await_expression.v1",
    "dart.async_modifier.v1",
    "dart.annotation.v1",
];
#[cfg(all(test, feature = "test-capability-matrix"))]
const PHP_PATTERN_IDS: &[&str] = &[
    "php.attribute.v1",
    "php.namespace_definition.v1",
    "php.namespace_use_declaration.v1",
    "php.trait_use_declaration.v1",
    "php.anonymous_function.v1",
    "php.match_expression.v1",
];
#[cfg(all(test, feature = "test-capability-matrix"))]
const RUBY_PATTERN_IDS: &[&str] = &[
    "ruby.require_call.v1",
    "ruby.mixin_call.v1",
    "ruby.block.v1",
    "ruby.rescue_clause.v1",
];
#[cfg(all(test, feature = "test-capability-matrix"))]
const ELIXIR_PATTERN_IDS: &[&str] = &[
    "elixir.defmodule_call.v1",
    "elixir.module_attribute.v1",
    "elixir.directive_call.v1",
    "elixir.pipeline_operator.v1",
    "elixir.with_expression.v1",
];
#[cfg(all(test, feature = "test-capability-matrix"))]
const LUA_PATTERN_IDS: &[&str] = &[
    "lua.require_call.v1",
    "lua.setmetatable_call.v1",
    "lua.coroutine_call.v1",
    "lua.module_return.v1",
    "lua.table_constructor.v1",
];
#[cfg(all(test, feature = "test-capability-matrix"))]
const R_PATTERN_IDS: &[&str] = &[
    "r.library_call.v1",
    "r.pipe_expression.v1",
    "r.formula_expression.v1",
];
#[cfg(all(test, feature = "test-capability-matrix"))]
const VBNET_PATTERN_IDS: &[&str] = &[
    "vbnet.handles_clause.v1",
    "vbnet.implements_clause.v1",
    "vbnet.event_declaration.v1",
    "vbnet.attribute.v1",
];

pub fn collect_code_structural_facts(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
    symbols: &[Symbol],
) -> Vec<StructuralFact> {
    let patterns = patterns_for_language(language);
    if patterns.is_empty() {
        return Vec::new();
    }

    let mut facts = Vec::new();
    collect_node(
        tree.root_node(),
        language,
        file_path,
        content,
        patterns,
        &mut facts,
    );
    attach_containing_symbols(&mut facts, symbols);
    sort_structural_facts(&mut facts);
    facts
}

#[cfg(all(test, feature = "test-capability-matrix"))]
pub(crate) fn code_structural_fact_pattern_ids_for_language(
    language: &str,
) -> &'static [&'static str] {
    match language {
        "dart" => DART_PATTERN_IDS,
        "elixir" => ELIXIR_PATTERN_IDS,
        "java" => JAVA_PATTERN_IDS,
        "kotlin" => KOTLIN_PATTERN_IDS,
        "lua" => LUA_PATTERN_IDS,
        "php" => PHP_PATTERN_IDS,
        "r" => R_PATTERN_IDS,
        "ruby" => RUBY_PATTERN_IDS,
        "scala" => SCALA_PATTERN_IDS,
        "swift" => SWIFT_PATTERN_IDS,
        "vbnet" => VBNET_PATTERN_IDS,
        _ => &[],
    }
}

fn patterns_for_language(language: &str) -> &'static [CodeStructuralPattern] {
    match language {
        "dart" => DART_PATTERNS,
        "elixir" => ELIXIR_PATTERNS,
        "java" => JAVA_PATTERNS,
        "kotlin" => KOTLIN_PATTERNS,
        "lua" => LUA_PATTERNS,
        "php" => PHP_PATTERNS,
        "r" => R_PATTERNS,
        "ruby" => RUBY_PATTERNS,
        "scala" => SCALA_PATTERNS,
        "swift" => SWIFT_PATTERNS,
        "vbnet" => VBNET_PATTERNS,
        _ => &[],
    }
}

fn collect_node(
    node: Node<'_>,
    language: &str,
    file_path: &str,
    content: &str,
    patterns: &[CodeStructuralPattern],
    facts: &mut Vec<StructuralFact>,
) {
    for pattern in patterns {
        if pattern.node_kinds.contains(&node.kind()) {
            if pattern.pattern_id == "swift.actor_declaration.v1"
                && !node_text(content, node).trim_start().starts_with("actor")
            {
                continue;
            }
            if !matches_pattern(language, content, node, pattern.pattern_id) {
                continue;
            }
            facts.push(fact_for_node(file_path, language, content, node, *pattern));
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_node(child, language, file_path, content, patterns, facts);
    }
}

fn fact_for_node(
    file_path: &str,
    language: &str,
    content: &str,
    node: Node<'_>,
    pattern: CodeStructuralPattern,
) -> StructuralFact {
    let span = NormalizedSpan::from_node(&node);
    let mut metadata = base_metadata(pattern.query_family);
    enrich_metadata(language, content, node, pattern.pattern_id, &mut metadata);

    StructuralFact {
        id: stable_location_id(
            file_path,
            &format!("{}:{}", pattern.pattern_id, pattern.capture_name),
            span,
        ),
        file_path: file_path.to_string(),
        language: language.to_string(),
        pattern_id: pattern.pattern_id.to_string(),
        capture_name: pattern.capture_name.to_string(),
        node_kind: node.kind().to_string(),
        containing_symbol_id: None,
        start_line: span.start_line,
        start_column: span.start_column,
        end_line: span.end_line,
        end_column: span.end_column,
        start_byte: span.start_byte,
        end_byte: span.end_byte,
        confidence: 1.0,
        metadata: Some(metadata),
    }
}

fn base_metadata(query_family: &str) -> HashMap<String, Value> {
    HashMap::from([
        (
            "pattern_version".to_string(),
            Value::Number(Number::from(1)),
        ),
        (
            "query_family".to_string(),
            Value::String(query_family.to_string()),
        ),
    ])
}

fn enrich_metadata(
    language: &str,
    content: &str,
    node: Node<'_>,
    pattern_id: &str,
    metadata: &mut HashMap<String, Value>,
) {
    match pattern_id {
        "java.marker_annotation.v1"
        | "java.annotation.v1"
        | "kotlin.annotation.v1"
        | "scala.annotation.v1"
        | "dart.annotation.v1" => {
            if let Some(name) = annotation_name(content, node) {
                insert_string(metadata, "annotation_name", &name);
            }
        }
        "swift.attribute.v1" | "vbnet.attribute.v1" => {
            if let Some(name) = attribute_name(content, node) {
                insert_string(metadata, "attribute_name", &name);
            }
        }
        "kotlin.property_delegate.v1" => {
            if let Some(delegate) = delegate_name(content, node) {
                insert_string(metadata, "delegate_name", &delegate);
            }
        }
        "vbnet.handles_clause.v1" => {
            if let Some(target) = handles_target(content, node) {
                insert_string(metadata, "handles_target", &target);
            }
        }
        "vbnet.implements_clause.v1" => {
            if let Some(target) = implements_target(content, node) {
                insert_string(metadata, "implements_target", &target);
            }
        }
        "scala.extension_definition.v1" => {
            if let Some(extended_type) = scala_extended_type(content, node) {
                insert_string(metadata, "extended_type", &extended_type);
            }
        }
        "scala.given_definition.v1" => {
            if let Some(name) = scala_given_name(content, node) {
                insert_string(metadata, "given_name", &name);
            } else if let Some(given_type) = scala_given_type(content, node) {
                insert_string(metadata, "given_type", &given_type);
            }
        }
        "php.attribute.v1" => {
            if let Some(name) = php_attribute_name(content, node) {
                insert_string(metadata, "attribute_name", &name);
            }
        }
        "php.namespace_definition.v1" => {
            if let Some(name) = php_namespace_name(content, node) {
                insert_string(metadata, "namespace_name", &name);
            }
        }
        "php.namespace_use_declaration.v1" => {
            if let Some(import_target) = php_namespace_use_target(content, node) {
                insert_string(metadata, "import_target", &import_target);
            }
            if let Some(alias) = php_namespace_use_alias(content, node) {
                insert_string(metadata, "import_alias", &alias);
            }
        }
        "php.trait_use_declaration.v1" => {
            if let Some(trait_name) = php_trait_use_target(content, node) {
                insert_string(metadata, "trait_name", &trait_name);
            }
        }
        "ruby.require_call.v1" => {
            if let Some(kind) = ruby_require_kind(content, node) {
                insert_string(metadata, "require_kind", kind);
            }
            if let Some(path) = ruby_require_path(content, node) {
                insert_string(metadata, "required_path", &path);
            }
        }
        "ruby.mixin_call.v1" => {
            if let Some(kind) = ruby_mixin_kind(content, node) {
                insert_string(metadata, "mixin_kind", kind);
            }
            if let Some(target) = ruby_mixin_target(content, node) {
                insert_string(metadata, "mixin_target", &target);
            }
        }
        "ruby.rescue_clause.v1" => {
            if let Some(exception) = ruby_rescue_exception(content, node) {
                insert_string(metadata, "exception_type", &exception);
            }
        }
        "elixir.defmodule_call.v1" => {
            if let Some(module_name) = elixir_defmodule_name(content, node) {
                insert_string(metadata, "module_name", &module_name);
            }
        }
        "elixir.module_attribute.v1" => {
            if let Some(name) = elixir_module_attribute_name(content, node) {
                insert_string(metadata, "attribute_name", &name);
            }
        }
        "elixir.directive_call.v1" => {
            if let Some(kind) = elixir_directive_kind(content, node) {
                insert_string(metadata, "directive_kind", kind);
            }
            if let Some(target) = elixir_directive_target(content, node) {
                insert_string(metadata, "directive_target", &target);
            }
        }
        "lua.require_call.v1" | "lua.setmetatable_call.v1" | "lua.coroutine_call.v1" => {
            if let Some(name) = lua_function_call_name(content, node) {
                insert_string(metadata, "call_name", &name);
            }
            if pattern_id == "lua.require_call.v1"
                && let Some(module) = lua_require_module(content, node)
            {
                insert_string(metadata, "required_module", &module);
            }
        }
        "lua.module_return.v1" => {
            if let Some(value) = lua_module_return_value(content, node) {
                insert_string(metadata, "returned_value", &value);
            }
        }
        "lua.table_constructor.v1" => {
            if let Some(count) = lua_table_field_count(node) {
                insert_number(metadata, "field_count", count);
            }
        }
        "r.library_call.v1" => {
            if let Some(kind) = r_library_kind(content, node) {
                insert_string(metadata, "load_kind", kind);
            }
            if let Some(package) = r_library_package(content, node) {
                insert_string(metadata, "package_name", &package);
            }
        }
        "r.formula_expression.v1" => {
            if let Some(formula) = r_formula_text(content, node) {
                insert_string(metadata, "formula_text", &formula);
            }
        }
        _ if language == "swift" && pattern_id == "swift.actor_declaration.v1" => {
            if let Some(name) = swift_actor_name(content, node) {
                insert_string(metadata, "actor_name", &name);
            }
        }
        _ => {}
    }
}

fn annotation_name(content: &str, node: Node<'_>) -> Option<String> {
    first_named_identifier(
        content,
        node,
        &[
            "identifier",
            "type_identifier",
            "simple_identifier",
            "scoped_identifier",
        ],
    )
}

fn attribute_name(content: &str, node: Node<'_>) -> Option<String> {
    first_named_identifier(
        content,
        node,
        &["type_identifier", "identifier", "simple_identifier"],
    )
}

fn delegate_name(content: &str, node: Node<'_>) -> Option<String> {
    if let Some(name) = first_named_identifier(
        content,
        node,
        &["identifier", "simple_identifier", "type_identifier"],
    ) {
        return Some(name);
    }

    find_descendant(node, "call_expression").and_then(|call| {
        first_named_identifier(content, call, &["identifier", "simple_identifier"])
    })
}

fn implements_target(content: &str, node: Node<'_>) -> Option<String> {
    let text = node_text(content, node);
    text.trim()
        .strip_prefix("Implements")
        .or_else(|| text.trim().strip_prefix("implements"))
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_string)
}

fn scala_extended_type(content: &str, node: Node<'_>) -> Option<String> {
    extension_parameter_type(content, node)
}

fn extension_parameter_type(content: &str, node: Node<'_>) -> Option<String> {
    let parameters = node.child_by_field_name("parameters")?;
    let parameter = first_direct_child(parameters, "parameter")?;
    parameter
        .child_by_field_name("type")
        .map(|type_node| node_text(content, type_node))
}

fn scala_given_name(content: &str, node: Node<'_>) -> Option<String> {
    let mut cursor = node.walk();
    let children: Vec<_> = node.children(&mut cursor).collect();
    for (index, child) in children.iter().enumerate() {
        if child.kind() != "identifier" {
            continue;
        }
        if children
            .get(index + 1)
            .is_some_and(|next| next.kind() == ":")
        {
            return Some(node_text(content, *child));
        }
    }
    None
}

fn scala_given_type(content: &str, node: Node<'_>) -> Option<String> {
    node.child_by_field_name("return_type")
        .map(|return_type| {
            node_text(content, return_type)
                .split('=')
                .next()
                .unwrap_or_default()
                .trim()
                .to_string()
        })
        .filter(|given_type| !given_type.is_empty())
}

fn handles_target(content: &str, node: Node<'_>) -> Option<String> {
    let text = node_text(content, node);
    text.trim()
        .strip_prefix("Handles")
        .or_else(|| text.trim().strip_prefix("handles"))
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_string)
}

fn first_direct_child<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find(|child| child.kind() == kind)
}

fn swift_actor_name(content: &str, node: Node<'_>) -> Option<String> {
    first_named_identifier(content, node, &["type_identifier", "simple_identifier"])
}

fn first_named_identifier(content: &str, node: Node<'_>, kinds: &[&str]) -> Option<String> {
    if kinds.contains(&node.kind()) {
        return Some(node_text(content, node));
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(name) = first_named_identifier(content, child, kinds) {
            return Some(name);
        }
    }
    None
}

fn find_descendant<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    if node.kind() == kind {
        return Some(node);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(found) = find_descendant(child, kind) {
            return Some(found);
        }
    }
    None
}

fn node_text(content: &str, node: Node<'_>) -> String {
    node.utf8_text(content.as_bytes())
        .unwrap_or_default()
        .to_string()
}

fn insert_string(metadata: &mut HashMap<String, Value>, key: &str, value: &str) {
    metadata.insert(key.to_string(), Value::String(value.to_string()));
}

fn insert_number(metadata: &mut HashMap<String, Value>, key: &str, value: u64) {
    metadata.insert(key.to_string(), Value::Number(Number::from(value)));
}

fn matches_pattern(language: &str, content: &str, node: Node<'_>, pattern_id: &str) -> bool {
    match (language, pattern_id) {
        ("ruby", "ruby.require_call.v1") => ruby_require_kind(content, node).is_some(),
        ("ruby", "ruby.mixin_call.v1") => ruby_mixin_kind(content, node).is_some(),
        ("ruby", "ruby.rescue_clause.v1") => {
            ruby_rescue_exception(content, node).is_some()
                || node_text(content, node).trim().len() > "rescue".len()
        }
        ("elixir", "elixir.defmodule_call.v1") => {
            elixir_call_target(content, node).as_deref() == Some("defmodule")
        }
        ("elixir", "elixir.module_attribute.v1") => elixir_is_module_attribute(node),
        ("elixir", "elixir.directive_call.v1") => elixir_directive_kind(content, node).is_some(),
        ("elixir", "elixir.pipeline_operator.v1") => node_contains_token(node, content, "|>"),
        ("elixir", "elixir.with_expression.v1") => {
            elixir_call_target(content, node).as_deref() == Some("with")
        }
        ("lua", "lua.require_call.v1") => {
            lua_function_call_name(content, node).as_deref() == Some("require")
        }
        ("lua", "lua.setmetatable_call.v1") => {
            lua_function_call_name(content, node).as_deref() == Some("setmetatable")
        }
        ("lua", "lua.coroutine_call.v1") => {
            lua_function_call_name(content, node).is_some_and(|name| name.starts_with("coroutine."))
        }
        ("lua", "lua.module_return.v1") => lua_is_module_return(node),
        ("lua", "lua.table_constructor.v1") => {
            lua_table_field_count(node).is_some_and(|count| count > 0)
        }
        ("r", "r.library_call.v1") => r_library_kind(content, node).is_some(),
        ("r", "r.pipe_expression.v1") => r_is_pipe_expression(node),
        ("r", "r.formula_expression.v1") => r_is_formula_expression(content, node),
        _ => true,
    }
}

fn php_attribute_name(content: &str, node: Node<'_>) -> Option<String> {
    first_named_identifier(
        content,
        node,
        &["name", "qualified_name", "relative_name", "namespace_name"],
    )
}

fn php_namespace_name(content: &str, node: Node<'_>) -> Option<String> {
    node.child_by_field_name("name")
        .map(|name| node_text(content, name))
}

fn php_namespace_use_target(content: &str, node: Node<'_>) -> Option<String> {
    first_descendant_of_kind(node, "namespace_use_clause").and_then(|clause| {
        first_named_identifier(
            content,
            clause,
            &["qualified_name", "name", "namespace_name"],
        )
    })
}

fn php_namespace_use_alias(content: &str, node: Node<'_>) -> Option<String> {
    first_descendant_of_kind(node, "namespace_use_clause").and_then(|clause| {
        clause
            .child_by_field_name("alias")
            .map(|alias| node_text(content, alias))
    })
}

fn php_trait_use_target(content: &str, node: Node<'_>) -> Option<String> {
    first_named_identifier(content, node, &["qualified_name", "name", "relative_name"])
}

fn ruby_call_method_name(content: &str, node: Node<'_>) -> Option<String> {
    if let Some(method) = node.child_by_field_name("method") {
        return Some(node_text(content, method));
    }
    let text = node_text(content, node);
    text.split(['(', ' '])
        .next()
        .filter(|name| !name.is_empty())
        .map(str::to_string)
}

fn ruby_require_kind(content: &str, node: Node<'_>) -> Option<&'static str> {
    match ruby_call_method_name(content, node).as_deref()? {
        "require" => Some("require"),
        "require_relative" => Some("require_relative"),
        _ => None,
    }
}

fn ruby_require_path(content: &str, node: Node<'_>) -> Option<String> {
    node.child_by_field_name("arguments").and_then(|args| {
        first_descendant_of_kind(args, "string").map(|string| {
            node_text(content, string)
                .trim_matches('"')
                .trim_matches('\'')
                .to_string()
        })
    })
}

fn ruby_mixin_kind(content: &str, node: Node<'_>) -> Option<&'static str> {
    match ruby_call_method_name(content, node).as_deref()? {
        "include" => Some("include"),
        "extend" => Some("extend"),
        "prepend" => Some("prepend"),
        _ => None,
    }
}

fn ruby_mixin_target(content: &str, node: Node<'_>) -> Option<String> {
    node.child_by_field_name("arguments").and_then(|args| {
        first_named_identifier(
            content,
            args,
            &["constant", "identifier", "scope_resolution"],
        )
    })
}

fn ruby_rescue_exception(content: &str, node: Node<'_>) -> Option<String> {
    first_named_identifier(content, node, &["constant", "identifier"])
}

fn elixir_call_target(content: &str, node: Node<'_>) -> Option<String> {
    node.child_by_field_name("target")
        .filter(|target| target.kind() == "identifier")
        .map(|target| node_text(content, target))
}

fn elixir_is_module_attribute(node: Node<'_>) -> bool {
    node.child_by_field_name("operator")
        .is_some_and(|operator| operator.kind() == "@")
}

fn elixir_module_attribute_name(content: &str, node: Node<'_>) -> Option<String> {
    let operand = node.child_by_field_name("operand")?;
    elixir_call_target(content, operand)
}

fn elixir_directive_kind(content: &str, node: Node<'_>) -> Option<&'static str> {
    match elixir_call_target(content, node).as_deref()? {
        "use" => Some("use"),
        "import" => Some("import"),
        "alias" => Some("alias"),
        "require" => Some("require"),
        _ => None,
    }
}

fn elixir_call_arguments<'tree>(node: Node<'tree>) -> Option<Node<'tree>> {
    first_direct_child(node, "arguments")
}

fn elixir_directive_target(content: &str, node: Node<'_>) -> Option<String> {
    let args = elixir_call_arguments(node)?;
    if let Some(alias) = first_descendant_of_kind(args, "alias") {
        return Some(node_text(content, alias));
    }
    let mut cursor = args.walk();
    for child in args.children(&mut cursor) {
        if child.kind() == "identifier" {
            return Some(node_text(content, child));
        }
        if child.kind() == "call" {
            if let Some(name) = elixir_call_target(content, child) {
                return Some(name);
            }
        }
    }
    None
}

fn elixir_defmodule_name(content: &str, node: Node<'_>) -> Option<String> {
    let args = elixir_call_arguments(node)?;
    first_descendant_of_kind(args, "alias").map(|alias| node_text(content, alias))
}

fn lua_function_call_name(content: &str, node: Node<'_>) -> Option<String> {
    let name = node.child_by_field_name("name")?;
    match name.kind() {
        "identifier" | "dot_index_expression" | "variable" => Some(node_text(content, name)),
        "method_index_expression" => name
            .child_by_field_name("property")
            .map(|property| node_text(content, property))
            .map(|property| {
                if let Some(table) = name.child_by_field_name("table") {
                    format!("{}.{}", node_text(content, table), property)
                } else {
                    property
                }
            }),
        _ => None,
    }
}

fn lua_require_module(content: &str, node: Node<'_>) -> Option<String> {
    node.child_by_field_name("arguments").and_then(|args| {
        first_descendant_of_kind(args, "string").map(|string| {
            node_text(content, string)
                .trim_matches('"')
                .trim_matches('\'')
                .to_string()
        })
    })
}

fn lua_is_module_return(node: Node<'_>) -> bool {
    node.kind() == "return_statement"
        && node.parent().is_some_and(|parent| parent.kind() == "chunk")
}

fn lua_module_return_value(content: &str, node: Node<'_>) -> Option<String> {
    if !lua_is_module_return(node) {
        return None;
    }
    let text = node_text(content, node);
    text.trim()
        .strip_prefix("return")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn lua_table_field_count(node: Node<'_>) -> Option<u64> {
    let mut count = 0u64;
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "field" {
            count += 1;
        }
    }
    (count > 0).then_some(count)
}

fn r_library_kind(content: &str, node: Node<'_>) -> Option<&'static str> {
    node.child_by_field_name("function")
        .filter(|function| function.kind() == "identifier")
        .map(|function| node_text(content, function))
        .and_then(|name| match name.as_str() {
            "library" => Some("library"),
            "require" => Some("require"),
            _ => None,
        })
}

fn r_library_package(content: &str, node: Node<'_>) -> Option<String> {
    node.child_by_field_name("arguments").and_then(|args| {
        first_named_identifier(content, args, &["string", "identifier"])
            .map(|package| package.trim_matches('"').to_string())
    })
}

fn r_formula_text(content: &str, node: Node<'_>) -> Option<String> {
    if !r_is_formula_expression(content, node) {
        return None;
    }
    Some(node_text(content, node))
}

fn r_is_formula_expression(content: &str, node: Node<'_>) -> bool {
    let text = node_text(content, node);
    text.contains('~') && !text.contains("<-") && !text.contains("<<-")
}

fn r_is_pipe_expression(node: Node<'_>) -> bool {
    node.kind() == "binary_operator"
        && node
            .child(1)
            .is_some_and(|operator| operator.kind() == "|>")
}

fn node_contains_token(node: Node<'_>, content: &str, token: &str) -> bool {
    if node.kind() == token {
        return true;
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if node_contains_token(child, content, token) {
            return true;
        }
    }
    false
}

fn first_descendant_of_kind<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    if node.kind() == kind {
        return Some(node);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(found) = first_descendant_of_kind(child, kind) {
            return Some(found);
        }
    }
    None
}

fn attach_containing_symbols(facts: &mut [StructuralFact], symbols: &[Symbol]) {
    for fact in facts {
        fact.containing_symbol_id = symbols
            .iter()
            .filter(|symbol| {
                symbol.start_byte <= fact.start_byte && symbol.end_byte >= fact.end_byte
            })
            .min_by_key(|symbol| symbol.end_byte.saturating_sub(symbol.start_byte))
            .map(|symbol| symbol.id.clone());
    }
}
