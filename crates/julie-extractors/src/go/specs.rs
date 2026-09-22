use crate::base::{Symbol, SymbolKind, SymbolOptions, Visibility};
use tree_sitter::Node;

/// Extraction of import, variable, and constant specifications
impl super::GoExtractor {
    pub(super) fn extract_import_symbols(
        &mut self,
        node: Node,
        parent_id: Option<&str>,
    ) -> Vec<Symbol> {
        let mut symbols = Vec::new();
        let mut cursor = node.walk();

        for child in node.children(&mut cursor) {
            match child.kind() {
                "import_spec" => {
                    if let Some(symbol) = self.extract_import_spec(child, parent_id) {
                        symbols.push(symbol);
                    }
                }
                "import_spec_list" => {
                    let mut nested_cursor = child.walk();
                    for nested_child in child.children(&mut nested_cursor) {
                        if nested_child.kind() == "import_spec"
                            && let Some(symbol) = self.extract_import_spec(nested_child, parent_id)
                        {
                            symbols.push(symbol);
                        }
                    }
                }
                _ => {}
            }
        }

        symbols
    }

    pub(super) fn extract_var_symbols(
        &mut self,
        node: Node,
        parent_id: Option<&str>,
    ) -> Vec<Symbol> {
        let mut symbols = Vec::new();
        let mut cursor = node.walk();

        for child in node.children(&mut cursor) {
            match child.kind() {
                "var_spec" => {
                    symbols.extend(self.extract_var_spec_symbols(child, parent_id));
                }
                "var_spec_list" => {
                    let mut nested_cursor = child.walk();
                    for nested_child in child.children(&mut nested_cursor) {
                        if nested_child.kind() == "var_spec" {
                            symbols.extend(self.extract_var_spec_symbols(nested_child, parent_id));
                        }
                    }
                }
                _ => {}
            }
        }

        symbols
    }

    pub(super) fn extract_const_symbols(
        &mut self,
        node: Node,
        parent_id: Option<&str>,
    ) -> Vec<Symbol> {
        let mut symbols = Vec::new();
        let mut cursor = node.walk();

        for child in node.children(&mut cursor) {
            match child.kind() {
                "const_spec" => {
                    symbols.extend(self.extract_const_spec_symbols(child, parent_id));
                }
                "const_spec_list" => {
                    let mut nested_cursor = child.walk();
                    for nested_child in child.children(&mut nested_cursor) {
                        if nested_child.kind() == "const_spec" {
                            symbols
                                .extend(self.extract_const_spec_symbols(nested_child, parent_id));
                        }
                    }
                }
                _ => {}
            }
        }

        symbols
    }

    pub(super) fn extract_import_spec(
        &mut self,
        node: Node,
        parent_id: Option<&str>,
    ) -> Option<Symbol> {
        let import_path = self.get_node_text(node.child_by_field_name("path")?);
        let name_node = node.child_by_field_name("name");
        let binding = name_node.map(|name| (name.kind(), self.get_node_text(name)));

        let (package_name, is_dot_import) = match binding {
            Some(("blank_identifier", _)) => return None,
            Some(("dot", _)) => (assumed_package_name(import_path.trim_matches('"')), true),
            Some((_, ref alias)) => (alias.clone(), false),
            None => (assumed_package_name(import_path.trim_matches('"')), false),
        };
        if package_name.is_empty() {
            return None;
        }

        let signature = match binding {
            Some((_, alias)) => format!("import {} {}", alias, import_path),
            None => format!("import {}", import_path),
        };
        let metadata = is_dot_import.then(|| {
            std::collections::HashMap::from([(
                "dotImport".to_string(),
                serde_json::Value::Bool(true),
            )])
        });

        let doc_comment = self.base.find_doc_comment(&node);

        Some(self.base.create_symbol(
            &node,
            package_name,
            SymbolKind::Import,
            SymbolOptions {
                signature: Some(signature),
                visibility: Some(Visibility::Public),
                parent_id: parent_id.map(|s| s.to_string()),
                metadata,
                doc_comment,
                annotations: Vec::new(),
            },
        ))
    }

    pub(super) fn extract_var_spec_symbols(
        &mut self,
        node: Node,
        parent_id: Option<&str>,
    ) -> Vec<Symbol> {
        let mut cursor = node.walk();
        let mut identifiers = Vec::new();
        let mut var_type = None;
        let mut values = Vec::new();

        for child in node.children(&mut cursor) {
            match child.kind() {
                "identifier" => identifiers.push((self.get_node_text(child), child)),
                "type_identifier" | "primitive_type" | "pointer_type" | "slice_type"
                | "map_type" => {
                    var_type = Some(self.extract_type_from_node(child));
                }
                "expression_list" => {
                    values = self.extract_spec_values(child);
                }
                _ => {}
            }
        }

        let doc_comment = self.base.find_doc_comment(&node);
        let type_node = node.child_by_field_name("type");
        identifiers
            .into_iter()
            .enumerate()
            .map(|(index, (name, _name_node))| {
                let visibility = if self.is_public(&name) {
                    Some(Visibility::Public)
                } else {
                    Some(Visibility::Private)
                };

                let value = values.get(index).or_else(|| values.first());
                let signature = if let Some(typ) = var_type.as_deref() {
                    if let Some(val) = value {
                        format!("var {} {} = {}", name, typ, val)
                    } else {
                        format!("var {} {}", name, typ)
                    }
                } else if let Some(val) = value {
                    format!("var {} = {}", name, val)
                } else {
                    format!("var {}", name)
                };

                let symbol = self.base.create_symbol(
                    &node,
                    name,
                    SymbolKind::Variable,
                    SymbolOptions {
                        signature: Some(signature),
                        visibility,
                        parent_id: parent_id.map(|s| s.to_string()),
                        metadata: None,
                        doc_comment: doc_comment.clone(),
                        annotations: Vec::new(),
                    },
                );
                if let Some(type_node) = type_node {
                    super::type_facts::record_type_node_fact(
                        &mut self.base,
                        &symbol.id,
                        type_node,
                        false,
                    );
                }
                symbol
            })
            .collect()
    }

