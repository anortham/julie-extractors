use std::collections::{HashMap, HashSet};

use tree_sitter::{Node, Tree};

use crate::base::{Identifier, IdentifierKind};

type Span = (u32, u32);

pub(crate) fn enrich(language: &str, tree: &Tree, source: &str, identifiers: &mut [Identifier]) {
    let language = crate::language::language_spec(language).map_or(language, |spec| spec.name);
    if matches!(language, "bash" | "css" | "html" | "regex" | "vue" | "xml") {
        return;
    }
    let grammar_language = match language {
        "jsx" => "javascript",
        "tsx" => "typescript",
        language => language,
    };

    let mut identifiers_by_span: HashMap<Span, Vec<usize>> = HashMap::new();
    for (index, identifier) in identifiers.iter().enumerate() {
        let identifier_language = crate::language::language_spec(&identifier.language)
            .map_or(identifier.language.as_str(), |spec| spec.name);
        if identifier_language == language
            && matches!(
                identifier.kind,
                IdentifierKind::Call | IdentifierKind::MemberAccess
            )
            && !identifier.metadata.as_ref().is_some_and(|metadata| {
                metadata.contains_key("receiver") || metadata.contains_key("receiver_qualifier")
            })
        {
            identifiers_by_span
                .entry((identifier.start_byte, identifier.end_byte))
                .or_default()
                .push(index);
        }
    }
    if identifiers_by_span.is_empty() {
        return;
    }

    let mut receivers_by_span = HashMap::new();
    let mut python_super_call_receiver_spans = HashSet::new();
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        if let Some((receiver, member)) = dart_cascade_call_parts(grammar_language, node) {
            record_candidate(
                named_chain(grammar_language, receiver, source),
                member,
                &identifiers_by_span,
                &mut receivers_by_span,
            );
        }
        if let Some((parts, member)) = flat_member_parts(grammar_language, node, source) {
            record_candidate(
                Some(parts),
                member,
                &identifiers_by_span,
                &mut receivers_by_span,
            );
        }
        if let Some((parts, member)) =
            flattened_identifier_call_parts(grammar_language, node, source)
        {
            record_candidate(
                Some(parts),
                member,
                &identifiers_by_span,
                &mut receivers_by_span,
            );
        }
        if let Some((receiver, member)) = candidate_parts(grammar_language, node, source) {
            if grammar_language == "python"
                && is_python_super_call_receiver(receiver, source)
                && let Some(member) = terminal_name(member)
            {
                python_super_call_receiver_spans
                    .insert((member.start_byte() as u32, member.end_byte() as u32));
            }
            record_candidate(
                named_chain(grammar_language, receiver, source),
                member,
                &identifiers_by_span,
                &mut receivers_by_span,
            );
        }

        let mut cursor = node.walk();
        stack.extend(node.named_children(&mut cursor));
    }

    for (span, parts) in receivers_by_span {
        let Some(indices) = identifiers_by_span.get(&span) else {
            continue;
        };
        let Some(receiver_name) = parts.last() else {
            continue;
        };
        for index in indices {
            let identifier = &mut identifiers[*index];
            if language == "python"
                && receiver_name == "super"
                && identifier.kind == IdentifierKind::Call
                && python_super_call_receiver_spans.contains(&span)
            {
                continue;
            }
            let metadata = identifier.metadata.get_or_insert_with(HashMap::new);
            metadata.insert("receiver".to_string(), receiver_name.clone().into());
            if parts.len() > 1 {
                metadata.insert(
                    "receiver_qualifier".to_string(),
                    parts[..parts.len() - 1].join(".").into(),
                );
            }
        }
    }
}

fn is_python_super_call_receiver(node: Node<'_>, source: &str) -> bool {
    if node.kind() != "call" {
        return false;
    }
    node.child_by_field_name("function")
        .is_some_and(|function| {
            function.kind() == "identifier" && node_text(function, source) == Some("super")
        })
}

fn record_candidate(
    parts: Option<Vec<String>>,
    member: Node<'_>,
    identifiers_by_span: &HashMap<Span, Vec<usize>>,
    receivers_by_span: &mut HashMap<Span, Vec<String>>,
) {
    let Some(member) = terminal_name(member) else {
        return;
    };
    let span = (member.start_byte() as u32, member.end_byte() as u32);
    if !identifiers_by_span.contains_key(&span) {
        return;
    }
    let Some(parts) = parts.filter(|parts| !parts.is_empty()) else {
        return;
    };
    receivers_by_span
        .entry(span)
        .and_modify(|existing| {
            if parts.len() > existing.len() {
                *existing = parts.clone();
            }
        })
        .or_insert(parts);
}

