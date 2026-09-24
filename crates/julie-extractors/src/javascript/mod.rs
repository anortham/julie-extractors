//! JavaScript Extractor for Julie
//!
//! Direct Implementation of JavaScript extractor logic ported to idiomatic Rust
//!
//! This follows the exact extraction strategy using Rust patterns:
//! - Uses node type switch statement logic
//! - Preserves signature building algorithms
//! - Maintains same edge case handling
//! - Converts to Rust `Option<T>`, `Result<T>`, iterators, ownership system

mod assignments;
pub(crate) mod exports;
mod functions;
mod helpers;
// pub(crate): identifiers exports `is_ecmascript_value_read_identifier`, the
// variable_ref rule-1/4 predicate shared with the TypeScript and Vue extractors.
pub(crate) mod identifiers;
mod imports;
pub(crate) mod parameters;
pub(crate) mod qml_directives;
mod relationships;
mod signatures;
// pub(crate): test_symbols carries the JS-family test classifier, shared with
// the TypeScript extractor because ts/tsx run the same test DSL.
pub(crate) mod test_symbols;
pub(crate) mod type_facts;
mod types;
mod variables;
mod visibility;

use crate::base::{
    BaseExtractor, PendingRelationship, Relationship, StructuredPendingRelationship, Symbol,
    SymbolKind, UnresolvedTarget,
};
use crate::ecmascript_imports::{
    ImportSourceKind, import_source_from_symbol, import_source_kind,
    is_ecmascript_global_direct_target,
};
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::{HashMap, HashSet};
use tree_sitter::Tree;

/// The function value a member-shaped declaration (`key: function () {}`,
/// `A.prototype.m = function () {}`, `handler = () => {}` in a class body)
/// binds. Its symbol is the member itself, never a second function.
fn member_function_value(node: tree_sitter::Node) -> Option<tree_sitter::Node> {
    let value = match node.kind() {
        "pair" | "field_definition" | "property_definition" | "public_field_definition" => {
            node.child_by_field_name("value")
        }
        "assignment_expression" => node.child_by_field_name("right"),
        _ => None,
    }?;
    matches!(
        value.kind(),
        "arrow_function" | "function_expression" | "generator_function"
    )
    .then_some(value)
}

/// An object literal whose pairs are named declarations: the value of a
/// variable, an `export default`, an assignment (`module.exports = {...}`), or
/// a class field, directly or through enclosing object literals. Object
/// literals in call arguments, JSX attributes, return values, and decorator
/// arguments are anonymous data.
fn is_declaration_bound_object(object: tree_sitter::Node) -> bool {
    if object.kind() != "object" {
        return false;
    }
    let mut current = object;
    loop {
        let Some(parent) = current.parent() else {
            return false;
        };
        match parent.kind() {
            "parenthesized_expression" => current = parent,
            "pair" => match parent.parent() {
                Some(enclosing) if enclosing.kind() == "object" => current = enclosing,
                _ => return false,
            },
            "variable_declarator" | "export_statement" => return true,
            "assignment_expression" => {
                return parent
                    .child_by_field_name("right")
                    .is_some_and(|right| right.id() == current.id());
            }
            "field_definition" | "public_field_definition" | "property_definition" => {
                return true;
            }
            _ => return false,
        }
    }
}

/// The owner index every ECMAScript pass uses to find the declaration that
/// owns a call or identifier: the innermost function, method, class field,
/// test call, or module-level variable around it. Export and import rows name
/// code without owning it, so they never own a reference.
pub(crate) fn ecmascript_owner_index<'a>(
    base: &BaseExtractor,
    root: tree_sitter::Node<'a>,
    symbols: &'a [Symbol],
) -> EcmaOwnerIndex<'a> {
    EcmaOwnerIndex(crate::base::OwnerIndex::new_filtered(
        base,
        root,
        symbols,
        |symbol| !matches!(symbol.kind, SymbolKind::Export | SymbolKind::Import),
    ))
}

pub(crate) struct EcmaOwnerIndex<'a>(crate::base::OwnerIndex<'a>);

