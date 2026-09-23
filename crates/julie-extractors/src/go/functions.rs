use crate::base::{Symbol, SymbolKind, SymbolOptions, Visibility};
use crate::test_detection::apply_callable_test_metadata;
use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;
use tree_sitter::Node;

/// Function and method extraction for Go
impl super::GoExtractor {
    /// Recover `func name(` declarations the parser lost inside error regions.
    /// A clean tree needs no recovery, and a match inside a string literal or
    /// comment is text, not a declaration.
    pub(super) fn recover_function_symbols_from_source(
        &mut self,
        root: Node,
        symbols: &mut Vec<Symbol>,
    ) {
        if !root.has_error() {
            return;
        }
        static GO_FUNCTION_SIGNATURE_RE: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r"(?m)^(?P<indent>[ \t]*)func\s+(?P<name>[A-Za-z_]\w*)\s*(?P<params>\([^)\n]*\))(?:\s+(?P<return_type>[^{\n]+))?(?:\s*\{)?")
                .expect("Go function recovery regex should compile")
        });

        let content = self.base.content.clone();
        for captures in GO_FUNCTION_SIGNATURE_RE.captures_iter(&content) {
            let Some(name_match) = captures.name("name") else {
                continue;
            };
            if is_inside_string_or_comment(root, name_match.start(), name_match.end()) {
                continue;
            }
            let name = name_match.as_str().to_string();
            let (start_line, start_column) = line_column_for_byte(&content, name_match.start());

            let already_extracted = symbols.iter().any(|symbol| {
                symbol.kind == SymbolKind::Function
                    && symbol.name == name
                    && symbol.start_line == start_line
            });
            if already_extracted {
                continue;
            }

            let Some(full_match) = captures.get(0) else {
                continue;
            };
            let (end_line, end_column) = line_column_for_byte(&content, full_match.end());
            let signature = full_match
                .as_str()
                .trim()
                .trim_end_matches('{')
                .trim()
                .to_string();
            let span = crate::base::NormalizedSpan {
                start_line,
                start_column,
                end_line,
                end_column,
                start_byte: name_match.start() as u32,
                end_byte: full_match.end() as u32,
            };
            let id = self.base.generate_id_for_span(&name, &span);
            let mut metadata = HashMap::new();
            apply_callable_test_metadata(
                "go",
                &name,
                &self.base.file_path,
                &SymbolKind::Function,
                &[],
                None,
                &mut metadata,
            );
            let symbol = Symbol {
                id,
                name: name.clone(),
                kind: SymbolKind::Function,
                language: self.base.language.clone(),
                file_path: self.base.file_path.clone(),
                start_line,
                start_column,
                end_line,
                end_column,
                start_byte: span.start_byte,
                end_byte: span.end_byte,
                body_span: None,
                body_hash: None,
                signature: Some(signature),
                doc_comment: None,
                visibility: if self.is_public(&name) {
                    Some(Visibility::Public)
                } else {
                    Some(Visibility::Private)
                },
                parent_id: None,
                metadata: Some(metadata),
                annotations: Vec::new(),
                semantic_group: None,
                confidence: Some(0.8),
                content_type: None,
            };

            symbols.push(symbol);
        }
    }

    pub(super) fn extract_function(
        &mut self,
        node: Node,
        parent_id: Option<&str>,
    ) -> Option<Symbol> {
        let name = self.get_node_text(node.child_by_field_name("name")?);
        let visibility = if name == "main" || name == "init" {
            Some(Visibility::Private) // Special Go functions
        } else if self.is_public(&name) {
            Some(Visibility::Public)
        } else {
            Some(Visibility::Private)
        };

        let type_params = node
            .child_by_field_name("type_parameters")
            .map(|type_params| self.get_node_text(type_params))
            .unwrap_or_default();
        let signature = format!(
            "func {name}{type_params}{}",
            self.callable_signature_tail(node)
        );

        let doc_comment = self.go_doc_comment(&node);
        let annotations = self.annotations_from_compiler_directives(&node);

        let mut metadata = HashMap::new();
        apply_callable_test_metadata(
            "go",
            &name,
            &self.base.file_path,
            &SymbolKind::Function,
            &[],
            doc_comment.as_deref(),
            &mut metadata,
        );

        let symbol = self.base.create_symbol(
            &node,
            name,
            SymbolKind::Function,
            SymbolOptions {
                signature: Some(signature),
                visibility,
                parent_id: parent_id.map(|s| s.to_string()),
                metadata: if metadata.is_empty() {
                    None
                } else {
                    Some(metadata)
                },
                doc_comment: doc_comment.clone(),
                annotations,
            },
        );
        super::type_facts::record_return_type(&mut self.base, &symbol.id, node);
        Some(super::helpers::finalize_function_symbol(
            symbol,
            doc_comment,
        ))
    }

    pub(super) fn extract_method(&mut self, node: Node, parent_id: Option<&str>) -> Option<Symbol> {
        let name = self.get_node_text(node.child_by_field_name("name")?);
        let visibility = if self.is_public(&name) {
            Some(Visibility::Public)
        } else {
            Some(Visibility::Private)
        };

        let receiver_decl = node.child_by_field_name("receiver").and_then(|receiver| {
            receiver
                .named_children(&mut receiver.walk())
                .find(|child| child.kind() == "parameter_declaration")
        });
        let receiver = receiver_decl.map(|decl| self.extract_parameter_declaration(decl));
        let tail = self.callable_signature_tail(node);
        let signature = match receiver {
            Some(receiver) => format!("func ({receiver}) {name}{tail}"),
            None => format!("func {name}{tail}"),
        };
        let receiver_type = receiver_decl.map(|decl| {
            let is_pointer = decl
                .child_by_field_name("type")
                .is_some_and(|type_node| type_node.kind() == "pointer_type");
            (self.extract_receiver_type_from_param(decl), is_pointer)
        });

        let doc_comment = self.go_doc_comment(&node);
        let annotations = self.annotations_from_compiler_directives(&node);

        let mut metadata = HashMap::new();
        apply_callable_test_metadata(
            "go",
            &name,
            &self.base.file_path,
            &SymbolKind::Method,
            &[],
            doc_comment.as_deref(),
            &mut metadata,
        );
        if let Some((receiver_type, is_pointer)) = receiver_type
            && !receiver_type.is_empty()
        {
            metadata.insert(
                "receiver_type".to_string(),
                serde_json::Value::String(receiver_type),
            );
            metadata.insert(
                "receiver_pointer".to_string(),
                serde_json::Value::Bool(is_pointer),
            );
        }

        let symbol = self.base.create_symbol(
            &node,
            name,
            SymbolKind::Method,
            SymbolOptions {
                signature: Some(signature),
                visibility,
                parent_id: parent_id.map(|s| s.to_string()),
                metadata: if metadata.is_empty() {
                    None
                } else {
                    Some(metadata)
                },
                doc_comment: doc_comment.clone(),
                annotations,
            },
        );
        super::type_facts::record_return_type(&mut self.base, &symbol.id, node);
        Some(super::helpers::finalize_function_symbol(
            symbol,
            doc_comment,
        ))
    }

    /// `(params) results` for a function, method, or interface method element,
    /// read from the grammar's `parameters` and `result` fields.
    fn callable_signature_tail(&self, node: Node) -> String {
        let parameters = node
            .child_by_field_name("parameters")
            .map(|parameters| self.extract_parameter_list(parameters))
            .unwrap_or_default();
        let result = match node.child_by_field_name("result") {
            None => String::new(),
            Some(result) if result.kind() == "parameter_list" => {
                let results = self.extract_parameter_list(result);
                let named = result
                    .named_children(&mut result.walk())
                    .any(|decl| decl.child_by_field_name("name").is_some());
                match results.as_slice() {
                    [single] if !named => format!(" {single}"),
                    _ => format!(" ({})", results.join(", ")),
                }
            }
            Some(result) => format!(" {}", self.extract_type_from_node(result)),
        };
        format!("({}){result}", parameters.join(", "))
    }

    /// An interface method element (`Get(id string) error`) as a method of the
    /// enclosing interface.
    pub(super) fn extract_method_elem(
        &mut self,
        node: Node,
        parent_id: Option<&str>,
    ) -> Option<Symbol> {
        let name = self.get_node_text(node.child_by_field_name("name")?);
        let visibility = if self.is_public(&name) {
            Visibility::Public
        } else {
            Visibility::Private
        };
        let signature = format!("{name}{}", self.callable_signature_tail(node));
        let doc_comment = self.go_doc_comment(&node);
        let symbol = self.base.create_symbol(
            &node,
            name,
            SymbolKind::Method,
            SymbolOptions {
                signature: Some(signature),
                visibility: Some(visibility),
                parent_id: parent_id.map(str::to_string),
                metadata: None,
                doc_comment: doc_comment.clone(),
                annotations: Vec::new(),
            },
        );
        super::type_facts::record_return_type(&mut self.base, &symbol.id, node);
        Some(super::helpers::finalize_function_symbol(
            symbol,
            doc_comment,
        ))
    }

    pub(super) fn extract_parameter_list(&self, node: Node) -> Vec<String> {
        let mut parameters = Vec::new();
        let mut cursor = node.walk();

        for child in node.children(&mut cursor) {
            match child.kind() {
                "parameter_declaration" | "variadic_parameter_declaration" => {
                    let param = self.extract_parameter_declaration(child);
                    if !param.is_empty() {
                        parameters.push(param);
                    }
                }
                _ => {}
            }
        }

        parameters
    }

    pub(super) fn extract_parameter_declaration(&self, node: Node) -> String {
        // Handle variadic parameter declarations
        if node.kind() == "variadic_parameter_declaration" {
            return self.get_node_text(node);
        }

        let mut names = Vec::new();
        let mut param_type = None;
        let mut cursor = node.walk();

        for child in node.children(&mut cursor) {
            match child.kind() {
                "identifier" => names.push(self.get_node_text(child)),
                "type_identifier" | "primitive_type" | "pointer_type" | "slice_type"
                | "map_type" | "channel_type" | "interface_type" | "function_type"
                | "array_type" | "qualified_type" | "generic_type" => {
                    param_type = Some(self.extract_type_from_node(child));
                }
                "variadic_parameter" => {
                    // Handle variadic parameters like ...interface{}
                    let variadic_text = self.get_node_text(child);
                    param_type = Some(variadic_text);
                }
                _ => {}
            }
        }

        if let Some(typ) = param_type {
            if names.is_empty() {
                typ // Anonymous parameter
            } else {
                format!("{} {}", names.join(", "), typ)
            }
        } else if !names.is_empty() {
            names[0].clone() // Just the name if no type found
        } else {
            String::new()
        }
    }

    pub(super) fn extract_from_error_node(
        &mut self,
        node: Node,
        parent_id: Option<&str>,
    ) -> Option<Symbol> {
        // Try to recover function signatures from ERROR nodes
        // Look for identifier + parenthesized_type pattern (function signature)
        let mut cursor = node.walk();
        let mut identifier = None;
        let mut param_type = None;

        for child in node.children(&mut cursor) {
            match child.kind() {
                "identifier" => identifier = Some(child),
                "parenthesized_type" => param_type = Some(child),
                _ => {}
            }
        }

        if let (Some(id_node), Some(param_node)) = (identifier, param_type) {
            let name = self.get_node_text(id_node);
            let params = self.get_node_text(param_node);

            // This looks like a function signature trapped in an ERROR node
            let signature = format!("func {}{}", name, params);

            let doc_comment = self.go_doc_comment(&node);

            let symbol = self.base.create_symbol(
                &node,
                name.clone(),
                SymbolKind::Function,
                SymbolOptions {
                    signature: Some(signature),
                    visibility: if self.is_public(&name) {
                        Some(Visibility::Public)
                    } else {
                        Some(Visibility::Private)
                    },
                    parent_id: parent_id.map(|s| s.to_string()),
                    metadata: None,
                    doc_comment: doc_comment.clone(),
                    annotations: Vec::new(),
                },
            );
            return Some(super::helpers::finalize_function_symbol(
                symbol,
                doc_comment,
            ));
        }

        None
    }
}

fn is_inside_string_or_comment(root: Node, start: usize, end: usize) -> bool {
    let mut node = root.descendant_for_byte_range(start, end);
    while let Some(current) = node {
        if matches!(
            current.kind(),
            "comment" | "raw_string_literal" | "interpreted_string_literal"
        ) {
            return true;
        }
        node = current.parent();
    }
    false
}

fn line_column_for_byte(content: &str, byte: usize) -> (u32, u32) {
    let prefix = content.get(..byte).unwrap_or(content);
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() as u32 + 1;
    let column = prefix
        .rsplit_once('\n')
        .map(|(_, line)| line)
        .unwrap_or(prefix)
        .chars()
        .count() as u32;
    (line, column)
}