fn candidate_parts<'tree>(
    language: &str,
    node: Node<'tree>,
    source: &str,
) -> Option<(Node<'tree>, Node<'tree>)> {
    access_parts(language, node, source).or_else(|| call_parts(language, node))
}

fn access_parts<'tree>(
    language: &str,
    node: Node<'tree>,
    source: &str,
) -> Option<(Node<'tree>, Node<'tree>)> {
    match (language, node.kind()) {
        ("c" | "cpp", "field_expression") => named_fields(node, "argument", "field"),
        ("rust" | "scala", "field_expression") => named_fields(node, "value", "field"),
        ("zig", "field_expression") => {
            let (object, member) = named_fields(node, "object", "member")?;
            if object.kind() == "error_union_type" {
                // The pinned Zig grammar wraps an unparenthesized `!self` receiver this way.
                let ok = object.child_by_field_name("ok")?;
                terminal_name(ok).map(|_| (ok, member))
            } else if object.kind() == "pointer_type" {
                let target =
                    object.named_child(object.named_child_count().checked_sub(1)? as u32)?;
                terminal_name(target).map(|_| (target, member))
            } else {
                Some((object, member))
            }
        }
        ("javascript" | "typescript" | "qml", "member_expression") => {
            named_fields(node, "object", "property")
        }
        ("dart", "member_expression") => {
            let (object, member) = named_fields(node, "object", "property")?;
            if object.kind() == "function_expression" {
                let body = object.child_by_field_name("body")?;
                if body.kind() != "function_expression_body" || body.named_child_count() != 1 {
                    return None;
                }
                let target = body.named_child(0)?;
                terminal_name(target).map(|_| (target, member))
            } else {
                Some((object, member))
            }
        }
        ("dart", "null_aware_member_expression") => named_fields(node, "object", "property"),
        ("dart", "cascade_member_expression" | "cascade_null_aware_member_expression") => {
            named_fields(node, "object", "property")
        }
        ("python", "attribute") => named_fields(node, "object", "attribute"),
        ("go", "selector_expression") => named_fields(node, "operand", "field"),
        ("java", "field_access") => named_fields(node, "object", "field"),
        ("csharp" | "razor", "member_access_expression") => {
            named_fields(node, "expression", "name")
        }
        ("csharp" | "razor", "conditional_access_expression") => {
            let mut cursor = node.walk();
            let binding = node
                .named_children(&mut cursor)
                .find(|child| child.kind() == "member_binding_expression")?;
            Some((
                node.child_by_field_name("condition")?,
                binding.child_by_field_name("name")?,
            ))
        }
        ("vbnet", "member_access" | "null_conditional_member_access") => {
            named_fields(node, "object", "member")
        }
        ("php", "class_constant_access_expression") => {
            Some((node.named_child(0)?, node.named_child(1)?))
        }
        ("php", "member_access_expression") => named_fields(node, "object", "name"),
        ("lua", "dot_index_expression") => named_fields(node, "table", "field"),
        ("ruby", "scope_resolution") => named_fields(node, "scope", "name"),
        ("fsharp", "dot_expression") => named_fields(node, "base", "field"),
        ("elixir", "dot") => named_fields(node, "left", "right"),
        ("r", "extract_operator" | "namespace_operator") => named_fields(node, "lhs", "rhs"),
        ("r", "binary_operator")
            if matches!(field_text(node, "operator", source), Some("$" | "@")) =>
        {
            named_fields(node, "lhs", "rhs")
        }
        ("sql", "field") => {
            let value = node.named_child(0)?;
            if value.kind() == "object_reference" {
                Some((value, node.child_by_field_name("name")?))
            } else {
                None
            }
        }
        ("erlang", "field_expr") => named_fields(node, "expr", "field"),
        ("kotlin", "navigation_expression") => Some((node.named_child(0)?, node.named_child(1)?)),
        ("swift", "navigation_expression") => {
            let (target, member) = named_fields(node, "target", "suffix")?;
            if target.kind() == "prefix_expression" {
                // The grammar puts an unparenthesized prefix on the receiver, before member access.
                let value = target.child_by_field_name("target")?;
                (value.kind() == "simple_identifier").then_some((value, member))
            } else {
                Some((target, member))
            }
        }
        ("powershell", "member_access") => Some((node.named_child(0)?, node.named_child(1)?)),
        ("elixir", "access_call") => named_fields(node, "target", "key"),
        _ => None,
    }
}

