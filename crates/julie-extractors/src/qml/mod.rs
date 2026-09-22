// QML (Qt Modeling Language) Extractor Implementation
// QML is JavaScript-based declarative UI language for Qt applications
// Tree-sitter-qmljs extends TypeScript grammar with QML-specific nodes

mod identifiers;
mod imports;
mod locals;
mod relationships;
mod semantics;
mod type_facts;
mod typeinfo;

pub(crate) use imports::{import_kind, source_kind as import_source_kind};
pub(crate) use typeinfo::is_typeinfo_path;

use crate::base::{
    BaseExtractor, ContainingSymbolIndex, Identifier, PendingRelationship, Relationship,
    StructuredPendingRelationship, Symbol, SymbolKind,
};
use crate::test_detection::{apply_callable_test_metadata, mark_base_type_test_containers};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashMap;
use tree_sitter::Tree;

pub struct QmlExtractor {
    pub(crate) base: BaseExtractor,
    symbols: Vec<Symbol>,
}

impl QmlExtractor {
    pub fn new(
        language: String,
        file_path: String,
        content: String,
        workspace_root: &std::path::Path,
    ) -> Self {
        Self {
            base: BaseExtractor::new(language, file_path, content, workspace_root),
            symbols: Vec::new(),
        }
    }

    pub fn extract_symbols(&mut self, tree: &Tree) -> Vec<Symbol> {
        let root_node = tree.root_node();
        self.symbols.clear();

        if is_typeinfo_path(&self.base.file_path) {
            typeinfo::extract(self, root_node);
        } else {
            self.traverse_node(root_node, None, 0);
        }
        mark_base_type_test_containers(&mut self.symbols, "TestCase");

        self.symbols.clone()
    }

    /// Recursively traverse the QML AST and extract symbols
    fn traverse_node(&mut self, node: tree_sitter::Node, parent_id: Option<String>, depth: u32) {
        use crate::base::{SymbolKind, SymbolOptions};

        if !should_visit_tree_depth(depth) {
            return;
        }

        let mut current_symbol: Option<Symbol> = None;

        match node.kind() {
            // QML import statements (import QtQuick 2.15, import org.kde.plasma.core as Plasma)
            "ui_import" => {
                if let Some(symbol) = imports::extract(self, &node, parent_id.clone()) {
                    self.symbols.push(symbol);
                }
            }

            // QML object definitions (Rectangle, Window, Button, etc.)
            // Only the root object (parent_id is None) is a true definition —
            // it declares the file's component base type. Nested objects are
            // component instantiations (usages), not definitions.
            //
            // In QML, the file name IS the component name (e.g., ScrollablePage.qml
            // defines "ScrollablePage"). The root element is the base type it extends.
            "ui_object_definition" => {
                if let Some(type_name) = node.child_by_field_name("type_name")
                    && parent_id.is_none()
                {
                    let base_type = self.base.get_node_text(&type_name);

                    let component_name = semantics::component_name(&self.base.file_path)
                        .unwrap_or_else(|| base_type.clone());

                    let singleton = semantics::file_declares_singleton(&self.base, node);
                    let signature = Some(if singleton {
                        format!("singleton extends {}", base_type)
                    } else {
                        format!("extends {}", base_type)
                    });
                    // Emit the root component's base type under the canonical
                    // `base_types` key. Artifact v1 preserves this metadata
                    // evidence without assigning old Julie test-container roles.
                    let mut metadata = HashMap::new();
                    metadata.insert(
                        "base_types".to_string(),
                        serde_json::Value::Array(vec![serde_json::Value::String(
                            base_type.clone(),
                        )]),
                    );
                    if singleton {
                        metadata.insert("singleton".to_string(), serde_json::Value::Bool(true));
                    }
                    let options = SymbolOptions {
                        parent_id: parent_id.clone(),
                        signature,
                        visibility: Some(crate::base::Visibility::Public),
                        metadata: Some(metadata),
                        doc_comment: semantics::extract_qml_doc_comment(self, &node),
                        ..Default::default()
                    };
                    let symbol =
                        self.base
                            .create_symbol(&node, component_name, SymbolKind::Class, options);
                    self.symbols.push(symbol.clone());
                    current_symbol = Some(symbol);
                } else if !semantics::object_has_class_row(node)
                    && !semantics::is_grouped_property_block(&self.base, node)
                {
                    current_symbol = self.push_object_symbol(node, parent_id.clone(), None);
                }
            }

            // Property value sources (Behavior on color { NumberAnimation {} })
            "ui_object_definition_binding" => {
                let value_source_property = node
                    .child_by_field_name("name")
                    .map(|name| self.base.get_node_text(&name));
                current_symbol =
                    self.push_object_symbol(node, parent_id.clone(), value_source_property);
            }

            // QML properties (property int age: 42, property alias foo: bar.baz)
            "ui_property" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = self.base.get_node_text(&name_node);
                    let signature = Some(semantics::property_signature(&self.base, node));
                    let options = SymbolOptions {
                        parent_id: parent_id.clone(),
                        signature,
                        visibility: Some(semantics::infer_visibility(&name, false)),
                        doc_comment: semantics::extract_qml_doc_comment(self, &node),
                        ..Default::default()
                    };
                    let symbol =
                        self.base
                            .create_symbol(&node, name, SymbolKind::Property, options);
                    type_facts::record_property_type(&mut self.base, &symbol.id, node);
                    self.symbols.push(symbol);
                }
            }

