/// Helper utilities for Elixir symbol extraction
use crate::base::BaseExtractor;
use tree_sitter::Node;

pub(super) use crate::base::find_child_by_type;

/// Extract the target name of a call node (e.g., "defmodule", "def", "use")
///
/// In tree-sitter-elixir, a call node has a `target` field which is typically
/// an `identifier` node containing the macro/function name.
pub(super) fn extract_call_target_name(base: &BaseExtractor, node: &Node) -> Option<String> {
    // `target` IS a named field in tree-sitter-elixir
    let target = node.child_by_field_name("target")?;
    match target.kind() {
        "identifier" | "dot" | "alias" => Some(base.get_node_text(&target)),
        _ => None,
    }
}

/// Extract module name from the first argument of defmodule.
/// The `arguments` node is a child type, NOT a named field.
pub(super) fn extract_module_name(base: &BaseExtractor, node: &Node) -> Option<String> {
    let args = find_child_by_type(node, "arguments")?;
    let mut cursor = args.walk();
    for child in args.children(&mut cursor) {
        if child.kind() == "alias" {
            return Some(base.get_node_text(&child));
        }
        if child.kind() == "dot" {
            return Some(base.get_node_text(&child));
        }
    }
    None
}

/// Extract function name and parameter string from a def/defp call.
///
/// The first argument of `def` is either:
/// - A `call` node (the function head): `def add(a, b)` -> call target="add", args="(a, b)"
/// - An `identifier` node for zero-arg functions: `def init` -> identifier "init"
/// - A `binary_operator` for guard clauses: `def validate(n) when is_number(n)` -> extract left
pub(super) fn extract_function_head(
    base: &BaseExtractor,
    node: &Node,
) -> Option<(String, Option<String>)> {
    let args = find_child_by_type(node, "arguments")?;
    let mut cursor = args.walk();
    for child in args.children(&mut cursor) {
        match child.kind() {
            "call" => {
                // The function head is itself a call: `add(a, b)`
                let fn_name_node = child.child_by_field_name("target")?;
                let fn_name = base.get_node_text(&fn_name_node);
                let params =
                    find_child_by_type(&child, "arguments").map(|a| base.get_node_text(&a));
                return Some((fn_name, params));
            }
            "identifier" => {
                // Zero-arg function: `def init`
                return Some((base.get_node_text(&child), None));
            }
            "binary_operator" => {
                // Guard clause: `validate(n) when is_number(n)`
                // The left side is the function head
                if let Some(left) = child.child_by_field_name("left") {
                    match left.kind() {
                        "call" => {
                            let fn_name_node = left.child_by_field_name("target")?;
                            let fn_name = base.get_node_text(&fn_name_node);
                            let params = find_child_by_type(&left, "arguments")
                                .map(|a| base.get_node_text(&a));
                            return Some((fn_name, params));
                        }
                        "identifier" => {
                            return Some((base.get_node_text(&left), None));
                        }
                        _ => {}
                    }
                }
            }
            _ => continue,
        }
    }
    None
}

/// Find the do_block child of a call node
pub(super) fn extract_do_block<'a>(node: &Node<'a>) -> Option<Node<'a>> {
    find_child_by_type(node, "do_block")
}

/// Extract the value for a keyword argument from a keywords/keyword list.
/// Used for extracting `for: Bar` from `defimpl Foo, for: Bar`.
pub(super) fn extract_keyword_value(
    base: &BaseExtractor,
    node: &Node,
    key: &str,
) -> Option<String> {
    let args = find_child_by_type(node, "arguments")?;
    let mut cursor = args.walk();
    for child in args.children(&mut cursor) {
        if child.kind() == "keywords" {
            return extract_keyword_from_keywords(base, &child, key);
        }
    }
    None
}

fn extract_keyword_from_keywords(
    base: &BaseExtractor,
    keywords: &Node,
    key: &str,
) -> Option<String> {
    let mut cursor = keywords.walk();
    for pair in keywords.children(&mut cursor) {
        if pair.kind() == "pair" {
            // In tree-sitter-elixir, pair has `key` and `value` fields
            if let Some(pair_key) = pair.child_by_field_name("key") {
                let key_text = base.get_node_text(&pair_key);
                // Keywords in Elixir: "for:" or "for: " — strip colon and whitespace
                let cleaned = key_text.trim().trim_end_matches(':').trim();
                if cleaned == key
                    && let Some(pair_val) = pair.child_by_field_name("value")
                {
                    return Some(base.get_node_text(&pair_val));
                }
            }
        }
    }
    None
}

