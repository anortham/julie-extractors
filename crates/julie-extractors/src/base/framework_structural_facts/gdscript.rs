//! Godot engine facts for GDScript: signal connections and emissions,
//! resource references (`preload`, `load`, `ResourceLoader.load`, and a script
//! path after `extends`), scene-tree node paths (`$Path`, `%Unique`,
//! `get_node("path")`), and `@rpc` functions.
//!
//! `a.b.sig.connect(handler)` parses as one flat `attribute` whose last
//! segment is the `attribute_call`, so the segment before the call names the
//! signal and the segments before that name the emitter.
use std::collections::HashMap;

use serde_json::Value;
use tree_sitter::{Node, Tree};

use super::helpers::{base_metadata, fact_for_node, insert_string, insert_string_array};
use crate::base::types::StructuralFact;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};

const SIGNAL_CONNECTION_PATTERN_ID: &str = "godot.signal_connection.v1";
const SIGNAL_EMISSION_PATTERN_ID: &str = "godot.signal_emission.v1";
const RESOURCE_REFERENCE_PATTERN_ID: &str = "godot.resource_reference.v1";
const NODE_PATH_PATTERN_ID: &str = "godot.node_path.v1";
const RPC_ANNOTATION_PATTERN_ID: &str = "godot.rpc_annotation.v1";

pub(super) fn collect_gdscript_framework_facts(
    language: &str,
    tree: &Tree,
    file_path: &str,
    content: &str,
) -> Vec<StructuralFact> {
    let mut scan = Scan {
        language,
        file_path,
        content,
        facts: Vec::new(),
    };
    scan.walk(tree.root_node(), 0);
    scan.facts
}

struct Scan<'a> {
    language: &'a str,
    file_path: &'a str,
    content: &'a str,
    facts: Vec<StructuralFact>,
}

/// One call in a flat `attribute` chain or a bare `call`: the method name,
/// its arguments, the node the fact spans, and the source text of the
/// segments before the method.
struct CallShape<'t> {
    anchor: Node<'t>,
    method: &'t str,
    arguments: Vec<Node<'t>>,
    receiver_segments: Vec<Node<'t>>,
}

