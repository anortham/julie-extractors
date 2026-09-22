// Vue SFC (Single File Component) parsing module
//
// Responsible for parsing .vue file structure and extracting template, script, and style sections

use std::fmt;
use std::sync::OnceLock;
use tree_sitter::Tree;

#[cfg(test)]
thread_local! {
    static SCRIPT_PARSE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn reset_script_parse_count() {
    SCRIPT_PARSE_COUNT.with(|c| c.set(0));
}

#[cfg(test)]
pub(crate) fn get_script_parse_count() -> usize {
    SCRIPT_PARSE_COUNT.with(|c| c.get())
}

/// A top-level block of a Vue SFC (template, script, or style).
#[derive(Debug, Clone)]
pub(crate) struct VueSection {
    pub(crate) section_type: String,
    /// The exact block text, from the byte after the opening tag's `>`.
    pub(crate) content: String,
    /// Host byte offset of `content[0]`.
    pub(crate) content_start: usize,
    /// 1-based host line of `content_start`.
    pub(crate) start_line: usize,
    pub(crate) lang: Option<String>,
    pub(crate) is_setup: bool,
}

impl fmt::Display for VueSection {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}@{}{}",
            self.section_type,
            self.start_line,
            self.lang
                .as_deref()
                .map(|l| format!("({})", l))
                .unwrap_or_default()
        )
    }
}

/// The pipeline language for a script block's `lang`.
pub(crate) fn script_language(lang: Option<&str>) -> &'static str {
    match lang.unwrap_or("js") {
        "ts" | "typescript" => "typescript",
        "tsx" => "tsx",
        "jsx" => "jsx",
        _ => "javascript",
    }
}

/// Parse a Vue `<script>` or `<script setup>` section with the grammar its `lang` names.
pub(crate) fn parse_script_section(section: &VueSection) -> Option<Tree> {
    #[cfg(test)]
    SCRIPT_PARSE_COUNT.with(|c| c.set(c.get() + 1));

    let mut parser =
        crate::pipeline::configured_parser_for_language(script_language(section.lang.as_deref()))
            .ok()?;
    parser.parse(&section.content, None)
}

/// Parsed Vue SFC structure holding extracted sections and lazily parsed `Tree`s per script section.
#[derive(Debug, Default)]
pub(crate) struct ParsedVueSfc {
    pub(crate) sections: Vec<VueSection>,
    script_trees: Vec<OnceLock<Option<Tree>>>,
}

impl Clone for ParsedVueSfc {
    fn clone(&self) -> Self {
        let script_trees = self
            .script_trees
            .iter()
            .map(|cell| {
                let new_cell = OnceLock::new();
                if let Some(tree_opt) = cell.get() {
                    let _ = new_cell.set(tree_opt.clone());
                }
                new_cell
            })
            .collect();
        Self {
            sections: self.sections.clone(),
            script_trees,
        }
    }
}

impl std::ops::Deref for ParsedVueSfc {
    type Target = [VueSection];

    fn deref(&self) -> &Self::Target {
        &self.sections
    }
}

impl ParsedVueSfc {
    /// Parse Vue SFC structure from content string slice without cloning.
    pub(crate) fn parse(content: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let sections = parse_vue_sfc_sections(content)?;
        let script_trees = (0..sections.len()).map(|_| OnceLock::new()).collect();
        Ok(Self {
            sections,
            script_trees,
        })
    }

    /// Returns a reference to the parsed sections.
    #[allow(dead_code)]
    pub(crate) fn sections(&self) -> &[VueSection] {
        &self.sections
    }

    /// Retrieve or lazily parse the tree-sitter `Tree` for a script section by section index.
    pub(crate) fn script_tree(&self, section_index: usize) -> Option<&Tree> {
        let cell = self.script_trees.get(section_index)?;
        cell.get_or_init(|| {
            let section = self.sections.get(section_index)?;
            if section.section_type == "script" {
                parse_script_section(section)
            } else {
                None
            }
        })
        .as_ref()
    }

    /// Retrieve or lazily parse the tree-sitter `Tree` for a specific `VueSection`.
    #[allow(dead_code)]
    pub(crate) fn script_tree_for_section(&self, section: &VueSection) -> Option<&Tree> {
        let idx = self.sections.iter().position(|s| {
            std::ptr::eq(s, section)
                || (s.start_line == section.start_line
                    && s.section_type == section.section_type
                    && s.is_setup == section.is_setup)
        })?;
        self.script_tree(idx)
    }
}

/// Parse Vue SFC structure to extract template, script, and style sections
#[allow(dead_code)]
pub(crate) fn parse_vue_sfc(content: &str) -> Result<Vec<VueSection>, Box<dyn std::error::Error>> {
    ParsedVueSfc::parse(content).map(|sfc| sfc.sections)
}

