use crate::base::{BaseExtractor, BodySpan, NormalizedSpan, Symbol, SymbolKind, Visibility};
use serde_json::Value;
use std::collections::HashMap;
use tree_sitter::Node;

pub fn extract_modifiers(base: &BaseExtractor, node: &Node) -> Vec<String> {
    let modifiers_node = node.child_by_field_name("modifiers");
    let Some(modifiers_node) = modifiers_node else {
        return Vec::new();
    };

    let mut cursor = modifiers_node.walk();
    modifiers_node
        .children(&mut cursor)
        .filter(|c| c.kind() == "modifier")
        .map(|c| base.get_node_text(&c).to_lowercase())
        .collect()
}

pub fn determine_visibility(modifiers: &[String], default_visibility: &str) -> Visibility {
    let default = match default_visibility {
        "public" => Visibility::Public,
        "protected" => Visibility::Protected,
        _ => Visibility::Private,
    };
    crate::base::visibility::visibility_from_modifiers_with_default(modifiers, default)
}

pub fn get_vb_visibility_string(modifiers: &[String], default_visibility: &str) -> String {
    let has_public = modifiers.iter().any(|m| m == "public");
    let has_private = modifiers.iter().any(|m| m == "private");
    let has_protected = modifiers.iter().any(|m| m == "protected");
    let has_friend = modifiers.iter().any(|m| m == "friend");

    if has_public {
        "public".to_string()
    } else if has_private && has_protected {
        "private protected".to_string()
    } else if has_protected && has_friend {
        "protected friend".to_string()
    } else if has_private {
        "private".to_string()
    } else if has_protected {
        "protected".to_string()
    } else if has_friend {
        "friend".to_string()
    } else {
        default_visibility.to_string()
    }
}

pub fn vb_visibility_metadata(
    modifiers: &[String],
    default_visibility: &str,
) -> HashMap<String, Value> {
    let mut metadata = HashMap::new();
    metadata.insert(
        "vb_visibility".to_string(),
        Value::String(get_vb_visibility_string(modifiers, default_visibility)),
    );
    metadata
}

pub fn default_type_visibility(parent_id: Option<&String>) -> &'static str {
    if parent_id.is_some() {
        "public"
    } else {
        "friend"
    }
}

pub fn extract_return_type(base: &BaseExtractor, node: &Node) -> Option<String> {
    let rt = node.child_by_field_name("return_type")?;
    Some(base.get_node_text(&rt))
}

pub fn extract_parameters(base: &BaseExtractor, node: &Node) -> String {
    node.child_by_field_name("parameters")
        .map(|p| base.get_node_text(&p))
        .unwrap_or_else(|| "()".to_string())
}

pub fn extract_type_parameters(base: &BaseExtractor, node: &Node) -> Option<String> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find(|c| c.kind() == "type_parameters")
        .map(|tp| base.get_node_text(&tp))
}

pub fn extract_as_clause_type(base: &BaseExtractor, node: &Node) -> Option<String> {
    let mut cursor = node.walk();
    let as_clause = node
        .children(&mut cursor)
        .find(|c| c.kind() == "as_clause")?;
    let type_node = as_clause.child_by_field_name("type")?;
    Some(base.get_node_text(&type_node))
}

pub fn extract_inherits(base: &BaseExtractor, node: &Node) -> Vec<String> {
    let inherits_node = node.child_by_field_name("inherits");
    let Some(inherits_node) = inherits_node else {
        return Vec::new();
    };

    let mut cursor = inherits_node.walk();
    inherits_node
        .children(&mut cursor)
        .filter(|c| c.kind() != "," && !base.get_node_text(c).eq_ignore_ascii_case("Inherits"))
        .map(|c| base.get_node_text(&c))
        .collect()
}

pub fn extract_implements(base: &BaseExtractor, node: &Node) -> Vec<String> {
    let mut result = Vec::new();
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "implements_clause" {
            let mut inner_cursor = child.walk();
            for inner in child.children(&mut inner_cursor) {
                let text = base.get_node_text(&inner);
                if inner.kind() != "," && !text.eq_ignore_ascii_case("Implements") {
                    result.push(text);
                }
            }
        }
    }
    result
}

