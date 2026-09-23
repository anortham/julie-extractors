/// Helper functions for relationship extraction (identifier/invocation resolution, symbol lookup)
use crate::base::{
    Relationship, RelationshipKind, StructuredPendingRelationship, Symbol, SymbolKind,
    UnresolvedTarget,
};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashMap;
use tree_sitter::Node;

fn symbol_type(symbol: &Symbol) -> Option<&str> {
    symbol
        .metadata
        .as_ref()
        .and_then(|meta| meta.get("type"))
        .and_then(|value| value.as_str())
}

pub(super) fn is_component_symbol(symbol: &Symbol) -> bool {
    matches!(
        symbol_type(symbol),
        Some("razor-component")
            | Some("razor-view")
            | Some("external-component")
            | Some("blazor-component")
    )
}

fn is_type_scope(symbol: &Symbol) -> bool {
    matches!(
        symbol.kind,
        SymbolKind::Class | SymbolKind::Struct | SymbolKind::Interface
    )
}

/// The symbol a scope node declares: a callable, a type, or, for the file
/// root, the file-derived class.
fn scope_symbol<'a>(node: Node, symbols: &'a [Symbol]) -> Option<&'a Symbol> {
    symbols.iter().find(|symbol| {
        symbol.start_byte == node.start_byte() as u32
            && symbol.end_byte == node.end_byte() as u32
            && (is_type_scope(symbol)
                || matches!(
                    symbol.kind,
                    SymbolKind::Method | SymbolKind::Function | SymbolKind::Constructor
                ))
    })
}

fn callable_member<'a>(symbols: &'a [Symbol], scope_id: &str, name: &str) -> Option<&'a Symbol> {
    symbols.iter().find(|symbol| {
        symbol.parent_id.as_deref() == Some(scope_id)
            && matches!(symbol.kind, SymbolKind::Method | SymbolKind::Function)
            && symbol.name == name
    })
}

/// The method name a bare (`Save`), `this.Save`, or `base.Save` call names,
/// without type arguments, and whether it names a base-type member.
fn scoped_call_name(callee_text: &str) -> Option<(String, bool)> {
    let name = callee_text.split('<').next().unwrap_or(callee_text).trim();
    let (name, base_only) = match name.split_once('.') {
        Some(("this", rest)) => (rest, false),
        Some(("base", rest)) => (rest, true),
        Some(_) => return None,
        None => (name, false),
    };
    (!name.is_empty() && !name.contains('.')).then(|| (name.to_string(), base_only))
}

/// The unresolved target of a call written as `Name`, `receiver.Name`, or
/// `A.B.Name`, without type arguments. A `this.`/`base.` receiver names the
/// component itself, and a receiver that is not a plain name chain keeps only
/// the method name.
fn call_target(callee_text: &str) -> UnresolvedTarget {
    let mut depth = 0usize;
    let without_type_arguments: String = callee_text
        .chars()
        .filter(|character| {
            match character {
                '<' => depth += 1,
                '>' => depth = depth.saturating_sub(1),
                _ => return depth == 0,
            }
            false
        })
        .collect();
    let callee = without_type_arguments
        .strip_prefix("this.")
        .or_else(|| without_type_arguments.strip_prefix("base."))
        .unwrap_or(&without_type_arguments);
    let is_name_chain = callee
        .chars()
        .all(|character| character == '.' || character == '_' || character.is_alphanumeric());
    let terminal = callee.rsplit('.').next().unwrap_or(callee);
    is_name_chain
        .then(|| UnresolvedTarget::from_qualified_text(callee, &["."]))
        .flatten()
        .unwrap_or_else(|| UnresolvedTarget::simple(terminal))
}

pub(super) fn trim_quotes(value: &str) -> &str {
    value.trim_matches(|c| c == '"' || c == '\'')
}