fn call_parts<'tree>(language: &str, node: Node<'tree>) -> Option<(Node<'tree>, Node<'tree>)> {
    match (language, node.kind()) {
        ("rust", "scoped_identifier") => named_fields(node, "path", "name"),
        ("java", "method_invocation") => named_fields(node, "object", "name"),
        ("java", "method_reference") => Some((
            node.named_child(0)?,
            node.named_child(node.named_child_count().checked_sub(1)? as u32)?,
        )),
        ("dart", "constructor_invocation" | "new_expression") => {
            named_fields(node, "type", "constructor")
        }
        ("ruby", "call") => named_fields(node, "receiver", "method"),
        ("php", "member_call_expression") => named_fields(node, "object", "name"),
        ("php", "scoped_call_expression") => named_fields(node, "scope", "name"),
        ("erlang", "remote") => named_fields(node, "module", "fun"),
        ("powershell", "member_access") => Some((node.named_child(0)?, node.named_child(1)?)),
        _ => None,
    }
}

fn flat_member_parts<'tree>(
    language: &str,
    node: Node<'tree>,
    source: &str,
) -> Option<(Vec<String>, Node<'tree>)> {
    let parent = node.parent()?;
    if !matches!(
        (language, parent.kind()),
        ("gdscript", "attribute")
            | ("python", "dotted_name")
            | ("vbnet", "namespace_name")
            | ("dart", "type")
    ) {
        return None;
    }
    if language == "dart"
        && !matches!(
            parent.parent()?.kind(),
            "new_expression" | "constructor_invocation"
        )
    {
        return None;
    }
    let member = if node.kind() == "attribute_call" {
        node.named_child(0)?
    } else {
        terminal_name(node)?
    };
    let mut parts = Vec::new();
    let mut cursor = parent.walk();
    for child in parent
        .named_children(&mut cursor)
        .filter(|child| !child.is_extra())
    {
        if child == node {
            return (!parts.is_empty()).then_some((parts, member));
        }
        parts.extend(named_chain(language, child, source)?);
    }
    None
}

fn dart_cascade_call_parts<'tree>(
    language: &str,
    node: Node<'tree>,
) -> Option<(Node<'tree>, Node<'tree>)> {
    if language != "dart" || node.kind() != "cascade_call_expression" {
        return None;
    }

    let mut section = node.parent()?;
    if section.kind() != "cascade_section" {
        return None;
    }
    let receiver = loop {
        let previous = section.prev_named_sibling()?;
        if previous.kind() == "cascade_section" {
            section = previous;
        } else {
            break previous;
        }
    };
    Some((receiver, node.child_by_field_name("property")?))
}

fn flattened_identifier_call_parts<'tree>(
    language: &str,
    node: Node<'tree>,
    source: &str,
) -> Option<(Vec<String>, Node<'tree>)> {
    let names = match language {
        "cpp" if node.kind() == "qualified_identifier" => {
            let mut names = Vec::new();
            cpp_qualified_names(node, &mut names, source)?;
            names
        }
        "fsharp" if matches!(node.kind(), "long_identifier" | "long_identifier_or_op") => {
            fsharp_name_parts(node, source)?
        }
        _ => return None,
    };
    if names.len() < 2 {
        return None;
    }

    let member = match language {
        "cpp" => cpp_qualified_member(node)?,
        "fsharp" => terminal_name(node)?,
        _ => return None,
    };
    Some((names[..names.len() - 1].to_vec(), member))
}

fn cpp_qualified_names<'tree>(
    node: Node<'tree>,
    names: &mut Vec<String>,
    source: &str,
) -> Option<()> {
    let mut stack = vec![node];
    while let Some(node) = stack.pop() {
        match node.kind() {
            "qualified_identifier" => {
                stack.push(node.child_by_field_name("name")?);
                stack.push(node.child_by_field_name("scope")?);
            }
            "template_type" | "template_function" | "template_method" => {
                stack.push(node.child_by_field_name("name")?);
            }
            "identifier" | "field_identifier" | "type_identifier" | "namespace_identifier" => {
                names.push(node_text(node, source)?.to_string());
            }
            _ => return None,
        }
    }
    Some(())
}

