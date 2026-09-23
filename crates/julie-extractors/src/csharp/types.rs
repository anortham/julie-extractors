// C# Type Declaration Extraction

use super::helpers;
use crate::base::{BaseExtractor, Symbol, SymbolKind, SymbolOptions, Visibility};
use std::collections::HashMap;
use tree_sitter::Node;

/// Extract namespace
pub fn extract_namespace(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
) -> Option<Symbol> {
    let mut cursor = node.walk();
    let name_node = node
        .children(&mut cursor)
        .find(|c| c.kind() == "qualified_name" || c.kind() == "identifier")?;

    let name = base.get_node_text(&name_node);
    let signature = format!("namespace {}", name);

    // Extract XML doc comment
    let doc_comment = base.find_doc_comment(&node);

    let options = SymbolOptions {
        signature: Some(signature),
        visibility: Some(Visibility::Public),
        parent_id,
        doc_comment,
        ..Default::default()
    };

    Some(base.create_symbol(&node, name, SymbolKind::Namespace, options))
}

/// Extract using statement
pub fn extract_using(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
) -> Option<Symbol> {
    let alias_node = node.child_by_field_name("name");
    let mut cursor = node.walk();
    let target_node = node
        .named_children(&mut cursor)
        .filter(|child| Some(*child) != alias_node && child.kind() != "comment")
        .last()?;
    let target = base.get_node_text(&target_node);
    let has_token = |token: &str| {
        let mut cursor = node.walk();
        node.children(&mut cursor).any(|c| c.kind() == token)
    };

    let mut signature = String::new();
    if has_token("global") {
        signature.push_str("global ");
    }
    signature.push_str("using ");
    if has_token("static") {
        signature.push_str("static ");
    }
    let name = match alias_node {
        Some(alias_node) => {
            let alias = base.get_node_text(&alias_node);
            signature.push_str(&format!("{alias} = "));
            alias
        }
        None => target.rsplit('.').next().unwrap_or(&target).to_string(),
    };
    signature.push_str(&target);

    // Extract XML doc comment
    let doc_comment = base.find_doc_comment(&node);

    let options = SymbolOptions {
        signature: Some(signature),
        visibility: Some(Visibility::Public),
        parent_id,
        doc_comment,
        ..Default::default()
    };

    Some(base.create_symbol(&node, name, SymbolKind::Import, options))
}

/// Extract class
pub fn extract_class(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
) -> Option<Symbol> {
    let mut cursor = node.walk();
    let name_node = node
        .children(&mut cursor)
        .find(|c| c.kind() == "identifier")?;

    let name = base.get_node_text(&name_node);
    let modifiers = helpers::extract_modifiers(base, &node);
    let visibility = helpers::determine_visibility(&modifiers, &node);

    let mut signature = if modifiers.is_empty() {
        format!("class {}", name)
    } else {
        format!("{} class {}", modifiers.join(" "), name)
    };

    if let Some(type_params) = helpers::extract_type_parameters(base, &node) {
        signature = signature.replace(
            &format!("class {}", name),
            &format!("class {}{}", name, type_params),
        );
    }

    let base_list = helpers::extract_base_list(base, &node);
    if !base_list.is_empty() {
        signature += &format!(" : {}", base_list.join(", "));
    }

    let mut node_cursor = node.walk();
    let where_clauses: Vec<String> = node
        .children(&mut node_cursor)
        .filter(|c| c.kind() == "type_parameter_constraints_clause")
        .map(|clause| base.get_node_text(&clause))
        .collect();

    if !where_clauses.is_empty() {
        signature += &format!(" {}", where_clauses.join(" "));
    }

    let mut metadata = HashMap::new();
    let csharp_visibility = helpers::get_csharp_visibility_string(&modifiers, &visibility);
    metadata.insert(
        "csharp_visibility".to_string(),
        serde_json::Value::String(csharp_visibility),
    );

    // Extract XML doc comment
    let doc_comment = base.find_doc_comment(&node);

    let annotations = helpers::extract_annotations(base, &node);
    let options = SymbolOptions {
        signature: Some(signature),
        visibility: Some(visibility),
        parent_id,
        metadata: Some(metadata),
        doc_comment,
        annotations,
    };

    Some(base.create_symbol(&node, name, SymbolKind::Class, options))
}

