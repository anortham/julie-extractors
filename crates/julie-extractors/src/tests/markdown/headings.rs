use std::path::Path;

use crate::base::{Symbol, SymbolKind};
use crate::tests::helpers::{facts_with_pattern, metadata_str};

fn extract(source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical("README.md", source, Path::new("/repo"))
        .expect("canonical Markdown extraction should succeed")
}

fn heading<'a>(results: &'a crate::ExtractionResults, name: &str) -> &'a Symbol {
    results
        .symbols
        .iter()
        .find(|symbol| symbol.kind == SymbolKind::Module && symbol.name == name)
        .unwrap_or_else(|| panic!("heading {name}"))
}

fn level(symbol: &Symbol) -> Option<u64> {
    symbol.metadata.as_ref()?.get("heading_level")?.as_u64()
}

#[test]
fn setext_headings_become_nested_module_symbols() {
    let source = "Project Title\n=============\n\nIntro with a [jump](#installation-guide).\n\nInstallation Guide\n------------------\n\nRun the installer.\n\n### Usage\n\nMore text.\n\nNext Chapter\n============\n\nEnd.\n";
    let results = extract(source);

    let title = heading(&results, "Project Title");
    let install = heading(&results, "Installation Guide");
    let usage = heading(&results, "Usage");
    let next = heading(&results, "Next Chapter");
    assert_eq!(
        (level(title), level(install), level(next)),
        (Some(1), Some(2), Some(1))
    );
    assert_eq!(title.parent_id, None);
    assert_eq!(install.parent_id.as_deref(), Some(title.id.as_str()));
    assert_eq!(usage.parent_id.as_deref(), Some(install.id.as_str()));
    assert_eq!(next.parent_id, None);
    assert_eq!(
        title.end_byte as usize,
        source.find("Next Chapter").unwrap()
    );
    assert_eq!(install.end_byte, title.end_byte);
    assert_eq!(
        title.doc_comment.as_deref(),
        Some("Intro with a [jump](#installation-guide).")
    );

    let jump = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "jump")
        .expect("jump link");
    assert_eq!(jump.parent_id.as_deref(), Some(title.id.as_str()));
    assert!(
        results
            .relationships
            .iter()
            .any(|edge| edge.from_symbol_id == title.id && edge.to_symbol_id == install.id)
    );
}

#[test]
fn setext_headings_mix_with_atx_sections() {
    let source = "# Intro\n\ntext\n\nRelease 1.0\n-----------\n\n* fix\n\n## Other\n\nx\n";
    let results = extract(source);

    let intro = heading(&results, "Intro");
    let release = heading(&results, "Release 1.0");
    let other = heading(&results, "Other");
    assert_eq!(release.parent_id.as_deref(), Some(intro.id.as_str()));
    assert_eq!(other.parent_id.as_deref(), Some(intro.id.as_str()));
    assert_eq!(release.end_byte as usize, source.find("## Other").unwrap());
    assert_eq!(intro.doc_comment.as_deref(), Some("text"));
}

#[test]
fn heading_facts_come_only_from_real_setext_headings() {
    let source = "Title\n=====\n\n- Added feature\n---\n\n| a | b |\n|---|---|\n| 1 | 2 |\n---\n\n> quoted text\n---\n\n```\ncode\n```\n---\n";
    let results = extract(source);

    let headings = facts_with_pattern(&results, "markdown.heading.v1");
    let texts = headings
        .iter()
        .filter_map(|fact| metadata_str(fact, "text"))
        .collect::<Vec<_>>();
    assert_eq!(texts, vec!["Title"]);
}

#[test]
fn empty_frontmatter_is_not_a_setext_heading() {
    let results = extract("---\n---\n\nBody.\n");

    assert!(facts_with_pattern(&results, "markdown.heading.v1").is_empty());
    assert!(
        results
            .symbols
            .iter()
            .all(|symbol| symbol.kind != SymbolKind::Module)
    );
}

#[test]
fn frontmatter_body_stops_at_a_setext_heading() {
    let source = "---\ntitle: Notes\n---\n\nSummary line.\n\nDetails\n=======\n\nInside.\n";
    let results = extract(source);

    let frontmatter = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "frontmatter")
        .expect("frontmatter");
    assert_eq!(
        frontmatter.doc_comment.as_deref(),
        Some("title: Notes\n\n---\n\nSummary line.")
    );
    assert_eq!(level(heading(&results, "Details")), Some(1));
}
