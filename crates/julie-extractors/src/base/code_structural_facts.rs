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

const ZIG_PATTERNS: &[CodeStructuralPattern] = &[
    CodeStructuralPattern {
        pattern_id: "zig.builtin_call.v1",
        capture_name: "builtin_call",
        node_kinds: &["builtin_function", "call_expression"],
        query_family: "builtin",
    },
    CodeStructuralPattern {
        pattern_id: "zig.threadlocal_variable.v1",
        capture_name: "threadlocal_variable",
        node_kinds: &["variable_declaration"],
        query_family: "storage",
    },
    CodeStructuralPattern {
        pattern_id: "zig.inline_function.v1",
        capture_name: "inline_function",
        node_kinds: &["function_declaration"],
        query_family: "functions",
    },
    CodeStructuralPattern {
        pattern_id: "zig.exported_function.v1",
        capture_name: "exported_function",
        node_kinds: &["function_declaration"],
        query_family: "ffi",
    },
    CodeStructuralPattern {
        pattern_id: "zig.comptime_parameter.v1",
        capture_name: "comptime_parameter",
        node_kinds: &["parameter"],
        query_family: "metaprogramming",
    },
];

const QML_PATTERNS: &[CodeStructuralPattern] = &[
    CodeStructuralPattern {
        pattern_id: "qml.import_statement.v1",
        capture_name: "import_statement",
        node_kinds: &["ui_import"],
        query_family: "imports",
    },
    CodeStructuralPattern {
        pattern_id: "qml.property_declaration.v1",
        capture_name: "property_declaration",
        node_kinds: &["ui_property"],
        query_family: "properties",
    },
    CodeStructuralPattern {
        pattern_id: "qml.signal_declaration.v1",
        capture_name: "signal_declaration",
        node_kinds: &["ui_signal"],
        query_family: "signals",
    },
    CodeStructuralPattern {
        pattern_id: "qml.binding.v1",
        capture_name: "binding",
        node_kinds: &["ui_binding"],
        query_family: "bindings",
    },
];

const BASH_PATTERNS: &[CodeStructuralPattern] = &[
    CodeStructuralPattern {
        pattern_id: "bash.shebang.v1",
        capture_name: "shebang",
        node_kinds: &["comment"],
        query_family: "script_header",
    },
    CodeStructuralPattern {
        pattern_id: "bash.command_substitution.v1",
        capture_name: "command_substitution",
        node_kinds: &["command_substitution"],
        query_family: "expansion",
    },
    CodeStructuralPattern {
        pattern_id: "bash.arithmetic_expansion.v1",
        capture_name: "arithmetic_expansion",
        node_kinds: &["arithmetic_expansion"],
        query_family: "expansion",
    },
    CodeStructuralPattern {
        pattern_id: "bash.export_declaration.v1",
        capture_name: "export_declaration",
        node_kinds: &["declaration_command"],
        query_family: "environment",
    },
];

const POWERSHELL_PATTERNS: &[CodeStructuralPattern] = &[
    CodeStructuralPattern {
        pattern_id: "powershell.cmdlet_binding_attribute.v1",
        capture_name: "cmdlet_binding_attribute",
        node_kinds: &["attribute"],
        query_family: "metadata",
    },
    CodeStructuralPattern {
        pattern_id: "powershell.param_block.v1",
        capture_name: "param_block",
        node_kinds: &["param_block"],
        query_family: "parameters",
    },
    CodeStructuralPattern {
        pattern_id: "powershell.pipeline_expression.v1",
        capture_name: "pipeline_expression",
        node_kinds: &["pipeline"],
        query_family: "pipeline",
    },
    CodeStructuralPattern {
        pattern_id: "powershell.class_definition.v1",
        capture_name: "class_definition",
        node_kinds: &["class_statement"],
        query_family: "types",
    },
];