pub fn extract_attributes(base: &BaseExtractor, node: &Node) -> Vec<String> {
    let mut attrs = Vec::new();
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "attribute_block" {
            let mut inner_cursor = child.walk();
            for attr in child.children(&mut inner_cursor) {
                if attr.kind() == "attribute"
                    && let Some(name_node) = attr.child_by_field_name("name")
                {
                    attrs.push(base.get_node_text(&name_node));
                }
            }
        }
    }
    attrs
}

pub fn modifier_prefix(modifiers: &[String]) -> String {
    if modifiers.is_empty() {
        String::new()
    } else {
        format!("{} ", modifiers.join(" "))
    }
}

pub fn unresolved_type_target(type_name: &str) -> Option<crate::base::UnresolvedTarget> {
    let normalized = normalize_type_name(type_name)?;
    let parts: Vec<String> = normalized
        .split('.')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(ToString::to_string)
        .collect();

    if parts.is_empty() {
        return None;
    }

    let terminal_name = parts.last().cloned()?;
    let namespace_path = if parts.len() > 1 {
        parts[..parts.len() - 1].to_vec()
    } else {
        Vec::new()
    };

    Some(crate::base::UnresolvedTarget {
        display_name: parts.join("."),
        terminal_name,
        receiver: None,
        namespace_path,
        import_context: None,
    })
}

