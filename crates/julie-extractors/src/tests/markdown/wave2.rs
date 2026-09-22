use crate::base::{ExtractionResults, RelationshipKind, SourceRegionKind, Symbol, SymbolKind};
use crate::tests::helpers::{facts_with_pattern, metadata_str};
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::Path;

fn extract(file_path: &str, source: &str) -> ExtractionResults {
    crate::pipeline::extract_canonical(file_path, source, Path::new("/repo"))
        .expect("canonical Markdown extraction should succeed")
}

fn headings(results: &ExtractionResults) -> Vec<&str> {
    results
        .symbols
        .iter()
        .filter(|symbol| symbol.kind == SymbolKind::Module)
        .map(|symbol| symbol.name.as_str())
        .collect()
}

fn name_of<'a>(results: &'a ExtractionResults, id: &str) -> &'a str {
    results
        .symbols
        .iter()
        .find(|symbol| symbol.id == id)
        .map_or("?", |symbol| symbol.name.as_str())
}

fn references(results: &ExtractionResults) -> BTreeSet<(String, String, String)> {
    results
        .relationships
        .iter()
        .filter(|relationship| relationship.kind == RelationshipKind::References)
        .map(|relationship| {
            let metadata = relationship.metadata.as_ref().unwrap();
            let key = metadata
                .get("anchor")
                .or_else(|| metadata.get("reference_label"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            (
                name_of(results, &relationship.from_symbol_id).to_string(),
                name_of(results, &relationship.to_symbol_id).to_string(),
                key.to_string(),
            )
        })
        .collect()
}

fn metadata<'a>(symbol: &'a Symbol, key: &str) -> Option<&'a str> {
    symbol.metadata.as_ref()?.get(key)?.as_str()
}

fn edge(from: &str, to: &str, key: &str) -> (String, String, String) {
    (from.to_string(), to.to_string(), key.to_string())
}

#[test]
fn heading_names_drop_markup_closing_hashes_and_attribute_blocks() {
    let results = extract(
        "headings.md",
        "# Index\n\n## [Changelog](CHANGELOG.md)\n\n## API Reference ##\n\n## Configuration {#custom-id}\n\n## FAQ #\n\n## Comparison with the [`glob`](https://github.com/rust-lang/glob) crate\n\n## The **`useState`** hook\n\n## C#\n\n#\n\n> ## Quoted heading\n",
    );

    assert_eq!(
        headings(&results),
        vec![
            "Index",
            "Changelog",
            "API Reference",
            "Configuration",
            "FAQ",
            "Comparison with the glob crate",
            "The useState hook",
            "C#",
        ]
    );
    let configuration = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "Configuration")
        .unwrap();
    assert_eq!(metadata(configuration, "anchor"), Some("custom-id"));
    let texts: Vec<_> = facts_with_pattern(&results, "markdown.heading.v1")
        .into_iter()
        .filter_map(|fact| metadata_str(fact, "text"))
        .collect();
    assert_eq!(texts[..texts.len() - 1], headings(&results));
    assert_eq!(texts.last(), Some(&"Quoted heading"));
}

#[test]
fn anchors_follow_github_slugs_with_unicode_duplicates_and_explicit_ids() {
    let results = extract(
        "slugs.md",
        "See [install](#安装), [usage](#使用), [second options](#options-1), [cfg](#config-section), and [glob](#comparison-with-the-glob-crate).\n\n# Guide\n\n## 安装\n\nInstall.\n\n## 使用\n\nUsage.\n\n## Options\n\nA.\n\n## Options\n\nB.\n\n## Configuration {#config-section}\n\n## Comparison with the [`glob`](https://github.com/rust-lang/glob) crate\n",
    );
    let options: Vec<_> = results
        .symbols
        .iter()
        .filter(|symbol| symbol.name == "Options")
        .collect();
    let second_options = options[1].id.clone();

    let edges = references(&results);
    for expected in [
        edge("install", "安装", "安装"),
        edge("usage", "使用", "使用"),
        edge("cfg", "Configuration", "config-section"),
        edge(
            "glob",
            "Comparison with the glob crate",
            "comparison-with-the-glob-crate",
        ),
    ] {
        assert!(edges.contains(&expected), "{expected:?} in {edges:?}");
    }
    let duplicate = results
        .relationships
        .iter()
        .find(|relationship| relationship.metadata.as_ref().unwrap()["anchor"] == "options-1")
        .expect("options-1 edge");
    assert_eq!(duplicate.to_symbol_id, second_options);
}