    pub(super) fn extract_short_var_symbols(
        &mut self,
        node: Node,
        parent_id: Option<&str>,
    ) -> Vec<Symbol> {
        let Some(left) = node.child_by_field_name("left") else {
            return Vec::new();
        };
        let Some(right) = node.child_by_field_name("right") else {
            return Vec::new();
        };
        let mut left_cursor = left.walk();
        let names: Vec<Node> = left.named_children(&mut left_cursor).collect();
        let mut right_cursor = right.walk();
        let values: Vec<Node> = right.named_children(&mut right_cursor).collect();
        let positional_values: Vec<Option<Node>> = if names.len() == values.len() {
            values.into_iter().map(Some).collect()
        } else {
            vec![None; names.len()]
        };

        names
            .into_iter()
            .zip(positional_values)
            .filter_map(|(name_node, value_node)| {
                if name_node.kind() != "identifier" {
                    return None;
                }
                let name = self.get_node_text(name_node);
                if name == "_" {
                    return None;
                }
                let type_node = value_node
                    .and_then(|value_node| {
                        super::type_facts::inferred_rhs_type_node(&self.base, value_node)
                    })
                    .filter(|type_node| super::type_facts::binds_base_type(*type_node));
                let visibility = if self.is_public(&name) {
                    Some(Visibility::Public)
                } else {
                    Some(Visibility::Private)
                };
                let signature = match value_node {
                    Some(value_node) => {
                        format!("{} := {}", name, self.get_node_text(value_node))
                    }
                    None => self.get_node_text(node),
                };
                let symbol = self.base.create_symbol(
                    &name_node,
                    name,
                    SymbolKind::Variable,
                    SymbolOptions {
                        signature: Some(signature),
                        visibility,
                        parent_id: parent_id.map(|s| s.to_string()),
                        metadata: None,
                        doc_comment: None,
                        annotations: Vec::new(),
                    },
                );
                if let Some(type_node) = type_node {
                    super::type_facts::record_type_node_fact(
                        &mut self.base,
                        &symbol.id,
                        type_node,
                        true,
                    );
                }
                Some(symbol)
            })
            .collect()
    }

    pub(super) fn extract_const_spec_symbols(
        &mut self,
        node: Node,
        parent_id: Option<&str>,
    ) -> Vec<Symbol> {
        let mut cursor = node.walk();
        let mut identifiers = Vec::new();
        let mut const_type = None;
        let mut values = Vec::new();

        for child in node.children(&mut cursor) {
            match child.kind() {
                "identifier" => identifiers.push((self.get_node_text(child), child)),
                "type_identifier" | "primitive_type" => {
                    const_type = Some(self.extract_type_from_node(child));
                }
                "expression_list" => {
                    values = self.extract_spec_values(child);
                }
                _ if child.kind().starts_with("literal")
                    || matches!(child.kind(), "true" | "false" | "nil") =>
                {
                    values.push(self.get_node_text(child));
                }
                _ => {}
            }
        }

        let doc_comment = self.base.find_doc_comment(&node);
        identifiers
            .into_iter()
            .enumerate()
            .map(|(index, (name, _name_node))| {
                let visibility = if self.is_public(&name) {
                    Some(Visibility::Public)
                } else {
                    Some(Visibility::Private)
                };

                let value = values.get(index).or_else(|| values.first());
                let signature = if let Some(val) = value {
                    if let Some(typ) = const_type.as_deref() {
                        format!("const {} {} = {}", name, typ, val)
                    } else {
                        format!("const {} = {}", name, val)
                    }
                } else {
                    format!("const {}", name)
                };

                self.base.create_symbol(
                    &node,
                    name,
                    SymbolKind::Constant,
                    SymbolOptions {
                        signature: Some(signature),
                        visibility,
                        parent_id: parent_id.map(|s| s.to_string()),
                        metadata: None,
                        doc_comment: doc_comment.clone(),
                        annotations: Vec::new(),
                    },
                )
            })
            .collect()
    }

    fn extract_spec_values(&self, node: Node) -> Vec<String> {
        let mut values = Vec::new();
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if !matches!(child.kind(), "," | " ") {
                values.push(self.get_node_text(child));
            }
        }
        values
    }
}

/// The package name an unaliased import binds, by the rule goimports uses: the
/// last path element, skipping a `/vN` major-version element, without a `go-`
/// prefix, cut at the first character that cannot appear in an identifier
/// (`gopkg.in/yaml.v3` -> `yaml`, `github.com/mattn/go-sqlite3` -> `sqlite3`).
fn assumed_package_name(import_path: &str) -> String {
    let mut elements = import_path.rsplit('/');
    let mut base = elements.next().unwrap_or(import_path);
    let is_major_version = base
        .strip_prefix('v')
        .is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()));
    if is_major_version && let Some(parent) = elements.next() {
        base = parent;
    }
    let base = base.strip_prefix("go-").unwrap_or(base);
    base.split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .next()
        .unwrap_or_default()
        .to_string()
}
