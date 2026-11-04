use crate::base::BaseExtractor;
use crate::base::syntax_body_hash::{HashString, HashSyntax};
use std::collections::HashMap;
use std::ops::Range;
use tree_sitter::{Node, Tree};

#[derive(Hash, PartialEq, Eq)]
struct EmbeddedKey {
    start: usize,
    end: usize,
    language: &'static str,
}

#[derive(Default)]
pub(super) struct EmbeddedBlocks {
    blocks: HashMap<EmbeddedKey, Option<Tree>>,
    syntax: HashSyntax,
    has_mocha_script: bool,
    has_bdd_setup: bool,
    #[cfg(test)]
    parse_calls: usize,
    #[cfg(test)]
    tree_reuses: usize,
    #[cfg(test)]
    node_visits: usize,
}

impl EmbeddedBlocks {
    pub(super) fn prepare(base: &BaseExtractor, root: Node<'_>) -> Self {
        let mut prepared = Self::default();
        let mut nodes = vec![root];

        while let Some(node) = nodes.pop() {
            #[cfg(test)]
            {
                prepared.node_visits += 1;
            }

            match node.kind() {
                "comment" => {
                    prepared.syntax.comments.push(node.byte_range());
                    continue;
                }
                "script_element" => {
                    let attributes = super::helpers::HTMLHelpers::extract_attributes(base, node);
                    if super::scripts::is_javascript_script_type(&attributes) {
                        if attributes
                            .get("src")
                            .is_some_and(|src| super::scripts::is_mocha_script_source(src))
                        {
                            prepared.has_mocha_script = true;
                        }

                        let has_src = attributes
                            .keys()
                            .any(|name| name.eq_ignore_ascii_case("src"));
                        if !has_src
                            && let Some(raw_text) = raw_text_node(node)
                            && prepared.prepare_block(
                                base,
                                raw_text,
                                super::scripts::script_block_language(&attributes),
                                true,
                            )
                        {
                            continue;
                        }
                    }
                }
                "style_element" => {
                    if let Some(raw_text) = raw_text_node(node)
                        && prepared.prepare_block(base, raw_text, "css", false)
                    {
                        continue;
                    }
                }
                "text" | "raw_text" => {
                    prepared.add_opaque_text(base, node);
                    continue;
                }
                _ => {}
            }

            let mut cursor = node.walk();
            nodes.extend(node.children(&mut cursor));
        }

        prepared
    }

    fn prepare_block(
        &mut self,
        base: &BaseExtractor,
        raw_text: Node<'_>,
        language: &'static str,
        detect_bdd_setup: bool,
    ) -> bool {
        let range = raw_text.byte_range();
        let key = embedded_key(&range, language);
        let Some(source) = base.content.get(range.clone()) else {
            self.blocks.insert(key, None);
            return false;
        };
        if source.trim().is_empty() {
            self.blocks.insert(key, None);
            return false;
        }

        let tree = crate::pipeline::configured_parser_for_language(language)
            .ok()
            .and_then(|mut parser| {
                #[cfg(test)]
                {
                    self.parse_calls += 1;
                }
                parser.parse(source, None)
            });

        let parsed = tree.is_some();
        if let Some(tree) = &tree {
            self.append_embedded_syntax(tree, source, range.start, language);
            if detect_bdd_setup && super::scripts::contains_mocha_bdd_setup(source, tree) {
                self.has_bdd_setup = true;
            }
        }

        self.blocks.insert(key, tree);
        parsed
    }

    fn append_embedded_syntax(
        &mut self,
        tree: &Tree,
        source: &str,
        host_byte_offset: usize,
        language: &'static str,
    ) {
        let syntax = if language == "css" {
            collect_css_comments(tree.root_node())
        } else {
            crate::base::body_hash_syntax::collect_hash_syntax(tree.root_node(), source, language)
        };

        self.syntax.comments.extend(
            syntax
                .comments
                .into_iter()
                .map(|range| shift_range(range, host_byte_offset)),
        );
        self.syntax
            .strings
            .extend(syntax.strings.into_iter().map(|mut string| {
                string.range = shift_range(string.range, host_byte_offset);
                string.token_start += host_byte_offset;
                string.code = string
                    .code
                    .into_iter()
                    .map(|range| shift_range(range, host_byte_offset))
                    .collect();
                string
            }));
    }

    fn add_opaque_text(&mut self, base: &BaseExtractor, node: Node<'_>) {
        let range = node.byte_range();
        let Some(source) = base.content.get(range.clone()) else {
            return;
        };
        if range.is_empty()
            || (node.kind() == "text"
                && !source
                    .bytes()
                    .any(|byte| matches!(byte, b'\'' | b'"' | b'`')))
        {
            return;
        }

        self.syntax.strings.push(HashString {
            token_start: range.start,
            range,
            interpolated: false,
            code: Vec::new(),
        });
    }

