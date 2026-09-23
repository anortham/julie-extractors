//! TypeScript/JavaScript symbol extractor with modular architecture
//!
//! This module provides comprehensive symbol extraction for TypeScript, JavaScript, and TSX/JSX files.
//! The architecture is organized into specialized modules for clarity and maintainability:
//!
//! - **symbols**: Core symbol extraction logic for classes, functions, interfaces, etc.
//! - **functions**: Function and method extraction with signature building
//! - **classes**: Class extraction with inheritance and modifiers
//! - **interfaces**: Interface and type alias extraction
//! - **imports_exports**: Import/export statement extraction
//! - **relationships**: Function call and inheritance relationship tracking
//! - **inference**: Type inference from assignments and return statements
//! - **identifiers**: Identifier usage extraction (calls, member access, etc.)
//! - **helpers**: Utility functions for tree traversal and text extraction

mod classes;
mod functions;
mod helpers;
mod identifiers;
mod imports_exports;
pub mod inference;
mod interfaces;
pub(crate) mod relationships;
mod symbols;
mod type_facts;

use crate::base::{
    BaseExtractor, Identifier, PendingRelationship, Relationship, RelationshipKind,
    StructuredPendingRelationship, Symbol, SymbolKind, UnresolvedTarget,
};
use crate::ecmascript_imports::{
    ImportSourceKind, import_source_from_symbol, import_source_kind,
    is_ecmascript_global_direct_target,
};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::{HashMap, HashSet};
use tree_sitter::Tree;

struct PendingCallContext<'a> {
    symbol_index: crate::base::ScopedSymbolIndex<'a>,
    owners: crate::javascript::EcmaOwnerIndex<'a>,
    typed_receivers: HashSet<String>,
    local_callables: HashSet<&'a str>,
}

/// Names of the file's bindings (variables, parameters, properties) that carry
/// a type fact, so a member call on them can be bound by the consumer.
pub(crate) fn typed_receiver_names(
    symbols: &[Symbol],
    type_info: &HashMap<String, crate::base::TypeInfo>,
) -> HashSet<String> {
    let external_imports: HashSet<&str> = symbols
        .iter()
        .filter(|symbol| {
            symbol.kind == SymbolKind::Import
                && import_source_from_symbol(symbol)
                    .is_some_and(|source| import_source_kind(source) == ImportSourceKind::External)
        })
        .map(|symbol| symbol.name.as_str())
        .collect();
    symbols
        .iter()
        .filter(|symbol| {
            matches!(
                symbol.kind,
                SymbolKind::Variable
                    | SymbolKind::Property
                    | SymbolKind::Field
                    | SymbolKind::Constant
            ) && type_info.get(&symbol.id).is_some_and(|info| {
                names_project_type(&info.resolved_type)
                    && !external_imports
                        .contains(info.resolved_type.split('.').next().unwrap_or_default())
            })
        })
        .map(|symbol| symbol.name.clone())
        .collect()
}

/// Built-in value types and arrays never bind to a project type, so member
/// calls on them stay local noise.
fn names_project_type(resolved_type: &str) -> bool {
    !resolved_type.ends_with("[]")
        && !matches!(
            resolved_type,
            "string"
                | "number"
                | "boolean"
                | "bigint"
                | "symbol"
                | "object"
                | "any"
                | "unknown"
                | "never"
                | "void"
                | "undefined"
                | "null"
                | "String"
                | "Number"
                | "Boolean"
                | "Object"
                | "Array"
                | "Promise"
                | "Map"
                | "Set"
                | "Date"
                | "RegExp"
                | "Error"
        )
}

/// Main TypeScript extractor that orchestrates modular extraction components
pub struct TypeScriptExtractor {
    pub(crate) base: BaseExtractor,
    import_bindings: Option<HashSet<String>>,
    import_binding_sources: Option<HashMap<String, String>>,
    receiver_import_contexts: HashMap<(usize, String), Option<String>>,
    pub(super) test_dsl_active: bool,
}

