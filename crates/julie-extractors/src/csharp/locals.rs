// Local variables and parameters for C# callables.

use super::helpers;
use crate::base::{BaseExtractor, Symbol, SymbolKind, SymbolOptions, Visibility};
use std::collections::HashMap;
use tree_sitter::Node;

/// Extract a local `variable_declaration` / `local_declaration_statement`.
///
/// Multiple declarators (`int a = 1, b = 2;`) produce one symbol each, all
/// sharing the declared type.
pub fn extract_local_declaration(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
) -> Vec<Symbol> {
    let mut symbols = Vec::new();
    collect_local_bindings(base, node, parent_id, &mut symbols, 0);
    symbols
}

fn collect_local_bindings(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
    out: &mut Vec<Symbol>,
    depth: u32,
) {
    if depth > 32 {
        return;
    }
    match node.kind() {
        "variable_declaration" | "local_declaration_statement" => {
            let declaration = if node.kind() == "local_declaration_statement" {
                find_child(node, "variable_declaration").unwrap_or(node)
            } else {
                node
            };
            emit_declarators(base, declaration, parent_id.clone(), out);
        }
        "declaration_expression" => {
            // foreach / for headers already emit these bindings via the parent
            // statement arm; re-visiting would duplicate symbols for
            // `foreach ((var x, var y) in …)`.
            if is_loop_header_binding(node) {
                return;
            }
            // Only true out/ref/in var declarations — never bare `*` multiplications
            // misparsed as declaration_expression by tree-sitter-c-sharp.
            // Check parent argument text for `out`/`ref`/`in` because the
            // declaration node itself is often just `T x` or `var x`.
            let text = base.get_node_text(&node);
            let parent_text = node
                .parent()
                .map(|p| base.get_node_text(&p))
                .unwrap_or_default();
            let has_var_keyword = text.split_whitespace().any(|t| t == "var")
                || parent_text.split_whitespace().any(|t| t == "var");
            let has_out_ref = parent_text
                .split_whitespace()
                .any(|t| matches!(t, "out" | "ref" | "in"))
                || text
                    .split_whitespace()
                    .any(|t| matches!(t, "out" | "ref" | "in"));
            let looks_like_binding = has_var_keyword || has_out_ref;
            if looks_like_binding {
                if let Some(decl) = find_child(node, "variable_declaration") {
                    emit_declarators(base, decl, parent_id.clone(), out);
                } else if let Some(name) = node
                    .child_by_field_name("name")
                    .or_else(|| find_child(node, "identifier"))
                {
                    let name_text = base.get_node_text(&name);
                    if name_text != "_" {
                        let ty = node
                            .child_by_field_name("type")
                            .map(|t| base.get_node_text(&t))
                            .or_else(|| type_name_from_declaration(base, node));
                        let is_var = has_var_keyword
                            || ty
                                .as_deref()
                                .is_some_and(|t| t == "var" || t == "implicit_type");
                        if let Some(symbol) = extract_named_binding(
                            base,
                            node,
                            &name_text,
                            parent_id.clone(),
                            ty,
                            is_var,
                        ) {
                            out.push(symbol);
                        }
                    }
                }
            }
        }
        "catch_declaration" => {
            if let Some(name) = node.child_by_field_name("name").or_else(|| {
                let mut c = node.walk();
                node.children(&mut c).find(|c| c.kind() == "identifier")
            }) {
                let text = base.get_node_text(&name);
                if text != "_" {
                    let ty = node
                        .child_by_field_name("type")
                        .map(|t| base.get_node_text(&t));
                    if let Some(symbol) =
                        extract_named_binding(base, node, &text, parent_id.clone(), ty, false)
                    {
                        out.push(symbol);
                    }
                }
            }
        }
        "foreach_statement" | "for_each_statement" => {
            // foreach (Type item in items)
            // foreach (var (a, b) in pairs)  — tuple_pattern
            // foreach ((int x, string y) in pairs) — tuple_expression of decls
            if let Some(left) = node.child_by_field_name("left").or_else(|| {
                let mut c = node.walk();
                node.children(&mut c).find(|c| {
                    matches!(
                        c.kind(),
                        "identifier"
                            | "variable_declaration"
                            | "tuple_pattern"
                            | "tuple_expression"
                            | "declaration_expression"
                    )
                })
            }) {
                match left.kind() {
                    "identifier" => {
                        let text = base.get_node_text(&left);
                        if text != "_" {
                            let ty = node
                                .child_by_field_name("type")
                                .map(|t| base.get_node_text(&t));
                            let is_var = ty.as_deref().is_some_and(|t| t == "var");
                            if let Some(symbol) = extract_named_binding(
                                base,
                                node,
                                &text,
                                parent_id.clone(),
                                ty,
                                is_var,
                            ) {
                                out.push(symbol);
                            }
                        }
                    }
                    "variable_declaration" => {
                        emit_declarators(base, left, parent_id.clone(), out);
                    }
                    "tuple_pattern" | "tuple_expression" | "declaration_expression" => {
                        collect_pattern_bindings(base, left, parent_id.clone(), out, depth + 1);
                    }
                    _ => {
                        collect_pattern_bindings(base, left, parent_id.clone(), out, depth + 1);
                    }
                }
            } else {
                // Some grammars put type + pattern as sibling children without
                // a `left` field (`foreach (var (a, b) in …)`).
                let mut c = node.walk();
                for child in node.children(&mut c) {
                    if matches!(
                        child.kind(),
                        "tuple_pattern"
                            | "tuple_expression"
                            | "declaration_expression"
                            | "variable_declaration"
                    ) {
                        collect_pattern_bindings(base, child, parent_id.clone(), out, depth + 1);
                    }
                }
            }
        }
        _ => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if matches!(
                    child.kind(),
                    "variable_declaration"
                        | "local_declaration_statement"
                        | "declaration_expression"
                        | "catch_declaration"
                        | "foreach_statement"
                        | "for_each_statement"
                        | "for_statement"
                        | "using_statement"
                        | "block"
                        | "expression_statement"
                ) {
                    collect_local_bindings(base, child, parent_id.clone(), out, depth + 1);
                }
            }
        }
    }
}

