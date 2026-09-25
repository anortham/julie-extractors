//! Member assignments that declare code: prototype methods
//! (`Queue.prototype.clear = function () {}`), static members
//! (`Queue.create = () => {}`), CommonJS exports (`module.exports = function
//! auth() {}`, named `default` when anonymous; `exports.helper = () => {}`),
//! and constructor-assigned instance properties (`this.logger = new Logger()`).

use crate::base::{Symbol, SymbolKind, SymbolOptions, Visibility};
use serde_json::json;
use std::collections::HashMap;
use tree_sitter::Node;

fn is_function_value(node: Node) -> bool {
    matches!(
        node.kind(),
        "function_expression" | "arrow_function" | "generator_function" | "function"
    )
}

impl super::JavaScriptExtractor {
    pub(super) fn extract_assignment(
        &mut self,
        node: Node,
        parent_id: Option<String>,
        symbols: &[Symbol],
    ) -> Option<Symbol> {
        let left = node
            .child_by_field_name("left")
            .filter(|left| left.kind() == "member_expression")?;
        let right = node.child_by_field_name("right")?;
        let object = left.child_by_field_name("object")?;
        let property_name = self
            .base
            .get_node_text(&left.child_by_field_name("property")?);
        let object_text = self.base.get_node_text(&object);

        if object.kind() == "this" {
            if is_function_value(right)
                && let Some(owner) = enclosing_constructor_function(node, symbols)
            {
                let owner_name = owner.name.clone();
                let owner_id = owner.id.clone();
                return Some(self.member_method(
                    node,
                    right,
                    property_name,
                    &owner_name,
                    Some(owner_id),
                    false,
                ));
            }
            return self.extract_constructor_property(node, right, property_name, symbols);
        }
        if object_text == "module" && property_name == "exports" {
            if !is_function_value(right) {
                return None;
            }
            let own_name = right.child_by_field_name("name").map_or_else(
                || "default".to_string(),
                |name| self.base.get_node_text(&name),
            );
            return Some(self.assigned_function(node, right, own_name, parent_id));
        }
        if matches!(object_text.as_str(), "exports" | "module.exports") {
            return is_function_value(right)
                .then(|| self.assigned_function(node, right, property_name, parent_id));
        }

        let (owner_name, is_prototype) = match object_text.strip_suffix(".prototype") {
            Some(owner) => (owner.to_string(), true),
            None => (object_text.clone(), false),
        };
        if !is_function_value(right) {
            return None;
        }
        let owner = symbols
            .iter()
            .find(|symbol| {
                symbol.name == owner_name
                    && symbol.parent_id.is_none()
                    && matches!(symbol.kind, SymbolKind::Function | SymbolKind::Class)
            })
            .map(|owner| owner.id.clone());
        Some(self.member_method(
            node,
            right,
            property_name,
            &owner_name,
            owner.or(parent_id),
            !is_prototype,
        ))
    }

    fn member_method(
        &mut self,
        node: Node,
        function: Node,
        name: String,
        owner_name: &str,
        parent_id: Option<String>,
        is_static: bool,
    ) -> Symbol {
        let mut metadata = HashMap::from([
            ("className".to_string(), json!(owner_name)),
            ("isFunction".to_string(), json!(true)),
        ]);
        let member_kind = if is_static {
            "isStaticMethod"
        } else {
            "isPrototypeMethod"
        };
        metadata.insert(member_kind.to_string(), json!(true));
        let doc_comment = self.base.find_doc_comment(&node);
        self.base.create_symbol(
            &node,
            name,
            SymbolKind::Method,
            SymbolOptions {
                signature: Some(self.assignment_header(node, function)),
                visibility: Some(Visibility::Public),
                parent_id,
                metadata: Some(metadata),
                doc_comment,
                annotations: Vec::new(),
            },
        )
    }

    fn assigned_function(
        &mut self,
        node: Node,
        function: Node,
        name: String,
        parent_id: Option<String>,
    ) -> Symbol {
        let metadata = HashMap::from([
            ("isCommonJSExport".to_string(), json!(true)),
            ("isAsync".to_string(), json!(self.is_async(&function))),
            (
                "isGenerator".to_string(),
                json!(self.is_generator(&function)),
            ),
            (
                "parameters".to_string(),
                json!(self.extract_parameters(&function)),
            ),
        ]);
        let doc_comment = self.base.find_doc_comment(&node);
        self.base.create_symbol(
            &node,
            name,
            SymbolKind::Function,
            SymbolOptions {
                signature: Some(self.assignment_header(node, function)),
                visibility: Some(Visibility::Public),
                parent_id,
                metadata: Some(metadata),
                doc_comment,
                annotations: Vec::new(),
            },
        )
    }

