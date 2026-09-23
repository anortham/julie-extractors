//! Go module manifest (`go.mod`) extractor.
//!
//! - `module` is a `module` symbol; `go`, `toolchain`, and each `godebug`
//!   key are `property` symbols; each `require` line and each `tool` package
//!   is an `import` symbol. Every symbol carries its leading `//` comment as
//!   its doc comment.
//! - Each required module and tool is an `Imports` edge from the module symbol.
//! - A `replace` whose target is a file path is a structured pending `Imports`
//!   row to `<path>/go.mod`, the file that declares the replacement module.
//! - Every directive line is a structural fact (see [`facts`]).
//! - A quoted value is a `string_literal` source region and a literal whose
//!   carrier is `<directive>.<role>`.

mod directives;
pub(crate) mod facts;

use std::collections::HashMap;

use serde_json::Value;
use tree_sitter::Tree;

use crate::base::types::stable_location_id;
use crate::base::{
    BaseExtractor, NormalizedSpan, Relationship, RelationshipKind, SourceRegion, SourceRegionKind,
    StructuredPendingRelationship, Symbol, SymbolKind, SymbolOptions, UnresolvedTarget, Visibility,
};
use directives::{Directive, Entry, entries};

pub(crate) use directives::doc_comment_starts;

pub struct GoModExtractor {
    pub(crate) base: BaseExtractor,
    string_regions: Vec<SourceRegion>,
}

impl GoModExtractor {
    pub fn new(
        language: String,
        file_path: String,
        content: String,
        workspace_root: &std::path::Path,
    ) -> Self {
        Self {
            base: BaseExtractor::new(language, file_path, content, workspace_root),
            string_regions: Vec::new(),
        }
    }

    pub fn extract_symbols(&mut self, tree: &Tree) -> Vec<Symbol> {
        let content = self.base.content.clone();
        let mut symbols = Vec::new();
        for entry in entries(tree.root_node(), &content) {
            let symbol = entry
                .symbol_name()
                .and_then(|name| self.symbol_for(&entry, name.to_string(), &content));
            let symbol_id = symbol.as_ref().map(|symbol| symbol.id.clone());
            for value in entry.values.iter().filter(|value| value.quoted) {
                self.base.record_literal(
                    &value.node,
                    value.text.clone(),
                    Some(format!("{}.{}", entry.directive.keyword(), value.role)),
                    0,
                    symbol_id.clone(),
                );
                self.push_string_region(value.node, symbol_id.clone());
            }
            symbols.extend(symbol);
        }
        symbols
    }

    /// The grammar lexes most quoted values as plain tokens, so string regions
    /// come from the values, not from string node kinds.
    fn push_string_region(&mut self, node: tree_sitter::Node<'_>, symbol_id: Option<String>) {
        let span = NormalizedSpan::from_node(&node);
        let kind = SourceRegionKind::StringLiteral;
        self.string_regions.push(SourceRegion {
            id: stable_location_id(&self.base.file_path, kind.as_str(), span),
            file_path: self.base.file_path.clone(),
            language: self.base.language.clone(),
            kind,
            containing_symbol_id: symbol_id,
            start_line: span.start_line,
            start_column: span.start_column,
            end_line: span.end_line,
            end_column: span.end_column,
            start_byte: span.start_byte,
            end_byte: span.end_byte,
            metadata: None,
        });
    }

    pub fn take_string_regions(&mut self) -> Vec<SourceRegion> {
        std::mem::take(&mut self.string_regions)
    }