#[test]
fn reference_links_and_footnotes_reference_their_definitions() {
    let results = extract(
        "refs.md",
        "# Setup\n\nRead the [install guide][install], then [config][] and [faq]. Stable[^stability].\n\n## Links\n\n[install]: ./docs/install.md\n[config]: ./docs/config.md\n[faq]: ./docs/faq.md\n\n[^stability]: Since version 2.0.\n",
    );

    assert_eq!(
        references(&results),
        BTreeSet::from([
            edge("Setup", "install", "install"),
            edge("Setup", "config", "config"),
            edge("Setup", "faq", "faq"),
            edge("Setup", "stability", "^stability"),
        ])
    );
    let kinds: Vec<_> = facts_with_pattern(&results, "markdown.reference_link.v1")
        .into_iter()
        .map(|fact| {
            (
                metadata_str(fact, "label"),
                metadata_str(fact, "reference_kind"),
                metadata_str(fact, "destination"),
            )
        })
        .collect();
    assert_eq!(
        kinds,
        vec![
            (Some("install"), Some("full"), Some("./docs/install.md")),
            (Some("config"), Some("collapsed"), Some("./docs/config.md")),
            (Some("faq"), Some("shortcut"), Some("./docs/faq.md")),
        ]
    );
}

#[test]
fn footnotes_have_one_definition_each_and_their_own_facts() {
    let results = extract(
        "notes.md",
        "# Notes\n\nClaim one[^src] and claim two[^2].\n\n[^src]: https://example.com/source\n[^2]: Plain sentence footnote.\n",
    );
    let definitions: Vec<_> = results
        .symbols
        .iter()
        .filter(|symbol| metadata(symbol, "markdown_kind") == Some("footnote_definition"))
        .map(|symbol| symbol.name.as_str())
        .collect();

    assert_eq!(definitions, vec!["src", "2"]);
    assert!(facts_with_pattern(&results, "markdown.link_definition.v1").is_empty());
    let facts: Vec<_> = facts_with_pattern(&results, "markdown.footnote_definition.v1")
        .into_iter()
        .map(|fact| (metadata_str(fact, "label"), metadata_str(fact, "text")))
        .collect();
    assert_eq!(
        facts,
        vec![
            (Some("src"), Some("https://example.com/source")),
            (Some("2"), Some("Plain sentence footnote.")),
        ]
    );
    assert_eq!(
        facts_with_pattern(&results, "markdown.footnote_reference.v1").len(),
        2
    );
}

#[test]
fn fence_languages_come_from_the_grammar_language_token() {
    let results = extract(
        "fences.md",
        "```rust,no_run\nfn main() {}\n```\n\n```{python}\nprint(1)\n```\n\n```{r echo=FALSE}\nsummary(cars)\n```\n\n```{.python}\nprint(2)\n```\n",
    );
    let languages: Vec<_> = results
        .symbols
        .iter()
        .filter_map(|symbol| metadata(symbol, "language"))
        .collect();
    let regions: Vec<_> = results
        .source_regions
        .iter()
        .filter_map(|region| region.metadata.as_ref()?.get("embedded_language")?.as_str())
        .collect();
    let facts: Vec<_> = facts_with_pattern(&results, "markdown.fenced_code_block.v1")
        .into_iter()
        .filter_map(|fact| metadata_str(fact, "language"))
        .collect();

    assert_eq!(languages, vec!["rust", "python", "r", "python"]);
    assert_eq!(regions, languages);
    assert_eq!(facts, languages);
    assert_eq!(results.symbols[0].name, "rust code block");
    assert_eq!(
        metadata(&results.symbols[0], "info_string"),
        Some("rust,no_run")
    );
}

#[test]
fn frontmatter_keys_are_child_properties_and_the_body_is_an_embedded_region() {
    let source = "---\ntitle: Getting Started\nslug: /getting-started\ntags: [intro, setup]\nauthors:\n  - ada\n---\n\nIntro text.\n";
    let results = extract("guide.md", source);
    let frontmatter = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "frontmatter")
        .unwrap();

    assert_eq!(metadata(frontmatter, "markdown_kind"), Some("frontmatter"));
    let keys: Vec<_> = results
        .symbols
        .iter()
        .filter(|symbol| symbol.parent_id.as_deref() == Some(frontmatter.id.as_str()))
        .map(|symbol| (symbol.name.as_str(), metadata(symbol, "value")))
        .collect();
    assert_eq!(
        keys,
        vec![
            ("title", Some("Getting Started")),
            ("slug", Some("/getting-started")),
            ("tags", Some("[intro, setup]")),
            ("authors", None),
        ]
    );
    let region = results
        .source_regions
        .iter()
        .find(|region| region.kind == SourceRegionKind::Embedded)
        .unwrap();
    assert_eq!(
        region.metadata.as_ref().unwrap()["embedded_language"],
        Value::from("yaml")
    );
    assert_eq!(
        &source[region.start_byte as usize..region.end_byte as usize],
        "title: Getting Started\nslug: /getting-started\ntags: [intro, setup]\nauthors:\n  - ada\n"
    );
    let fact = &facts_with_pattern(&results, "markdown.frontmatter.v1")[0];
    assert_eq!(
        fact.metadata.as_ref().unwrap()["keys"],
        serde_json::json!(["title", "slug", "tags", "authors"])
    );
}