/// Collect binding identifiers from tuple / deconstruction patterns.
fn collect_pattern_bindings(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
    out: &mut Vec<Symbol>,
    depth: u32,
) {
    if depth > 32 {
        return;
    }
    match node.kind() {
        "identifier" => {
            let text = base.get_node_text(&node);
            if text != "_"
                && let Some(symbol) =
                    extract_named_binding(base, node, &text, parent_id, None, true)
            {
                out.push(symbol);
            }
        }
        "declaration_expression" => {
            if let Some(name) = node
                .child_by_field_name("name")
                .or_else(|| find_child(node, "identifier"))
            {
                let name_text = base.get_node_text(&name);
                if name_text != "_" {
                    let ty = node
                        .child_by_field_name("type")
                        .map(|t| base.get_node_text(&t))
                        .or_else(|| type_name_from_declaration(base, node));
                    let is_var = ty
                        .as_deref()
                        .is_some_and(|t| t == "var" || t == "implicit_type");
                    if let Some(symbol) =
                        extract_named_binding(base, node, &name_text, parent_id, ty, is_var)
                    {
                        out.push(symbol);
                    }
                }
            }
        }
        "variable_declaration" => {
            emit_declarators(base, node, parent_id, out);
        }
        "variable_declarator" => {
            if let Some(symbol) = extract_declarator(base, node, parent_id, None, true, "local") {
                out.push(symbol);
            }
        }
        "tuple_pattern" | "tuple_expression" | "argument" | "parenthesized_expression" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect_pattern_bindings(base, child, parent_id.clone(), out, depth + 1);
            }
        }
        _ => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if matches!(
                    child.kind(),
                    "identifier"
                        | "declaration_expression"
                        | "variable_declaration"
                        | "variable_declarator"
                        | "tuple_pattern"
                        | "tuple_expression"
                        | "argument"
                ) {
                    collect_pattern_bindings(base, child, parent_id.clone(), out, depth + 1);
                }
            }
        }
    }
}

fn emit_declarators(
    base: &mut BaseExtractor,
    declaration: Node,
    parent_id: Option<String>,
    out: &mut Vec<Symbol>,
) {
    let declared_type = type_name_from_declaration(base, declaration);
    let is_var = declared_type
        .as_deref()
        .is_some_and(|t| t == "var" || t == "using" || t == "implicit_type");

    let mut cursor = declaration.walk();
    for child in declaration.children(&mut cursor) {
        if child.kind() != "variable_declarator" {
            continue;
        }
        if let Some(symbol) = extract_declarator(
            base,
            child,
            parent_id.clone(),
            declared_type.as_deref(),
            is_var,
            "local",
        ) {
            out.push(symbol);
        }
    }
}

fn extract_named_binding(
    base: &mut BaseExtractor,
    node: Node,
    name: &str,
    parent_id: Option<String>,
    declared_type: Option<String>,
    is_var: bool,
) -> Option<Symbol> {
    let mut signature_parts = Vec::new();
    if let Some(ref ty) = declared_type {
        signature_parts.push(ty.clone());
    }
    signature_parts.push(name.to_string());
    let mut metadata = HashMap::new();
    metadata.insert("role".to_string(), serde_json::json!("local"));
    if let Some(ref ty) = declared_type {
        metadata.insert("variableType".to_string(), serde_json::json!(ty));
    }
    metadata.insert(
        "isInferred".to_string(),
        serde_json::json!(is_var || declared_type.is_none()),
    );
    Some(base.create_symbol(
        &node,
        name.to_string(),
        SymbolKind::Variable,
        SymbolOptions {
            signature: Some(signature_parts.join(" ")),
            visibility: Some(Visibility::Private),
            parent_id,
            metadata: Some(metadata),
            doc_comment: None,
            annotations: Vec::new(),
        },
    ))
}

