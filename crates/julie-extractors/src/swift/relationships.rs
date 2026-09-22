use crate::base::{
    LocalTargetResolution, OwnerIndex, Relationship, RelationshipKind, ScopedSymbolIndex, Symbol,
    SymbolKind, UnresolvedTarget, is_test_call_symbol,
};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashMap;
use tree_sitter::Node;

use super::SwiftExtractor;
use super::external_symbols::{
    SwiftImportContext, is_external_call_target, is_external_inheritance_target,
};
use super::identifiers::call_callee;

/// Extracts inheritance, protocol conformance, and call relationships in Swift
impl SwiftExtractor {
    /// Extract relationships between Swift types and function calls
    /// Implementation of extractRelationships method
    pub fn extract_relationships(
        &mut self,
        tree: &tree_sitter::Tree,
        symbols: &[Symbol],
    ) -> Vec<Relationship> {
        let mut relationships = Vec::new();
        let targets: Vec<Symbol> = symbols
            .iter()
            .filter(|symbol| !is_test_call_symbol(symbol))
            .cloned()
            .collect();
        let symbol_index = ScopedSymbolIndex::new(&targets);
        let owners = OwnerIndex::new(&self.base, symbols);
        let import_context = SwiftImportContext::from_symbols(symbols);
        self.visit_node_for_relationships(
            tree.root_node(),
            symbols,
            &CallScope {
                owners: &owners,
                symbols: &symbol_index,
            },
            &import_context,
            &mut relationships,
            0,
        );
        relationships
    }