#[test]
fn toml_frontmatter_keys_decode_values_and_name_tables() {
    let results = extract(
        "post.md",
        "+++\ntitle = \"Hugo Post\"\ndate = 2024-01-01\n[params]\nauthor = \"me\"\n+++\n\n# Heading\n",
    );
    let keys: Vec<_> = results
        .symbols
        .iter()
        .filter(|symbol| metadata(symbol, "markdown_kind") == Some("frontmatter_key"))
        .map(|symbol| (symbol.name.as_str(), metadata(symbol, "value")))
        .collect();

    assert_eq!(
        keys,
        vec![
            ("title", Some("Hugo Post")),
            ("date", Some("2024-01-01")),
            ("params", None),
        ]
    );
    let params = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "params")
        .unwrap();
    assert_eq!(params.end_line, 5);
    assert!(results.source_regions.iter().any(|region| {
        region
            .metadata
            .as_ref()
            .and_then(|m| m.get("embedded_language"))
            == Some(&Value::from("toml"))
    }));
}

#[test]
fn html_blocks_are_embedded_regions_with_link_and_image_symbols() {
    let results = extract(
        "html.md",
        "# Page\n\n<!-- TODO -->\n\n<div class=\"warning\">\n  <a href=\"#page\">Back to top</a>\n  <a href=\"https://example.com/html-link\">HTML link</a>\n</div>\n\n<p align=\"center\"><img src=\"docs/logo.png\" alt=\"Logo\"></p>\n",
    );
    let embedded: Vec<_> = results
        .source_regions
        .iter()
        .filter(|region| region.kind == SourceRegionKind::Embedded)
        .map(|region| region.metadata.as_ref().unwrap()["host_node_kind"].clone())
        .collect();

    assert_eq!(
        embedded,
        vec![Value::from("html_block"), Value::from("html_block")]
    );
    let links: Vec<_> = results
        .symbols
        .iter()
        .filter(|symbol| symbol.kind == SymbolKind::Import)
        .map(|symbol| {
            (
                symbol.name.as_str(),
                metadata(symbol, "markdown_kind"),
                metadata(symbol, "destination"),
            )
        })
        .collect();
    assert_eq!(
        links,
        vec![
            ("Back to top", Some("inline_link"), Some("#page")),
            (
                "HTML link",
                Some("inline_link"),
                Some("https://example.com/html-link")
            ),
            ("Logo", Some("image"), Some("docs/logo.png")),
        ]
    );
    assert!(
        results
            .literals
            .iter()
            .any(|literal| literal.literal_text == "https://example.com/html-link")
    );
}

#[test]
fn links_to_headings_in_other_documents_are_pending_references() {
    let results = extract(
        "README.md",
        "# Readme\n\nSee [Getting started](docs/usage.md#getting-started), [site](https://example.com/page#top), and [plain](docs/usage.md).\n",
    );
    let pending: Vec<_> = results
        .structured_pending_relationships
        .iter()
        .map(|pending| {
            (
                name_of(&results, &pending.pending.from_symbol_id),
                pending.pending.kind.clone(),
                pending.target.terminal_name.as_str(),
                pending.target.import_context.as_deref(),
            )
        })
        .collect();

    assert_eq!(
        pending,
        vec![(
            "Readme",
            RelationshipKind::References,
            "getting-started",
            Some("docs/usage.md")
        )]
    );
    assert!(results.structured_pending_relationships[0].reference_site_is_exact);
}

#[test]
fn section_docs_include_pipe_tables() {
    let results = extract(
        "table.md",
        "## Rationalizations\n\n| Excuse | Reality |\n| ------ | ------- |\n| too simple | still needs approval |\n",
    );

    assert!(
        results.symbols[0]
            .doc_comment
            .as_deref()
            .is_some_and(|doc| doc.contains("| too simple | still needs approval |"))
    );
}

#[test]
fn task_lists_autolinks_and_definition_lists_are_facts() {
    let results = extract(
        "extended.md",
        "# Plan\n\n- [x] Ship the parser\n- [ ] Add incremental mode\n\nVisit <https://autolink.example.com> or <team@example.com>.\n\nTerm\n: Definition text\n",
    );
    let tasks: Vec<_> = facts_with_pattern(&results, "markdown.task_list_item.v1")
        .into_iter()
        .map(|fact| {
            (
                fact.metadata.as_ref().unwrap()["checked"].clone(),
                metadata_str(fact, "text"),
            )
        })
        .collect();
    let autolinks: Vec<_> = facts_with_pattern(&results, "markdown.autolink.v1")
        .into_iter()
        .map(|fact| {
            (
                metadata_str(fact, "destination"),
                metadata_str(fact, "autolink_kind"),
            )
        })
        .collect();
    let definitions: Vec<_> = facts_with_pattern(&results, "markdown.definition_list_item.v1")
        .into_iter()
        .map(|fact| (metadata_str(fact, "term"), metadata_str(fact, "definition")))
        .collect();

    assert_eq!(
        tasks,
        vec![
            (Value::Bool(true), Some("Ship the parser")),
            (Value::Bool(false), Some("Add incremental mode")),
        ]
    );
    assert_eq!(
        autolinks,
        vec![
            (Some("https://autolink.example.com"), Some("uri")),
            (Some("team@example.com"), Some("email")),
        ]
    );
    assert_eq!(definitions, vec![(Some("Term"), Some("Definition text"))]);
}