impl super::RazorExtractor {
    /// Extract identifier component relationships
    pub(super) fn extract_identifier_component_relationships(
        &self,
        node: Node,
        symbols: &[Symbol],
        relationships: &mut Vec<Relationship>,
    ) {
        let identifier = self.base.get_node_text(&node);
        if identifier.is_empty()
            || node
                .parent()
                .is_some_and(|parent| parent.kind().ends_with("_declaration"))
        {
            return;
        }

        // Only consider potential component identifiers (PascalCase)
        if !identifier
            .chars()
            .next()
            .map(|c| c.is_ascii_uppercase())
            .unwrap_or(false)
        {
            return;
        }

        let component_symbol = symbols
            .iter()
            .find(|symbol| is_component_symbol(symbol) && symbol.name == identifier);

        let Some(component_symbol) = component_symbol else {
            return;
        };

        let Some(caller_symbol) = self.resolve_calling_symbol(node, symbols) else {
            return;
        };

        if caller_symbol.id == component_symbol.id {
            return;
        }

        // Avoid duplicate entries
        if relationships.iter().any(|rel| {
            rel.kind == RelationshipKind::Uses
                && rel.from_symbol_id == caller_symbol.id
                && rel.to_symbol_id == component_symbol.id
        }) {
            return;
        }

        relationships.push(self.base.create_relationship(
            caller_symbol.id.clone(),
            component_symbol.id.clone(),
            RelationshipKind::Uses,
            &node,
            Some(0.85),
            Some({
                let mut metadata = HashMap::new();
                metadata.insert(
                    "type".to_string(),
                    serde_json::Value::String("component-identifier".to_string()),
                );
                metadata.insert(
                    "component".to_string(),
                    serde_json::Value::String(identifier),
                );
                metadata
            }),
        ));
    }

    /// Extract invocation relationships. A bare, `this.`, or `base.` call
    /// resolves within its enclosing scopes; any other call stays pending.
    pub(super) fn extract_invocation_relationships(
        &self,
        node: Node,
        symbols: &[Symbol],
        relationships: &mut Vec<Relationship>,
        pending: &mut Vec<StructuredPendingRelationship>,
    ) {
        let method_node = self.find_child_by_types(
            node,
            &[
                "identifier",
                "generic_name",
                "member_access_expression",
                "qualified_name",
            ],
        );
        let Some(method_node) = method_node else {
            return;
        };

        let method_name = self.base.get_node_text(&method_node);
        if method_name.is_empty() {
            return;
        }

        let Some(caller_symbol) = self.resolve_calling_symbol(node, symbols) else {
            return;
        };

        let target = call_target(&method_name);
        let component_target = if method_name.contains("Component.InvokeAsync") {
            self.find_component_target_for_invocation(node, symbols)
        } else {
            None
        };
        let callee_symbol = scoped_call_name(&method_name).and_then(|(name, base_only)| {
            self.resolve_scoped_callee(node, &name, base_only, symbols)
        });

        let target_id = if let Some(component_symbol) = component_target {
            component_symbol.id.clone()
        } else if let Some(target) = callee_symbol {
            target.id.clone()
        } else {
            pending.push(
                self.base
                    .create_pending_relationship_at_target(
                        caller_symbol.id.clone(),
                        target,
                        RelationshipKind::Calls,
                        &method_node,
                        Some(caller_symbol.id.clone()),
                        Some(0.7),
                    )
                    .with_receiver_type(super::identifiers::self_receiver_type(&self.base, node)),
            );
            return;
        };

        if relationships.iter().any(|rel| {
            rel.kind == RelationshipKind::Calls
                && rel.from_symbol_id == caller_symbol.id
                && rel.to_symbol_id == target_id
        }) {
            return;
        }

        let mut metadata = HashMap::new();
        metadata.insert(
            "method".to_string(),
            serde_json::Value::String(method_name.clone()),
        );
        if let Some(component_symbol) = component_target {
            metadata.insert(
                "component".to_string(),
                serde_json::Value::String(component_symbol.name.clone()),
            );
        }

        relationships.push(self.base.create_relationship(
            caller_symbol.id.clone(),
            target_id,
            RelationshipKind::Calls,
            &node,
            Some(0.9),
            Some(metadata),
        ));
    }