/// Extract the protocol name from defimpl's first argument
pub(super) fn extract_impl_protocol_name(base: &BaseExtractor, node: &Node) -> Option<String> {
    let args = find_child_by_type(node, "arguments")?;
    let mut cursor = args.walk();
    for child in args.children(&mut cursor) {
        if child.kind() == "alias" {
            return Some(base.get_node_text(&child));
        }
    }
    None
}

/// Struct field names from a defstruct/defexception argument list, each with
/// the node that declares it: `:name` atoms of the list form and the keys of
/// the keyword form. Default values are never fields.
pub(super) fn extract_struct_fields<'a>(
    base: &BaseExtractor,
    node: &Node<'a>,
) -> Vec<(String, Node<'a>)> {
    let mut fields = Vec::new();
    let Some(args) = find_child_by_type(node, "arguments") else {
        return fields;
    };
    let mut cursor = args.walk();
    for arg in args.named_children(&mut cursor) {
        match arg.kind() {
            "list" => {
                let mut list_cursor = arg.walk();
                for item in arg.named_children(&mut list_cursor) {
                    collect_field(base, &item, &mut fields);
                }
            }
            _ => collect_field(base, &arg, &mut fields),
        }
    }
    fields
}

fn collect_field<'a>(base: &BaseExtractor, node: &Node<'a>, fields: &mut Vec<(String, Node<'a>)>) {
    match node.kind() {
        "atom" => {
            let name = base.get_node_text(node).trim_start_matches(':').to_string();
            if !name.is_empty() {
                fields.push((name, *node));
            }
        }
        "keywords" => {
            let mut cursor = node.walk();
            for pair in node.named_children(&mut cursor) {
                if let Some(key) = pair.child_by_field_name("key") {
                    let name = base
                        .get_node_text(&key)
                        .trim()
                        .trim_end_matches(':')
                        .to_string();
                    if !name.is_empty() {
                        fields.push((name, key));
                    }
                }
            }
        }
        _ => {}
    }
}

/// Extract the text content of the first string argument in a call's `(arguments)`.
///
/// Used for ExUnit constructs like `test "description" do ... end` and
/// `describe "context" do ... end`, where the first argument is a string literal.
/// Returns the string content with surrounding quotes stripped.
pub(super) fn extract_first_string_arg(base: &BaseExtractor, node: &Node) -> Option<String> {
    let args = find_child_by_type(node, "arguments")?;
    let mut cursor = args.walk();
    for child in args.children(&mut cursor) {
        if child.kind() == "string" {
            let text = base.get_node_text(&child);
            // Strip surrounding double quotes
            return Some(text.trim_matches('"').to_string());
        }
    }
    None
}

/// Modules named by an import/use/alias/require directive. The multi-alias
/// form `alias MyApp.{A, B}` names `MyApp.A` and `MyApp.B`.
pub(super) fn directive_modules(base: &BaseExtractor, node: &Node) -> Vec<String> {
    let Some(target) = find_child_by_type(node, "arguments").and_then(|args| {
        let mut cursor = args.walk();
        args.named_children(&mut cursor)
            .find(|child| matches!(child.kind(), "alias" | "dot"))
    }) else {
        return Vec::new();
    };
    let tuple = target
        .child_by_field_name("right")
        .filter(|right| target.kind() == "dot" && right.kind() == "tuple");
    match (tuple, target.child_by_field_name("left")) {
        (Some(tuple), Some(prefix)) => {
            let prefix = base.get_node_text(&prefix);
            let mut cursor = tuple.walk();
            tuple
                .named_children(&mut cursor)
                .filter(|member| member.kind() == "alias")
                .map(|member| format!("{prefix}.{}", base.get_node_text(&member)))
                .collect()
        }
        _ => vec![base.get_node_text(&target)],
    }
}

/// True when a typespec `call` applies at least one type parameter, e.g.
/// `list(integer())` but not zero-argument primitives like `integer()`.
pub(super) fn is_elixir_parameterized_type_call(node: &Node) -> bool {
    if node.kind() != "call" {
        return false;
    }
    let Some(target) = node.child_by_field_name("target") else {
        return false;
    };
    if target.kind() != "identifier" {
        return false;
    }
    let Some(args) = find_child_by_type(node, "arguments") else {
        return false;
    };
    elixir_args_has_type_param_call(&args)
}

fn elixir_args_has_type_param_call(args: &Node) -> bool {
    let mut cursor = args.walk();
    args.named_children(&mut cursor)
        .any(|child| child.kind() == "call")
}