fn cpp_qualified_member(mut node: Node<'_>) -> Option<Node<'_>> {
    loop {
        match node.kind() {
            "qualified_identifier" => node = node.child_by_field_name("name")?,
            "template_function" | "template_method" => {
                node = node.child_by_field_name("name")?;
            }
            _ => return terminal_name(node),
        }
    }
}

fn fsharp_name_parts(node: Node<'_>, source: &str) -> Option<Vec<String>> {
    let mut parts = Vec::new();
    let mut stack = vec![node];
    while let Some(node) = stack.pop() {
        match node.kind() {
            "long_identifier" | "long_identifier_or_op" => {
                let mut cursor = node.walk();
                let children = node.named_children(&mut cursor).collect::<Vec<_>>();
                if children.is_empty() {
                    return None;
                }
                stack.extend(children.into_iter().rev());
            }
            _ => parts.push(node_text(terminal_name(node)?, source)?.to_string()),
        }
    }
    Some(parts)
}

fn php_qualified_names(node: Node<'_>, names: &mut Vec<String>, source: &str) -> Option<()> {
    let mut stack = vec![node];
    while let Some(node) = stack.pop() {
        match node.kind() {
            "qualified_name" | "namespace_name" => {
                let mut cursor = node.walk();
                let children = node.named_children(&mut cursor).collect::<Vec<_>>();
                if children.is_empty() {
                    return None;
                }
                stack.extend(children.into_iter().rev());
            }
            "name" | "identifier" => names.push(node_text(node, source)?.to_string()),
            _ => return None,
        }
    }
    Some(())
}

fn named_fields<'tree>(
    node: Node<'tree>,
    receiver: &str,
    member: &str,
) -> Option<(Node<'tree>, Node<'tree>)> {
    Some((
        node.child_by_field_name(receiver)?,
        node.child_by_field_name(member)?,
    ))
}

fn terminal_name<'tree>(node: Node<'tree>) -> Option<Node<'tree>> {
    match node.kind() {
        "identifier"
        | "field_identifier"
        | "property_identifier"
        | "private_property_identifier"
        | "type_identifier"
        | "namespace_identifier"
        | "simple_identifier"
        | "simple_name"
        | "name"
        | "constant"
        | "variable_name"
        | "variable"
        | "this"
        | "self"
        | "super"
        | "self_expression"
        | "this_expression"
        | "super_expression" => Some(node),
        "navigation_suffix" => terminal_name(node.child_by_field_name("suffix")?),
        "member_name" => terminal_name(node.named_child(0)?),
        "long_identifier_or_op" => {
            terminal_name(node.named_child(node.named_child_count().checked_sub(1)? as u32)?)
        }
        "long_identifier" => {
            terminal_name(node.named_child(node.named_child_count().checked_sub(1)? as u32)?)
        }
        "generic_name" => terminal_name(node.named_child(0)?),
        "template_function" | "template_method" | "template_type" => {
            terminal_name(node.child_by_field_name("name")?)
        }
        _ => None,
    }
}