impl<'a> EcmaOwnerIndex<'a> {
    /// The owner of `node`. A decorator written before `export`
    /// (`@Component() export class A {}`) belongs to the declaration it
    /// decorates, as it does without the `export`.
    pub(crate) fn find(&self, node: tree_sitter::Node<'a>) -> Option<&'a Symbol> {
        let ancestors = self.0.ancestors(node);
        let upward = std::iter::once(node).chain(ancestors.iter().rev().copied());
        for (current, parent) in upward.zip(ancestors.iter().rev().copied()) {
            if current.kind() == "decorator" && parent.kind() == "export_statement" {
                if let Some(body) = parent
                    .child_by_field_name("declaration")
                    .and_then(|declaration| declaration.child_by_field_name("body"))
                {
                    return self.0.find(body);
                }
                break;
            }
        }
        self.0.find_with_ancestors(node, &ancestors)
    }
}

/// Callable node kinds that own the calls inside them.
fn is_pending_scope_kind(kind: &str) -> bool {
    matches!(
        kind,
        "function_declaration"
            | "method_definition"
            | "arrow_function"
            | "function_expression"
            | "generator_function"
            | "generator_function_declaration"
    )
}

struct PendingCallContext<'a> {
    symbol_index: crate::base::ScopedSymbolIndex<'a>,
    owners: EcmaOwnerIndex<'a>,
    typed_receivers: HashSet<String>,
    local_bindings: HashSet<String>,
}

pub struct JavaScriptExtractor {
    pub(crate) base: BaseExtractor,
    import_bindings: Option<HashSet<String>>,
    import_binding_sources: Option<HashMap<String, String>>,
    receiver_import_contexts: HashMap<(usize, String), Option<String>>,
    test_dsl_active: bool,
    member_callables: HashSet<usize>,
}

