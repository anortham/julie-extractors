// Vue SFC (Single File Component) parsing module
//
// Responsible for parsing .vue file structure and extracting template, script, and style sections

use super::helpers::{
    LANG_ATTR_RE, SCRIPT_START_RE, SECTION_END_RE, STYLE_START_RE, TEMPLATE_START_RE,
};
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

/// Represents a section within a Vue SFC file (template, script, or style)
#[derive(Debug, Clone)]
pub(crate) struct VueSection {
    pub(crate) section_type: String, // "template", "script", "style"
    pub(crate) content: String,
    pub(crate) start_line: usize,
    #[allow(dead_code)]
    pub(crate) end_line: usize,
    pub(crate) lang: Option<String>, // e.g., 'ts', 'scss'
    pub(crate) is_setup: bool,       // true for <script setup>
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

/// Helper struct for building VueSection during parsing
#[derive(Debug)]
pub(crate) struct VueSectionBuilder {
    pub(crate) section_type: String,
    pub(crate) start_line: usize,
    pub(crate) lang: Option<String>,
    pub(crate) is_setup: bool,
}

impl VueSectionBuilder {
    pub(crate) fn build(self, content: String, end_line: usize) -> VueSection {
        VueSection {
            section_type: self.section_type,
            content,
            start_line: self.start_line,
            end_line,
            lang: self.lang,
            is_setup: self.is_setup,
        }
    }
}

/// Parse a Vue `<script>` or `<script setup>` section using JavaScript or TypeScript tree-sitter.
pub(crate) fn parse_script_section(section: &VueSection) -> Option<Tree> {
    #[cfg(test)]
    SCRIPT_PARSE_COUNT.with(|c| c.set(c.get() + 1));

    let mut parser = tree_sitter::Parser::new();
    let lang = section.lang.as_deref().unwrap_or("js");
    let tree_sitter_lang = if lang == "ts" || lang == "typescript" {
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
    } else {
        tree_sitter_javascript::LANGUAGE.into()
    };

    parser.set_language(&tree_sitter_lang).ok()?;
    parser.parse(&section.content, None)
}

/// Parsed Vue SFC structure holding extracted sections and lazily parsed `Tree`s per script section.
#[derive(Debug)]
pub(crate) struct ParsedVueSfc {
    pub(crate) sections: Vec<VueSection>,
    script_trees: Vec<OnceLock<Option<Tree>>>,
}

impl Default for ParsedVueSfc {
    fn default() -> Self {
        Self {
            sections: Vec::new(),
            script_trees: Vec::new(),
        }
    }
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
/// Implementation of parseVueSFC logic
#[allow(dead_code)]
pub(crate) fn parse_vue_sfc(content: &str) -> Result<Vec<VueSection>, Box<dyn std::error::Error>> {
    ParsedVueSfc::parse(content).map(|sfc| sfc.sections)
}

fn parse_vue_sfc_sections(content: &str) -> Result<Vec<VueSection>, Box<dyn std::error::Error>> {
    let mut sections = Vec::new();
    let lines: Vec<&str> = content.lines().collect();

    let mut current_section: Option<VueSectionBuilder> = None;
    let mut section_content = Vec::new();

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();

        // Check for section start - following regex patterns
        let template_match = TEMPLATE_START_RE.captures(trimmed);
        let script_match = SCRIPT_START_RE.captures(trimmed);
        let style_match = STYLE_START_RE.captures(trimmed);

        if template_match.is_some() || script_match.is_some() || style_match.is_some() {
            // End previous section
            if let Some(section) = current_section.take() {
                sections.push(section.build(section_content.join("\n"), i));
            }

            // Start new section
            let section_type = if template_match.is_some() {
                "template"
            } else if script_match.is_some() {
                "script"
            } else {
                "style"
            };

            let attrs = template_match
                .or(script_match)
                .or(style_match)
                .and_then(|m| m.get(1))
                .map(|m| m.as_str())
                .unwrap_or("");

            let lang = LANG_ATTR_RE
                .captures(attrs)
                .and_then(|m| m.get(1))
                .map(|m| m.as_str().to_string())
                .unwrap_or_else(|| match section_type {
                    "script" => "js".to_string(),
                    "style" => "css".to_string(),
                    _ => "html".to_string(),
                });

            // Detect <script setup> attribute
            let is_setup =
                section_type == "script" && attrs.split_whitespace().any(|a| a == "setup");

            current_section = Some(VueSectionBuilder {
                section_type: section_type.to_string(),
                start_line: i + 1,
                lang: Some(lang),
                is_setup,
            });
            section_content.clear();
            continue;
        }

        // Check for section end
        if SECTION_END_RE.is_match(trimmed) {
            if let Some(section) = current_section.take() {
                sections.push(section.build(section_content.join("\n"), i));
                section_content.clear();
            }
            continue;
        }

        // Add content to current section
        if current_section.is_some() {
            section_content.push(line.to_string());
        }
    }

    // Handle unclosed section - following reference logic
    if let Some(section) = current_section {
        sections.push(section.build(section_content.join("\n"), lines.len()));
    }

    Ok(sections)
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

        // First access to script tree triggers parse
        let tree1 = sfc.script_tree(1);
        assert!(tree1.is_some());
        assert_eq!(
            get_script_parse_count(),
            1,
            "Script parsed once on first access"
        );

        // Second access reuses the cached tree
        let tree2 = sfc.script_tree(1);
        assert!(tree2.is_some());
        assert_eq!(
            get_script_parse_count(),
            1,
            "Script not reparsed on second access"
        );

        // Access via script_tree_for_section reuses cached tree
        let script_sec = &sfc.sections()[1];
        let tree3 = sfc.script_tree_for_section(script_sec);
        assert!(tree3.is_some());
        assert_eq!(
            get_script_parse_count(),
            1,
            "Script not reparsed when accessed by section reference"
        );

        // Non-script sections return None without incrementing parse count
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
}
