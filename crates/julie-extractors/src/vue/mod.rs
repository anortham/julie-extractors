// Vue Single File Component (SFC) Extractor
//
// Splits a .vue file into its top-level blocks. Script and style blocks run
// through the native JavaScript, TypeScript, or CSS pipeline in host
// coordinates; the component owns template bindings and top-level calls.

use crate::base::relationship_resolution::StructuredPendingRelationship;
use crate::base::{
    BaseExtractor, ExtractionResults, Identifier, ParseDiagnostic, Relationship, SourceRegion,
    Symbol, SymbolKind,
};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use tree_sitter::Tree;

mod component;
mod identifiers;
mod manual_symbols;
pub(crate) mod parsing;
mod relationships;
mod script;
mod script_setup;
mod template;

use manual_symbols::create_symbol_manual;
use parsing::ParsedVueSfc;
use relationships::ComponentRows;

/// Vue Single File Component (SFC) Extractor
pub struct VueExtractor {
    pub(crate) base: BaseExtractor,
    pub(crate) parsed_sfc: ParsedVueSfc,
    embedded: Vec<ExtractionResults>,
    script_blocks: HashSet<usize>,
    script_symbol_ids: HashSet<String>,
    component_rows: Option<ComponentRows>,
}

impl VueExtractor {
    pub fn new(
        language: String,
        file_path: String,
        content: String,
        workspace_root: &std::path::Path,
    ) -> Self {
        let parsed_sfc = ParsedVueSfc::parse(&content).unwrap_or_default();
        Self {
            base: BaseExtractor::new(language, file_path, content, workspace_root),
            parsed_sfc,
            embedded: Vec::new(),
            script_blocks: HashSet::new(),
            script_symbol_ids: HashSet::new(),
            component_rows: None,
        }
    }

    pub fn extract_symbols(&mut self, _tree: Option<&Tree>) -> Vec<Symbol> {
        self.embedded.clear();
        self.script_blocks.clear();
        self.script_symbol_ids.clear();
        self.component_rows = None;
        let mut symbols = Vec::new();

        for (idx, section) in self.parsed_sfc.sections.iter().enumerate() {
            match section.section_type.as_str() {
                "script" => {
                    let Some(tree) = self.parsed_sfc.script_tree(idx) else {
                        continue;
                    };
                    if let Some((section_symbols, results)) =
                        script::extract_script(&self.base, section, tree)
                    {
                        self.script_symbol_ids
                            .extend(section_symbols.iter().map(|symbol| symbol.id.clone()));
                        symbols.extend(section_symbols);
                        self.script_blocks.insert(self.embedded.len());
                        self.embedded.push(results);
                    }
                }
                "template" => {
                    symbols.extend(template::extract_template_symbols(&self.base, section));
                }
                "style" => {
                    if let Some((section_symbols, results)) =
                        script::extract_style(&self.base, section)
                    {
                        symbols.extend(section_symbols);
                        self.embedded.push(results);
                    }
                }
                _ => {}
            }
        }
        for results in &mut self.embedded {
            self.base.literals.append(&mut results.literals);
            self.base
                .type_argument_usages
                .append(&mut results.type_argument_usages);
            self.base
                .type_info
                .extend(std::mem::take(&mut results.types));
        }

        if let Some(component_name) =
            component::extract_component_name(&self.base.file_path, &self.parsed_sfc)
        {
            symbols.push(self.component_symbol(&component_name));
        }

        script_setup::apply_script_setup_annotations(&mut symbols, &self.parsed_sfc);

        symbols
    }

    fn component_symbol(&self, component_name: &str) -> Symbol {
        let doc_comment = extract_component_doc_comment(&self.base.content);

        // Span the component over the file's real lines: end on the last
        // line, one past its final byte, so byte-based containment of
        // section facts covers the whole file without reporting a line that
        // does not exist.
        let component_end_line = self.base.content.lines().count().max(1);
        let component_end_column = self
            .base
            .content
            .lines()
            .next_back()
            .map_or(1, |line| line.len() + 1);
        let mut metadata = HashMap::new();
        metadata.insert("type".to_string(), Value::String("vue-sfc".to_string()));
        metadata.insert(
            "sections".to_string(),
            Value::String(
                self.parsed_sfc
                    .sections
                    .iter()
                    .map(|section| section.section_type.clone())
                    .collect::<Vec<_>>()
                    .join(","),
            ),
        );
        create_symbol_manual(
            &self.base,
            component_name,
            SymbolKind::Class,
            1,
            1,
            component_end_line,
            component_end_column,
            Some(format!("<{} />", component_name)),
            doc_comment.or_else(|| Some(format!("Vue Single File Component: {}", component_name))),
            Some(metadata),
        )
    }

    fn component_rows(&mut self, symbols: &[Symbol]) -> &mut ComponentRows {
        if self.component_rows.is_none() {
            let mut script_identifiers: Vec<&mut Identifier> = Vec::new();
            for (index, results) in self.embedded.iter_mut().enumerate() {
                if self.script_blocks.contains(&index) {
                    script_identifiers.extend(results.identifiers.iter_mut());
                }
            }
            let mut owned: Vec<Identifier> = script_identifiers
                .iter()
                .map(|identifier| (*identifier).clone())
                .collect();
            let rows = relationships::collect_component_rows(
                &self.base,
                &self.parsed_sfc,
                symbols,
                &self.script_symbol_ids,
                &mut owned,
            );
            for (target, updated) in script_identifiers.into_iter().zip(owned) {
                *target = updated;
            }
            self.component_rows = Some(rows);
        }
        self.component_rows
            .get_or_insert_with(ComponentRows::default)
    }