fn parse_vue_sfc_sections(content: &str) -> Result<Vec<VueSection>, Box<dyn std::error::Error>> {
    Ok(
        crate::base::web_structural_facts::vue_section_ranges(content)
            .into_iter()
            .filter_map(|range| {
                let text = content.get(range.content_start..range.content_end)?;
                let lang = range.lang.unwrap_or_else(|| {
                    match range.section_type {
                        "script" => "js",
                        "style" => "css",
                        _ => "html",
                    }
                    .to_string()
                });
                Some(VueSection {
                    section_type: range.section_type.to_string(),
                    content: text.to_string(),
                    content_start: range.content_start,
                    start_line: content[..range.content_start].matches('\n').count() + 1,
                    lang: Some(lang),
                    is_setup: range.section_type == "script" && range.setup,
                })
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_script_section_parsed_only_once() {
        reset_script_parse_count();
        let sfc_content = r#"<template>
  <div>{{ msg }}</div>
</template>

<script setup lang="ts">
import { ref } from 'vue';
const msg = ref('hello');
function greet() {
  return msg.value;
}
</script>

<style scoped>
div { color: red; }
</style>"#;

        let sfc = ParsedVueSfc::parse(sfc_content).expect("SFC parse failed");
        assert_eq!(sfc.sections().len(), 3);
        assert_eq!(
            get_script_parse_count(),
            0,
            "No script parse upon initial SFC split"
        );

        let tree1 = sfc.script_tree(1);
        assert!(tree1.is_some());
        assert_eq!(
            get_script_parse_count(),
            1,
            "Script parsed once on first access"
        );

        let tree2 = sfc.script_tree(1);
        assert!(tree2.is_some());
        assert_eq!(
            get_script_parse_count(),
            1,
            "Script not reparsed on second access"
        );

        let script_sec = &sfc.sections()[1];
        let tree3 = sfc.script_tree_for_section(script_sec);
        assert!(tree3.is_some());
        assert_eq!(
            get_script_parse_count(),
            1,
            "Script not reparsed when accessed by section reference"
        );

        assert!(sfc.script_tree(0).is_none());
        assert!(sfc.script_tree(2).is_none());
        assert_eq!(get_script_parse_count(), 1);
    }

    #[test]
    fn test_vue_extractor_parses_script_section_only_once() {
        use crate::vue::VueExtractor;
        use std::path::Path;

        reset_script_parse_count();
        let sfc_content = r#"<template>
  <div>{{ count }}</div>
</template>

<script setup lang="ts">
import { ref } from 'vue';
const count = ref(0);
function increment() {
  count.value++;
}
</script>"#;

        let mut extractor = VueExtractor::new(
            "vue".to_string(),
            "Counter.vue".to_string(),
            sfc_content.to_string(),
            Path::new(""),
        );

        let symbols = extractor.extract_symbols(None);
        assert!(!symbols.is_empty());
        assert_eq!(
            get_script_parse_count(),
            1,
            "symbols extraction should parse script section once"
        );

        let _rels = extractor.extract_relationships(None, &symbols);
        assert_eq!(
            get_script_parse_count(),
            1,
            "relationships extraction should reuse cached tree"
        );

        let _idents = extractor.extract_identifiers(&symbols);
        assert_eq!(
            get_script_parse_count(),
            1,
            "identifiers extraction should reuse cached tree"
        );

        let _pending = extractor.extract_structured_pending_relationships(&symbols);
        assert_eq!(
            get_script_parse_count(),
            1,
            "structured pending extraction should reuse cached tree"
        );

        let _complexity = extractor.extract_complexity_metrics(&symbols);
        assert_eq!(
            get_script_parse_count(),
            1,
            "complexity extraction should reuse cached tree"
        );
    }

    #[test]
    fn test_canonical_vue_pipeline_parses_script_section_only_once() {
        use crate::{ExtractionLevel, extract_canonical_at};
        use std::path::Path;

        reset_script_parse_count();
        let sfc_content = r#"<template>
  <div>{{ count }}</div>
</template>

<script setup lang="ts">
import { ref } from 'vue';
const count = ref(0);
function increment() {
  count.value++;
}
</script>"#;

        let results = extract_canonical_at(
            "Counter.vue",
            sfc_content,
            Path::new("."),
            ExtractionLevel::Full,
        )
        .unwrap();

        assert!(!results.symbols.is_empty());
        assert!(!results.complexity_metrics.is_empty());
        assert_eq!(get_script_parse_count(), 1);
    }
}