/// Extract interface
pub fn extract_interface(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
) -> Option<Symbol> {
    let mut cursor = node.walk();
    let name_node = node
        .children(&mut cursor)
        .find(|c| c.kind() == "identifier")?;

    let name = base.get_node_text(&name_node);
    let modifiers = helpers::extract_modifiers(base, &node);
    let visibility = helpers::determine_visibility(&modifiers, &node);

    let mut signature = if modifiers.is_empty() {
        format!("interface {}", name)
    } else {
        format!("{} interface {}", modifiers.join(" "), name)
    };

    if let Some(type_params) = helpers::extract_type_parameters(base, &node) {
        signature = signature.replace(
            &format!("interface {}", name),
            &format!("interface {}{}", name, type_params),
        );
    }

    let base_list = helpers::extract_base_list(base, &node);
    if !base_list.is_empty() {
        signature += &format!(" : {}", base_list.join(", "));
    }

    let mut node_cursor = node.walk();
    let where_clauses: Vec<String> = node
        .children(&mut node_cursor)
        .filter(|c| c.kind() == "type_parameter_constraints_clause")
        .map(|clause| base.get_node_text(&clause))
        .collect();

    if !where_clauses.is_empty() {
        signature += &format!(" {}", where_clauses.join(" "));
    }

    // Extract XML doc comment
    let doc_comment = base.find_doc_comment(&node);

    let annotations = helpers::extract_annotations(base, &node);
    let options = SymbolOptions {
        signature: Some(signature),
        visibility: Some(visibility),
        parent_id,
        doc_comment,
        annotations,
        ..Default::default()
    };

    Some(base.create_symbol(&node, name, SymbolKind::Interface, options))
}

/// Extract struct
pub fn extract_struct(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
) -> Option<Symbol> {
    let mut cursor = node.walk();
    let name_node = node
        .children(&mut cursor)
        .find(|c| c.kind() == "identifier")?;

    let name = base.get_node_text(&name_node);
    let modifiers = helpers::extract_modifiers(base, &node);
    let visibility = helpers::determine_visibility(&modifiers, &node);

    let mut signature = if modifiers.is_empty() {
        format!("struct {}", name)
    } else {
        format!("{} struct {}", modifiers.join(" "), name)
    };

    if let Some(type_params) = helpers::extract_type_parameters(base, &node) {
        signature = signature.replace(
            &format!("struct {}", name),
            &format!("struct {}{}", name, type_params),
        );
    }

    let base_list = helpers::extract_base_list(base, &node);
    if !base_list.is_empty() {
        signature += &format!(" : {}", base_list.join(", "));
    }

    // Extract XML doc comment
    let doc_comment = base.find_doc_comment(&node);

    let annotations = helpers::extract_annotations(base, &node);
    let options = SymbolOptions {
        signature: Some(signature),
        visibility: Some(visibility),
        parent_id,
        doc_comment,
        annotations,
        ..Default::default()
    };

    Some(base.create_symbol(&node, name, SymbolKind::Struct, options))
}

/// Extract enum
pub fn extract_enum(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
) -> Option<Symbol> {
    let mut cursor = node.walk();
    let name_node = node
        .children(&mut cursor)
        .find(|c| c.kind() == "identifier")?;

    let name = base.get_node_text(&name_node);
    let modifiers = helpers::extract_modifiers(base, &node);
    let visibility = helpers::determine_visibility(&modifiers, &node);

    let mut signature = if modifiers.is_empty() {
        format!("enum {}", name)
    } else {
        format!("{} enum {}", modifiers.join(" "), name)
    };

    let base_list = helpers::extract_base_list(base, &node);
    if !base_list.is_empty() {
        signature += &format!(" : {}", base_list[0]);
    }

    // Extract XML doc comment
    let doc_comment = base.find_doc_comment(&node);

    let annotations = helpers::extract_annotations(base, &node);
    let options = SymbolOptions {
        signature: Some(signature),
        visibility: Some(visibility),
        parent_id,
        doc_comment,
        annotations,
        ..Default::default()
    };

    Some(base.create_symbol(&node, name, SymbolKind::Enum, options))
}