    pub fn extract_relationships(
        &mut self,
        _tree: Option<&Tree>,
        symbols: &[Symbol],
    ) -> Vec<Relationship> {
        let mut relationships: Vec<Relationship> = self
            .embedded
            .iter_mut()
            .flat_map(|results| std::mem::take(&mut results.relationships))
            .collect();
        relationships.append(&mut self.component_rows(symbols).relationships);
        relationships
    }

    pub fn extract_structured_pending_relationships(
        &mut self,
        symbols: &[Symbol],
    ) -> Vec<StructuredPendingRelationship> {
        let mut pending: Vec<StructuredPendingRelationship> = self
            .embedded
            .iter_mut()
            .flat_map(|results| std::mem::take(&mut results.structured_pending_relationships))
            .collect();
        pending.append(&mut self.component_rows(symbols).pending);
        pending
    }

    /// Infer types from Vue SFC
    pub fn infer_types(&mut self, symbols: &[Symbol]) -> HashMap<String, String> {
        let mut types = HashMap::new();
        for symbol in symbols {
            let metadata = &symbol.metadata;
            if let Some(return_type) = metadata.as_ref().and_then(|m| m.get("returnType")) {
                if let Some(type_str) = return_type
                    .as_str()
                    .filter(|type_str| !matches!(*type_str, "void" | "never"))
                {
                    types.insert(symbol.id.clone(), type_str.to_string());
                }
            } else if let Some(property_type) =
                metadata.as_ref().and_then(|m| m.get("propertyType"))
            {
                if let Some(type_str) = property_type.as_str() {
                    types.insert(symbol.id.clone(), type_str.to_string());
                }
            } else if let Some(type_val) = metadata.as_ref().and_then(|m| m.get("type"))
                && let Some(type_str) = type_val.as_str()
                && !matches!(type_str, "function" | "property" | "method")
                && !metadata
                    .as_ref()
                    .is_some_and(|m| m.contains_key("compositionApi"))
            {
                types.insert(symbol.id.clone(), type_str.to_string());
            }
        }
        types
    }

    pub fn get_type_argument_usages(&self) -> Vec<crate::base::TypeArgumentUsage> {
        self.base.get_type_argument_usages()
    }

    /// Clone captured call-argument literals.
    pub fn get_literals(&self) -> Vec<crate::base::Literal> {
        self.base.get_literals()
    }

    pub fn extract_identifiers(&mut self, symbols: &[Symbol]) -> Vec<Identifier> {
        let rows = self.component_rows(symbols);
        let mut template_identifiers = std::mem::take(&mut rows.identifiers);
        let template_literals = std::mem::take(&mut rows.literals);
        self.base.literals.extend(template_literals);

        let mut identifiers: Vec<Identifier> = self
            .embedded
            .iter_mut()
            .flat_map(|results| std::mem::take(&mut results.identifiers))
            .collect();
        identifiers.append(&mut template_identifiers);

        let containing_symbols = self.base.containing_symbol_index(symbols);
        for section in self
            .parsed_sfc
            .sections
            .iter()
            .filter(|section| section.section_type == "template")
        {
            identifiers::extract_template_attribute_literals(
                &mut self.base,
                section,
                &containing_symbols,
            );
        }
        identifiers
    }

    pub fn extract_complexity_metrics(
        &mut self,
        _symbols: &[Symbol],
    ) -> Vec<crate::base::ComplexityMetric> {
        let mut metrics: Vec<crate::base::ComplexityMetric> = self
            .embedded
            .iter_mut()
            .flat_map(|results| std::mem::take(&mut results.complexity_metrics))
            .collect();
        crate::embedded::merge_file_complexity(&mut metrics, &self.base.file_path, "vue");
        metrics
    }

    /// Source regions and parse diagnostics of the script and style blocks.
    pub(crate) fn take_embedded_regions_and_diagnostics(
        &mut self,
    ) -> (Vec<SourceRegion>, Vec<ParseDiagnostic>) {
        let mut regions = Vec::new();
        let mut diagnostics = Vec::new();
        for results in &mut self.embedded {
            regions.append(&mut results.source_regions);
            diagnostics.append(&mut results.parse_diagnostics);
        }
        (regions, diagnostics)
    }
}

/// Extract HTML comment from the beginning of a Vue file (component-level doc)
/// Looks for HTML comments at the very start of the file before any tags
fn extract_component_doc_comment(content: &str) -> Option<String> {
    let lines: Vec<&str> = content.lines().collect();
    let mut comments = Vec::new();

    for line in lines.iter() {
        let trimmed = line.trim();

        // Stop if we hit a non-comment, non-empty line (like <template>)
        if !trimmed.is_empty()
            && !trimmed.starts_with("<!--")
            && !trimmed.starts_with("-->")
            && !trimmed.starts_with("*")
        {
            break;
        }

        // Collect comment lines
        if trimmed.starts_with("<!--")
            || trimmed.starts_with("-->")
            || (trimmed.starts_with("*") && !comments.is_empty())
        {
            comments.push(*line);
        } else if trimmed.is_empty() && !comments.is_empty() {
            // Include empty lines within comment blocks
            comments.push(*line);
        }
    }

    if comments.is_empty() {
        None
    } else {
        Some(comments.join("\n"))
    }
}
