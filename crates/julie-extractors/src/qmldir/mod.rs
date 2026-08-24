use crate::base::types::stable_location_id;
use crate::base::{
    BaseExtractor, NormalizedSpan, StructuralFact, Symbol, SymbolKind, SymbolOptions, Visibility,
};
use std::collections::HashMap;
use tree_sitter::{Node, Tree};

pub(crate) const STRUCTURAL_FACT_PATTERN_IDS: [&str; 13] = [
    "qmldir.module.v1",
    "qmldir.object_type.v1",
    "qmldir.singleton_type.v1",
    "qmldir.internal_type.v1",
    "qmldir.javascript_resource.v1",
    "qmldir.plugin.v1",
    "qmldir.classname.v1",
    "qmldir.typeinfo.v1",
    "qmldir.depends.v1",
    "qmldir.import.v1",
    "qmldir.designer_supported.v1",
    "qmldir.prefer.v1",
    "qmldir.linktarget.v1",
];

pub struct QmldirExtractor {
    pub(crate) base: BaseExtractor,
    symbols: Vec<Symbol>,
    structural_facts: Vec<StructuralFact>,
}

impl QmldirExtractor {
    pub fn new(
        language: String,
        file_path: String,
        content: String,
        workspace_root: &std::path::Path,
    ) -> Self {
        Self {
            base: BaseExtractor::new(language, file_path, content, workspace_root),
            symbols: Vec::new(),
            structural_facts: Vec::new(),
        }
    }

    pub fn extract_symbols(&mut self, tree: &Tree) -> Vec<Symbol> {
        self.symbols.clear();
        self.structural_facts.clear();
        let mut cursor = tree.root_node().walk();
        for node in tree.root_node().children(&mut cursor) {
            if node.kind() == "command" {
                self.extract_command(node);
            }
        }
        self.symbols.clone()
    }

    pub fn extract_identifiers(
        &mut self,
        _tree: &Tree,
        _symbols: &[Symbol],
    ) -> Vec<crate::base::Identifier> {
        Vec::new()
    }

    pub fn take_structural_facts(&mut self) -> Vec<StructuralFact> {
        std::mem::take(&mut self.structural_facts)
    }

    fn extract_command(&mut self, node: Node<'_>) {
        let Some(directive_node) = node.named_child(0) else {
            return;
        };
        let directive = self.base.get_node_text(&directive_node);
        let args = (1..node.named_child_count())
            .filter_map(|index| node.named_child(index as u32))
            .map(|child| self.base.get_node_text(&child))
            .collect::<Vec<_>>();

        match directive.as_str() {
            "module" => self.extract_module(node, &args),
            "singleton" => self.extract_singleton(node, &args),
            "internal" => self.extract_internal(node, &args),
            "plugin" => self.extract_plugin(node, &args, false),
            "optional" if args.first().is_some_and(|value| value == "plugin") => {
                self.extract_plugin(node, &args[1..], true)
            }
            "classname" => self.extract_classname(node, &args),
            "typeinfo" => self.extract_typeinfo(node, &args),
            "depends" => self.extract_module_reference(node, &args, "depends"),
            "import" => self.extract_module_reference(node, &args, "import"),
            "designersupported" if args.is_empty() => self.push_fact(
                node,
                "qmldir.designer_supported.v1",
                "designer_supported",
                base_metadata(
                    "designersupported",
                    [("supported", serde_json::Value::Bool(true))],
                ),
            ),
            "prefer" => self.extract_path_fact(node, &args, "prefer", "path"),
            "linktarget" => self.extract_path_fact(node, &args, "linktarget", "target"),
            _ if !is_directive(&directive) => {
                self.extract_type_or_resource(node, &directive, &args)
            }
            _ => {}
        }
    }

    fn extract_module(&mut self, node: Node<'_>, args: &[String]) {
        let Some(module_name) = args.first() else {
            return;
        };
        self.symbols.push(self.base.create_symbol(
            &node,
            module_name.clone(),
            SymbolKind::Module,
            SymbolOptions {
                visibility: Some(Visibility::Public),
                metadata: Some(symbol_metadata(
                    "module",
                    [(
                        "module_name",
                        serde_json::Value::String(module_name.clone()),
                    )],
                )),
                ..Default::default()
            },
        ));
        self.push_fact(
            node,
            "qmldir.module.v1",
            "module",
            base_metadata(
                "module",
                [("module", serde_json::Value::String(module_name.clone()))],
            ),
        );
    }