    fn symbol_for(&mut self, entry: &Entry<'_>, name: String, content: &str) -> Option<Symbol> {
        let span = self
            .base
            .span_for_byte_range(entry.start_byte, entry.end_byte)?;
        let kind = match entry.directive {
            Directive::Module => SymbolKind::Module,
            Directive::Go | Directive::Toolchain | Directive::Godebug => SymbolKind::Property,
            _ => SymbolKind::Import,
        };
        let indirect =
            entry.directive == Directive::Require && directives::is_indirect(content, entry);
        let mut metadata = HashMap::from([(
            "directive".to_string(),
            Value::String(entry.directive.keyword().to_string()),
        )]);
        if entry.directive == Directive::Require {
            if let Some(version) = entry.value("version") {
                metadata.insert("version".to_string(), Value::String(version.to_string()));
            }
            metadata.insert("indirect".to_string(), Value::Bool(indirect));
        }
        if entry.directive == Directive::Module
            && let Some(deprecated) = directives::deprecation(content, entry)
        {
            metadata.insert("deprecated".to_string(), Value::String(deprecated));
        }
        let mut signature = content[entry.start_byte..entry.end_byte].to_string();
        if entry.block.is_some() {
            signature = format!("{} {signature}", entry.directive.keyword());
        }
        if indirect {
            signature.push_str(" // indirect");
        }
        let options = SymbolOptions {
            signature: Some(signature),
            visibility: (entry.directive == Directive::Module).then_some(Visibility::Public),
            doc_comment: directives::doc_comment(content, entry),
            metadata: Some(metadata),
            ..Default::default()
        };
        let mut symbol = self
            .base
            .create_symbol_from_span(&entry.node, span, name, kind, options);
        let body = entry
            .values
            .last()
            .map(|value| NormalizedSpan::from_node(&value.node));
        self.base.set_body_span(&mut symbol, body);
        Some(symbol)
    }

    /// `Imports` edges from the module symbol to every required module and
    /// tool, and structured pending `Imports` rows for file-path replacements.
    pub fn extract_relationships(&mut self, tree: &Tree, symbols: &[Symbol]) -> Vec<Relationship> {
        let content = self.base.content.clone();
        let Some(module) = symbols
            .iter()
            .find(|symbol| symbol.kind == SymbolKind::Module)
        else {
            return Vec::new();
        };
        let mut relationships = Vec::new();
        for entry in entries(tree.root_node(), &content) {
            match entry.directive {
                Directive::Require | Directive::Tool => {
                    let Some(to) = symbols.iter().find(|symbol| {
                        symbol.start_byte as usize == entry.start_byte
                            && symbol.end_byte as usize == entry.end_byte
                    }) else {
                        continue;
                    };
                    relationships.push(self.dependency_edge(module, to, &entry, &content));
                }
                Directive::Replace if entry.value("replacement_version").is_none() => {
                    if let Some(path) = entry.value("replacement") {
                        self.push_replacement_pending(module, path, &entry);
                    }
                }
                _ => {}
            }
        }
        relationships
    }

    fn dependency_edge(
        &self,
        from: &Symbol,
        to: &Symbol,
        entry: &Entry<'_>,
        content: &str,
    ) -> Relationship {
        let mut metadata = HashMap::from([
            (
                "dependencyKind".to_string(),
                Value::String(entry.directive.keyword().to_string()),
            ),
            ("dependencyName".to_string(), Value::String(to.name.clone())),
        ]);
        if entry.directive == Directive::Require {
            metadata.insert(
                "indirect".to_string(),
                Value::Bool(directives::is_indirect(content, entry)),
            );
        }
        Relationship {
            id: format!(
                "{}_{}_{:?}_{}",
                from.id,
                to.id,
                RelationshipKind::Imports,
                entry.node.start_position().row
            ),
            from_symbol_id: from.id.clone(),
            to_symbol_id: to.id.clone(),
            kind: RelationshipKind::Imports,
            file_path: self.base.file_path.clone(),
            line_number: to.start_line,
            span: self
                .base
                .span_for_byte_range(entry.start_byte, entry.end_byte),
            reference_site_is_exact: false,
            confidence: 1.0,
            metadata: Some(metadata),
        }
    }

    fn push_replacement_pending(&mut self, module: &Symbol, path: &str, entry: &Entry<'_>) {
        let manifest = format!("{}/go.mod", path.replace('\\', "/").trim_end_matches('/'));
        let pending = StructuredPendingRelationship::new(
            module.id.clone(),
            UnresolvedTarget {
                display_name: manifest.clone(),
                terminal_name: "go.mod".to_string(),
                receiver: None,
                namespace_path: Vec::new(),
                import_context: Some(manifest),
            },
            Some(module.id.clone()),
            RelationshipKind::Imports,
            self.base.file_path.clone(),
            entry.node.start_position().row as u32 + 1,
            1.0,
        );
        self.base.add_structured_pending_relationship(pending);
    }
}