impl JavaScriptExtractor {
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
            member_callables: HashSet::new(),
        }
    }

    /// Access base extractor (needed by relationship module)
    pub(super) fn base(&self) -> &BaseExtractor {
        &self.base
    }

    pub fn extract_symbols(&mut self, tree: &Tree) -> Vec<Symbol> {
        let mut symbols = qml_directives::import_symbols(&self.base);
        self.test_dsl_active = test_symbols::test_dsl_is_active(&self.base, tree.root_node());
        self.visit_node(tree.root_node(), &mut symbols, None, 0);
        type_facts::record_initializer_facts(&mut self.base, tree.root_node(), &symbols);
        visibility::apply_module_visibility(&self.base, tree.root_node(), &mut symbols);
        symbols
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
        let mut symbol_map: HashMap<String, &Symbol> =
            crate::base::ScopedSymbolIndex::unique_symbol_map(symbols);
        for import in symbols
            .iter()
            .filter(|symbol| symbol.kind == SymbolKind::Import)
        {
            symbol_map.entry(import.name.clone()).or_insert(import);
        }
        let context = PendingCallContext {
            symbol_index: crate::base::ScopedSymbolIndex::new(symbols),
            owners: ecmascript_owner_index(&self.base, tree.root_node(), symbols),
            typed_receivers: crate::typescript::typed_receiver_names(symbols, &self.base.type_info),
            local_bindings: symbols
                .iter()
                .filter(|symbol| {
                    matches!(
                        symbol.kind,
                        SymbolKind::Variable | SymbolKind::Function | SymbolKind::Constant
                    )
                })
                .map(|symbol| symbol.name.clone())
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
            && !(self.test_dsl_active && test_symbols::is_test_dsl_call(&self.base, node))
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
            } else if function_node.kind() == "identifier" {
                let function_name = self.base.get_node_text(&function_node);
                let confidence = match symbol_map.get(function_name.as_str()) {
                    Some(called_symbol) if called_symbol.kind == SymbolKind::Import => Some(0.8),
                    None if !context.local_bindings.contains(&function_name) => Some(0.7),
                    _ => None,
                };
                if let Some(confidence) = confidence
                    && let Some(target) =
                        self.build_unresolved_target(node, function_node, symbol_map)
                {
                    let pending = self.base.create_pending_relationship(
                        caller_symbol.id.clone(),
                        target,
                        crate::base::RelationshipKind::Calls,
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
            .and_then(|object| identifiers::ecmascript_self_receiver_type(&self.base, object));
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
                crate::base::RelationshipKind::Calls,
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
        symbol_map: &HashMap<String, &Symbol>,
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
            if is_pending_scope_kind(current_node.kind()) {
                return Some(current_node);
            }
            current = current_node.parent();
        }
        None
    }

    #[cfg(test)]
    fn file_imports_binding(&mut self, node: tree_sitter::Node, binding_name: &str) -> bool {
        self.file_import_bindings(node).contains(binding_name)
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
            .find(|child| child.kind() == "import_clause")
        else {
            return;
        };

        let mut cursor = clause.walk();
        for child in clause.children(&mut cursor) {
            match child.kind() {
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

    /// Infer types from JSDoc comments (@returns, @type)
    /// JSDoc and constructor type facts are recorded during symbol
    /// extraction, so nothing is inferred afterwards.
    pub fn infer_types(&self, _symbols: &[Symbol]) -> std::collections::HashMap<String, String> {
        std::collections::HashMap::new()
    }

    /// The symbol `node` declares, if any. Symbols that never open a scope
    /// (import and export rows, destructured bindings) go straight into
    /// `symbols`. Kept out of the recursive frame so the walker stays small.
    #[inline(never)]
    fn extract_node_symbol(
        &mut self,
        node: tree_sitter::Node,
        symbols: &mut Vec<Symbol>,
        parent_id: Option<&str>,
    ) -> Option<Symbol> {
        let parent = || parent_id.map(str::to_string);
        let mut symbol = match node.kind() {
            "class_declaration" | "class" => self.extract_class(node, parent()),
            "function_declaration"
            | "function"
            | "arrow_function"
            | "function_expression"
            | "generator_function"
            | "generator_function_declaration"
                if !self.member_callables.contains(&node.id()) =>
            {
                self.extract_function(node, parent())
            }
            "method_definition" => self.extract_method(node, parent()),
            "variable_declarator"
                if node.child_by_field_name("name").is_some_and(|name| {
                    matches!(name.kind(), "object_pattern" | "array_pattern")
                }) =>
            {
                symbols.extend(self.extract_destructuring_variables(node, parent()));
                None
            }
            "variable_declarator" => self.extract_variable(node, parent()),
            "import_statement" | "import_declaration" => {
                let import_symbols = self.extract_import_specifiers(&node);
                for specifier in import_symbols {
                    let import_symbol = self.create_import_symbol(node, &specifier, parent());
                    symbols.push(import_symbol);
                }
                None
            }
            "export_statement" | "export_declaration" => {
                symbols.extend(exports::extract_export_rows(
                    &mut self.base,
                    node,
                    parent_id,
                ));
                None
            }
            "call_expression"
                if node
                    .child_by_field_name("function")
                    .is_some_and(|function| function.kind() == "import") =>
            {
                exports::dynamic_import_row(&mut self.base, node, parent_id)
            }
            "property_definition" | "public_field_definition" | "field_definition" => {
                self.extract_property(node, parent())
            }
            "pair"
                if member_function_value(node).is_some()
                    || node.parent().is_some_and(is_declaration_bound_object) =>
            {
                self.extract_property(node, parent())
            }
            "assignment_expression" => self.extract_assignment(node, parent(), symbols),
            "call_expression"
                if self.test_dsl_active && test_symbols::is_test_dsl_call(&self.base, node) =>
            {
                let container = symbols
                    .iter()
                    .rev()
                    .find(|s| {
                        s.metadata
                            .as_ref()
                            .and_then(|m| m.get("test_container"))
                            .and_then(|v| v.as_bool())
                            == Some(true)
                            && s.start_byte <= node.start_byte() as u32
                            && s.end_byte >= node.end_byte() as u32
                    })
                    .map(|s| s.id.as_str());
                test_symbols::extract_test_call(&mut self.base, node, container)
            }
            _ => None,
        };
        if helpers::is_later_declarator(node)
            && let Some(symbol) = symbol.as_mut()
        {
            symbol.doc_comment = None;
        }
        symbol
    }

    /// Main tree traversal - ports visitNode function exactly
    fn visit_node(
        &mut self,
        node: tree_sitter::Node,
        symbols: &mut Vec<Symbol>,
        parent_id: Option<String>,
        depth: u32,
    ) {
        if !should_visit_tree_depth(depth) {
            return;
        }

        let symbol = self.extract_node_symbol(node, symbols, parent_id.as_deref());

        let current_parent_id = if let Some(sym) = &symbol {
            type_facts::record_jsdoc_symbol_fact(&mut self.base, sym);
            symbols.push(sym.clone());
            let callable_node = if parameters::is_parameter_owner(node.kind()) {
                Some(node)
            } else {
                member_function_value(node)
                    .filter(|_| matches!(sym.kind, SymbolKind::Method | SymbolKind::Function))
            };
            if let Some(callable_node) = callable_node
                && matches!(
                    sym.kind,
                    SymbolKind::Function | SymbolKind::Method | SymbolKind::Constructor
                )
            {
                self.member_callables.insert(callable_node.id());
                for (param_symbol, _) in
                    parameters::extract_parameter_symbols(&mut self.base, callable_node, &sym.id)
                {
                    type_facts::record_jsdoc_param_fact(
                        &mut self.base,
                        &param_symbol,
                        sym.doc_comment.as_deref(),
                    );
                    symbols.push(param_symbol);
                }
            }
            Some(sym.id.clone())
        } else {
            parent_id
        };

        // Recursively visit children (pattern)
        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_node(child, symbols, current_parent_id.clone(), child_depth);
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use tree_sitter::Parser;

    #[test]
    fn file_import_binding_matches_binding_names_not_substrings() {
        let source = r#"
            import { helper as renamed } from "./deps";

            function run() {
                help();
                renamed();
            }
        "#;
        let tree = parse_javascript(source);
        let mut extractor = JavaScriptExtractor::new(
            "javascript".to_string(),
            "src/app.js".to_string(),
            source.to_string(),
            Path::new("src"),
        );

        assert!(!extractor.file_imports_binding(tree.root_node(), "help"));
        assert!(extractor.file_imports_binding(tree.root_node(), "renamed"));
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
        let tree = parse_javascript(source);
        let mut extractor = JavaScriptExtractor::new(
            "javascript".to_string(),
            "src/app.js".to_string(),
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
        let tree = parse_javascript(source);
        let mut extractor = JavaScriptExtractor::new(
            "javascript".to_string(),
            "src/app.js".to_string(),
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
        let tree = parse_javascript(source);
        let mut extractor = JavaScriptExtractor::new(
            "javascript".to_string(),
            "src/app.js".to_string(),
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

            function run(value) {
                expect(value);
                path.join("a", "b");
                helper(value);
                projectTools.doWork(value);
            }
        "#;
        let tree = parse_javascript(source);
        let mut extractor = JavaScriptExtractor::new(
            "javascript".to_string(),
            "src/app.js".to_string(),
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
            function run(value, result) {
                value.trim();
                expect(result).toBe(1);
            }
        "#;
        let tree = parse_javascript(source);
        let mut extractor = JavaScriptExtractor::new(
            "javascript".to_string(),
            "src/app.js".to_string(),
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
            function run(input) {
                Error("bad");
                String(input);
                Boolean(input);
                Number(input);
                setTimeout(() => {}, 1);
                clearTimeout(1);
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
        let tree = parse_javascript(source);
        let mut extractor = JavaScriptExtractor::new(
            "javascript".to_string(),
            "src/app.js".to_string(),
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
        let tree = parse_javascript(source);
        let mut extractor = JavaScriptExtractor::new(
            "javascript".to_string(),
            "src/app.js".to_string(),
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
                && relationship.kind == crate::base::RelationshipKind::Calls
        }));
        assert!(relationships.iter().any(|relationship| {
            relationship.to_symbol_id == error_symbol.id
                && relationship.kind == crate::base::RelationshipKind::Instantiates
        }));
        assert!(
            extractor
                .get_structured_pending_relationships()
                .iter()
                .all(|pending| !matches!(pending.target.terminal_name.as_str(), "fetch" | "Error")),
            "same-file globals should resolve locally, not disappear as pending"
        );
    }

    fn parse_javascript(source: &str) -> tree_sitter::Tree {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_javascript::LANGUAGE.into())
            .expect("failed to set JavaScript language");
        parser
            .parse(source, None)
            .expect("failed to parse JavaScript")
    }
}