    fn extract_singleton(&mut self, node: Node<'_>, args: &[String]) {
        let (Some(type_name), Some(version), Some(file)) = (args.first(), args.get(1), args.get(2))
        else {
            return;
        };
        if !is_version(version) || !is_qml_file(file) {
            return;
        }
        self.push_type_symbol(
            node,
            type_name,
            version,
            file,
            Visibility::Public,
            "singleton",
            true,
        );
        self.push_fact(
            node,
            "qmldir.singleton_type.v1",
            "singleton_type",
            base_metadata(
                "singleton",
                [
                    ("type_name", serde_json::Value::String(type_name.clone())),
                    ("version", serde_json::Value::String(version.clone())),
                    ("file", serde_json::Value::String(file.clone())),
                    ("singleton", serde_json::Value::Bool(true)),
                ],
            ),
        );
    }

    fn extract_internal(&mut self, node: Node<'_>, args: &[String]) {
        let (Some(type_name), Some(file)) = (args.first(), args.get(1)) else {
            return;
        };
        if !is_qml_file(file) {
            return;
        }
        self.push_type_symbol(
            node,
            type_name,
            "",
            file,
            Visibility::Internal,
            "internal",
            false,
        );
        self.push_fact(
            node,
            "qmldir.internal_type.v1",
            "internal_type",
            base_metadata(
                "internal",
                [
                    ("type_name", serde_json::Value::String(type_name.clone())),
                    ("file", serde_json::Value::String(file.clone())),
                    ("internal", serde_json::Value::Bool(true)),
                ],
            ),
        );
    }

    fn extract_type_or_resource(&mut self, node: Node<'_>, type_name: &str, args: &[String]) {
        let (Some(version), Some(file)) = (args.first(), args.get(1)) else {
            return;
        };
        if !is_version(version) {
            return;
        }
        if is_js_file(file) {
            self.push_fact(
                node,
                "qmldir.javascript_resource.v1",
                "javascript_resource",
                base_metadata(
                    "javascript_resource",
                    [
                        (
                            "resource_name",
                            serde_json::Value::String(type_name.to_string()),
                        ),
                        ("version", serde_json::Value::String(version.clone())),
                        ("file", serde_json::Value::String(file.clone())),
                    ],
                ),
            );
        } else if is_qml_file(file) {
            self.push_type_symbol(
                node,
                type_name,
                version,
                file,
                Visibility::Public,
                "object_type",
                false,
            );
            self.push_fact(
                node,
                "qmldir.object_type.v1",
                "object_type",
                base_metadata(
                    "object_type",
                    [
                        (
                            "type_name",
                            serde_json::Value::String(type_name.to_string()),
                        ),
                        ("version", serde_json::Value::String(version.clone())),
                        ("file", serde_json::Value::String(file.clone())),
                    ],
                ),
            );
        }
    }

    fn extract_plugin(&mut self, node: Node<'_>, args: &[String], optional: bool) {
        let Some(name) = args.first() else {
            return;
        };
        let mut attrs = base_metadata(
            "plugin",
            [
                ("name", serde_json::Value::String(name.clone())),
                ("optional", serde_json::Value::Bool(optional)),
            ],
        );
        if let Some(path) = args.get(1) {
            attrs.insert("path".to_string(), serde_json::Value::String(path.clone()));
        }
        self.push_fact(node, "qmldir.plugin.v1", "plugin", attrs);
    }

    fn extract_classname(&mut self, node: Node<'_>, args: &[String]) {
        let Some(class_name) = args.first() else {
            return;
        };
        self.push_fact(
            node,
            "qmldir.classname.v1",
            "classname",
            base_metadata(
                "classname",
                [("class_name", serde_json::Value::String(class_name.clone()))],
            ),
        );
    }

    fn extract_typeinfo(&mut self, node: Node<'_>, args: &[String]) {
        let Some(file) = args.first() else {
            return;
        };
        self.push_fact(
            node,
            "qmldir.typeinfo.v1",
            "typeinfo",
            base_metadata(
                "typeinfo",
                [("file", serde_json::Value::String(file.clone()))],
            ),
        );
    }

    fn extract_module_reference(&mut self, node: Node<'_>, args: &[String], directive: &str) {
        let Some(module) = args.first() else {
            return;
        };
        let mut attrs = base_metadata(
            directive,
            [("module", serde_json::Value::String(module.clone()))],
        );
        if let Some(version) = args.get(1) {
            if !is_version(version) {
                return;
            }
            attrs.insert(
                "version".to_string(),
                serde_json::Value::String(version.clone()),
            );
        }
        let pattern_id = format!("qmldir.{directive}.v1");
        let capture_name = directive.to_string();
        self.push_fact(node, &pattern_id, &capture_name, attrs);
    }