/// Definition macros whose first argument is a function head.
pub(super) const HEADED_DEFINITIONS: &[&str] = &[
    "def",
    "defp",
    "defmacro",
    "defmacrop",
    "defguard",
    "defguardp",
    "defdelegate",
];

/// The first argument of a headed definition: the head call, a bare
/// identifier, or a `when` operator whose left side is the head.
pub(super) fn definition_head_argument<'a>(
    base: &BaseExtractor,
    node: &Node<'a>,
) -> Option<Node<'a>> {
    let target = extract_call_target_name(base, node)?;
    if !HEADED_DEFINITIONS.contains(&target.as_str()) {
        return None;
    }
    find_child_by_type(node, "arguments")?.named_child(0)
}

/// True when `node` is the function-head call of a definition (`run(id)` in
/// `def run(id)`), which names the definition rather than calling it.
pub(super) fn is_definition_head(base: &BaseExtractor, node: &Node) -> bool {
    let Some(mut arg) = node.parent() else {
        return false;
    };
    if arg.kind() == "binary_operator" {
        if arg.child_by_field_name("left").map(|l| l.id()) != Some(node.id()) {
            return false;
        }
    } else {
        arg = *node;
    }
    let Some(definition) = arg.parent().and_then(|args| args.parent()) else {
        return false;
    };
    definition_head_argument(base, &definition).is_some_and(|head| head.id() == arg.id())
}

/// The body of a def-style definition: its `do ... end` block, the value of a
/// `do:` keyword, or for guards the `when` expression. Bodyless heads have none.
pub(super) fn definition_body<'a>(base: &BaseExtractor, node: &Node<'a>) -> Option<Node<'a>> {
    if let Some(do_block) = extract_do_block(node) {
        return Some(do_block);
    }
    let args = find_child_by_type(node, "arguments")?;
    let mut cursor = args.walk();
    let do_value = args
        .named_children(&mut cursor)
        .filter(|child| child.kind() == "keywords")
        .find_map(|keywords| keyword_pair_value(base, &keywords, "do"));
    if do_value.is_some() {
        return do_value;
    }
    let target = extract_call_target_name(base, node)?;
    if matches!(target.as_str(), "defguard" | "defguardp") {
        return definition_head_argument(base, node)
            .filter(|head| head.kind() == "binary_operator")
            .and_then(|head| head.child_by_field_name("right"));
    }
    None
}

fn keyword_pair_value<'a>(
    base: &BaseExtractor,
    keywords: &Node<'a>,
    key: &str,
) -> Option<Node<'a>> {
    let mut cursor = keywords.walk();
    keywords
        .named_children(&mut cursor)
        .filter(|pair| pair.kind() == "pair")
        .find(|pair| {
            pair.child_by_field_name("key")
                .is_some_and(|k| base.get_node_text(&k).trim().trim_end_matches(':').trim() == key)
        })
        .and_then(|pair| pair.child_by_field_name("value"))
}

/// The text of a string or sigil literal with delimiters removed and heredoc
/// indentation stripped. Returns `None` for any other node kind.
pub(super) fn string_literal_content(base: &BaseExtractor, node: &Node) -> Option<String> {
    if !matches!(node.kind(), "string" | "sigil") {
        return None;
    }
    let text = base.get_node_text(node);
    let mut body = text.trim();
    if node.kind() == "sigil" {
        body = body.strip_prefix('~')?.get(1..)?;
    }
    let inner = ["\"\"\"", "'''", "\"", "'"]
        .iter()
        .find_map(|quote| body.strip_prefix(quote)?.strip_suffix(quote))?;
    let indent = inner
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.len() - line.trim_start().len())
        .min()
        .unwrap_or(0);
    let content = inner
        .lines()
        .map(|line| line.get(indent..).unwrap_or(line.trim_start()).trim_end())
        .collect::<Vec<_>>()
        .join("\n");
    let content = content.trim().to_string();
    (!content.is_empty()).then_some(content)
}

/// Replace the inferred body span with `body`, or clear it for bodyless
/// declarations, and keep the body hash in step.
pub(super) fn set_body_span(
    base: &BaseExtractor,
    symbol: &mut crate::base::Symbol,
    body: Option<Node>,
) {
    symbol.body_span = body.map(|node| crate::base::NormalizedSpan::from_node(&node));
    symbol.body_hash = symbol
        .body_span
        .and_then(|span| crate::base::body::body_hash(&base.content, span, &base.language));
}