const GDSCRIPT_PATTERNS: &[CodeStructuralPattern] = &[
    CodeStructuralPattern {
        pattern_id: "gdscript.class_name.v1",
        capture_name: "class_name",
        node_kinds: &["class_name_statement"],
        query_family: "types",
    },
    CodeStructuralPattern {
        pattern_id: "gdscript.extends_declaration.v1",
        capture_name: "extends_declaration",
        node_kinds: &["extends_statement"],
        query_family: "inheritance",
    },
    CodeStructuralPattern {
        pattern_id: "gdscript.signal_declaration.v1",
        capture_name: "signal_declaration",
        node_kinds: &["signal_statement"],
        query_family: "signals",
    },
    CodeStructuralPattern {
        pattern_id: "gdscript.export_annotation.v1",
        capture_name: "export_annotation",
        node_kinds: &["annotation"],
        query_family: "metadata",
    },
    CodeStructuralPattern {
        pattern_id: "gdscript.match_statement.v1",
        capture_name: "match_statement",
        node_kinds: &["match_statement"],
        query_family: "control_flow",
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
#[cfg(all(test, feature = "test-capability-matrix"))]
const ZIG_PATTERN_IDS: &[&str] = &[
    "zig.builtin_call.v1",
    "zig.threadlocal_variable.v1",
    "zig.inline_function.v1",
    "zig.exported_function.v1",
    "zig.comptime_parameter.v1",
];
#[cfg(all(test, feature = "test-capability-matrix"))]
const QML_PATTERN_IDS: &[&str] = &[
    "qml.import_statement.v1",
    "qml.property_declaration.v1",
    "qml.signal_declaration.v1",
    "qml.binding.v1",
];
#[cfg(all(test, feature = "test-capability-matrix"))]
const BASH_PATTERN_IDS: &[&str] = &[
    "bash.shebang.v1",
    "bash.command_substitution.v1",
    "bash.arithmetic_expansion.v1",
    "bash.export_declaration.v1",
];
#[cfg(all(test, feature = "test-capability-matrix"))]
const POWERSHELL_PATTERN_IDS: &[&str] = &[
    "powershell.cmdlet_binding_attribute.v1",
    "powershell.param_block.v1",
    "powershell.pipeline_expression.v1",
    "powershell.class_definition.v1",
];
#[cfg(all(test, feature = "test-capability-matrix"))]
const GDSCRIPT_PATTERN_IDS: &[&str] = &[
    "gdscript.class_name.v1",
    "gdscript.extends_declaration.v1",
    "gdscript.signal_declaration.v1",
    "gdscript.export_annotation.v1",
    "gdscript.match_statement.v1",
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
        "bash" => BASH_PATTERN_IDS,
        "gdscript" => GDSCRIPT_PATTERN_IDS,
        "powershell" => POWERSHELL_PATTERN_IDS,
        "qml" => QML_PATTERN_IDS,
        "vbnet" => VBNET_PATTERN_IDS,
        "zig" => ZIG_PATTERN_IDS,
        _ => &[],
    }
}

fn patterns_for_language(language: &str) -> &'static [CodeStructuralPattern] {
    match language {
        "bash" => BASH_PATTERNS,
        "dart" => DART_PATTERNS,
        "elixir" => ELIXIR_PATTERNS,
        "gdscript" => GDSCRIPT_PATTERNS,
        "java" => JAVA_PATTERNS,
        "kotlin" => KOTLIN_PATTERNS,
        "lua" => LUA_PATTERNS,
        "php" => PHP_PATTERNS,
        "powershell" => POWERSHELL_PATTERNS,
        "qml" => QML_PATTERNS,
        "r" => R_PATTERNS,
        "ruby" => RUBY_PATTERNS,
        "scala" => SCALA_PATTERNS,
        "swift" => SWIFT_PATTERNS,
        "vbnet" => VBNET_PATTERNS,
        "zig" => ZIG_PATTERNS,
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
        "zig.builtin_call.v1" => {
            if let Some(name) = zig_builtin_name(content, node) {
                insert_string(metadata, "builtin_name", &name);
            }
        }
        "zig.threadlocal_variable.v1" => {
            if let Some(name) = zig_variable_name(content, node) {
                insert_string(metadata, "variable_name", &name);
            }
        }
        "zig.inline_function.v1" | "zig.exported_function.v1" => {
            if let Some(name) = zig_function_name(content, node) {
                insert_string(metadata, "function_name", &name);
            }
        }
        "zig.comptime_parameter.v1" => {
            if let Some(name) = zig_parameter_name(content, node) {
                insert_string(metadata, "parameter_name", &name);
            }
        }
        "qml.import_statement.v1" => {
            if let Some(module) = qml_import_module(content, node) {
                insert_string(metadata, "import_module", &module);
            }
        }
        "qml.property_declaration.v1" => {
            if let Some(name) = qml_field_name(content, node, "name") {
                insert_string(metadata, "property_name", &name);
            }
            if let Some(property_type) = qml_field_name(content, node, "type") {
                insert_string(metadata, "property_type", &property_type);
            }
        }
        "qml.signal_declaration.v1" => {
            if let Some(name) = qml_field_name(content, node, "name") {
                insert_string(metadata, "signal_name", &name);
            }
        }
        "qml.binding.v1" => {
            if let Some(name) = qml_field_name(content, node, "name") {
                insert_string(metadata, "property_name", &name);
            }
        }
        "bash.export_declaration.v1" => {
            if let Some(name) = bash_export_variable_name(content, node) {
                insert_string(metadata, "variable_name", &name);
            }
        }
        "powershell.cmdlet_binding_attribute.v1" => {
            if let Some(name) = powershell_attribute_name(content, node) {
                insert_string(metadata, "attribute_name", &name);
            }
        }
        "powershell.pipeline_expression.v1" => {
            if let Some(marker) = powershell_pipeline_marker(content, node) {
                insert_string(metadata, "pipeline_marker", &marker);
            }
        }
        "powershell.class_definition.v1" => {
            if let Some(name) = powershell_class_name(content, node) {
                insert_string(metadata, "class_name", &name);
            }
        }
        "gdscript.class_name.v1" => {
            if let Some(name) = gdscript_named_field(content, node, "name") {
                insert_string(metadata, "class_name", &name);
            }
        }
        "gdscript.extends_declaration.v1" => {
            if let Some(base_type) = gdscript_extends_base_type(content, node) {
                insert_string(metadata, "base_type", &base_type);
            }
        }
        "gdscript.signal_declaration.v1" => {
            if let Some(name) = gdscript_named_field(content, node, "name") {
                insert_string(metadata, "signal_name", &name);
            }
        }
        "gdscript.export_annotation.v1" => {
            insert_string(metadata, "annotation_name", "export");
            if let Some(name) = gdscript_exported_variable_name(content, node) {
                insert_string(metadata, "exported_variable", &name);
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
        ("zig", "zig.builtin_call.v1") => zig_is_builtin_call(content, node),
        ("zig", "zig.threadlocal_variable.v1") => zig_has_keyword(content, node, "threadlocal"),
        ("zig", "zig.inline_function.v1") => zig_has_keyword(content, node, "inline"),
        ("zig", "zig.exported_function.v1") => zig_has_keyword(content, node, "export"),
        ("zig", "zig.comptime_parameter.v1") => zig_has_keyword(content, node, "comptime"),
        ("qml", "qml.binding.v1") => qml_is_semantic_property_binding(content, node),
        ("bash", "bash.shebang.v1") => node_text(content, node).trim_start().starts_with("#!"),
        ("bash", "bash.command_substitution.v1") => true,
        ("bash", "bash.arithmetic_expansion.v1") => true,
        ("bash", "bash.export_declaration.v1") => {
            bash_declaration_command(content, node).as_deref() == Some("export")
        }
        ("powershell", "powershell.cmdlet_binding_attribute.v1") => {
            powershell_attribute_name(content, node).as_deref() == Some("CmdletBinding")
        }
        ("powershell", "powershell.param_block.v1") => true,
        ("powershell", "powershell.pipeline_expression.v1") => {
            node_text(content, node).contains('|')
        }
        ("powershell", "powershell.class_definition.v1") => true,
        ("gdscript", "gdscript.class_name.v1") => true,
        ("gdscript", "gdscript.extends_declaration.v1") => true,
        ("gdscript", "gdscript.signal_declaration.v1") => true,
        ("gdscript", "gdscript.export_annotation.v1") => {
            gdscript_is_export_annotation(content, node)
        }
        ("gdscript", "gdscript.match_statement.v1") => true,
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

fn zig_has_keyword(content: &str, node: Node<'_>, keyword: &str) -> bool {
    if node.kind() == keyword || node_text(content, node) == keyword {
        return true;
    }
    if let Some(prev) = node.prev_sibling()
        && (prev.kind() == keyword || node_text(content, prev) == keyword)
    {
        return true;
    }
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .any(|child| child.kind() == keyword || node_text(content, child) == keyword)
}

fn zig_is_builtin_call(content: &str, node: Node<'_>) -> bool {
    match node.kind() {
        "builtin_function" => node
            .parent()
            .is_none_or(|parent| parent.kind() != "call_expression"),
        "call_expression" => node
            .child_by_field_name("function")
            .is_some_and(|function| {
                function.kind() == "builtin_function"
                    || node_text(content, function).starts_with('@')
            }),
        _ => false,
    }
}

fn zig_builtin_name(content: &str, node: Node<'_>) -> Option<String> {
    let raw = match node.kind() {
        "builtin_function" => first_named_identifier(content, node, &["builtin_identifier"])
            .or_else(|| {
                let text = node_text(content, node);
                text.split('(').next().map(str::trim).map(str::to_string)
            }),
        "call_expression" => node
            .child_by_field_name("function")
            .and_then(|function| zig_builtin_name(content, function)),
        _ => None,
    }?;
    Some(raw.strip_prefix('@').unwrap_or(&raw).to_string())
}

fn zig_variable_name(content: &str, node: Node<'_>) -> Option<String> {
    node.child_by_field_name("name")
        .map(|name| node_text(content, name))
        .or_else(|| first_named_identifier(content, node, &["identifier"]))
}

fn zig_function_name(content: &str, node: Node<'_>) -> Option<String> {
    node.child_by_field_name("name")
        .map(|name| node_text(content, name))
}

fn zig_parameter_name(content: &str, node: Node<'_>) -> Option<String> {
    node.child_by_field_name("name")
        .map(|name| node_text(content, name))
        .or_else(|| first_named_identifier(content, node, &["identifier"]))
}

fn qml_field_name(content: &str, node: Node<'_>, field: &str) -> Option<String> {
    node.child_by_field_name(field)
        .map(|field_node| node_text(content, field_node))
}

fn qml_import_module(content: &str, node: Node<'_>) -> Option<String> {
    qml_field_name(content, node, "source")
}

fn qml_is_semantic_property_binding(content: &str, node: Node<'_>) -> bool {
    let Some(name) = qml_field_name(content, node, "name") else {
        return false;
    };
    if name == "id" {
        return false;
    }
    if name.starts_with("on")
        && name.len() > 2
        && name
            .as_bytes()
            .get(2)
            .is_some_and(|b| b.is_ascii_uppercase())
    {
        return false;
    }
    true
}

fn bash_declaration_command(content: &str, node: Node<'_>) -> Option<String> {
    let text = node_text(content, node);
    text.split_whitespace().next().map(str::to_string)
}

fn bash_export_variable_name(content: &str, node: Node<'_>) -> Option<String> {
    first_descendant_of_kind(node, "variable_assignment").and_then(|assignment| {
        assignment
            .child_by_field_name("name")
            .map(|name| node_text(content, name))
            .or_else(|| first_named_identifier(content, assignment, &["variable_name"]))
    })
}

fn powershell_attribute_name(content: &str, node: Node<'_>) -> Option<String> {
    node.child_by_field_name("name")
        .and_then(|name| {
            if name.kind() == "attribute_name" {
                first_named_identifier(content, name, &["type_spec"])
            } else {
                Some(node_text(content, name))
            }
        })
        .or_else(|| {
            first_named_identifier(content, node, &["attribute_name"]).and_then(|name| {
                name.split('(')
                    .next()
                    .map(str::trim)
                    .filter(|part| !part.is_empty())
                    .map(str::to_string)
            })
        })
}

fn powershell_pipeline_marker(content: &str, node: Node<'_>) -> Option<String> {
    let text = node_text(content, node);
    if text.contains('|') {
        Some("|".to_string())
    } else {
        None
    }
}

fn powershell_class_name(content: &str, node: Node<'_>) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if matches!(child.kind(), "simple_name" | "identifier" | "type_name") {
            return Some(node_text(content, child));
        }
    }
    None
}

fn gdscript_is_export_annotation(content: &str, node: Node<'_>) -> bool {
    first_named_identifier(content, node, &["identifier"]).is_some_and(|name| name == "export")
}

fn gdscript_exported_variable_name(content: &str, node: Node<'_>) -> Option<String> {
    if let Some(parent) = node.parent()
        && parent.kind() == "annotations"
        && let Some(variable) = parent.parent().filter(|grandparent| {
            matches!(
                grandparent.kind(),
                "variable_statement" | "export_variable_statement" | "onready_variable_statement"
            )
        })
    {
        return gdscript_named_field(content, variable, "name");
    }

    let mut sibling = node.next_sibling();
    while let Some(next) = sibling {
        match next.kind() {
            "variable_statement" | "export_variable_statement" | "onready_variable_statement" => {
                return gdscript_named_field(content, next, "name");
            }
            "annotations" => {}
            _ => break,
        }
        sibling = next.next_sibling();
    }
    None
}

fn gdscript_named_field(content: &str, node: Node<'_>, field: &str) -> Option<String> {
    node.child_by_field_name(field)
        .map(|field_node| node_text(content, field_node))
}

fn gdscript_extends_base_type(content: &str, node: Node<'_>) -> Option<String> {
    first_named_identifier(content, node, &["type", "identifier", "string"])
        .map(|name| name.trim_matches('"').to_string())
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
