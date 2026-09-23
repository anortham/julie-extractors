use tree_sitter::{Node, Tree};

use super::helpers::{base_metadata, child_of_kind, fact_for_node, insert_string, node_text};
use super::{
    EFCORE_DB_SET_PATTERN_ID, EFCORE_ENTITY_CONFIGURATION_PATTERN_ID,
    EFCORE_TABLE_MAPPING_PATTERN_ID,
};
use crate::base::types::StructuralFact;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

/// Entity Framework Core model facts for C#: `DbSet<T>` properties of a
/// context, `ToTable("t")` mappings on `Entity<T>()` or an
/// `EntityTypeBuilder<T>` parameter, `[Table("t")]` entity classes, and
/// `IEntityTypeConfiguration<T>` classes.
pub(super) fn collect_efcore_facts(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let mut facts = Vec::new();
    visit(
        tree.root_node(),
        language,
        file_path,
        content,
        &mut facts,
        0,
    );
    facts
}

fn visit(
    node: Node<'_>,
    language: &str,
    file_path: &str,
    content: &str,
    facts: &mut Vec<StructuralFact>,
    depth: u32,
) {
    if !should_visit_tree_depth(depth) {
        return;
    }
    match node.kind() {
        "property_declaration" => db_set(node, language, file_path, content, facts),
        "class_declaration" | "record_declaration" => {
            entity_configuration(node, language, file_path, content, facts);
            table_attribute(node, language, file_path, content, facts);
        }
        "invocation_expression" => to_table(node, language, file_path, content, facts),
        _ => {}
    }
    let Some(child_depth) = child_tree_depth(depth) else {
        return;
    };
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        visit(child, language, file_path, content, facts, child_depth);
    }
}

fn db_set(
    property: Node<'_>,
    language: &str,
    file_path: &str,
    content: &str,
    facts: &mut Vec<StructuralFact>,
) {
    let Some(entity_type) = property
        .child_by_field_name("type")
        .and_then(|type_node| generic_argument_of(type_node, content, "DbSet"))
    else {
        return;
    };
    let Some(context_type) = enclosing_type_name(property, content) else {
        return;
    };
    let Some(property_name) = property
        .child_by_field_name("name")
        .and_then(|name| node_text(content, name))
    else {
        return;
    };
    let mut metadata = base_metadata("framework", "efcore");
    insert_string(&mut metadata, "context_type", &context_type);
    insert_string(&mut metadata, "property_name", property_name);
    insert_string(&mut metadata, "entity_type", &entity_type);
    facts.push(fact_for_node(
        file_path,
        language,
        EFCORE_DB_SET_PATTERN_ID,
        "db_set",
        property,
        metadata,
    ));
}

fn entity_configuration(
    class: Node<'_>,
    language: &str,
    file_path: &str,
    content: &str,
    facts: &mut Vec<StructuralFact>,
) {
    let Some(base_list) = child_of_kind(class, "base_list") else {
        return;
    };
    let mut cursor = base_list.walk();
    let entity_type = base_list
        .named_children(&mut cursor)
        .find_map(|base_type| generic_argument_of(base_type, content, "IEntityTypeConfiguration"));
    let (Some(entity_type), Some(configuration_type)) = (
        entity_type,
        class
            .child_by_field_name("name")
            .and_then(|name| node_text(content, name)),
    ) else {
        return;
    };
    let mut metadata = base_metadata("framework", "efcore");
    insert_string(&mut metadata, "configuration_type", configuration_type);
    insert_string(&mut metadata, "entity_type", &entity_type);
    facts.push(fact_for_node(
        file_path,
        language,
        EFCORE_ENTITY_CONFIGURATION_PATTERN_ID,
        "entity_configuration",
        class,
        metadata,
    ));
}

fn table_attribute(
    class: Node<'_>,
    language: &str,
    file_path: &str,
    content: &str,
    facts: &mut Vec<StructuralFact>,
) {
    let Some(entity_type) = class
        .child_by_field_name("name")
        .and_then(|name| node_text(content, name))
    else {
        return;
    };
    let mut cursor = class.walk();
    for list in class
        .children(&mut cursor)
        .filter(|child| child.kind() == "attribute_list")
    {
        let mut list_cursor = list.walk();
        for attribute in list
            .named_children(&mut list_cursor)
            .filter(|child| child.kind() == "attribute")
        {
            let is_table = attribute
                .child_by_field_name("name")
                .and_then(|name| node_text(content, name))
                .is_some_and(|name| {
                    let last = name.rsplit('.').next().unwrap_or(name);
                    last == "Table" || last == "TableAttribute"
                });
            let Some(table_name) = is_table
                .then(|| child_of_kind(attribute, "attribute_argument_list"))
                .flatten()
                .and_then(|arguments| first_string_argument(arguments, content))
            else {
                continue;
            };
            push_table_mapping(
                attribute,
                entity_type,
                &table_name,
                "table_attribute",
                language,
                file_path,
                facts,
            );
        }
    }
}