fn normalize_type_name(type_name: &str) -> Option<String> {
    let mut normalized = type_name.trim().to_string();
    if normalized.is_empty() {
        return None;
    }

    while let Some(stripped) = normalized.strip_suffix("()") {
        normalized = stripped.trim_end().to_string();
    }

    if let Some(generic_start) = normalized.find("(Of") {
        normalized.truncate(generic_start);
        normalized = normalized.trim_end().to_string();
    }

    if let Some(generic_start) = normalized.find("(of") {
        normalized.truncate(generic_start);
        normalized = normalized.trim_end().to_string();
    }

    let predefined = [
        "boolean", "byte", "sbyte", "short", "ushort", "integer", "uinteger", "long", "ulong",
        "single", "double", "decimal", "char", "string", "date", "object",
    ];

    if predefined.contains(&normalized.to_ascii_lowercase().as_str()) {
        return None;
    }

    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

pub fn find_vbnet_doc_comment(base: &BaseExtractor, node: &Node) -> Option<String> {
    let spec = crate::language::language_spec("vbnet")?;
    let start = node
        .parent()
        .filter(|parent| parent.kind() == "type_declaration")
        .unwrap_or(*node);
    let mut comments = Vec::new();
    let mut current = start.prev_sibling();
    while let Some(sibling) = current {
        match sibling.kind() {
            "comment" => {
                let text = base.get_node_text(&sibling);
                if spec.is_doc_comment(text.trim_start()) {
                    comments.push(text);
                    current = sibling.prev_sibling();
                } else {
                    break;
                }
            }
            "blank_line" if comments.is_empty() || ends_comment_line(&sibling) => {
                current = sibling.prev_sibling();
            }
            _ if is_vbnet_doc_barrier(&sibling) => {
                current = sibling.prev_sibling();
            }
            _ => break,
        }
    }

    comments.reverse();
    if comments.is_empty() {
        None
    } else {
        Some(comments.join("\n"))
    }
}

/// The grammar emits the line break after a doc comment as a `blank_line`
/// on the comment's own row; a real empty line starts on a later row.
fn ends_comment_line(blank_line: &Node) -> bool {
    blank_line.prev_sibling().is_some_and(|previous| {
        previous.kind() == "comment"
            && previous.end_position().row == blank_line.start_position().row
    })
}

fn is_vbnet_doc_barrier(node: &Node) -> bool {
    matches!(node.kind(), "attribute" | "attribute_list" | "attributes")
        || node.kind().contains("attribute")
}

/// The body span of a VB declaration block: from the line after the header
/// (after any `Inherits`, `Implements`, or `Handles` clause) to the `End`
/// keyword. Declarations without an `End` line have no body.
pub(super) fn body_span(node: &Node, content: &str) -> Option<BodySpan> {
    if !matches!(
        node.kind(),
        "class_block"
            | "module_block"
            | "structure_block"
            | "interface_block"
            | "enum_block"
            | "namespace_block"
            | "method_declaration"
            | "constructor_declaration"
            | "operator_declaration"
            | "property_declaration"
            | "event_declaration"
            | "get_accessor"
            | "set_accessor"
            | "add_handler_block"
            | "remove_handler_block"
            | "raise_event_block"
    ) {
        return None;
    }
    let text = content.get(node.start_byte()..node.end_byte())?;
    let trimmed_len = text.trim_end().len();
    let last_line_start = text[..trimmed_len].rfind('\n').map_or(0, |index| index + 1);
    let last_line = &text[last_line_start..trimmed_len];
    let end_keyword = last_line.trim_start();
    let is_end_line = end_keyword.len() >= 3
        && end_keyword.is_char_boundary(3)
        && end_keyword[..3].eq_ignore_ascii_case("end")
        && end_keyword[3..]
            .chars()
            .next()
            .is_none_or(|next| !next.is_alphanumeric() && next != '_');
    if last_line_start == 0 || !is_end_line {
        return None;
    }
    let body_end = node.start_byte() + last_line_start + (last_line.len() - end_keyword.len());

    let header_end = header_end_byte(node);
    let body_start = content
        .get(header_end..body_end)
        .and_then(|rest| rest.find('\n'))
        .map_or(body_end, |index| header_end + index + 1);
    NormalizedSpan::from_content_range(content, body_start.min(body_end), body_end)
}

fn header_end_byte(node: &Node) -> usize {
    let mut end = node.start_byte();
    let mut cursor = node.walk();
    for (index, child) in node.children(&mut cursor).enumerate() {
        if !child.is_named() || child.kind() == "blank_line" {
            continue;
        }
        let is_header = node.field_name_for_child(index as u32).is_some()
            || matches!(
                child.kind(),
                "as_clause"
                    | "parameter_list"
                    | "type_parameter_list"
                    | "handles_clause"
                    | "implements_clause"
                    | "inherits_clause"
                    | "attribute_block"
            );
        if !is_header {
            break;
        }
        end = child.end_byte();
    }
    end
}

/// The expression that names the callee of a call site, or `None` when
/// `node` is not a call. Covers `F()`, `x.F()`, `x?.F()`, `.F()` inside a
/// `With` block, and parenless call statements (`x.F`, `Call F`).
pub(super) fn call_callee(node: Node) -> Option<Node> {
    match node.kind() {
        "invocation" | "invocation_expression" => {
            node.child_by_field_name("target").or_else(|| {
                let mut cursor = node.walk();
                node.children(&mut cursor).next()
            })
        }
        "element_access" => node.child_by_field_name("object").filter(|object| {
            matches!(
                object.kind(),
                "null_conditional_member_access" | "implicit_member_access"
            )
        }),
        "call_statement" => {
            let mut cursor = node.walk();
            let mut named = node.named_children(&mut cursor);
            let callee = named.next()?;
            (named.next().is_none()
                && matches!(
                    callee.kind(),
                    "identifier"
                        | "member_access"
                        | "null_conditional_member_access"
                        | "implicit_member_access"
                ))
            .then_some(callee)
        }
        _ => None,
    }
}

/// True when `callee` is the callee expression of its parent call site.
pub(super) fn is_call_callee(callee: Node) -> bool {
    callee
        .parent()
        .and_then(call_callee)
        .is_some_and(|found| found.id() == callee.id())
}

/// The receiver of a `.Member` access: the enclosing `With` target.
pub(super) fn with_target(node: Node) -> Option<Node> {
    let mut current = node.parent();
    while let Some(candidate) = current {
        if candidate.kind() == "with_statement" {
            return candidate.child_by_field_name("target");
        }
        current = candidate.parent();
    }
    None
}

/// The member symbol whose declaration encloses `node`: a method,
/// constructor, operator, property, or event. Code outside those members
/// (field initializers) yields `None`, so callers fall back to the type.
pub(super) fn enclosing_member_symbol<'a>(node: Node, symbols: &'a [Symbol]) -> Option<&'a Symbol> {
    let mut current = node.parent();
    while let Some(candidate) = current {
        match candidate.kind() {
            "method_declaration"
            | "constructor_declaration"
            | "operator_declaration"
            | "property_declaration"
            | "event_declaration" => {
                let start = candidate.start_byte() as u32;
                return symbols.iter().find(|symbol| {
                    symbol.start_byte == start
                        && matches!(
                            symbol.kind,
                            SymbolKind::Method
                                | SymbolKind::Function
                                | SymbolKind::Constructor
                                | SymbolKind::Operator
                                | SymbolKind::Property
                                | SymbolKind::Event
                        )
                });
            }
            "class_block" | "module_block" | "structure_block" | "interface_block" => {
                return None;
            }
            _ => current = candidate.parent(),
        }
    }
    None
}