    fn visit_node_for_relationships<'a>(
        &mut self,
        node: Node,
        symbols: &'a [Symbol],
        scope: &CallScope<'_, 'a>,
        import_context: &SwiftImportContext,
        relationships: &mut Vec<Relationship>,
        depth: u32,
    ) {
        if !should_visit_tree_depth(depth) {
            return;
        }

        match node.kind() {
            "class_declaration"
            | "struct_declaration"
            | "enum_declaration"
            | "extension_declaration"
            | "protocol_declaration" => {
                self.extract_inheritance_relationships(
                    node,
                    symbols,
                    import_context,
                    relationships,
                );
            }
            "call_expression" | "constructor_expression" => {
                self.extract_call_relationship(node, scope, import_context, relationships);
            }
            _ => {}
        }

        // Recursively visit children
        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_node_for_relationships(
                child,
                symbols,
                scope,
                import_context,
                relationships,
                child_depth,
            );
        }
    }

    /// Implementation of extractInheritanceRelationships method
    fn extract_inheritance_relationships(
        &mut self,
        node: Node,
        symbols: &[Symbol],
        import_context: &SwiftImportContext,
        relationships: &mut Vec<Relationship>,
    ) {
        if let Some(type_symbol) = self.find_type_symbol(node, symbols) {
            let mut inheritance_entry_index = 0usize;
            let declaration_kind = self.declaration_kind_for_relationships(node);
            let declaration_kind = declaration_kind.as_str();

            // Try type_inheritance_clause first
            if let Some(inheritance) = node
                .children(&mut node.walk())
                .find(|c| c.kind() == "type_inheritance_clause")
            {
                for child in inheritance.children(&mut inheritance.walk()) {
                    if let Some(base_type_name) = self.inheritance_type_name(child) {
                        let pending_kind = Self::pending_inheritance_kind(
                            &type_symbol,
                            declaration_kind,
                            inheritance_entry_index,
                        );
                        self.add_inheritance_relationship(
                            &type_symbol,
                            &base_type_name,
                            pending_kind,
                            symbols,
                            import_context,
                            relationships,
                            node,
                        );
                        inheritance_entry_index += 1;
                    }
                }
            }

            // Also handle direct inheritance_specifier nodes
            for spec in node
                .children(&mut node.walk())
                .filter(|c| c.kind() == "inheritance_specifier")
            {
                if let Some(type_node) = spec
                    .children(&mut spec.walk())
                    .find(|c| matches!(c.kind(), "user_type" | "type_identifier" | "type"))
                {
                    let base_type_name = if type_node.kind() == "user_type" {
                        if let Some(inner_type_node) = type_node
                            .children(&mut type_node.walk())
                            .find(|c| c.kind() == "type_identifier")
                        {
                            self.base.get_node_text(&inner_type_node)
                        } else {
                            self.base.get_node_text(&type_node)
                        }
                    } else {
                        self.base.get_node_text(&type_node)
                    };
                    let pending_kind = Self::pending_inheritance_kind(
                        &type_symbol,
                        declaration_kind,
                        inheritance_entry_index,
                    );
                    self.add_inheritance_relationship(
                        &type_symbol,
                        &base_type_name,
                        pending_kind,
                        symbols,
                        import_context,
                        relationships,
                        node,
                    );
                    inheritance_entry_index += 1;
                }
            }
        }
    }

    /// The `declaration_kind` keyword: `class`, `struct`, `enum`, `actor`,
    /// `extension`, or `protocol`.
    fn declaration_kind_for_relationships(&self, node: Node) -> String {
        node.child_by_field_name("declaration_kind")
            .map(|kind| self.base.get_node_text(&kind))
            .unwrap_or_else(|| node.kind().trim_end_matches("_declaration").to_string())
    }

    fn inheritance_type_name(&self, node: Node) -> Option<String> {
        match node.kind() {
            "type_identifier" | "type" => Some(self.base.get_node_text(&node)),
            "user_type" => node
                .children(&mut node.walk())
                .find(|child| child.kind() == "type_identifier")
                .map(|child| self.base.get_node_text(&child)),
            _ => None,
        }
    }

    /// A class or actor extends its first base and implements the rest; a
    /// protocol refines (extends) protocols; value types and extensions conform.
    fn pending_inheritance_kind(
        type_symbol: &Symbol,
        declaration_kind: &str,
        inheritance_entry_index: usize,
    ) -> RelationshipKind {
        match declaration_kind {
            "extension" | "struct" | "enum" => RelationshipKind::Implements,
            "class" | "actor" if inheritance_entry_index == 0 => RelationshipKind::Extends,
            "class" | "actor" => RelationshipKind::Implements,
            "protocol" => RelationshipKind::Extends,
            _ => match type_symbol.kind {
                SymbolKind::Interface => RelationshipKind::Extends,
                SymbolKind::Class if inheritance_entry_index == 0 => RelationshipKind::Extends,
                _ => RelationshipKind::Implements,
            },
        }
    }

    /// Implementation of addInheritanceRelationship method
    #[allow(clippy::too_many_arguments)]
    fn add_inheritance_relationship(
        &mut self,
        type_symbol: &Symbol,
        base_type_name: &str,
        pending_kind: RelationshipKind,
        symbols: &[Symbol],
        import_context: &SwiftImportContext,
        relationships: &mut Vec<Relationship>,
        node: Node,
    ) {
        // Find the actual base type symbol
        if let Some(base_type_symbol) = symbols.iter().find(|s| {
            s.name == base_type_name
                && matches!(
                    s.kind,
                    SymbolKind::Class | SymbolKind::Interface | SymbolKind::Struct
                )
        }) {
            let relationship_kind = if type_symbol.kind == SymbolKind::Interface
                || base_type_symbol.kind != SymbolKind::Interface
            {
                RelationshipKind::Extends
            } else {
                RelationshipKind::Implements
            };

            let metadata = HashMap::from([(
                "baseType".to_string(),
                serde_json::Value::String(base_type_name.to_string()),
            )]);

            relationships.push(Relationship {
                id: format!(
                    "{}_{}_{:?}_{}",
                    type_symbol.id,
                    base_type_symbol.id,
                    relationship_kind,
                    node.start_position().row
                ),
                from_symbol_id: type_symbol.id.clone(),
                to_symbol_id: base_type_symbol.id.clone(),
                kind: relationship_kind,
                file_path: self.base.file_path.clone(),
                line_number: (node.start_position().row + 1) as u32,
                span: Some(crate::base::NormalizedSpan::from_node(&node)),
                reference_site_is_exact: false,
                confidence: 1.0,
                metadata: Some(metadata),
            });
        } else if !is_external_inheritance_target(base_type_name, import_context) {
            let pending = self.base.create_pending_relationship(
                type_symbol.id.clone(),
                UnresolvedTarget::simple(base_type_name.to_string()),
                pending_kind,
                &node,
                Some(type_symbol.id.clone()),
                Some(0.9),
            );
            self.add_structured_pending_relationship(pending);
        }
    }

    /// Legacy metadata-derived types. Every value is reduced to a base type
    /// name; metadata text with no single base name records nothing. Rows the
    /// base recorded during extraction win over these in the registry.
    pub fn infer_types(&self, symbols: &[Symbol]) -> HashMap<String, String> {
        let mut types = HashMap::new();
        for symbol in symbols {
            let metadata_text = |key: &str| {
                symbol
                    .metadata
                    .as_ref()
                    .and_then(|m| m.get(key))
                    .and_then(|value| value.as_str())
            };
            let declared = match symbol.kind {
                SymbolKind::Function | SymbolKind::Method => metadata_text("returnType"),
                SymbolKind::Property | SymbolKind::Variable => {
                    metadata_text("propertyType").or_else(|| metadata_text("variableType"))
                }
                _ => None,
            };
            if let Some(resolved) = declared.and_then(super::type_facts::legacy_base_type_name) {
                types.insert(symbol.id.clone(), resolved);
            }
        }
        types
    }

    /// The symbol a type declaration or extension declares. An extension of a
    /// type declared in this file stands for that type; any other extension
    /// stands for itself.
    pub(super) fn find_type_symbol(&self, node: Node, symbols: &[Symbol]) -> Option<Symbol> {
        let own = symbols.iter().find(|s| {
            s.start_byte as usize == node.start_byte()
                && s.end_byte as usize == node.end_byte()
                && matches!(
                    s.kind,
                    SymbolKind::Class
                        | SymbolKind::Struct
                        | SymbolKind::Interface
                        | SymbolKind::Enum
                        | SymbolKind::Module
                )
        })?;
        if own.kind != SymbolKind::Module {
            return Some(own.clone());
        }
        symbols
            .iter()
            .find(|s| {
                s.name == own.name
                    && matches!(
                        s.kind,
                        SymbolKind::Class
                            | SymbolKind::Struct
                            | SymbolKind::Interface
                            | SymbolKind::Enum
                    )
            })
            .unwrap_or(own)
            .clone()
            .into()
    }

    /// A call's target comes from its callee node: the called name and the
    /// receiver expression before it. `Type(...)` constructions call the type.
    fn extract_call_relationship(
        &mut self,
        node: Node,
        scope: &CallScope<'_, '_>,
        import_context: &SwiftImportContext,
        relationships: &mut Vec<Relationship>,
    ) {
        let (function_name, receiver) = if node.kind() == "constructor_expression" {
            let Some(name) = node
                .child_by_field_name("constructed_type")
                .and_then(|type_node| super::type_facts::base_type_name(&self.base, type_node))
            else {
                return;
            };
            (name, None)
        } else {
            let Some(callee) = call_callee(&self.base, node) else {
                return;
            };
            (
                self.base.get_node_text(&callee.name),
                callee
                    .receiver
                    .map(|receiver| self.base.get_node_text(&receiver)),
            )
        };

        let receiver_type = super::identifiers::self_receiver_type(&self.base, node);

        let Some(caller) = scope.owners.find(node) else {
            return;
        };

        let target = self.unresolved_call_target(receiver.as_deref(), &function_name);
        let line_number = node.start_position().row as u32 + 1;
        let file_path = self.base.file_path.clone();

        let resolution = if target.namespace_path.is_empty() {
            scope.symbols.resolve_call_target(
                target.terminal_name.as_str(),
                Some(caller),
                target.receiver.as_deref(),
            )
        } else {
            LocalTargetResolution::Missing
        };
        match resolution {
            LocalTargetResolution::Resolved(called_symbol) => {
                relationships.push(Relationship {
                    id: format!(
                        "{}_{}_{:?}_{}",
                        caller.id,
                        called_symbol.id,
                        RelationshipKind::Calls,
                        node.start_position().row
                    ),
                    from_symbol_id: caller.id.clone(),
                    to_symbol_id: called_symbol.id.clone(),
                    kind: RelationshipKind::Calls,
                    file_path,
                    line_number,
                    span: Some(crate::base::NormalizedSpan::from_node(&node)),
                    reference_site_is_exact: false,
                    confidence: 0.9,
                    metadata: None,
                });
            }
            LocalTargetResolution::Import(_)
            | LocalTargetResolution::Ambiguous
            | LocalTargetResolution::ReceiverQualified
            | LocalTargetResolution::Missing => {
                if is_external_call_target(&target, import_context) {
                    return;
                }

                let pending = self
                    .base
                    .create_pending_relationship(
                        caller.id.clone(),
                        target,
                        RelationshipKind::Calls,
                        &node,
                        Some(caller.id.clone()),
                        Some(0.7),
                    )
                    .with_receiver_type(receiver_type);
                self.add_structured_pending_relationship(pending);
            }
        }
    }

    fn unresolved_call_target(&self, receiver: Option<&str>, name: &str) -> UnresolvedTarget {
        let Some(receiver) = receiver.map(str::trim).filter(|r| !r.is_empty()) else {
            return UnresolvedTarget::from_qualified_text(name, &["."])
                .unwrap_or_else(|| UnresolvedTarget::simple(name.to_string()));
        };
        if let Some(target) =
            UnresolvedTarget::from_qualified_text(&format!("{receiver}.{name}"), &["."])
        {
            return target;
        }
        let mut parts = receiver
            .split('.')
            .map(str::trim)
            .map(str::to_string)
            .collect::<Vec<_>>();
        if parts.iter().all(|part| {
            part.strip_prefix('`')
                .and_then(|part| part.strip_suffix('`'))
                .is_some_and(|part| !part.is_empty())
                || UnresolvedTarget::from_qualified_text(part, &["."]).is_some()
        }) {
            parts.push(name.to_string());
            return UnresolvedTarget::from_chain(parts);
        }
        UnresolvedTarget {
            display_name: format!("{receiver}.{name}"),
            terminal_name: name.to_string(),
            receiver: Some(receiver.to_string()),
            namespace_path: Vec::new(),
            import_context: None,
        }
    }
}

struct CallScope<'o, 'a> {
    owners: &'o OwnerIndex<'a>,
    symbols: &'o ScopedSymbolIndex<'a>,
}