impl TypeScriptExtractor {
    /// Create a new TypeScript extractor
    ///
    /// # Phase 2: Relative Unix-Style Path Storage
    /// Now accepts workspace_root to enable relative path storage
    pub fn new(
        language: String,
        file_path: String,
        content: String,
        workspace_root: &std::path::Path,
    ) -> Self {
        Self {
            base: BaseExtractor::new(language, file_path, content, workspace_root),
            import_bindings: None,
            import_binding_sources: None,
            receiver_import_contexts: HashMap::new(),
            test_dsl_active: false,
        }
    }

    /// Extract all symbols from the syntax tree
    pub fn extract_symbols(&mut self, tree: &Tree) -> Vec<Symbol> {
        symbols::extract_symbols(self, tree)
    }

    /// Extract all relationships (calls, inheritance, etc.)
    pub fn extract_relationships(&mut self, tree: &Tree, symbols: &[Symbol]) -> Vec<Relationship> {
        let rels = relationships::extract_relationships(self, tree, symbols);
        // Extract pending relationships (cross-file calls) and add them to our internal list
        self.extract_pending_relationships(tree, symbols);
        rels
    }

    /// Extract pending relationships from the syntax tree
    /// This handles cross-file function calls that need resolution
    fn extract_pending_relationships(&mut self, tree: &Tree, symbols: &[Symbol]) {
        let symbol_map: HashMap<String, &Symbol> =
            crate::base::ScopedSymbolIndex::unique_symbol_map(symbols);
        let context = PendingCallContext {
            symbol_index: crate::base::ScopedSymbolIndex::new(symbols),
            owners: crate::javascript::ecmascript_owner_index(&self.base, symbols),
            typed_receivers: typed_receiver_names(symbols, &self.base.type_info),
            local_callables: symbols
                .iter()
                .filter(|symbol| {
                    matches!(
                        symbol.kind,
                        SymbolKind::Function | SymbolKind::Variable | SymbolKind::Class
                    ) && !crate::base::is_test_call_symbol(symbol)
                })
                .map(|symbol| symbol.name.as_str())
                .collect(),
        };

        self.walk_for_pending_calls(tree.root_node(), &context, &symbol_map, 0);
    }