    pub(super) fn source_and_tree<'a>(
        &'a mut self,
        base: &'a BaseExtractor,
        node: Node<'_>,
        language: &'static str,
    ) -> Option<(&'a str, &'a Tree, usize)> {
        let raw_text = raw_text_node(node)?;
        let range = raw_text.byte_range();
        let source = base.content.get(range.clone())?;
        if source.trim().is_empty() {
            return None;
        }
        let key = embedded_key(&range, language);
        if !self.blocks.contains_key(&key) {
            return None;
        }

        let tree = self.blocks.get(&key)?.as_ref()?;
        #[cfg(test)]
        {
            self.tree_reuses += 1;
        }
        Some((source, tree, range.start))
    }

    pub(super) fn take_syntax(&mut self) -> HashSyntax {
        std::mem::take(&mut self.syntax)
    }

    pub(super) fn mocha_bdd_contract(&self) -> bool {
        self.has_mocha_script && self.has_bdd_setup
    }

    pub(super) fn release_trees(&mut self) {
        self.blocks.clear();
    }
}

fn embedded_key(range: &Range<usize>, language: &'static str) -> EmbeddedKey {
    EmbeddedKey {
        start: range.start,
        end: range.end,
        language,
    }
}

fn raw_text_node(node: Node<'_>) -> Option<Node<'_>> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find(|child| matches!(child.kind(), "text" | "raw_text"))
}

fn shift_range(mut range: Range<usize>, offset: usize) -> Range<usize> {
    range.start += offset;
    range.end += offset;
    range
}

fn collect_css_comments(root: Node<'_>) -> HashSyntax {
    let mut syntax = HashSyntax::default();
    let mut nodes = vec![root];

    while let Some(node) = nodes.pop() {
        if node.kind() == "comment" {
            syntax.comments.push(node.byte_range());
            continue;
        }

        let mut cursor = node.walk();
        nodes.extend(node.children(&mut cursor));
    }

    syntax
}

#[cfg(test)]
mod tests {
    use super::EmbeddedBlocks;
    use crate::base::BaseExtractor;
    use crate::html::HTMLExtractor;
    use std::path::Path;
    use tree_sitter::{Parser, Tree};

    fn parse_html(source: &str) -> Tree {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_html::LANGUAGE.into())
            .expect("HTML grammar");
        parser.parse(source, None).expect("HTML tree")
    }

    fn extraction(source: &str) -> HTMLExtractor {
        let tree = parse_html(source);
        let mut extractor = HTMLExtractor::new(
            "html".to_string(),
            "page.html".to_string(),
            source.to_string(),
            Path::new("/repo"),
        );
        extractor.extract_symbols(&tree);
        extractor
    }

    fn host_visits(source: &str) -> usize {
        let tree = parse_html(source);
        let base = BaseExtractor::new(
            "html".to_string(),
            "page.html".to_string(),
            source.to_string(),
            Path::new("/repo"),
        );
        EmbeddedBlocks::prepare(&base, tree.root_node()).node_visits
    }

    #[test]
    fn inline_native_blocks_are_parsed_once_and_extracted_from_cached_trees() {
        let source = r#"<div><script src="mocha.js"></script><script SRC="remote.js">function external() {}</script><script type="application/ld+json">{"key":"value"}</script><script>mocha.setup('bdd'); describe('suite', () => { it('works', () => {}); });</script><style>.card { color: red; }</style></div>"#;
        let extractor = extraction(source);

        assert_eq!(extractor.embedded_blocks.parse_calls, 2);
        assert_eq!(extractor.embedded_blocks.tree_reuses, 2);
        assert!(extractor.embedded_blocks.mocha_bdd_contract());
    }

    #[test]
    fn flat_and_nested_host_walks_grow_with_the_tree() {
        let flat_small = format!("<main>{}</main>", "<span>x</span>".repeat(32));
        let flat_large = format!("<main>{}</main>", "<span>x</span>".repeat(64));
        let nested_small = format!("{}x{}", "<div>".repeat(32), "</div>".repeat(32));
        let nested_large = format!("{}x{}", "<div>".repeat(64), "</div>".repeat(64));

        let flat_small_visits = host_visits(&flat_small);
        let flat_large_visits = host_visits(&flat_large);
        let nested_small_visits = host_visits(&nested_small);
        let nested_large_visits = host_visits(&nested_large);

        assert!(flat_large_visits > flat_small_visits);
        assert!(flat_large_visits <= flat_small_visits * 3);
        assert!(nested_large_visits > nested_small_visits);
        assert!(nested_large_visits <= nested_small_visits * 3);
    }
}