/// Extract a formal parameter (`parameter`, `parameter_array`).
pub fn extract_parameter(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
) -> Option<Symbol> {
    let name = parameter_name(base, node)?;
    let declared_type = parameter_type_name(base, node);
    let is_var = declared_type.as_deref() == Some("var");

    let mut signature_parts = Vec::new();
    let modifiers = helpers::extract_modifiers(base, &node);
    if !modifiers.is_empty() {
        signature_parts.push(modifiers.join(" "));
    }
    if node.kind() == "parameter_array" {
        signature_parts.push("params".to_string());
    }
    if let Some(ref ty) = declared_type {
        signature_parts.push(ty.clone());
    }
    signature_parts.push(name.clone());

    let mut metadata = HashMap::new();
    metadata.insert("role".to_string(), serde_json::json!("parameter"));
    if let Some(ref ty) = declared_type {
        metadata.insert("variableType".to_string(), serde_json::json!(ty));
    }
    metadata.insert(
        "isInferred".to_string(),
        serde_json::json!(is_var || declared_type.is_none()),
    );

    Some(base.create_symbol(
        &node,
        name,
        SymbolKind::Variable,
        SymbolOptions {
            signature: Some(signature_parts.join(" ")),
            visibility: Some(Visibility::Private),
            parent_id,
            metadata: Some(metadata),
            doc_comment: None,
            annotations: Vec::new(),
        },
    ))
}

fn extract_declarator(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
    declared_type: Option<&str>,
    is_var: bool,
    role: &str,
) -> Option<Symbol> {
    let name_node = find_child(node, "identifier")?;
    let name = base.get_node_text(&name_node);

    let mut initializer = None;
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    if let Some(eq) = children.iter().position(|c| c.kind() == "=")
        && eq + 1 < children.len()
    {
        initializer = Some(base.get_node_text(&children[eq + 1]));
    }

    let mut signature_parts = Vec::new();
    if let Some(ty) = declared_type {
        signature_parts.push(ty.to_string());
    }
    signature_parts.push(name.clone());
    if let Some(ref init) = initializer {
        signature_parts.push(format!("= {init}"));
    }

    let mut metadata = HashMap::new();
    metadata.insert("role".to_string(), serde_json::json!(role));
    if let Some(ty) = declared_type {
        metadata.insert("variableType".to_string(), serde_json::json!(ty));
    }
    if let Some(init) = initializer {
        metadata.insert("initializer".to_string(), serde_json::json!(init));
    }
    metadata.insert(
        "isInferred".to_string(),
        serde_json::json!(is_var || declared_type.is_none()),
    );

    Some(base.create_symbol(
        &node,
        name,
        SymbolKind::Variable,
        SymbolOptions {
            signature: Some(signature_parts.join(" ")),
            visibility: Some(Visibility::Private),
            parent_id,
            metadata: Some(metadata),
            doc_comment: None,
            annotations: Vec::new(),
        },
    ))
}

fn type_name_from_declaration(base: &BaseExtractor, declaration: Node) -> Option<String> {
    let mut cursor = declaration.walk();
    for child in declaration.children(&mut cursor) {
        match child.kind() {
            "predefined_type" | "identifier" | "generic_name" | "qualified_name"
            | "nullable_type" | "array_type" | "tuple_type" | "pointer_type" | "ref_type"
            | "implicit_type" => {
                return Some(base.get_node_text(&child));
            }
            _ => {}
        }
    }
    None
}

fn parameter_name(base: &BaseExtractor, node: Node) -> Option<String> {
    if let Some(name) = node.child_by_field_name("name") {
        return Some(base.get_node_text(&name));
    }
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    for child in children.into_iter().rev() {
        if child.kind() == "identifier" {
            return Some(base.get_node_text(&child));
        }
    }
    None
}

fn parameter_type_name(base: &BaseExtractor, node: Node) -> Option<String> {
    if let Some(ty) = node.child_by_field_name("type") {
        return Some(base.get_node_text(&ty));
    }
    type_name_from_declaration(base, node)
}

fn find_child<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    node.children(&mut cursor).find(|c| c.kind() == kind)
}

/// True when `node` sits in a foreach/for header (not the body block), so the
/// parent statement owns binding emission.
fn is_loop_header_binding(node: Node<'_>) -> bool {
    let mut cur = node;
    while let Some(parent) = cur.parent() {
        match parent.kind() {
            "foreach_statement" | "for_each_statement" | "for_statement" => {
                if let Some(body) = parent.child_by_field_name("body") {
                    return node.start_byte() < body.start_byte();
                }
                return true;
            }
            "method_declaration"
            | "local_function_statement"
            | "constructor_declaration"
            | "class_declaration"
            | "struct_declaration"
            | "record_declaration" => return false,
            _ => cur = parent,
        }
    }
    false
}