    /// Walk the tree looking for calls that need cross-file resolution. A
    /// call's caller is the declaration that owns it, the same owner the
    /// identifier and relationship passes use.
    fn walk_for_pending_calls<'a>(
        &mut self,
        node: tree_sitter::Node,
        context: &PendingCallContext<'a>,
        symbol_map: &HashMap<String, &'a Symbol>,
        depth: u32,
    ) {
        if !should_visit_tree_depth(depth) {
            return;
        }

        if let Some(function_node) = relationships::call_site_callee(self, node)
            && matches!(function_node.kind(), "identifier" | "member_expression")
            && !(self.test_dsl_active
                && crate::javascript::test_symbols::is_test_dsl_call(&self.base, node))
            && let Some(caller_symbol) = context.owners.find(node)
        {
            if function_node.kind() == "member_expression" {
                self.emit_pending_member_call(
                    node,
                    function_node,
                    caller_symbol,
                    context,
                    symbol_map,
                );
            } else {
                let confidence = match symbol_map
                    .get(self.base.get_node_text(&function_node).as_str())
                {
                    Some(called_symbol) if called_symbol.kind == SymbolKind::Import => Some(0.8),
                    None if !context
                        .local_callables
                        .contains(self.base.get_node_text(&function_node).as_str()) =>
                    {
                        Some(0.7)
                    }
                    _ => None,
                };
                if let Some(confidence) = confidence
                    && let Some(target) =
                        self.build_unresolved_target(node, function_node, symbol_map)
                {
                    let pending = self.base.create_pending_relationship(
                        caller_symbol.id.clone(),
                        target,
                        RelationshipKind::Calls,
                        &node,
                        Some(caller_symbol.id.clone()),
                        Some(confidence),
                    );
                    self.add_structured_pending_relationship(pending);
                }
            }
        }

        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        for index in 0..node.named_child_count() {
            if let Some(child) = node.named_child(index as u32) {
                self.walk_for_pending_calls(child, context, symbol_map, child_depth);
            }
        }
    }

    /// A member call is pending when its receiver names an import, a binding
    /// with a type fact, or `this`/`super` with a known class and no
    /// same-class target. The terminal name alone never makes it local.
    fn emit_pending_member_call(
        &mut self,
        call_node: tree_sitter::Node,
        function_node: tree_sitter::Node,
        caller_symbol: &Symbol,
        context: &PendingCallContext<'_>,
        symbol_map: &HashMap<String, &Symbol>,
    ) {
        let Some(target) = self.build_unresolved_target(call_node, function_node, symbol_map)
        else {
            return;
        };
        let Some(receiver) = target.receiver.as_deref() else {
            return;
        };
        let receiver_type = function_node
            .child_by_field_name("object")
            .and_then(|object| {
                crate::javascript::identifiers::ecmascript_self_receiver_type(&self.base, object)
            });
        let emit = if matches!(receiver, "this" | "super") {
            receiver_type.is_some()
                && !matches!(
                    context.symbol_index.resolve_call_target(
                        &target.terminal_name,
                        Some(caller_symbol),
                        Some(receiver),
                    ),
                    crate::base::LocalTargetResolution::Resolved(_)
                )
        } else {
            target.import_context.is_some() || context.typed_receivers.contains(receiver)
        };
        if !emit {
            return;
        }
        let pending = self
            .base
            .create_pending_relationship(
                caller_symbol.id.clone(),
                target,
                RelationshipKind::Calls,
                &call_node,
                Some(caller_symbol.id.clone()),
                Some(0.7),
            )
            .with_receiver_type(receiver_type);
        self.add_structured_pending_relationship(pending);
    }

    fn call_terminal_name(&self, function_node: tree_sitter::Node) -> String {
        if function_node.kind() == "member_expression" {
            return function_node
                .child_by_field_name("property")
                .map(|node| self.base.get_node_text(&node))
                .unwrap_or_else(|| self.base.get_node_text(&function_node));
        }

        self.base.get_node_text(&function_node)
    }

    fn build_unresolved_target(
        &mut self,
        call_node: tree_sitter::Node,
        function_node: tree_sitter::Node,
        symbol_map: &std::collections::HashMap<String, &Symbol>,
    ) -> Option<UnresolvedTarget> {
        if function_node.kind() == "member_expression" {
            if let Some((parts, root_node)) = relationships::member_chain(self, function_node) {
                let mut target = UnresolvedTarget::from_chain(parts);
                target.import_context =
                    self.member_receiver_import_context(call_node, root_node, symbol_map);
                return Some(target);
            }
            let receiver_node = function_node.child_by_field_name("object");
            let receiver = receiver_node.map(|node| self.base.get_node_text(&node));
            let property = function_node
                .child_by_field_name("property")
                .map(|node| self.base.get_node_text(&node))
                .unwrap_or_else(|| self.base.get_node_text(&function_node));
            let display_name = receiver
                .as_ref()
                .map(|receiver| format!("{receiver}.{property}"))
                .unwrap_or_else(|| property.clone());
            let import_context = receiver_node.and_then(|receiver_node| {
                self.member_receiver_import_context(call_node, receiver_node, symbol_map)
            });

            return Some(UnresolvedTarget {
                display_name,
                terminal_name: property,
                receiver,
                namespace_path: Vec::new(),
                import_context,
            });
        }

        let function_name = self.base.get_node_text(&function_node);
        let source_kind = self.import_binding_source_kind(call_node, &function_name, symbol_map);
        if matches!(source_kind, Some(ImportSourceKind::External)) {
            return None;
        }
        if source_kind.is_none() && is_ecmascript_global_direct_target(&function_name) {
            return None;
        }

        let import_context = matches!(source_kind, Some(ImportSourceKind::ProjectRelative))
            .then_some(function_name.clone());
        Some(UnresolvedTarget {
            display_name: function_name.clone(),
            terminal_name: function_name,
            receiver: None,
            namespace_path: Vec::new(),
            import_context,
        })
    }

    fn member_receiver_import_context(
        &mut self,
        call_node: tree_sitter::Node,
        receiver_node: tree_sitter::Node,
        symbol_map: &HashMap<String, &Symbol>,
    ) -> Option<String> {
        if receiver_node.kind() != "identifier" {
            return None;
        }

        let receiver_name = self.base.get_node_text(&receiver_node);
        self.imported_binding_context(call_node, &receiver_name, symbol_map)
            .or_else(|| self.find_receiver_import_context(call_node, &receiver_name, symbol_map))
    }

    fn imported_binding_context(
        &mut self,
        node: tree_sitter::Node,
        binding_name: &str,
        symbol_map: &HashMap<String, &Symbol>,
    ) -> Option<String> {
        matches!(
            self.import_binding_source_kind(node, binding_name, symbol_map),
            Some(ImportSourceKind::ProjectRelative)
        )
        .then_some(binding_name.to_string())
    }

    fn import_binding_source_kind(
        &mut self,
        node: tree_sitter::Node,
        binding_name: &str,
        symbol_map: &HashMap<String, &Symbol>,
    ) -> Option<ImportSourceKind> {
        if let Some(source_kind) = symbol_map
            .get(binding_name)
            .filter(|symbol| symbol.kind == SymbolKind::Import)
            .and_then(|symbol| import_source_from_symbol(symbol).map(import_source_kind))
        {
            return Some(source_kind);
        }

        self.file_import_binding_source(node, binding_name)
            .map(|source| import_source_kind(&source))
    }

    fn find_receiver_import_context(
        &mut self,
        call_node: tree_sitter::Node,
        receiver_name: &str,
        symbol_map: &HashMap<String, &Symbol>,
    ) -> Option<String> {
        let caller_scope = self.find_containing_scope_node(call_node)?;
        let cache_key = (caller_scope.start_byte(), receiver_name.to_string());
        if let Some(import_context) = self.receiver_import_contexts.get(&cache_key) {
            return import_context.clone();
        }

        let import_context =
            self.resolve_receiver_import_context(caller_scope, receiver_name, symbol_map);
        self.receiver_import_contexts
            .insert(cache_key, import_context.clone());
        import_context
    }

    fn resolve_receiver_import_context(
        &mut self,
        caller_scope: tree_sitter::Node,
        receiver_name: &str,
        symbol_map: &HashMap<String, &Symbol>,
    ) -> Option<String> {
        let mut stack = vec![caller_scope];
        while let Some(candidate) = stack.pop() {
            let mut cursor = candidate.walk();
            for child in candidate.children(&mut cursor) {
                stack.push(child);
            }

            if candidate.kind() != "variable_declarator" {
                continue;
            }

            let Some(name_node) = candidate.child_by_field_name("name") else {
                continue;
            };
            if self.base.get_node_text(&name_node) != receiver_name {
                continue;
            }

            let Some(value_node) = candidate.child_by_field_name("value") else {
                continue;
            };
            if value_node.kind() != "new_expression" {
                continue;
            }

            let constructor_node = value_node
                .child_by_field_name("constructor")
                .or_else(|| value_node.child_by_field_name("callee"))
                .or_else(|| {
                    let mut cursor = value_node.walk();
                    value_node
                        .named_children(&mut cursor)
                        .find(|child| !matches!(child.kind(), "arguments" | "type_arguments"))
                });
            let Some(constructor_node) = constructor_node else {
                continue;
            };
            let constructor_name = self.base.get_node_text(&constructor_node);
            if self
                .imported_binding_context(caller_scope, &constructor_name, symbol_map)
                .is_some()
            {
                return Some(constructor_name);
            }
        }

        None
    }

    fn find_containing_scope_node<'a>(
        &self,
        node: tree_sitter::Node<'a>,
    ) -> Option<tree_sitter::Node<'a>> {
        let mut current = node.parent();
        while let Some(current_node) = current {
            if matches!(
                current_node.kind(),
                "function_declaration" | "method_definition" | "arrow_function"
            ) {
                return Some(current_node);
            }
            current = current_node.parent();
        }
        None
    }

    #[cfg(test)]
    fn file_import_bindings(&mut self, node: tree_sitter::Node) -> &HashSet<String> {
        self.ensure_import_binding_cache(node);
        self.import_bindings
            .as_ref()
            .expect("import binding cache is initialized")
    }

    fn file_import_binding_source(
        &mut self,
        node: tree_sitter::Node,
        binding_name: &str,
    ) -> Option<String> {
        self.ensure_import_binding_cache(node);
        self.import_binding_sources
            .as_ref()
            .and_then(|sources| sources.get(binding_name))
            .cloned()
    }

    fn ensure_import_binding_cache(&mut self, node: tree_sitter::Node) {
        if self.import_bindings.is_some() && self.import_binding_sources.is_some() {
            return;
        }

        let mut current = Some(node);
        let mut root = node;
        while let Some(candidate) = current {
            root = candidate;
            current = candidate.parent();
        }

        let mut bindings = HashSet::new();
        let mut binding_sources = HashMap::new();
        let mut stack = vec![root];
        while let Some(candidate) = stack.pop() {
            let mut cursor = candidate.walk();
            for child in candidate.children(&mut cursor) {
                stack.push(child);
            }

            if !matches!(candidate.kind(), "import_statement" | "import_declaration") {
                continue;
            }

            let source = self.import_source_for_import_node(candidate);
            self.collect_import_bindings(
                candidate,
                source.as_deref().unwrap_or_default(),
                &mut bindings,
                &mut binding_sources,
            );
        }

        self.import_bindings = Some(bindings);
        self.import_binding_sources = Some(binding_sources);
    }

    fn import_source_for_import_node(&self, import_node: tree_sitter::Node) -> Option<String> {
        import_node
            .child_by_field_name("source")
            .map(|source| {
                self.base
                    .get_node_text(&source)
                    .trim_matches(|c| c == '"' || c == '\'' || c == '`')
                    .to_string()
            })
            .filter(|source| !source.is_empty())
    }

    fn collect_import_bindings(
        &self,
        import_node: tree_sitter::Node,
        source: &str,
        bindings: &mut HashSet<String>,
        binding_sources: &mut HashMap<String, String>,
    ) {
        let Some(clause) = import_node
            .children(&mut import_node.walk())
            .find(|child| matches!(child.kind(), "import_clause" | "import_require_clause"))
        else {
            return;
        };

        let mut cursor = clause.walk();
        for child in clause.children(&mut cursor) {
            match child.kind() {
                "identifier" if clause.kind() == "import_require_clause" => {
                    let source = clause
                        .child_by_field_name("source")
                        .map(|source| {
                            self.base
                                .get_node_text(&source)
                                .trim_matches(|c| c == '"' || c == '\'' || c == '`')
                                .to_string()
                        })
                        .unwrap_or_default();
                    self.record_import_binding(
                        self.base.get_node_text(&child),
                        &source,
                        bindings,
                        binding_sources,
                    );
                }
                "identifier" => {
                    self.record_import_binding(
                        self.base.get_node_text(&child),
                        source,
                        bindings,
                        binding_sources,
                    );
                }
                "named_imports" => {
                    self.collect_named_import_bindings(child, source, bindings, binding_sources)
                }
                "namespace_import" => {
                    if let Some(local_node) = child
                        .children(&mut child.walk())
                        .find(|candidate| candidate.kind() == "identifier")
                    {
                        self.record_import_binding(
                            self.base.get_node_text(&local_node),
                            source,
                            bindings,
                            binding_sources,
                        );
                    }
                }
                _ => {}
            }
        }
    }

    fn collect_named_import_bindings(
        &self,
        named_imports: tree_sitter::Node,
        source: &str,
        bindings: &mut HashSet<String>,
        binding_sources: &mut HashMap<String, String>,
    ) {
        let mut cursor = named_imports.walk();
        for specifier in named_imports.children(&mut cursor) {
            if specifier.kind() != "import_specifier" {
                continue;
            }

            let Some(local_node) = specifier
                .child_by_field_name("alias")
                .or_else(|| specifier.child_by_field_name("name"))
            else {
                continue;
            };
            self.record_import_binding(
                self.base.get_node_text(&local_node),
                source,
                bindings,
                binding_sources,
            );
        }
    }

    fn record_import_binding(
        &self,
        binding_name: String,
        source: &str,
        bindings: &mut HashSet<String>,
        binding_sources: &mut HashMap<String, String>,
    ) {
        bindings.insert(binding_name.clone());
        if !source.is_empty() {
            binding_sources.insert(binding_name, source.to_string());
        }
    }

    /// Extract all identifiers (function calls, member access, etc.)
    pub fn extract_identifiers(&mut self, tree: &Tree, symbols: &[Symbol]) -> Vec<Identifier> {
        identifiers::extract_identifiers(self, tree, symbols)
    }

    /// Infer types from variable assignments and function returns
    pub fn infer_types(&self, symbols: &[Symbol]) -> HashMap<String, String> {
        inference::infer_types(self, symbols)
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

    pub fn get_structured_pending_relationships(&self) -> Vec<StructuredPendingRelationship> {
        self.base.get_structured_pending_relationships()
    }

    /// Add a pending relationship (used during extraction)
    pub fn add_pending_relationship(&mut self, pending: PendingRelationship) {
        self.base.add_pending_relationship(pending);
    }

    pub fn add_structured_pending_relationship(&mut self, pending: StructuredPendingRelationship) {
        self.base.add_structured_pending_relationship(pending);
    }

    // ========================================================================
    // Public access to base for sub-modules (pub(super) scoped internal access)
    // ========================================================================

    /// Get mutable reference to base extractor (for sub-modules)
    pub(crate) fn base_mut(&mut self) -> &mut BaseExtractor {
        &mut self.base
    }

    /// Get immutable reference to base extractor (for sub-modules)
    pub(crate) fn base(&self) -> &BaseExtractor {
        &self.base
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use tree_sitter::Parser;

    #[test]
    fn file_import_bindings_are_cached_from_one_tree_walk() {
        let source = r#"
            import defaultThing, { helper as renamed, other } from "./deps";
            import * as namespaceThing from "./namespace";

            function run() {
                renamed();
                namespaceThing.call();
            }
        "#;
        let tree = parse_typescript(source);
        let mut extractor = TypeScriptExtractor::new(
            "typescript".to_string(),
            "src/app.ts".to_string(),
            source.to_string(),
            Path::new("src"),
        );

        let bindings = extractor.file_import_bindings(tree.root_node());
        let first_bindings = bindings as *const HashSet<String>;

        assert!(bindings.contains("defaultThing"));
        assert!(bindings.contains("renamed"));
        assert!(bindings.contains("other"));
        assert!(bindings.contains("namespaceThing"));
        let second_bindings =
            extractor.file_import_bindings(tree.root_node()) as *const HashSet<String>;
        assert_eq!(first_bindings, second_bindings);
    }

    #[test]
    fn local_calls_do_not_build_import_binding_cache() {
        let source = r#"
            function helper() {
                return 1;
            }

            function run() {
                helper();
                helper();
            }
        "#;
        let tree = parse_typescript(source);
        let mut extractor = TypeScriptExtractor::new(
            "typescript".to_string(),
            "src/app.ts".to_string(),
            source.to_string(),
            Path::new("src"),
        );

        let symbols = extractor.extract_symbols(&tree);
        extractor.extract_relationships(&tree, &symbols);

        assert!(
            extractor.import_bindings.is_none(),
            "local calls should not pay file-level import binding lookup cost"
        );
        assert!(
            !extractor
                .get_structured_pending_relationships()
                .iter()
                .any(|pending| pending.target.terminal_name == "helper"),
            "known local functions should not emit pending relationships"
        );
    }

    #[test]
    fn repeated_receiver_calls_cache_import_context_by_scope() {
        let source = r#"
            import { Service } from "./service";

            function run() {
                const service = new Service();
                service.one();
                service.two();
                service.three();
            }
        "#;
        let tree = parse_typescript(source);
        let mut extractor = TypeScriptExtractor::new(
            "typescript".to_string(),
            "src/app.ts".to_string(),
            source.to_string(),
            Path::new("src"),
        );

        let symbols = extractor.extract_symbols(&tree);
        extractor.extract_relationships(&tree, &symbols);

        assert_eq!(
            extractor.receiver_import_contexts.len(),
            1,
            "same receiver in the same function scope should be resolved once"
        );
        assert_eq!(
            extractor
                .receiver_import_contexts
                .values()
                .next()
                .and_then(|context| context.as_deref()),
            Some("Service")
        );
        let service_pending = extractor
            .get_structured_pending_relationships()
            .into_iter()
            .filter(|pending| pending.target.receiver.as_deref() == Some("service"))
            .collect::<Vec<_>>();
        assert_eq!(service_pending.len(), 3);
        assert!(
            service_pending
                .iter()
                .all(|pending| { pending.target.import_context.as_deref() == Some("Service") })
        );
    }

    #[test]
    fn project_relative_imported_receiver_member_calls_emit_pending_relationships() {
        let source = r#"
            import * as path from "./path";

            function run() {
                path.join("a", "b");
            }
        "#;
        let tree = parse_typescript(source);
        let mut extractor = TypeScriptExtractor::new(
            "typescript".to_string(),
            "src/app.ts".to_string(),
            source.to_string(),
            Path::new("src"),
        );

        let symbols = extractor.extract_symbols(&tree);
        extractor.extract_relationships(&tree, &symbols);

        let pending = extractor.get_structured_pending_relationships();
        let path_join = pending
            .iter()
            .find(|pending| {
                pending.target.receiver.as_deref() == Some("path")
                    && pending.target.terminal_name == "join"
            })
            .expect(
                "project-relative imported receiver member call should emit a pending relationship",
            );
        assert_eq!(path_join.target.import_context.as_deref(), Some("path"));
    }

    #[test]
    fn external_imported_calls_do_not_emit_pending_relationships() {
        let source = r#"
            import { expect } from "vitest";
            import * as path from "node:path";
            import { helper } from "./helper";
            import * as projectTools from "../tools";

            function run(value: unknown) {
                expect(value);
                path.join("a", "b");
                helper(value);
                projectTools.doWork(value);
            }
        "#;
        let tree = parse_typescript(source);
        let mut extractor = TypeScriptExtractor::new(
            "typescript".to_string(),
            "src/app.ts".to_string(),
            source.to_string(),
            Path::new("src"),
        );

        let symbols = extractor.extract_symbols(&tree);
        extractor.extract_relationships(&tree, &symbols);

        let pending = extractor.get_structured_pending_relationships();
        assert!(
            !pending
                .iter()
                .any(|pending| pending.target.terminal_name == "expect"),
            "package imported direct calls should not be cross-file pending calls: {pending:#?}"
        );
        assert!(
            !pending.iter().any(|pending| {
                pending.target.receiver.as_deref() == Some("path")
                    && pending.target.terminal_name == "join"
            }),
            "package imported receiver calls should not be cross-file pending calls: {pending:#?}"
        );

        let helper = pending
            .iter()
            .find(|pending| pending.target.terminal_name == "helper")
            .expect("project-relative direct import calls should still emit pending relationships");
        assert_eq!(helper.target.import_context.as_deref(), Some("helper"));

        let project_tools = pending
            .iter()
            .find(|pending| {
                pending.target.receiver.as_deref() == Some("projectTools")
                    && pending.target.terminal_name == "doWork"
            })
            .expect(
                "project-relative namespace import calls should still emit pending relationships",
            );
        assert_eq!(
            project_tools.target.import_context.as_deref(),
            Some("projectTools")
        );
    }

    #[test]
    fn unimported_member_calls_do_not_emit_pending_relationships() {
        let source = r#"
            function run(value: string, result: number) {
                value.trim();
                expect(result).toBe(1);
            }
        "#;
        let tree = parse_typescript(source);
        let mut extractor = TypeScriptExtractor::new(
            "typescript".to_string(),
            "src/app.ts".to_string(),
            source.to_string(),
            Path::new("src"),
        );

        let symbols = extractor.extract_symbols(&tree);
        extractor.extract_relationships(&tree, &symbols);

        let pending = extractor.get_structured_pending_relationships();
        assert!(
            !pending
                .iter()
                .any(|pending| pending.target.terminal_name == "trim"),
            "unimported local value methods should not be cross-file pending calls"
        );
        assert!(
            !pending
                .iter()
                .any(|pending| pending.target.terminal_name == "toBe"),
            "matcher chains without an imported receiver should not be cross-file pending calls"
        );
    }

    #[test]
    fn ecmascript_globals_do_not_emit_pending_relationships() {
        let source = r#"
            function run(input: unknown) {
                Error("bad");
                String(input);
                Boolean(input);
                Number(input);
                setTimeout(() => {}, 1);
                clearTimeout(1 as unknown as number);
                fetch("/health");
                import("./lazy");
                new Error("bad");
                new Promise(resolve => resolve(1));
                new Set([1]);
                new Map();
                new Date();
                new URL("https://example.com");
                new AbortController();
                new Uint8Array();
                projectGlobal();
            }
        "#;
        let tree = parse_typescript(source);
        let mut extractor = TypeScriptExtractor::new(
            "typescript".to_string(),
            "src/app.ts".to_string(),
            source.to_string(),
            Path::new("src"),
        );

        let symbols = extractor.extract_symbols(&tree);
        extractor.extract_relationships(&tree, &symbols);

        let pending = extractor.get_structured_pending_relationships();
        for target_name in [
            "Error",
            "String",
            "Boolean",
            "Number",
            "setTimeout",
            "clearTimeout",
            "fetch",
            "import",
            "Promise",
            "Set",
            "Map",
            "Date",
            "URL",
            "AbortController",
            "Uint8Array",
        ] {
            assert!(
                !pending
                    .iter()
                    .any(|pending| pending.target.terminal_name == target_name),
                "ECMAScript global {target_name} should not emit pending relationships: {pending:#?}"
            );
        }
        assert!(
            pending
                .iter()
                .any(|pending| pending.target.terminal_name == "projectGlobal"),
            "unknown project-looking direct calls should still emit pending relationships"
        );
    }

    #[test]
    fn local_symbols_named_like_ecmascript_globals_still_resolve() {
        let source = r#"
            class Error {}
            function fetch() {
                return "local";
            }

            function run() {
                fetch();
                new Error();
            }
        "#;
        let tree = parse_typescript(source);
        let mut extractor = TypeScriptExtractor::new(
            "typescript".to_string(),
            "src/app.ts".to_string(),
            source.to_string(),
            Path::new("src"),
        );

        let symbols = extractor.extract_symbols(&tree);
        let relationships = extractor.extract_relationships(&tree, &symbols);
        let fetch_symbol = symbols
            .iter()
            .find(|symbol| symbol.name == "fetch" && symbol.kind == SymbolKind::Function)
            .expect("local fetch function should be extracted");
        let error_symbol = symbols
            .iter()
            .find(|symbol| symbol.name == "Error" && symbol.kind == SymbolKind::Class)
            .expect("local Error class should be extracted");

        assert!(relationships.iter().any(|relationship| {
            relationship.to_symbol_id == fetch_symbol.id
                && relationship.kind == RelationshipKind::Calls
        }));
        assert!(relationships.iter().any(|relationship| {
            relationship.to_symbol_id == error_symbol.id
                && relationship.kind == RelationshipKind::Instantiates
        }));
        assert!(
            extractor
                .get_structured_pending_relationships()
                .iter()
                .all(|pending| !matches!(pending.target.terminal_name.as_str(), "fetch" | "Error")),
            "same-file globals should resolve locally, not disappear as pending"
        );
    }

    fn parse_typescript(source: &str) -> tree_sitter::Tree {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
            .expect("failed to set TypeScript language");
        parser
            .parse(source, None)
            .expect("failed to parse TypeScript")
    }
}