impl<'a> Scan<'a> {
    fn walk(&mut self, node: Node, depth: u32) {
        if !should_visit_tree_depth(depth) {
            return;
        }
        match node.kind() {
            "attribute" => self.attribute_calls(node),
            "call" => {
                if let Some(shape) = self.bare_call(node) {
                    self.call_facts(&shape);
                }
            }
            "get_node" => self.dollar_node_path(node),
            "extends_statement" => self.script_extends(node),
            "annotation" => self.rpc_annotation(node),
            _ => {}
        }
        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.walk(child, child_depth);
        }
    }

    fn text(&self, node: Node) -> &'a str {
        self.content
            .get(node.start_byte()..node.end_byte())
            .unwrap_or("")
    }

    fn push(
        &mut self,
        pattern_id: &str,
        capture: &str,
        node: Node,
        metadata: HashMap<String, Value>,
    ) {
        self.facts.push(fact_for_node(
            self.file_path,
            self.language,
            pattern_id,
            capture,
            node,
            metadata,
        ));
    }

    fn attribute_calls(&mut self, attribute: Node) {
        let mut cursor = attribute.walk();
        let segments: Vec<Node> = attribute.named_children(&mut cursor).collect();
        for (index, segment) in segments.iter().enumerate().skip(1) {
            if segment.kind() != "attribute_call" {
                continue;
            }
            let Some(method) = identifier_child(*segment).map(|name| self.text(name)) else {
                continue;
            };
            let shape = CallShape {
                anchor: attribute,
                method,
                arguments: arguments(*segment),
                receiver_segments: segments[..index].to_vec(),
            };
            self.call_facts(&shape);
        }
    }

    fn bare_call(&self, call: Node<'a>) -> Option<CallShape<'a>> {
        let callee = identifier_child(call)?;
        Some(CallShape {
            anchor: call,
            method: self.text(callee),
            arguments: call
                .child_by_field_name("arguments")
                .map(|list| named_children(list))
                .unwrap_or_default(),
            receiver_segments: Vec::new(),
        })
    }

    fn call_facts(&mut self, shape: &CallShape) {
        match shape.method {
            "connect" => self.signal_connection(shape),
            "emit" => self.signal_emit(shape),
            "emit_signal" => self.signal_emit_by_name(shape),
            "preload" | "load" => self.resource_load(shape),
            "get_node" | "get_node_or_null" => self.called_node_path(shape),
            _ => {}
        }
    }

    /// The text of `receiver_segments[..end]`, or nothing when it is empty or
    /// only `self`.
    fn emitter(&self, shape: &CallShape, end: usize) -> Option<&'a str> {
        let segments = shape.receiver_segments.get(..end)?;
        let (first, last) = (segments.first()?, segments.last()?);
        let text = self
            .content
            .get(first.start_byte()..last.end_byte())
            .unwrap_or("");
        (!text.is_empty() && text != "self").then_some(text)
    }

    /// `emitter.sig.connect(handler)` or `emitter.connect("sig", handler)`;
    /// Godot 3 passes the handler as a target object and a method name string.
    fn signal_connection(&mut self, shape: &CallShape) {
        let mut metadata = base_metadata("signals", "godot");
        let (signal, emitter, handler) = match shape.arguments.first() {
            Some(first) if static_string(*first, self.content).is_some() => {
                let signal = static_string(*first, self.content).unwrap_or_default();
                let emitter = self.emitter(shape, shape.receiver_segments.len());
                let handler = match shape.arguments.get(1..) {
                    Some([callable]) => self.handler_name(*callable),
                    Some([_, method, ..]) => static_string(*method, self.content),
                    _ => None,
                };
                (signal, emitter, handler)
            }
            Some(callable) => {
                let Some(signal_node) = shape.receiver_segments.last() else {
                    return;
                };
                if signal_node.kind() != "identifier" {
                    return;
                }
                let signal = self.text(*signal_node);
                let emitter = self.emitter(shape, shape.receiver_segments.len() - 1);
                (signal, emitter, self.handler_name(*callable))
            }
            None => return,
        };
        insert_string(&mut metadata, "signal_name", signal);
        if let Some(emitter) = emitter {
            insert_string(&mut metadata, "emitter", emitter);
        }
        if let Some(handler) = handler {
            insert_string(&mut metadata, "handler", handler);
        }
        self.push(
            SIGNAL_CONNECTION_PATTERN_ID,
            "signal_connection",
            shape.anchor,
            metadata,
        );
    }

    /// A named callable: `handler`, `obj.handler`, or `handler.bind(...)`.
    fn handler_name(&self, callable: Node) -> Option<&'a str> {
        match callable.kind() {
            "identifier" => Some(self.text(callable)),
            "attribute" => {
                let segments = named_children(callable);
                let names = segments
                    .iter()
                    .take_while(|segment| segment.kind() == "identifier")
                    .count();
                let (first, last) = (segments.first()?, segments.get(names.checked_sub(1)?)?);
                self.content.get(first.start_byte()..last.end_byte())
            }
            _ => None,
        }
    }

    fn signal_emit(&mut self, shape: &CallShape) {
        let Some(signal_node) = shape
            .receiver_segments
            .last()
            .filter(|node| node.kind() == "identifier")
        else {
            return;
        };
        let mut metadata = base_metadata("signals", "godot");
        insert_string(&mut metadata, "signal_name", self.text(*signal_node));
        if let Some(emitter) = self.emitter(shape, shape.receiver_segments.len() - 1) {
            insert_string(&mut metadata, "emitter", emitter);
        }
        self.push(
            SIGNAL_EMISSION_PATTERN_ID,
            "signal_emission",
            shape.anchor,
            metadata,
        );
    }

    fn signal_emit_by_name(&mut self, shape: &CallShape) {
        let Some(signal) = shape
            .arguments
            .first()
            .and_then(|first| static_string(*first, self.content))
        else {
            return;
        };
        let mut metadata = base_metadata("signals", "godot");
        insert_string(&mut metadata, "signal_name", signal);
        if let Some(emitter) = self.emitter(shape, shape.receiver_segments.len()) {
            insert_string(&mut metadata, "emitter", emitter);
        }
        self.push(
            SIGNAL_EMISSION_PATTERN_ID,
            "signal_emission",
            shape.anchor,
            metadata,
        );
    }

    /// `preload("res://...")`, `load("res://...")`, `ResourceLoader.load(...)`.
    fn resource_load(&mut self, shape: &CallShape) {
        let loader = match shape.receiver_segments.as_slice() {
            [] => shape.method.to_string(),
            [receiver] if shape.method == "load" && self.text(*receiver) == "ResourceLoader" => {
                "ResourceLoader.load".to_string()
            }
            _ => return,
        };
        let Some(path) = shape
            .arguments
            .first()
            .and_then(|first| static_string(*first, self.content))
        else {
            return;
        };
        let mut metadata = base_metadata("imports", "godot");
        insert_string(&mut metadata, "resource_path", path);
        insert_string(&mut metadata, "loader", &loader);
        if let Some(name) = bound_name(shape.anchor, self.content) {
            insert_string(&mut metadata, "bound_name", name);
        }
        self.push(
            RESOURCE_REFERENCE_PATTERN_ID,
            "resource_reference",
            shape.anchor,
            metadata,
        );
    }

    /// `extends "res://base.gd"`: the script extends another script file.
    fn script_extends(&mut self, extends: Node) {
        let Some(path) = named_children(extends)
            .into_iter()
            .find_map(|child| static_string(child, self.content))
        else {
            return;
        };
        let mut metadata = base_metadata("imports", "godot");
        insert_string(&mut metadata, "resource_path", path);
        insert_string(&mut metadata, "loader", "extends");
        self.push(
            RESOURCE_REFERENCE_PATTERN_ID,
            "resource_reference",
            extends,
            metadata,
        );
    }

    /// `$Path`, `$"Path/With Spaces"`, or `%UniqueName`.
    fn dollar_node_path(&mut self, node: Node) {
        let text = self.text(node);
        let (access, path) = match text.strip_prefix('%') {
            Some(unique) => ("unique_name", unique),
            None => ("dollar", text.strip_prefix('$').unwrap_or(text)),
        };
        let path = unquote(path).unwrap_or(path);
        self.push_node_path(node, access, path);
    }

    fn called_node_path(&mut self, shape: &CallShape) {
        let Some(path) = shape
            .arguments
            .first()
            .and_then(|first| static_string(*first, self.content))
        else {
            return;
        };
        self.push_node_path(shape.anchor, "get_node", path);
    }

    fn push_node_path(&mut self, node: Node, access: &str, path: &str) {
        if path.is_empty() {
            return;
        }
        let mut metadata = base_metadata("scene_tree", "godot");
        insert_string(&mut metadata, "node_path", path);
        insert_string(&mut metadata, "access", access);
        self.push(NODE_PATH_PATTERN_ID, "node_path", node, metadata);
    }

    /// `@rpc(...)` above a `func`: the function Godot may call over the network.
    fn rpc_annotation(&mut self, annotation: Node) {
        if identifier_child(annotation).map(|name| self.text(name)) != Some("rpc") {
            return;
        }
        let function = if annotation
            .parent()
            .is_some_and(|parent| parent.kind() == "annotations")
        {
            annotation.parent().and_then(|group| group.parent())
        } else {
            let mut next = annotation.next_named_sibling();
            while let Some(sibling) = next.filter(|sibling| sibling.kind() == "annotation") {
                next = sibling.next_named_sibling();
            }
            next
        };
        let Some(name) = function
            .filter(|function| function.kind() == "function_definition")
            .and_then(|function| function.child_by_field_name("name"))
        else {
            return;
        };
        let rpc_arguments = annotation
            .child_by_field_name("arguments")
            .map(named_children)
            .unwrap_or_default()
            .into_iter()
            .map(|argument| {
                static_string(argument, self.content)
                    .unwrap_or(self.text(argument))
                    .to_string()
            })
            .collect();
        let mut metadata = base_metadata("networking", "godot");
        insert_string(&mut metadata, "function_name", self.text(name));
        insert_string_array(&mut metadata, "rpc_arguments", rpc_arguments);
        self.push(
            RPC_ANNOTATION_PATTERN_ID,
            "rpc_annotation",
            annotation,
            metadata,
        );
    }
}