    /// `this.name = value` inside a class constructor declares an instance
    /// property of the class, unless the class already declares the member.
    fn extract_constructor_property(
        &mut self,
        node: Node,
        value: Node,
        name: String,
        symbols: &[Symbol],
    ) -> Option<Symbol> {
        let constructor = enclosing_constructor(node, &self.base.content)?;
        let class_body = constructor.parent()?;
        let class = class_body.parent()?;
        let class_symbol = symbols.iter().find(|symbol| {
            symbol.kind == SymbolKind::Class && symbol.start_byte == class.start_byte() as u32
        })?;
        if symbols.iter().any(|symbol| {
            symbol.parent_id.as_deref() == Some(&class_symbol.id)
                && symbol.name == name
                && symbol.kind != SymbolKind::Method
        }) || class_body_declares_property(class_body, &name, &self.base.content)
        {
            return None;
        }
        let class_id = class_symbol.id.clone();
        let metadata = HashMap::from([("isConstructorAssigned".to_string(), json!(true))]);
        let property = self.base.create_symbol(
            &node,
            name,
            SymbolKind::Property,
            SymbolOptions {
                signature: Some(self.base.get_node_text(&node)),
                visibility: Some(self.extract_visibility(&node.child_by_field_name("left")?)),
                parent_id: Some(class_id),
                metadata: Some(metadata),
                ..Default::default()
            },
        );
        super::type_facts::record_new_expression_fact(
            &mut self.base,
            &property.id,
            value,
            &super::type_facts::TYPE_NAME_RULES,
        );
        Some(property)
    }

    /// The assignment text up to the function body:
    /// `Queue.prototype.clear = function clear()`.
    fn assignment_header(&self, node: Node, function: Node) -> String {
        let end = function
            .child_by_field_name("body")
            .map_or(node.end_byte(), |body| body.start_byte());
        self.base
            .content
            .get(node.start_byte()..end)
            .unwrap_or_default()
            .trim_end()
            .to_string()
    }
}

/// The class constructor a statement sits in directly, not through a nested
/// function.
fn enclosing_constructor<'t>(node: Node<'t>, content: &str) -> Option<Node<'t>> {
    let mut current = node.parent();
    while let Some(candidate) = current {
        match candidate.kind() {
            "method_definition" => {
                let is_constructor = candidate
                    .child_by_field_name("name")
                    .and_then(|name| content.get(name.byte_range()))
                    == Some("constructor");
                return (is_constructor
                    && candidate
                        .parent()
                        .is_some_and(|body| body.kind() == "class_body"))
                .then_some(candidate);
            }
            "function_declaration"
            | "function_expression"
            | "generator_function"
            | "generator_function_declaration"
            | "class_body" => return None,
            _ => current = candidate.parent(),
        }
    }
    None
}

/// The function declaration a `this.m = function () {}` statement sits in
/// directly: a pre-class constructor such as `function Counter() {}`.
fn enclosing_constructor_function<'a>(node: Node, symbols: &'a [Symbol]) -> Option<&'a Symbol> {
    let mut current = node.parent();
    while let Some(candidate) = current {
        match candidate.kind() {
            "function_declaration" | "function_expression" => {
                return symbols.iter().find(|symbol| {
                    symbol.kind == SymbolKind::Function
                        && symbol.start_byte == candidate.start_byte() as u32
                        && symbol.end_byte == candidate.end_byte() as u32
                });
            }
            "method_definition"
            | "arrow_function"
            | "generator_function"
            | "generator_function_declaration"
            | "class_body" => return None,
            _ => current = candidate.parent(),
        }
    }
    None
}

fn class_body_declares_property(class_body: Node, name: &str, content: &str) -> bool {
    let mut cursor = class_body.walk();
    for child in class_body.children(&mut cursor) {
        if matches!(
            child.kind(),
            "public_field_definition" | "property_definition" | "field_definition"
        ) {
            let name_node = child
                .child_by_field_name("name")
                .or_else(|| child.child_by_field_name("property"))
                .or_else(|| child.child_by_field_name("key"));
            if let Some(n) = name_node
                && content.get(n.byte_range()) == Some(name)
            {
                return true;
            }
        }
    }
    false
}
