// R Language Extractor Implementation
// R is a statistical computing and graphics language
// Tree-sitter-r parser provides AST nodes for R syntax

mod identifiers;
mod idioms;
mod non_s3;
mod parameters;
pub(crate) mod plumber;
mod relationships;
pub(crate) mod test_calls;
mod text_args;
mod type_facts;

use crate::base::{BaseExtractor, Identifier, PendingRelationship, Relationship, Symbol};
use crate::base::{SymbolKind, SymbolOptions};
use crate::test_detection::apply_callable_test_metadata;
use crate::tree_traversal::{child_tree_depth, should_visit_tree_depth};
use std::collections::HashMap;
use tree_sitter::{Node, Tree};

pub struct RExtractor {
    pub(crate) base: BaseExtractor,
    symbols: Vec<Symbol>,
    same_file_class_names: std::collections::HashSet<String>,
    same_file_generics: std::collections::HashSet<String>,
    /// Function values of class-list members, keyed by node id, mapped to the member symbol.
    value_owners: HashMap<usize, String>,
    /// Class inheritance declarations waiting for relationship resolution.
    pub(crate) extends_requests: Vec<idioms::ExtendsRequest>,
}

impl RExtractor {
    pub fn new(
        language: String,
        file_path: String,
        content: String,
        workspace_root: &std::path::Path,
    ) -> Self {
        Self {
            base: BaseExtractor::new(language, file_path, content, workspace_root),
            symbols: Vec::new(),
            same_file_class_names: std::collections::HashSet::new(),
            same_file_generics: std::collections::HashSet::new(),
            value_owners: HashMap::new(),
            extends_requests: Vec::new(),
        }
    }

    pub fn extract_symbols(&mut self, tree: &Tree) -> Vec<Symbol> {
        let root_node = tree.root_node();
        self.symbols.clear();
        self.same_file_class_names = type_facts::collect_same_file_class_names(self, root_node);
        self.same_file_generics = idioms::collect_same_file_generics(self, root_node);
        self.value_owners.clear();
        self.extends_requests.clear();

        // Build exclusion set once for S3 checking
        let non_s3: std::collections::HashSet<&str> =
            non_s3::NON_S3_DOT_FUNCTIONS.iter().copied().collect();

        self.traverse_node(root_node, None, &non_s3, 0);

        self.symbols.clone()
    }