fn to_table(
    invocation: Node<'_>,
    language: &str,
    file_path: &str,
    content: &str,
    facts: &mut Vec<StructuralFact>,
) {
    let Some(function) = invocation
        .child_by_field_name("function")
        .filter(|function| function.kind() == "member_access_expression")
    else {
        return;
    };
    if function
        .child_by_field_name("name")
        .and_then(|name| node_text(content, name))
        != Some("ToTable")
    {
        return;
    }
    let Some(table_name) = invocation
        .child_by_field_name("arguments")
        .and_then(|arguments| first_string_argument(arguments, content))
    else {
        return;
    };
    let Some(receiver) = function.child_by_field_name("expression") else {
        return;
    };
    let Some((entity_type, source)) = entity_of_builder(receiver, content) else {
        return;
    };
    push_table_mapping(
        invocation,
        &entity_type,
        &table_name,
        source,
        language,
        file_path,
        facts,
    );
}

/// The entity a builder expression configures: the type argument of the
/// nearest `Entity<T>()` call in the receiver chain, else the
/// `EntityTypeBuilder<T>` type of the parameter the chain starts from.
fn entity_of_builder(receiver: Node<'_>, content: &str) -> Option<(String, &'static str)> {
    let mut current = receiver;
    for _ in 0..64 {
        match current.kind() {
            "invocation_expression" => {
                let function = current.child_by_field_name("function")?;
                let name = if function.kind() == "member_access_expression" {
                    function.child_by_field_name("name")?
                } else {
                    function
                };
                if let Some(entity) = generic_argument_of(name, content, "Entity") {
                    return Some((entity, "model_builder_entity"));
                }
                current = function;
            }
            "member_access_expression" => current = current.child_by_field_name("expression")?,
            "identifier" => {
                let name = node_text(content, current)?;
                return builder_parameter_entity(current, name, content)
                    .map(|entity| (entity, "entity_type_builder"));
            }
            _ => return None,
        }
    }
    None
}

fn builder_parameter_entity(node: Node<'_>, name: &str, content: &str) -> Option<String> {
    let mut current = node.parent();
    while let Some(candidate) = current {
        if matches!(
            candidate.kind(),
            "method_declaration" | "local_function_statement" | "lambda_expression"
        ) {
            let parameters = candidate.child_by_field_name("parameters")?;
            let mut cursor = parameters.walk();
            return parameters
                .named_children(&mut cursor)
                .filter(|parameter| parameter.kind() == "parameter")
                .find(|parameter| {
                    parameter
                        .child_by_field_name("name")
                        .and_then(|name_node| node_text(content, name_node))
                        == Some(name)
                })
                .and_then(|parameter| parameter.child_by_field_name("type"))
                .and_then(|type_node| {
                    generic_argument_of(type_node, content, "EntityTypeBuilder")
                });
        }
        current = candidate.parent();
    }
    None
}

fn push_table_mapping(
    node: Node<'_>,
    entity_type: &str,
    table_name: &str,
    mapping_source: &str,
    language: &str,
    file_path: &str,
    facts: &mut Vec<StructuralFact>,
) {
    let mut metadata = base_metadata("framework", "efcore");
    insert_string(&mut metadata, "entity_type", entity_type);
    insert_string(&mut metadata, "table_name", table_name);
    insert_string(&mut metadata, "mapping_source", mapping_source);
    facts.push(fact_for_node(
        file_path,
        language,
        EFCORE_TABLE_MAPPING_PATTERN_ID,
        "table_mapping",
        node,
        metadata,
    ));
}

/// The single type argument of `generic` when it names `expected` (last
/// qualified segment): `DbSet<Order>` -> `Order`.
fn generic_argument_of(node: Node<'_>, content: &str, expected: &str) -> Option<String> {
    let generic = match node.kind() {
        "generic_name" => node,
        "qualified_name" => node
            .child_by_field_name("name")
            .filter(|name| name.kind() == "generic_name")?,
        _ => return None,
    };
    let mut cursor = generic.walk();
    let mut children = generic.named_children(&mut cursor);
    let name = children.next().and_then(|name| node_text(content, name))?;
    if name != expected {
        return None;
    }
    let arguments = children.find(|child| child.kind() == "type_argument_list")?;
    let mut argument_cursor = arguments.walk();
    let mut type_arguments = arguments.named_children(&mut argument_cursor);
    let argument = type_arguments.next()?;
    if type_arguments.next().is_some() {
        return None;
    }
    node_text(content, argument).map(str::to_string)
}

fn first_string_argument(arguments: Node<'_>, content: &str) -> Option<String> {
    let mut cursor = arguments.walk();
    let argument = arguments
        .named_children(&mut cursor)
        .find(|child| matches!(child.kind(), "argument" | "attribute_argument"))?;
    let literal = argument.named_child(0)?;
    if literal.kind() != "string_literal" {
        return None;
    }
    let mut literal_cursor = literal.walk();
    let parts: Vec<Node<'_>> = literal.named_children(&mut literal_cursor).collect();
    match parts.as_slice() {
        [content_node] if content_node.kind() == "string_literal_content" => {
            node_text(content, *content_node).map(str::to_string)
        }
        _ => None,
    }
}

fn enclosing_type_name(node: Node<'_>, content: &str) -> Option<String> {
    let mut current = node.parent();
    while let Some(candidate) = current {
        if matches!(
            candidate.kind(),
            "class_declaration" | "record_declaration" | "struct_declaration"
        ) {
            return candidate
                .child_by_field_name("name")
                .and_then(|name| node_text(content, name))
                .map(str::to_string);
        }
        current = candidate.parent();
    }
    None
}
