//! F# controllers mapped onto the shared attribute-routing model.

use tree_sitter::Node;

use super::super::helpers::node_text;
use super::{AttributeRouteArgument, RouteAction, RouteAttribute, RouteController};

/// A class type body (`anon_type_defn`); its attributes sit on the enclosing
/// `type_definition` and each member's attributes on its `member_defn`.
pub(super) fn fsharp_controller<'t>(body: Node<'t>, content: &str) -> RouteController<'t> {
    let type_attributes = body
        .parent()
        .filter(|definition| definition.kind() == "type_definition")
        .map(|definition| route_attributes(definition, content))
        .unwrap_or_default();
    let mut actions = Vec::new();
    let mut cursor = body.walk();
    for block in body.children_by_field_name("block", &mut cursor) {
        let mut block_cursor = block.walk();
        for member in block
            .named_children(&mut block_cursor)
            .filter(|member| member.kind() == "member_defn")
        {
            let name = child_of_kind(member, "method_or_prop_defn")
                .and_then(|definition| definition.child_by_field_name("name"))
                .and_then(|name| name.child_by_field_name("method"))
                .and_then(|name| node_text(content, name))
                .map(str::to_string);
            actions.push(RouteAction {
                name,
                attributes: route_attributes(member, content),
            });
        }
    }
    RouteController {
        name: child_of_kind(body, "type_name")
            .and_then(|type_name| type_name.child_by_field_name("type_name"))
            .and_then(|name| node_text(content, name))
            .map(str::to_string),
        attributes: type_attributes,
        actions,
    }
}

fn route_attributes<'t>(declaration: Node<'t>, content: &str) -> Vec<RouteAttribute<'t>> {
    let Some(list) = child_of_kind(declaration, "attributes") else {
        return Vec::new();
    };
    let mut attributes = Vec::new();
    let mut cursor = list.walk();
    for attribute in list
        .named_children(&mut cursor)
        .filter(|child| child.kind() == "attribute")
    {
        let Some(raw_name) =
            child_of_kind(attribute, "simple_type").and_then(|name| node_text(content, name))
        else {
            continue;
        };
        let last = raw_name.rsplit('.').next().unwrap_or(raw_name).trim();
        let name = last.strip_suffix("Attribute").unwrap_or(last);
        if name.is_empty() {
            continue;
        }
        attributes.push(RouteAttribute {
            node: attribute,
            text: node_text(content, attribute)
                .unwrap_or_default()
                .to_string(),
            name: name.to_string(),
            argument: route_argument(attribute, content),
        });
    }
    attributes
}

fn route_argument(attribute: Node<'_>, content: &str) -> AttributeRouteArgument {
    let Some(arguments) = child_of_kind(attribute, "paren_expression") else {
        return AttributeRouteArgument::Absent;
    };
    let Some(first) = first_named(arguments) else {
        return AttributeRouteArgument::Absent;
    };
    let first = if first.kind() == "tuple_expression" {
        first_named(first).unwrap_or(first)
    } else {
        first
    };
    let literal = (first.kind() == "const")
        .then(|| child_of_kind(first, "string"))
        .flatten()
        .and_then(|string| node_text(content, string))
        .and_then(|text| text.strip_prefix('"')?.strip_suffix('"'));
    match literal {
        Some(template) => AttributeRouteArgument::Literal(template.to_string()),
        None => AttributeRouteArgument::NonLiteral,
    }
}

fn child_of_kind<'t>(node: Node<'t>, kind: &str) -> Option<Node<'t>> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find(|child| child.kind() == kind)
}

fn first_named(node: Node<'_>) -> Option<Node<'_>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).next()
}