            // QML id bindings (id: root) — critical for QML component referencing
            "ui_binding" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let binding_name = self.base.get_node_text(&name_node);
                    if binding_name == "id"
                        && semantics::enclosing_object(node)
                            .is_none_or(semantics::object_has_class_row)
                    {
                        // Extract the id value from the expression_statement > identifier
                        if let Some(value_node) = node.child_by_field_name("value") {
                            // value is an expression_statement wrapping an identifier
                            let id_value = if value_node.kind() == "expression_statement" {
                                if let Some(inner) = value_node.named_child(0) {
                                    self.base.get_node_text(&inner)
                                } else {
                                    self.base.get_node_text(&value_node)
                                }
                            } else {
                                self.base.get_node_text(&value_node)
                            };
                            let signature = Some(format!("id: {}", id_value));
                            let options = SymbolOptions {
                                parent_id: parent_id.clone(),
                                signature,
                                visibility: Some(crate::base::Visibility::Private),
                                metadata: Some({
                                    let mut meta = HashMap::new();
                                    meta.insert(
                                        "binding_kind".to_string(),
                                        serde_json::Value::String("id".to_string()),
                                    );
                                    meta
                                }),
                                doc_comment: semantics::extract_qml_doc_comment(self, &node),
                                ..Default::default()
                            };
                            let symbol = self.base.create_symbol(
                                &node,
                                id_value,
                                SymbolKind::Property,
                                options,
                            );
                            if let Some(component) = parent_id.as_deref().and_then(|id| {
                                self.symbols.iter().find(|candidate| candidate.id == id)
                            }) {
                                let component_name = component.name.clone();
                                type_facts::record_named_type(
                                    &mut self.base,
                                    &symbol.id,
                                    &component_name,
                                );
                            }
                            self.symbols.push(symbol);
                        }
                    } else if semantics::is_signal_handler_binding_name(&binding_name) {
                        let options = SymbolOptions {
                            parent_id: parent_id.clone(),
                            signature: Some(self.base.get_node_text(&node)),
                            visibility: Some(crate::base::Visibility::Private),
                            metadata: Some({
                                let mut meta = HashMap::new();
                                meta.insert(
                                    "binding_kind".to_string(),
                                    serde_json::Value::String("signal_handler".to_string()),
                                );
                                if let Some(signal_name) =
                                    semantics::handled_signal_from_binding_name(&binding_name)
                                {
                                    meta.insert(
                                        "handled_signal".to_string(),
                                        serde_json::Value::String(signal_name),
                                    );
                                }
                                meta
                            }),
                            doc_comment: semantics::extract_qml_doc_comment(self, &node),
                            ..Default::default()
                        };
                        let symbol = self.base.create_symbol(
                            &node,
                            binding_name,
                            SymbolKind::Function,
                            options,
                        );
                        self.symbols.push(symbol);
                    }
                }
            }

            // QML enum declarations (enum Direction { Left, Right, Up, Down })
            "enum_declaration" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = self.base.get_node_text(&name_node);
                    let options = SymbolOptions {
                        parent_id: parent_id.clone(),
                        visibility: Some(semantics::infer_visibility(&name, false)),
                        doc_comment: semantics::extract_qml_doc_comment(self, &node),
                        ..Default::default()
                    };
                    let enum_symbol =
                        self.base
                            .create_symbol(&node, name, SymbolKind::Enum, options);
                    let enum_id = enum_symbol.id.clone();
                    self.symbols.push(enum_symbol);

                    // Extract enum members from the enum_body
                    if let Some(body) = node.child_by_field_name("body") {
                        let mut body_cursor = body.walk();
                        for member in body.children_by_field_name("name", &mut body_cursor) {
                            let member_name = self.base.get_node_text(&member);
                            let member_options = SymbolOptions {
                                parent_id: Some(enum_id.clone()),
                                ..Default::default()
                            };
                            let member_symbol = self.base.create_symbol(
                                &member,
                                member_name,
                                SymbolKind::EnumMember,
                                member_options,
                            );
                            self.symbols.push(member_symbol);
                        }
                    }
                }
            }

            // QML signals (signal clicked(x, y))
            "ui_signal" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = self.base.get_node_text(&name_node);
                    let parameters = semantics::signal_parameters(&self.base, node);
                    let mut metadata = HashMap::new();
                    if !parameters.is_empty() {
                        metadata.insert(
                            "parameters".to_string(),
                            serde_json::Value::Array(parameters),
                        );
                    }
                    let options = SymbolOptions {
                        parent_id: parent_id.clone(),
                        signature: Some(self.base.get_node_text(&node)),
                        metadata: (!metadata.is_empty()).then_some(metadata),
                        ..Default::default()
                    };
                    let symbol = self
                        .base
                        .create_symbol(&node, name, SymbolKind::Event, options);
                    self.symbols.push(symbol);
                }
            }

            "ui_inline_component" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = self.base.get_node_text(&name_node);
                    let base_type = node
                        .child_by_field_name("component")
                        .and_then(|component| component.child_by_field_name("type_name"))
                        .map(|type_name| self.base.get_node_text(&type_name));
                    let signature = Some(match &base_type {
                        Some(base_type) => format!("component {}: {}", name, base_type),
                        None => format!("component {}", name),
                    });
                    let mut metadata = HashMap::new();
                    if let Some(base_type) = base_type {
                        metadata.insert(
                            "base_types".to_string(),
                            serde_json::Value::Array(vec![serde_json::Value::String(base_type)]),
                        );
                    }
                    let options = SymbolOptions {
                        parent_id: parent_id.clone(),
                        signature,
                        visibility: Some(crate::base::Visibility::Public),
                        metadata: (!metadata.is_empty()).then_some(metadata),
                        doc_comment: semantics::extract_qml_doc_comment(self, &node),
                        ..Default::default()
                    };
                    let symbol = self
                        .base
                        .create_symbol(&node, name, SymbolKind::Class, options);
                    self.symbols.push(symbol.clone());
                    current_symbol = Some(symbol);
                }
            }

            "ui_required" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = self.base.get_node_text(&name_node);
                    let mut metadata = HashMap::new();
                    metadata.insert("required".to_string(), serde_json::Value::Bool(true));
                    metadata.insert("inherited".to_string(), serde_json::Value::Bool(true));
                    let options = SymbolOptions {
                        parent_id: parent_id.clone(),
                        signature: Some(self.base.get_node_text(&node)),
                        visibility: Some(semantics::infer_visibility(&name, false)),
                        metadata: Some(metadata),
                        doc_comment: semantics::extract_qml_doc_comment(self, &node),
                        ..Default::default()
                    };
                    let symbol =
                        self.base
                            .create_symbol(&node, name, SymbolKind::Property, options);
                    self.symbols.push(symbol);
                }
            }

            // JavaScript functions (inherited from TypeScript grammar)
            "function_declaration" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = self.base.get_node_text(&name_node);
                    let mut metadata = std::collections::HashMap::new();
                    apply_callable_test_metadata(
                        "qml",
                        &name,
                        &self.base.file_path,
                        &SymbolKind::Function,
                        &[],
                        None,
                        &mut metadata,
                    );
                    if semantics::encloses_connections_object(&self.base, node)
                        && semantics::is_signal_handler_binding_name(&name)
                        && let Some(signal) = semantics::handled_signal_from_binding_name(&name)
                    {
                        metadata.insert(
                            "handled_signal".to_string(),
                            serde_json::Value::String(signal),
                        );
                    }
                    let options = SymbolOptions {
                        parent_id: parent_id.clone(),
                        signature: Some(semantics::function_signature(
                            self.base.get_node_text(&node),
                        )),
                        visibility: Some(semantics::infer_visibility(&name, false)),
                        metadata: if metadata.is_empty() {
                            None
                        } else {
                            Some(metadata)
                        },
                        doc_comment: semantics::extract_qml_doc_comment(self, &node),
                        ..Default::default()
                    };
                    let symbol =
                        self.base
                            .create_symbol(&node, name, SymbolKind::Function, options);
                    let function_id = symbol.id.clone();
                    type_facts::record_annotation_type(
                        &mut self.base,
                        &function_id,
                        node,
                        "return_type",
                    );
                    for (param, param_node) in
                        crate::javascript::parameters::extract_parameter_symbols(
                            &mut self.base,
                            node,
                            &function_id,
                        )
                    {
                        type_facts::record_annotation_type(
                            &mut self.base,
                            &param.id,
                            param_node,
                            "type",
                        );
                        self.symbols.push(param);
                    }
                    self.symbols.extend(locals::extract_function_locals(
                        &mut self.base,
                        node,
                        &function_id,
                        depth,
                    ));
                    self.symbols.push(symbol);
                }
            }

            _ => {}
        }

        // Recursively traverse children
        let next_parent_id = current_symbol.as_ref().map(|s| s.id.clone()).or(parent_id);
        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.traverse_node(child, next_parent_id.clone(), child_depth);
        }
    }

    /// Emit the `Field` row for a nested QML object, named after its `id` when
    /// it declares one and after its type otherwise.
    fn push_object_symbol(
        &mut self,
        node: tree_sitter::Node,
        parent_id: Option<String>,
        value_source_property: Option<String>,
    ) -> Option<Symbol> {
        use crate::base::{SymbolKind, SymbolOptions};

        let object_type = self
            .base
            .get_node_text(&node.child_by_field_name("type_name")?);
        let object_id = relationships::object_id_binding(&self.base, node);
        let signature = match (&object_id, &value_source_property) {
            (Some(object_id), _) => format!("{}: {}", object_id, object_type),
            (None, Some(property)) => format!("{} on {}", object_type, property),
            (None, None) => object_type.clone(),
        };
        let mut metadata = HashMap::new();
        metadata.insert(
            "object_type".to_string(),
            serde_json::Value::String(object_type.clone()),
        );
        metadata.insert(
            "binding_kind".to_string(),
            serde_json::Value::String("object".to_string()),
        );
        if let Some(property) = value_source_property {
            metadata.insert(
                "value_source_property".to_string(),
                serde_json::Value::String(property),
            );
        }
        let options = SymbolOptions {
            parent_id,
            signature: Some(signature),
            visibility: Some(crate::base::Visibility::Private),
            metadata: Some(metadata),
            doc_comment: semantics::extract_qml_doc_comment(self, &node),
            ..Default::default()
        };
        let has_id = object_id.is_some();
        let symbol = self.base.create_symbol(
            &node,
            object_id.unwrap_or_else(|| object_type.clone()),
            SymbolKind::Field,
            options,
        );
        if has_id {
            type_facts::record_named_type(&mut self.base, &symbol.id, &object_type);
        }
        self.symbols.push(symbol.clone());
        Some(symbol)
    }

    pub fn extract_relationships(&mut self, tree: &Tree, symbols: &[Symbol]) -> Vec<Relationship> {
        let rels = relationships::extract_relationships(self, tree, symbols);
        // Extract pending relationships (cross-file calls) and add them to our internal list
        self.extract_pending_relationships(tree, symbols);
        rels
    }

    /// Extract pending relationships from the syntax tree
    /// This handles cross-file function calls that need resolution
    fn extract_pending_relationships(&mut self, tree: &Tree, symbols: &[Symbol]) {
        let symbol_map: std::collections::HashMap<String, &Symbol> =
            crate::base::ScopedSymbolIndex::unique_symbol_map(symbols);
        let class_symbols = ContainingSymbolIndex::from_iter(
            symbols
                .iter()
                .filter(|symbol| symbol.kind == SymbolKind::Class),
        );

        let object_owners = relationships::object_owner_map(symbols);
        self.walk_for_pending_calls(
            tree.root_node(),
            symbols,
            &symbol_map,
            &class_symbols,
            &object_owners,
            0,
        );
    }

    /// Walk the tree for calls that cannot resolve in their lexical component scope
    fn walk_for_pending_calls(
        &mut self,
        node: tree_sitter::Node,
        symbols: &[Symbol],
        symbol_map: &std::collections::HashMap<String, &Symbol>,
        class_symbols: &ContainingSymbolIndex<'_>,
        object_owners: &std::collections::HashMap<u32, &Symbol>,
        depth: u32,
    ) {
        if !should_visit_tree_depth(depth) {
            return;
        }

        // Look for call expressions
        if node.kind() == "call_expression"
            && let Some(function_node) = node.child_by_field_name("function")
        {
            // Extract function name - handle both direct calls and member access
            let function_name = if function_node.kind() == "member_expression" {
                // For obj.method(), get just "method"
                if let Some(property) = function_node.child_by_field_name("property") {
                    self.base.get_node_text(&property)
                } else {
                    self.base.get_node_text(&function_node)
                }
            } else {
                self.base.get_node_text(&function_node)
            };

            if let Some(caller_symbol) =
                relationships::find_containing_function(node, symbols, class_symbols)
            {
                let receiver = function_node
                    .child_by_field_name("object")
                    .map(|object| self.base.get_node_text(&object));
                let resolves_locally = relationships::resolve_local_callee(
                    &relationships::LocalCall {
                        node,
                        function_name: &function_name,
                        receiver: receiver.as_deref(),
                        caller: caller_symbol,
                    },
                    symbols,
                    symbol_map,
                    class_symbols,
                    object_owners,
                )
                .is_some();
                if !resolves_locally {
                    let pending = self
                        .base
                        .create_pending_relationship(
                            caller_symbol.id.clone(),
                            semantics::build_unresolved_target(
                                &self.base,
                                function_node,
                                &function_name,
                            ),
                            crate::base::RelationshipKind::Calls,
                            &node,
                            Some(caller_symbol.id.clone()),
                            Some(0.7),
                        )
                        .with_receiver_type(relationships::call_receiver_type(
                            &self.base,
                            function_node,
                        ));
                    self.add_structured_pending_relationship(pending);
                }
            }
        }

        // Recursively process children
        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.walk_for_pending_calls(
                child,
                symbols,
                symbol_map,
                class_symbols,
                object_owners,
                child_depth,
            );
        }
    }

    /// Get pending relationships that need cross-file resolution
    pub fn get_pending_relationships(&self) -> Vec<PendingRelationship> {
        self.base.get_pending_relationships()
    }

    pub fn get_type_argument_usages(&self) -> Vec<crate::base::TypeArgumentUsage> {
        self.base.get_type_argument_usages()
    }

    /// Clone captured call-argument literals.
    pub fn get_literals(&self) -> Vec<crate::base::Literal> {
        self.base.get_literals()
    }

    /// Add a pending relationship (used during extraction)
    pub fn add_pending_relationship(&mut self, pending: PendingRelationship) {
        self.base.add_pending_relationship(pending);
    }

    pub fn add_structured_pending_relationship(&mut self, pending: StructuredPendingRelationship) {
        self.base.add_structured_pending_relationship(pending);
    }

    pub fn get_structured_pending_relationships(&self) -> Vec<StructuredPendingRelationship> {
        self.base.get_structured_pending_relationships()
    }

    pub fn infer_types(&self, symbols: &[Symbol]) -> HashMap<String, String> {
        semantics::infer_types(symbols)
    }

    pub fn extract_identifiers(&mut self, tree: &Tree, symbols: &[Symbol]) -> Vec<Identifier> {
        identifiers::extract_identifiers(self, tree, symbols)
    }
}