    fn extract_path_fact(&mut self, node: Node<'_>, args: &[String], directive: &str, key: &str) {
        let Some(value) = args.first() else {
            return;
        };
        let mut attrs = base_metadata(directive, std::iter::empty());
        attrs.insert(key.to_string(), serde_json::Value::String(value.clone()));
        self.push_fact(node, &format!("qmldir.{directive}.v1"), directive, attrs);
    }

    fn push_type_symbol(
        &mut self,
        node: Node<'_>,
        type_name: &str,
        version: &str,
        file: &str,
        visibility: Visibility,
        kind: &str,
        singleton: bool,
    ) {
        let mut attrs = symbol_metadata(
            kind,
            [
                (
                    "type_name",
                    serde_json::Value::String(type_name.to_string()),
                ),
                ("file", serde_json::Value::String(file.to_string())),
            ],
        );
        if !version.is_empty() {
            attrs.insert(
                "version".to_string(),
                serde_json::Value::String(version.to_string()),
            );
        }
        if singleton {
            attrs.insert("singleton".to_string(), serde_json::Value::Bool(true));
        }
        if matches!(visibility, Visibility::Internal) {
            attrs.insert("internal".to_string(), serde_json::Value::Bool(true));
        }
        self.symbols.push(self.base.create_symbol(
            &node,
            type_name.to_string(),
            SymbolKind::Class,
            SymbolOptions {
                visibility: Some(visibility),
                metadata: Some(attrs),
                ..Default::default()
            },
        ));
    }

    fn push_fact(
        &mut self,
        node: Node<'_>,
        pattern_id: &str,
        capture_name: &str,
        metadata: HashMap<String, serde_json::Value>,
    ) {
        debug_assert!(STRUCTURAL_FACT_PATTERN_IDS.contains(&pattern_id));
        let span = NormalizedSpan::from_node(&node);
        self.structural_facts.push(StructuralFact {
            id: stable_location_id(
                &self.base.file_path,
                &format!("{pattern_id}:{capture_name}"),
                span,
            ),
            file_path: self.base.file_path.clone(),
            language: self.base.language.clone(),
            pattern_id: pattern_id.to_string(),
            capture_name: capture_name.to_string(),
            node_kind: node.kind().to_string(),
            containing_symbol_id: None,
            start_line: span.start_line,
            start_column: span.start_column,
            end_line: span.end_line,
            end_column: span.end_column,
            start_byte: span.start_byte,
            end_byte: span.end_byte,
            confidence: 1.0,
            metadata: Some(metadata),
        });
    }
}

fn is_directive(value: &str) -> bool {
    matches!(
        value,
        "classname"
            | "depends"
            | "designersupported"
            | "import"
            | "internal"
            | "linktarget"
            | "module"
            | "optional"
            | "plugin"
            | "prefer"
            | "singleton"
            | "typeinfo"
    )
}

fn is_version(value: &str) -> bool {
    value == "auto"
        || value.split('.').all(|part| {
            !part.is_empty() && part.chars().all(|character| character.is_ascii_digit())
        })
}

fn is_qml_file(value: &str) -> bool {
    value.ends_with(".qml")
}

fn is_js_file(value: &str) -> bool {
    value.ends_with(".js")
}

fn symbol_metadata(
    kind: &str,
    entries: impl IntoIterator<Item = (&'static str, serde_json::Value)>,
) -> HashMap<String, serde_json::Value> {
    let mut metadata = HashMap::new();
    metadata.insert(
        "qmldir_kind".to_string(),
        serde_json::Value::String(kind.to_string()),
    );
    metadata.extend(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_string(), value)),
    );
    metadata
}

fn base_metadata(
    directive: &str,
    entries: impl IntoIterator<Item = (&'static str, serde_json::Value)>,
) -> HashMap<String, serde_json::Value> {
    let mut metadata = HashMap::new();
    metadata.insert(
        "pattern_version".to_string(),
        serde_json::Value::Number(serde_json::Number::from(1)),
    );
    metadata.insert(
        "query_family".to_string(),
        serde_json::Value::String("qmldir".to_string()),
    );
    metadata.insert(
        "directive".to_string(),
        serde_json::Value::String(directive.to_string()),
    );
    metadata.extend(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_string(), value)),
    );
    metadata
}