    /// Recursively traverse the R AST and extract symbols
    fn traverse_node(
        &mut self,
        node: Node,
        parent_id: Option<String>,
        non_s3: &std::collections::HashSet<&str>,
        depth: u32,
    ) {
        if !should_visit_tree_depth(depth) {
            return;
        }

        let current_symbol_id: Option<String> = match node.kind() {
            "binary_operator" => self
                .extract_from_binary_op(node, &parent_id, non_s3)
                .map(|symbol| symbol.id),
            "call" => self
                .extract_from_call(node, &parent_id)
                .map(|symbol| symbol.id),
            "function_definition" => match self.value_owners.remove(&node.id()) {
                Some(owner_id) => {
                    let parameters = parameters::extract_parameter_symbols(self, node, &owner_id);
                    self.symbols.extend(parameters);
                    Some(owner_id)
                }
                None => idioms::extract_plumber_handler(self, node, &parent_id).map(|s| s.id),
            },
            _ => None,
        };

        let next_parent_id = current_symbol_id.or(parent_id);
        let Some(child_depth) = child_tree_depth(depth) else {
            return;
        };
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.traverse_node(child, next_parent_id.clone(), non_s3, child_depth);
        }
    }

    /// Handle binary_operator nodes (assignments)
    fn extract_from_binary_op(
        &mut self,
        node: Node,
        parent_id: &Option<String>,
        non_s3: &std::collections::HashSet<&str>,
    ) -> Option<Symbol> {
        let operator = node.child(1)?;
        let op_text = self.base.get_node_text(&operator);

        match op_text.as_str() {
            // Left-to-right assignment: x <- value, x = value, x <<- value
            "<-" | "=" | "<<-" => {
                if op_text == "<<-" && idioms::inside_function(node) {
                    return None;
                }
                let left = node.child(0)?;
                let right = node.child(2)?;
                if right.kind() == "function_definition"
                    && let Some(symbol) =
                        idioms::extract_s7_method(self, node, left, right, parent_id)
                {
                    return Some(symbol);
                }
                if right.kind() == "call"
                    && idioms::call_name(self, right).as_deref() == Some("setClass")
                {
                    return None;
                }
                if right.kind() == "function_definition"
                    && let Some((receiver, member)) = idioms::member_function_target(self, left)
                {
                    let symbol =
                        self.extract_function_assignment(node, member, right, parent_id, non_s3);
                    return Some(idioms::with_receiver(self, symbol, receiver));
                }
                let name = idioms::assignment_name(self, left)?;

                if let Some(symbol) =
                    idioms::extract_assignment_class_factory(self, node, &name, right, parent_id)
                {
                    Some(symbol)
                } else if right.kind() == "function_definition" {
                    Some(self.extract_function_assignment(node, name, right, parent_id, non_s3))
                } else if idioms::is_container_assignment(self, left, right) {
                    None
                } else {
                    let mut metadata = idioms::member_metadata(self, node, parent_id);
                    let options = SymbolOptions {
                        parent_id: parent_id.clone(),
                        doc_comment: self.base.find_doc_comment(&node),
                        metadata: if metadata.is_empty() {
                            None
                        } else {
                            Some(std::mem::take(&mut metadata))
                        },
                        ..Default::default()
                    };
                    let kind = if idioms::member_metadata(self, node, parent_id)
                        .contains_key("member_visibility")
                    {
                        SymbolKind::Field
                    } else {
                        SymbolKind::Variable
                    };
                    let symbol = self.base.create_symbol(&node, name, kind, options);
                    self.symbols.push(symbol.clone());
                    if symbol.kind == SymbolKind::Variable
                        && let Some(class_name) =
                            type_facts::same_file_constructor_class(self, right)
                    {
                        type_facts::record_constructor_fact(
                            &mut self.base,
                            &symbol.id,
                            &class_name,
                        );
                    }
                    Some(symbol)
                }
            }
            // Right-to-left assignment: value -> x, value ->> x
            "->" | "->>" => {
                let right = node.child(2)?;
                if right.kind() != "identifier" {
                    return None;
                }
                let name = self.base.get_node_text(&right);
                let options = SymbolOptions {
                    parent_id: parent_id.clone(),
                    doc_comment: self.base.find_doc_comment(&node),
                    ..Default::default()
                };
                let symbol = self
                    .base
                    .create_symbol(&node, name, SymbolKind::Variable, options);
                self.symbols.push(symbol.clone());
                Some(symbol)
            }
            _ => None,
        }
    }

    /// Extract a function assignment with proper signature, S3 detection, and UseMethod detection
    fn extract_function_assignment(
        &mut self,
        node: Node,
        name: String,
        func_def: Node,
        parent_id: &Option<String>,
        non_s3: &std::collections::HashSet<&str>,
    ) -> Symbol {
        let signature = self.build_function_signature(&name, func_def);
        let mut metadata: HashMap<String, serde_json::Value> = HashMap::new();

        let doc_comment = self.base.find_doc_comment(&node);
        let s3 = self.classify_s3(&name, doc_comment.as_deref(), non_s3);
        let kind = if s3.is_some() {
            SymbolKind::Method
        } else {
            SymbolKind::Function
        };
        if let Some((generic, class_name)) = s3 {
            metadata.insert("s3_method".to_string(), serde_json::Value::String(generic));
            metadata.insert(
                "s3_class".to_string(),
                serde_json::Value::String(class_name),
            );
        }

        // Check for UseMethod() in body -> mark as S3 generic
        if self.body_contains_usemethod(func_def) {
            metadata.insert("s3_generic".to_string(), serde_json::Value::Bool(true));
        }

        metadata.extend(idioms::member_metadata(self, node, parent_id));

        // Test detection
        apply_callable_test_metadata(
            "r",
            &name,
            &self.base.file_path,
            &kind,
            &[],
            None,
            &mut metadata,
        );

        let visibility = self.function_visibility(&name, doc_comment.as_deref(), parent_id);
        let options = SymbolOptions {
            parent_id: parent_id.clone(),
            signature: Some(signature),
            metadata: if metadata.is_empty() {
                None
            } else {
                Some(metadata)
            },
            doc_comment,
            visibility: Some(visibility),
            ..Default::default()
        };
        let symbol = self.base.create_symbol_from_span(
            &func_def,
            crate::base::NormalizedSpan::from_node(&node),
            name,
            kind,
            options,
        );
        self.symbols.push(symbol.clone());
        let parameter_symbols = parameters::extract_parameter_symbols(self, func_def, &symbol.id);
        self.symbols.extend(parameter_symbols);
        symbol
    }

    /// A function defined inside another callable is local to it. A top-level
    /// function is public unless its name starts with a dot or roxygen marks
    /// it `@keywords internal`.
    fn function_visibility(
        &self,
        name: &str,
        doc_comment: Option<&str>,
        parent_id: &Option<String>,
    ) -> crate::base::Visibility {
        let nested = parent_id.as_ref().is_some_and(|parent_id| {
            self.symbols.iter().any(|symbol| {
                symbol.id == *parent_id
                    && matches!(symbol.kind, SymbolKind::Function | SymbolKind::Method)
            })
        });
        let internal = doc_comment.is_some_and(|doc| {
            doc.lines()
                .any(|line| line.contains("@keywords") && line.contains("internal"))
        });
        if nested || internal {
            crate::base::Visibility::Private
        } else {
            idioms::name_visibility(name)
        }
    }

    /// Split an S3 method name into `(generic, class)`.
    ///
    /// A name is an S3 method only when a roxygen `@method generic class` tag
    /// says so, or when it starts with a known or same-file generic followed by
    /// a dot and a class. Leading-dot names are internal helpers.
    fn classify_s3(
        &self,
        name: &str,
        doc_comment: Option<&str>,
        non_s3: &std::collections::HashSet<&str>,
    ) -> Option<(String, String)> {
        if let Some(tagged) = doc_comment.and_then(roxygen_s3_method) {
            return Some(tagged);
        }
        if name.starts_with('.') || non_s3.contains(name) {
            return None;
        }
        non_s3::KNOWN_S3_GENERICS
            .iter()
            .copied()
            .chain(self.same_file_generics.iter().map(String::as_str))
            .filter(|generic| {
                name.len() > generic.len() + 1
                    && name.starts_with(generic)
                    && name.as_bytes()[generic.len()] == b'.'
            })
            .max_by_key(|generic| generic.len())
            .map(|generic| (generic.to_string(), name[generic.len() + 1..].to_string()))
    }

    /// Build a function signature like `name <- function(x, y = 0)`
    fn build_function_signature(&self, name: &str, func_def: Node) -> String {
        let params = self.extract_parameters(func_def);
        format!("{} <- function({})", name, params)
    }

    /// Extract parameter list from a function_definition node
    fn extract_parameters(&self, func_def: Node) -> String {
        // The parameters node is a named field "parameters" on function_definition
        let params_node = match func_def.child_by_field_name("parameters") {
            Some(n) => n,
            None => {
                // Fall back: walk children looking for "formal_parameters"
                let mut found = None;
                let mut cursor = func_def.walk();
                for child in func_def.children(&mut cursor) {
                    if child.kind() == "formal_parameters" || child.kind() == "parameters" {
                        found = Some(child);
                        break;
                    }
                }
                match found {
                    Some(n) => n,
                    None => return String::new(),
                }
            }
        };

        let mut params = Vec::new();
        let mut cursor = params_node.walk();
        for child in params_node.children(&mut cursor) {
            match child.kind() {
                "parameter" | "default_parameter" => {
                    let param_text = self.format_parameter(child);
                    if !param_text.is_empty() {
                        params.push(param_text);
                    }
                }
                "dots" | "..." => {
                    params.push("...".to_string());
                }
                "identifier" => {
                    // Bare identifier as parameter (tree-sitter-r sometimes uses this)
                    let text = self.base.get_node_text(&child);
                    if !text.is_empty() {
                        params.push(text);
                    }
                }
                _ => {}
            }
        }
        params.join(", ")
    }

    /// Format a single parameter, truncating long defaults
    fn format_parameter(&self, param_node: Node) -> String {
        let full_text = self.base.get_node_text(&param_node);

        // Check if there's a default value (contains '=')
        if let Some(eq_pos) = full_text.find('=') {
            let param_name = full_text[..eq_pos].trim();
            let default_val = full_text[eq_pos + 1..].trim();

            // Truncate long defaults by character count, not byte index: slicing at a
            // raw byte offset panics when it lands inside a multibyte UTF-8 codepoint.
            let truncated: String = default_val.chars().take(30).collect();
            if truncated.len() < default_val.len() {
                format!("{param_name} = {truncated}...")
            } else {
                full_text.to_string()
            }
        } else {
            full_text.to_string()
        }
    }

    /// Check if a function body contains UseMethod("...")
    fn body_contains_usemethod(&self, func_def: Node) -> bool {
        let body = match func_def.child_by_field_name("body") {
            Some(b) => b,
            None => {
                // Fall back: last child is usually the body
                let count = func_def.child_count();
                if count == 0 {
                    return false;
                }
                match func_def.child((count - 1) as u32) {
                    Some(b) => b,
                    None => return false,
                }
            }
        };
        let body_text = self.base.get_node_text(&body);
        body_text.contains("UseMethod(")
    }

    fn extract_from_call(&mut self, node: Node, parent_id: &Option<String>) -> Option<Symbol> {
        // testthat call-style tests (`test_that`/`describe`/`it`) take priority.
        // The shared core pushes nothing, so push here; a recognized container/
        // test symbol becomes the parent for nested test calls. Non-DSL calls
        // fall through to the existing S4/import detectors.
        if let Some(test_sym) =
            test_calls::extract_r_test_call(&mut self.base, &node, parent_id.as_deref())
        {
            self.symbols.push(test_sym.clone());
            return Some(test_sym);
        }
        if idioms::extract_refclass_methods(self, node) {
            return None;
        }
        idioms::extract_s4_call(self, node, parent_id)
            .or_else(|| idioms::extract_import_call(self, node, parent_id))
    }

    pub fn extract_relationships(&mut self, tree: &Tree, symbols: &[Symbol]) -> Vec<Relationship> {
        relationships::extract_relationships(self, tree, symbols)
    }

    pub fn extract_identifiers(&mut self, tree: &Tree, symbols: &[Symbol]) -> Vec<Identifier> {
        identifiers::extract_identifiers(self, tree, symbols)
    }

    /// Read-only access to the underlying `BaseExtractor` (captured literals, etc.).
    #[cfg(test)]
    pub(crate) fn base(&self) -> &BaseExtractor {
        &self.base
    }

    pub fn infer_types(&self, _symbols: &[Symbol]) -> HashMap<String, String> {
        HashMap::new()
    }

    // ========================================================================
    // Pending Relationship Management
    // ========================================================================

    pub(crate) fn add_structured_pending_relationship(
        &mut self,
        pending: crate::base::StructuredPendingRelationship,
    ) {
        self.base.add_structured_pending_relationship(pending);
    }

    /// Get all pending relationships collected during extraction
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

    pub fn get_structured_pending_relationships(
        &self,
    ) -> Vec<crate::base::StructuredPendingRelationship> {
        self.base.get_structured_pending_relationships()
    }
}

fn roxygen_s3_method(doc_comment: &str) -> Option<(String, String)> {
    doc_comment.lines().find_map(|line| {
        let mut words = line
            .split_whitespace()
            .skip_while(|word| *word != "@method");
        words.next()?;
        Some((words.next()?.to_string(), words.next()?.to_string()))
    })
}