/// `Dim x = New A.B.C()` parses as member accesses on a bare `New A`. When
/// `node` is a `member_access` rooted at such a `New`, returns that
/// `new_expression`.
pub(super) fn misparsed_new_root(node: Node) -> Option<Node> {
    if node.kind() != "member_access" {
        return None;
    }
    let mut current = node;
    while current.kind() == "member_access" {
        current = current.child_by_field_name("object")?;
    }
    (current.kind() == "new_expression" && is_bare_new(current)).then_some(current)
}

/// A `new_expression` with only a type: no arguments, initializer, or rank.
pub(super) fn is_bare_new(node: Node) -> bool {
    let type_id = node.child_by_field_name("type").map(|ty| ty.id());
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .all(|child| Some(child.id()) == type_id)
}

/// The qualified type name of a misparsed `New A.B.C` chain ending at
/// `outer` (a `member_access`), with a leading `Global.` removed.
pub(super) fn misparsed_new_type_name(base: &BaseExtractor, outer: Node) -> Option<String> {
    let root = misparsed_new_root(outer)?;
    let mut parts = vec![type_name_text(base, root.child_by_field_name("type")?)?];
    let mut members = Vec::new();
    let mut current = outer;
    while current.kind() == "member_access" {
        members.push(base.get_node_text(&current.child_by_field_name("member")?));
        current = current.child_by_field_name("object")?;
    }
    members.reverse();
    parts.extend(members);
    Some(strip_global(&parts.join(".")))
}

/// The dotted name of a type node: `namespace_name` text, the base of a
/// generic type, or the element of an array or nullable type.
pub(super) fn type_name_text(base: &BaseExtractor, node: Node) -> Option<String> {
    match node.kind() {
        "namespace_name" => {
            let mut cursor = node.walk();
            let parts: Vec<String> = node
                .named_children(&mut cursor)
                .filter(|child| child.kind() == "identifier")
                .map(|child| base.get_node_text(&child))
                .collect();
            (!parts.is_empty()).then(|| strip_global(&parts.join(".")))
        }
        "generic_type" => {
            let mut cursor = node.walk();
            let name = node
                .named_children(&mut cursor)
                .find(|child| child.kind() == "namespace_name")?;
            type_name_text(base, name)
        }
        "array_type" | "nullable_type" => {
            let element = node
                .child_by_field_name("element")
                .or_else(|| node.named_child(0))?;
            type_name_text(base, element)
        }
        _ => None,
    }
}

fn strip_global(name: &str) -> String {
    match name.get(..7) {
        Some(prefix) if prefix.eq_ignore_ascii_case("global.") => name[7..].to_string(),
        _ => name.to_string(),
    }
}
