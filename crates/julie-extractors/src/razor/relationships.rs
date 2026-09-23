/// Relationship extraction (component usage, bindings, method calls)
use crate::base::{
    NormalizedSpan, Relationship, RelationshipKind, StructuredPendingRelationship, Symbol,
    SymbolKind, UnresolvedTarget,
};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;
use tree_sitter::Node;

static BIND_PROPERTY_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"@bind-(\w+)").unwrap());

impl super::RazorExtractor {
    /// Extract relationships between symbols
    pub fn extract_relationships(
        &mut self,
        tree: &tree_sitter::Tree,
        symbols: &[Symbol],
    ) -> Vec<Relationship> {
        let mut relationships = Vec::new();
        let mut pending = Vec::new();
        self.visit_relationships(
            tree.root_node(),
            symbols,
            &mut relationships,
            &mut pending,
            0,
        );
        self.extract_using_line_relationships(tree.root_node(), symbols, &mut relationships);
        for row in pending {
            self.base.add_structured_pending_relationship(row);
        }
        relationships
    }

    fn visit_relationships(
        &self,
        node: Node,
        symbols: &[Symbol],
        relationships: &mut Vec<Relationship>,
        pending: &mut Vec<StructuredPendingRelationship>,
        depth: u32,
    ) {
        if !should_visit_tree_depth(depth) {
            return;
        }

        match node.kind() {
            "razor_component" => self.extract_component_relationships(node, symbols, relationships),
            "using_directive" => self.extract_using_relationships(node, symbols, relationships),
            "html_element" | "element" => {
                self.extract_element_relationships(node, symbols, relationships, pending)
            }
            "identifier" => {
                self.extract_identifier_component_relationships(node, symbols, relationships);
                self.extract_method_group_relationship(node, symbols, relationships, pending);
            }
            "invocation_expression" => {
                self.extract_invocation_relationships(node, symbols, relationships, pending)
            }
            "razor_inherits_directive"
            | "razor_implements_directive"
            | "razor_layout_directive" => {
                self.extract_directive_base_relationship(node, symbols, pending)
            }
            "class_declaration"
            | "record_declaration"
            | "struct_declaration"
            | "interface_declaration" => {
                self.extract_base_list_relationships(node, symbols, relationships, pending)
            }
            _ => {}
        }

        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_relationships(child, symbols, relationships, pending, child_depth);
        }
    }

    /// `@inherits Base` and `@implements IFace` declare the component's bases;
    /// `@layout MainLayout` names the layout component it renders inside.
    fn extract_directive_base_relationship(
        &self,
        node: Node,
        symbols: &[Symbol],
        pending: &mut Vec<StructuredPendingRelationship>,
    ) {
        let Some(component_id) = super::component_symbol_id(symbols) else {
            return;
        };
        let Some(type_node) = super::directives::directive_type_operand(node) else {
            return;
        };
        let kind = match node.kind() {
            "razor_implements_directive" => RelationshipKind::Implements,
            "razor_layout_directive" => RelationshipKind::Uses,
            _ => RelationshipKind::Extends,
        };
        pending.push(self.base.create_pending_relationship_at_target(
            component_id.clone(),
            UnresolvedTarget::simple(self.base.get_node_text(&type_node)),
            kind,
            &type_node,
            Some(component_id),
            Some(0.9),
        ));
    }

    /// A method group bound in markup (`@onclick="Refresh"`,
    /// `OnDelete="DeleteAsync"`, `@bind:after="Search"`) is a call from the
    /// markup's owner to that method. A name that no enclosing scope declares
    /// stays pending when the attribute is an event binding.
    fn extract_method_group_relationship(
        &self,
        node: Node,
        symbols: &[Symbol],
        relationships: &mut Vec<Relationship>,
        pending: &mut Vec<StructuredPendingRelationship>,
    ) {
        let Some(attribute) = super::method_group_attribute(node) else {
            return;
        };
        let name = self.base.get_node_text(&node);
        let Some(caller) = self.resolve_calling_symbol(node, symbols) else {
            return;
        };
        match self.resolve_scoped_callee(node, &name, false, symbols) {
            Some(handler) => relationships.push(self.base.create_relationship_at_target(
                caller.id.clone(),
                handler.id.clone(),
                RelationshipKind::Calls,
                &node,
                None,
                None,
            )),
            None if super::is_event_attribute(attribute, &self.base.content) => {
                pending.push(self.base.create_pending_relationship_at_target(
                    caller.id.clone(),
                    UnresolvedTarget::simple(name),
                    RelationshipKind::Calls,
                    &node,
                    Some(caller.id.clone()),
                    Some(0.7),
                ));
            }
            None => {}
        }
    }

    /// Base types of a type declared in a code block. A same-file base resolves
    /// to a relationship; .NET naming (`IName`) decides a cross-file base's kind.
    fn extract_base_list_relationships(
        &self,
        node: Node,
        symbols: &[Symbol],
        relationships: &mut Vec<Relationship>,
        pending: &mut Vec<StructuredPendingRelationship>,
    ) {
        let Some(base_list) = self.find_child_by_type(node, "base_list") else {
            return;
        };
        let Some(declared) = symbols
            .iter()
            .find(|symbol| symbol.start_byte == node.start_byte() as u32 && is_type_symbol(symbol))
        else {
            return;
        };
        let mut cursor = base_list.walk();
        for base_node in base_list.named_children(&mut cursor) {
            let base_name = self.base.get_node_text(&base_node);
            let terminal = base_name
                .split('<')
                .next()
                .and_then(|name| name.rsplit('.').next())
                .unwrap_or(&base_name)
                .trim()
                .to_string();
            match symbols
                .iter()
                .find(|symbol| is_type_symbol(symbol) && symbol.name == terminal)
            {
                Some(target) => relationships.push(self.base.create_relationship_at_target(
                    declared.id.clone(),
                    target.id.clone(),
                    if target.kind == SymbolKind::Interface {
                        RelationshipKind::Implements
                    } else {
                        RelationshipKind::Extends
                    },
                    &base_node,
                    None,
                    None,
                )),
                None => pending.push(self.base.create_pending_relationship_at_target(
                    declared.id.clone(),
                    UnresolvedTarget::simple(terminal.clone()),
                    if is_interface_name(&terminal) {
                        RelationshipKind::Implements
                    } else {
                        RelationshipKind::Extends
                    },
                    &base_node,
                    Some(declared.id.clone()),
                    Some(0.9),
                )),
            }
        }
    }

    /// Extract relationships between Razor components
    fn extract_component_relationships(
        &self,
        node: Node,
        symbols: &[Symbol],
        relationships: &mut Vec<Relationship>,
    ) {
        // Extract relationships between Razor components
        let _element_text = self.base.get_node_text(&node);

        // Look for component tag names (uppercase elements indicate components)
        if let Some(name_node) = self.find_child_by_type(node, "identifier") {
            let component_name = self.base.get_node_text(&name_node);

            // Find the using component (from symbols) - prefer the main page/component
            let from_symbol = symbols
                .iter()
                .find(|s| s.kind == SymbolKind::Class)
                .or_else(|| {
                    symbols.iter().find(|s| {
                        s.signature
                            .as_ref()
                            .is_some_and(|sig| sig.contains("@page"))
                    })
                })
                .or_else(|| symbols.iter().find(|s| s.kind == SymbolKind::Module));

            if let Some(from_sym) = from_symbol {
                // Create synthetic relationship to used component
                let to_symbol_id = format!("component-{}", component_name);

                relationships.push(self.base.create_relationship(
                    from_sym.id.clone(),
                    to_symbol_id,
                    RelationshipKind::Uses,
                    &node,
                    Some(1.0),
                    Some({
                        let mut metadata = HashMap::new();
                        metadata.insert(
                            "component".to_string(),
                            serde_json::Value::String(component_name.clone()),
                        );
                        metadata.insert(
                            "type".to_string(),
                            serde_json::Value::String("component-usage".to_string()),
                        );
                        metadata
                    }),
                ));
            }
        }
    }

    /// Extract using directive relationships
    fn extract_using_relationships(
        &self,
        node: Node,
        symbols: &[Symbol],
        relationships: &mut Vec<Relationship>,
    ) {
        // Extract using directive relationships
        if let Some(namespace_name) = self
            .find_child_by_type(node, "qualified_name")
            .map(|qualified_name| self.base.get_node_text(&qualified_name))
            .or_else(|| {
                self.base
                    .get_node_text(&node)
                    .strip_prefix("@using")
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .map(ToString::to_string)
            })
        {
            // Find any symbol that could be using this namespace
            if let Some(from_symbol) = symbols
                .iter()
                .find(|s| s.kind == SymbolKind::Class)
                .or_else(|| symbols.iter().find(|s| s.name == "@page"))
                .or_else(|| symbols.first())
            {
                let mut relationship = self.base.create_relationship(
                    from_symbol.id.clone(),
                    format!("namespace:{}", namespace_name),
                    RelationshipKind::Uses,
                    &node,
                    Some(0.8),
                    Some({
                        let mut metadata = HashMap::new();
                        metadata.insert(
                            "namespace".to_string(),
                            serde_json::Value::String(namespace_name.clone()),
                        );
                        metadata.insert(
                            "type".to_string(),
                            serde_json::Value::String("using-directive".to_string()),
                        );
                        metadata
                    }),
                );
                relationship.span =
                    unique_content_occurrence_span(&self.base.content, &namespace_name);
                if let Some(span) = relationship.span {
                    relationship.line_number = span.start_line;
                }
                relationships.push(relationship);
            }
        }
    }

    fn extract_using_line_relationships(
        &self,
        root: Node,
        symbols: &[Symbol],
        relationships: &mut Vec<Relationship>,
    ) {
        let Some(from_symbol) = symbols
            .iter()
            .find(|s| s.kind == SymbolKind::Class)
            .or_else(|| symbols.iter().find(|s| s.name == "@page"))
            .or_else(|| symbols.first())
        else {
            return;
        };

        for (line_index, line) in self.base.content.lines().enumerate() {
            let line_number = line_index as u32 + 1;
            let namespace_name = line
                .trim()
                .strip_prefix("@using")
                .map(str::trim)
                .filter(|name| is_namespace_like(name));
            if let Some(namespace_name) = namespace_name {
                let to_symbol_id = format!("namespace:{}", namespace_name);
                if relationships.iter().any(|relationship| {
                    relationship.from_symbol_id == from_symbol.id
                        && relationship.to_symbol_id == to_symbol_id
                }) {
                    continue;
                }

                let mut relationship = self.base.create_relationship(
                    from_symbol.id.clone(),
                    to_symbol_id,
                    RelationshipKind::Uses,
                    &root,
                    Some(0.8),
                    Some({
                        let mut metadata = HashMap::new();
                        metadata.insert(
                            "namespace".to_string(),
                            serde_json::Value::String(namespace_name.to_string()),
                        );
                        metadata.insert(
                            "type".to_string(),
                            serde_json::Value::String("using-directive".to_string()),
                        );
                        metadata
                    }),
                );
                relationship.span = line.find(namespace_name).and_then(|start_column| {
                    NormalizedSpan::from_line_match(
                        &self.base.content,
                        line_number,
                        start_column,
                        namespace_name,
                    )
                });
                relationship.line_number = line_number;
                relationships.push(relationship);
            }
        }
    }

    /// Extract relationships from HTML elements with bindings
    fn extract_element_relationships(
        &self,
        node: Node,
        symbols: &[Symbol],
        relationships: &mut Vec<Relationship>,
        pending: &mut Vec<StructuredPendingRelationship>,
    ) {
        let element_text = self.base.get_node_text(&node);

        if self.is_razor_component_file()
            && let Some((tag_start, tag_name)) =
                super::element_component_tag(node, &self.base.content)
            && let Some(from_symbol) = self.resolve_calling_symbol(node, symbols)
        {
            let tag_name = tag_name.to_string();
            match symbols
                .iter()
                .find(|s| s.name == tag_name && s.id != from_symbol.id)
            {
                Some(component_symbol) => relationships.push(self.base.create_relationship(
                    from_symbol.id.clone(),
                    component_symbol.id.clone(),
                    RelationshipKind::Uses,
                    &node,
                    Some(1.0),
                    Some(HashMap::from([
                        (
                            "component".to_string(),
                            serde_json::Value::String(tag_name.clone()),
                        ),
                        (
                            "type".to_string(),
                            serde_json::Value::String("component-usage".to_string()),
                        ),
                    ])),
                )),
                None => {
                    let row = StructuredPendingRelationship::new(
                        from_symbol.id.clone(),
                        UnresolvedTarget::from_qualified_text(&tag_name, &["."])
                            .unwrap_or_else(|| UnresolvedTarget::simple(tag_name.clone())),
                        Some(from_symbol.id.clone()),
                        RelationshipKind::Uses,
                        self.base.file_path.clone(),
                        node.start_position().row as u32 + 1,
                        0.9,
                    );
                    pending.push(
                        match NormalizedSpan::from_content_range(
                            &self.base.content,
                            tag_start,
                            tag_start + tag_name.len(),
                        ) {
                            Some(span) => row.with_target_span(span),
                            None => row,
                        },
                    );
                }
            }
        }

        if element_text.contains("@bind")
            && let Some(from_symbol) = symbols.iter().find(|s| s.kind == SymbolKind::Class)
        {
            // Extract property being bound
            if let Some(captures) = BIND_PROPERTY_RE.captures(&element_text)
                && let Some(property_match) = captures.get(1)
            {
                let property_name = property_match.as_str().to_string();

                relationships.push(self.base.create_relationship(
                    from_symbol.id.clone(),
                    format!("property-{}", property_name), // Create synthetic ID for bound properties
                    RelationshipKind::Uses,
                    &node,
                    Some(0.9),
                    Some({
                        let mut metadata = HashMap::new();
                        metadata.insert(
                            "property".to_string(),
                            serde_json::Value::String(property_name),
                        );
                        metadata.insert(
                            "type".to_string(),
                            serde_json::Value::String("data-binding".to_string()),
                        );
                        metadata
                    }),
                ));
            }
        }
    }
}

fn unique_content_occurrence_span(content: &str, needle: &str) -> Option<NormalizedSpan> {
    let mut matches = content.match_indices(needle);
    let (start, _) = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    NormalizedSpan::from_content_range(content, start, start + needle.len())
}

fn is_namespace_like(name: &str) -> bool {
    !name.is_empty()
        && name
            .split('.')
            .all(|segment| is_identifier_segment(segment.trim()))
}

fn is_identifier_segment(segment: &str) -> bool {
    let mut chars = segment.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn is_type_symbol(symbol: &Symbol) -> bool {
    matches!(
        symbol.kind,
        SymbolKind::Class | SymbolKind::Interface | SymbolKind::Struct
    )
}

fn is_interface_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!((chars.next(), chars.next()), (Some('I'), Some(c)) if c.is_ascii_uppercase())
}
