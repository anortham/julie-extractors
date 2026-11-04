// Vue Single File Component (SFC) Extractor
//
// Splits a .vue file into its top-level blocks. Script and style blocks run
// through the native JavaScript, TypeScript, or CSS pipeline in host
// coordinates; the component owns template bindings and top-level calls.

use crate::base::relationship_resolution::StructuredPendingRelationship;
use crate::base::syntax_body_hash::{HashString, HashSyntax};
use crate::base::{
    BaseExtractor, ExtractionResults, Identifier, ParseDiagnostic, Relationship, SourceRegion,
    Symbol, SymbolKind,
};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use tree_sitter::{Node, Tree};

mod component;
mod identifiers;
mod macros;
mod manual_symbols;
pub(crate) mod parsing;
mod relationships;
mod script;
mod script_setup;
mod template;

use manual_symbols::create_symbol_manual;
use parsing::ParsedVueSfc;
use relationships::ComponentRows;
pub(crate) use script::is_plain_css as is_plain_css_style;

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

    pub fn extract_symbols(&mut self, tree: Option<&Tree>) -> Vec<Symbol> {
        self.embedded.clear();
        self.script_blocks.clear();
        self.script_symbol_ids.clear();
        self.component_rows = None;
        let host_tree = self.parsed_sfc.host_tree(tree, &self.base.content);
        self.base
            .init_body_hash_syntax(Self::collect_vue_hash_syntax(
                &self.parsed_sfc,
                &self.base.content,
                host_tree.as_ref(),
            ));
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
                    let Some(tree) = self.parsed_sfc.style_tree(idx) else {
                        continue;
                    };
                    if let Some((section_symbols, results)) =
                        script::extract_style(&self.base, section, tree)
                    {
                        symbols.extend(section_symbols);
                        self.embedded.push(results);
                    }
                }
                _ => {}
            }
        }
        let script_bindings: HashSet<String> = symbols
            .iter()
            .filter(|symbol| symbol.parent_id.is_none())
            .filter(|symbol| self.script_symbol_ids.contains(&symbol.id))
            .map(|symbol| symbol.name.clone())
            .collect();
        symbols.retain(|symbol| {
            !(template::declares_script_binding(symbol) && script_bindings.contains(&symbol.name))
        });
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
            symbols.extend(self.component_symbol(&component_name));
        }

        script_setup::apply_script_setup_annotations(&mut symbols, &self.parsed_sfc);

        symbols
    }

    /// The component spans the whole file, which is also its body.
    fn component_symbol(&self, component_name: &str) -> Option<Symbol> {
        let end = self.base.content.trim_end().len();
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
        let mut symbol = create_symbol_manual(
            &self.base,
            component_name,
            SymbolKind::Class,
            0,
            end,
            Some(format!("<{} />", component_name)),
            extract_component_doc_comment(&self.base.content),
            Some(metadata),
        )?;
        let body = self.base.span_for_byte_range(0, end);
        self.base.set_body_span(&mut symbol, body);
        Some(symbol)
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

    fn collect_vue_hash_syntax(
        parsed_sfc: &ParsedVueSfc,
        content: &str,
        host_tree: Option<&Tree>,
    ) -> HashSyntax {
        let mut syntax = HashSyntax::default();

        if let Some(tree) = host_tree {
            Self::collect_markup_syntax(
                tree.root_node(),
                content,
                &parsed_sfc.sections,
                &mut syntax,
            );
        }

        for (index, section) in parsed_sfc.sections.iter().enumerate() {
            match section.section_type.as_str() {
                "script" => {
                    let Some(tree) = parsed_sfc.script_tree(index) else {
                        continue;
                    };
                    let section_syntax = crate::base::body_hash_syntax::collect_hash_syntax(
                        tree.root_node(),
                        &section.content,
                        parsing::script_language(section.lang.as_deref()),
                    );
                    Self::append_shifted_hash_syntax(
                        &mut syntax,
                        section_syntax,
                        section.content_start,
                    );
                }
                "style" => {
                    let Some(tree) = parsed_sfc.style_tree(index) else {
                        continue;
                    };
                    Self::collect_css_comments(
                        tree.root_node(),
                        section.content_start,
                        &mut syntax.comments,
                    );
                }
                _ => {}
            }
        }

        syntax
    }

    fn append_shifted_hash_syntax(target: &mut HashSyntax, source: HashSyntax, offset: usize) {
        target.comments.extend(
            source
                .comments
                .into_iter()
                .map(|range| Self::shift_range(range, offset)),
        );
        target
            .strings
            .extend(source.strings.into_iter().map(|string| {
                HashString {
                    range: Self::shift_range(string.range, offset),
                    token_start: string.token_start + offset,
                    interpolated: string.interpolated,
                    code: string
                        .code
                        .into_iter()
                        .map(|range| Self::shift_range(range, offset))
                        .collect(),
                }
            }));
    }

    fn shift_range(range: Range<usize>, offset: usize) -> Range<usize> {
        range.start + offset..range.end + offset
    }

    fn collect_markup_syntax(
        root: Node<'_>,
        content: &str,
        sections: &[parsing::VueSection],
        syntax: &mut HashSyntax,
    ) {
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            let range = node.byte_range();
            if node.kind() == "comment" {
                let belongs_to_code_section = sections.iter().any(|section| {
                    section.section_type != "template"
                        && range.start >= section.content_start
                        && range.end <= section.content_start + section.content.len()
                });
                if !belongs_to_code_section && content.get(range.clone()).is_some() {
                    syntax.comments.push(range);
                }
                continue;
            }
            if node.kind() == "text"
                && sections.iter().any(|section| {
                    section.section_type == "template"
                        && range.start >= section.content_start
                        && range.end <= section.content_start + section.content.len()
                })
                && content.get(range.clone()).is_some()
            {
                syntax.strings.push(HashString {
                    token_start: range.start,
                    range,
                    interpolated: false,
                    code: Vec::new(),
                });
                continue;
            }

            let mut cursor = node.walk();
            stack.extend(node.children(&mut cursor));
        }
    }

    fn collect_css_comments(root: Node<'_>, offset: usize, comments: &mut Vec<Range<usize>>) {
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            if node.kind() == "comment" {
                comments.push(Self::shift_range(node.byte_range(), offset));
                continue;
            }

            let mut cursor = node.walk();
            stack.extend(node.children(&mut cursor));
        }
    }

    /// Declared types come from the script blocks; component, template, and
    /// style symbols have none.
    pub fn infer_types(&mut self, _symbols: &[Symbol]) -> HashMap<String, String> {
        HashMap::new()
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

    /// `code.marker.v1` facts from the comments of the script blocks.
    pub(crate) fn take_embedded_marker_facts(&mut self) -> Vec<crate::base::StructuralFact> {
        self.embedded
            .iter_mut()
            .flat_map(|results| std::mem::take(&mut results.structural_facts))
            .collect()
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