fn identifier_child(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find(|child| child.kind() == "identifier")
}

fn named_children(node: Node) -> Vec<Node> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

fn arguments(call: Node) -> Vec<Node> {
    call.child_by_field_name("arguments")
        .map(named_children)
        .unwrap_or_default()
}

/// The text of a plain `"..."` or `'...'` string with no interpolation.
fn static_string<'a>(node: Node, content: &'a str) -> Option<&'a str> {
    if node.kind() != "string" {
        return None;
    }
    unquote(content.get(node.start_byte()..node.end_byte())?)
}

fn unquote(text: &str) -> Option<&str> {
    ['"', '\'']
        .into_iter()
        .find_map(|quote| text.strip_prefix(quote)?.strip_suffix(quote))
}

/// The `const`/`var` whose initializer is this call, or this call cast with
/// `as`.
fn bound_name<'a>(call: Node, content: &'a str) -> Option<&'a str> {
    let mut value = call;
    let mut statement = call.parent()?;
    if statement.kind() == "binary_operator"
        && statement.child_by_field_name("left")?.id() == call.id()
        && statement.child_by_field_name("op")?.kind() == "as"
    {
        value = statement;
        statement = statement.parent()?;
    }
    let is_binding = matches!(
        statement.kind(),
        "const_statement"
            | "variable_statement"
            | "export_variable_statement"
            | "onready_variable_statement"
    );
    if !is_binding || statement.child_by_field_name("value")?.id() != value.id() {
        return None;
    }
    let name = statement.child_by_field_name("name")?;
    content.get(name.start_byte()..name.end_byte())
}