fn named_chain(language: &str, mut node: Node<'_>, source: &str) -> Option<Vec<String>> {
    let mut reversed = Vec::new();
    loop {
        if let Some((receiver, member)) = access_parts(language, node, source) {
            reversed.push(node_text(terminal_name(member)?, source)?.to_string());
            node = receiver;
            continue;
        }

        match node.kind() {
            "call"
                if language == "ruby"
                    && node.child_by_field_name("arguments").is_none()
                    && node.child_by_field_name("block").is_none() =>
            {
                let (receiver, member) = named_fields(node, "receiver", "method")?;
                reversed.push(node_text(terminal_name(member)?, source)?.to_string());
                node = receiver;
            }
            "call"
                if language == "elixir"
                    && node.child_by_field_name("arguments").is_none()
                    && node.child_by_field_name("do_block").is_none() =>
            {
                let target = node.child_by_field_name("target")?;
                if target.kind() != "dot" {
                    return None;
                }
                node = target;
            }
            "call" if language == "python" => {
                let function = node.child_by_field_name("function")?;
                if function.kind() != "identifier" || node_text(function, source)? != "super" {
                    return None;
                }
                reversed.push("super".to_string());
                break;
            }
            "alias" if language == "elixir" => {
                reversed.extend(node_text(node, source)?.split('.').rev().map(str::to_owned));
                break;
            }
            "atom" if language == "elixir" => {
                reversed.push(node_text(node, source)?.strip_prefix(':')?.to_string());
                break;
            }
            "type" if language == "dart" => {
                let mut cursor = node.walk();
                let mut parts = Vec::new();
                for child in node
                    .named_children(&mut cursor)
                    .filter(|child| !child.is_extra())
                {
                    if child.kind() == "type_arguments" {
                        continue;
                    }
                    if child.kind() != "type_identifier" {
                        return None;
                    }
                    parts.push(node_text(child, source)?.to_string());
                }
                if parts.is_empty() {
                    return None;
                }
                reversed.extend(parts.into_iter().rev());
                break;
            }
            "parenthesized_expression" => {
                let mut cursor = node.walk();
                node = node
                    .named_children(&mut cursor)
                    .find(|child| !child.is_extra())?;
            }
            "generic_function" => node = node.child_by_field_name("function")?,
            "generic_type" => node = node.child_by_field_name("type")?,
            "template_type" => node = node.child_by_field_name("name")?,
            "scoped_identifier" => {
                reversed.push(node_text(node.child_by_field_name("name")?, source)?.to_string());
                node = node.child_by_field_name("path")?;
            }
            "qualified_identifier" | "scope_resolution" => {
                reversed.push(node_text(node.child_by_field_name("name")?, source)?.to_string());
                node = node.child_by_field_name("scope")?;
            }
            "namespace_operator" | "extract_operator" => {
                reversed.push(node_text(node.child_by_field_name("rhs")?, source)?.to_string());
                node = node.child_by_field_name("lhs")?;
            }
            "binary_operator"
                if language == "r"
                    && matches!(field_text(node, "operator", source), Some("$" | "@")) =>
            {
                reversed.push(node_text(node.child_by_field_name("rhs")?, source)?.to_string());
                node = node.child_by_field_name("lhs")?;
            }
            "long_identifier_or_op" | "long_identifier" => {
                let parts = fsharp_name_parts(node, source)?;
                if parts.is_empty() {
                    return None;
                }
                reversed.extend(parts.into_iter().rev());
                break;
            }
            "qualified_name" if language == "php" => {
                let mut parts = Vec::new();
                php_qualified_names(node, &mut parts, source)?;
                if parts.is_empty() {
                    return None;
                }
                reversed.extend(parts.into_iter().rev());
                break;
            }
            "object_reference" if language == "sql" => {
                let parts = ["database", "schema", "name"]
                    .into_iter()
                    .filter_map(|field| node.child_by_field_name(field))
                    .map(|part| {
                        node_text(part, source).map(crate::sql::helpers::normalize_sql_identifier)
                    })
                    .collect::<Option<Vec<_>>>()?;
                if parts.is_empty() {
                    return None;
                }
                reversed.extend(parts.into_iter().rev());
                break;
            }
            "identifier"
            | "field_identifier"
            | "property_identifier"
            | "private_property_identifier"
            | "type_identifier"
            | "namespace_identifier"
            | "simple_identifier"
            | "name"
            | "constant"
            | "variable_name"
            | "variable"
            | "instance_variable"
            | "class_variable"
            | "global_variable"
            | "predefined_type"
            | "relative_scope"
            | "me_expression"
            | "get_node"
            | "base"
            | "this"
            | "self"
            | "super"
            | "self_expression"
            | "this_expression"
            | "super_expression"
            | "alias" => {
                reversed.push(node_text(node, source)?.to_string());
                break;
            }
            _ => return None,
        }
    }

    reversed.reverse();
    Some(reversed)
}

fn field_text<'source>(node: Node<'_>, field: &str, source: &'source str) -> Option<&'source str> {
    node.child_by_field_name(field)
        .and_then(|child| child.utf8_text(source.as_bytes()).ok())
}

fn node_text<'tree>(node: Node<'tree>, source: &'tree str) -> Option<&'tree str> {
    node.utf8_text(source.as_bytes()).ok()
}