    /// The same-file method or local function a scoped call runs: the
    /// innermost enclosing scope that declares a callable of that name, where
    /// a type scope also searches its same-file base types. A `base.` call
    /// searches only the base types of the innermost enclosing type.
    pub(super) fn resolve_scoped_callee<'a>(
        &self,
        node: Node,
        name: &str,
        base_only: bool,
        symbols: &'a [Symbol],
    ) -> Option<&'a Symbol> {
        let mut current = node.parent();
        while let Some(ancestor) = current {
            current = ancestor.parent();
            let Some(scope) = scope_symbol(ancestor, symbols) else {
                continue;
            };
            let is_type = is_type_scope(scope);
            if !(base_only && is_type)
                && let Some(member) = callable_member(symbols, &scope.id, name)
            {
                return Some(member);
            }
            if is_type && ancestor.kind() != "compilation_unit" {
                if let Some(inherited) = self.inherited_callable(ancestor, name, symbols, 0) {
                    return Some(inherited);
                }
                if base_only {
                    return None;
                }
            }
        }
        None
    }

    fn inherited_callable<'a>(
        &self,
        type_node: Node,
        name: &str,
        symbols: &'a [Symbol],
        depth: u32,
    ) -> Option<&'a Symbol> {
        if !should_visit_tree_depth(depth) {
            return None;
        }
        let child_depth = child_tree_depth(depth)?;
        let base_list = self.find_child_by_type(type_node, "base_list")?;
        let mut root = type_node;
        while let Some(parent) = root.parent() {
            root = parent;
        }
        let mut cursor = base_list.walk();
        let base_names: Vec<String> = base_list
            .named_children(&mut cursor)
            .map(|base| {
                let text = self.base.get_node_text(&base);
                let unqualified = text.split('<').next().unwrap_or(&text);
                unqualified
                    .rsplit('.')
                    .next()
                    .unwrap_or(unqualified)
                    .trim()
                    .to_string()
            })
            .collect();
        base_names.iter().find_map(|base_name| {
            let base_type = symbols
                .iter()
                .find(|symbol| is_type_scope(symbol) && &symbol.name == base_name)?;
            callable_member(symbols, &base_type.id, name).or_else(|| {
                let base_node = root.descendant_for_byte_range(
                    base_type.start_byte as usize,
                    base_type.end_byte as usize,
                )?;
                self.inherited_callable(base_node, name, symbols, child_depth)
            })
        })
    }

    pub(super) fn resolve_calling_symbol<'a>(
        &self,
        node: Node<'a>,
        symbols: &'a [Symbol],
    ) -> Option<&'a Symbol> {
        self.base.find_containing_symbol(&node, symbols)
    }

    pub(super) fn find_component_target_for_invocation<'a>(
        &self,
        node: Node<'a>,
        symbols: &'a [Symbol],
    ) -> Option<&'a Symbol> {
        let component_name = self.extract_first_string_literal(node)?;
        symbols
            .iter()
            .find(|symbol| is_component_symbol(symbol) && symbol.name == component_name)
    }

    fn extract_first_string_literal(&self, node: Node) -> Option<String> {
        self.extract_first_string_literal_at_depth(node, 0)
    }

    fn extract_first_string_literal_at_depth(&self, node: Node, depth: u32) -> Option<String> {
        if !should_visit_tree_depth(depth) {
            return None;
        }

        if node.kind() == "string_literal" {
            let text = self.base.get_node_text(&node);
            return Some(trim_quotes(&text).to_string());
        }

        let child_depth = child_tree_depth(depth)?;
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if let Some(value) = self.extract_first_string_literal_at_depth(child, child_depth) {
                return Some(value);
            }
        }

        None
    }
}