/// Extract enum member
pub fn extract_enum_member(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
) -> Option<Symbol> {
    let mut cursor = node.walk();
    let name_node = node
        .children(&mut cursor)
        .find(|c| c.kind() == "identifier")?;

    let name = base.get_node_text(&name_node);

    let mut signature = name.clone();
    let children: Vec<Node> = node.children(&mut cursor).collect();
    if let Some(equals_index) = children.iter().position(|c| c.kind() == "=")
        && equals_index + 1 < children.len()
    {
        let value_nodes: Vec<String> = children[equals_index + 1..]
            .iter()
            .map(|n| base.get_node_text(n))
            .collect();
        let value = value_nodes.join("").trim().to_string();
        if !value.is_empty() {
            signature += &format!(" = {}", value);
        }
    }

    // Extract XML doc comment
    let doc_comment = base.find_doc_comment(&node);

    let annotations = helpers::extract_annotations(base, &node);
    let options = SymbolOptions {
        signature: Some(signature),
        visibility: Some(Visibility::Public),
        parent_id,
        doc_comment,
        annotations,
        ..Default::default()
    };

    Some(base.create_symbol(&node, name, SymbolKind::EnumMember, options))
}

/// Extract record
pub fn extract_record(
    base: &mut BaseExtractor,
    node: Node,
    parent_id: Option<String>,
) -> Option<Symbol> {
    let mut cursor = node.walk();
    let name_node = node
        .children(&mut cursor)
        .find(|c| c.kind() == "identifier")?;

    let name = base.get_node_text(&name_node);
    let modifiers = helpers::extract_modifiers(base, &node);
    let visibility = helpers::determine_visibility(&modifiers, &node);

    let is_struct = modifiers.contains(&"struct".to_string())
        || node.children(&mut cursor).any(|c| c.kind() == "struct");

    let record_type = if is_struct { "record struct" } else { "record" };
    let mut signature = if modifiers.is_empty() {
        format!("{} {}", record_type, name)
    } else {
        format!("{} {} {}", modifiers.join(" "), record_type, name)
    };

    if let Some(param_list) = node
        .children(&mut cursor)
        .find(|c| c.kind() == "parameter_list")
    {
        signature += &base.get_node_text(&param_list);
    }

    if let Some(base_list) = node.children(&mut cursor).find(|c| c.kind() == "base_list") {
        signature += &format!(" {}", base.get_node_text(&base_list));
    }

    let symbol_kind = if is_struct {
        SymbolKind::Struct
    } else {
        SymbolKind::Class
    };

    // Extract XML doc comment
    let doc_comment = base.find_doc_comment(&node);

    let annotations = helpers::extract_annotations(base, &node);
    let options = SymbolOptions {
        signature: Some(signature),
        visibility: Some(visibility),
        parent_id,
        doc_comment,
        annotations,
        ..Default::default()
    };

    Some(base.create_symbol(&node, name, symbol_kind, options))
}

/// The record declaration whose positional parameter list holds `parameter`.
pub(super) fn positional_record(parameter: Node) -> Option<Node> {
    parameter
        .parent()
        .filter(|list| list.kind() == "parameter_list")?
        .parent()
        .filter(|owner| owner.kind() == "record_declaration")
}

/// A positional record parameter declares a public property: `init`-only
/// for a record class and a `readonly record struct`, settable for a
/// mutable `record struct`.
pub fn extract_positional_property(
    base: &mut BaseExtractor,
    parameter: Node,
    record: Node,
    parent_id: Option<String>,
) -> Option<Symbol> {
    let name = base.get_node_text(&parameter.child_by_field_name("name")?);
    let type_node = parameter.child_by_field_name("type")?;
    let modifiers = helpers::extract_modifiers(base, &record);
    let mut cursor = record.walk();
    let is_struct = record.children(&mut cursor).any(|c| c.kind() == "struct");
    let accessor = if is_struct && !modifiers.iter().any(|m| m == "readonly") {
        "set"
    } else {
        "init"
    };
    let signature = format!(
        "public {} {name} {{ get; {accessor}; }}",
        base.get_node_text(&type_node)
    );
    let annotations = helpers::extract_annotations(base, &parameter);
    let symbol = base.create_symbol(
        &parameter,
        name,
        SymbolKind::Property,
        SymbolOptions {
            signature: Some(signature),
            visibility: Some(Visibility::Public),
            parent_id,
            annotations,
            ..Default::default()
        },
    );
    super::type_inference::record_declared_type(base, &symbol.id, type_node);
    Some(symbol)
}

/// The receiver type of the `extension(T receiver)` block that directly
/// declares `member`.
pub(super) fn extension_receiver_type(base: &BaseExtractor, member: Node) -> Option<String> {
    let block = member
        .parent()
        .filter(|body| body.kind() == "extension_body")?
        .parent()?;
    let mut cursor = block.walk();
    let receiver = block
        .children(&mut cursor)
        .find(|child| child.kind() == "receiver_parameter")?;
    Some(base.get_node_text(&receiver.child_by_field_name("type")?))
}
