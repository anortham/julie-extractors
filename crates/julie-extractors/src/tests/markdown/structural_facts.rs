use std::collections::BTreeSet;
use std::path::Path;

use crate::base::StructuralFact;

fn extract(source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical("source.md", source, Path::new("/repo"))
        .expect("canonical Markdown extraction should succeed")
}

fn facts_with_pattern<'a>(
    results: &'a crate::ExtractionResults,
    pattern_id: &str,
) -> Vec<&'a StructuralFact> {
    results
        .structural_facts
        .iter()
        .filter(|fact| fact.pattern_id == pattern_id)
        .collect()
}

fn metadata_str<'a>(fact: &'a StructuralFact, key: &str) -> Option<&'a str> {
    fact.metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|value| value.as_str())
}

fn metadata_u64(fact: &StructuralFact, key: &str) -> Option<u64> {
    fact.metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(|value| value.as_u64())
}

#[test]
fn markdown_emits_document_structural_facts() {
    let source = r#"---
title: Worker Guide
tags: [docs, api]
---

# Worker Guide

See [the API](https://api.example.com/workers).

```rust
fn helper(value: i32) -> i32 {
    value + 1
}
```

[ext-ref]: https://example.com/external "External"

| Name | ID |
| ---- | -- |
| fixture | 1 |
"#;

    let results = extract(source);

    let frontmatter = facts_with_pattern(&results, "markdown.frontmatter.v1")
        .into_iter()
        .next()
        .expect("expected frontmatter fact");
    assert_eq!(frontmatter.capture_name, "frontmatter");
    assert_eq!(metadata_str(frontmatter, "format"), Some("yaml"));
    assert!(metadata_u64(frontmatter, "key_count").unwrap_or(0) >= 2);

    let headings = facts_with_pattern(&results, "markdown.heading.v1");
    assert_eq!(
        headings
            .iter()
            .filter_map(|fact| metadata_str(fact, "text"))
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["Worker Guide"])
    );
    assert!(
        headings
            .iter()
            .any(|fact| metadata_u64(fact, "level") == Some(1))
    );

    let code_block = facts_with_pattern(&results, "markdown.fenced_code_block.v1")
        .into_iter()
        .next()
        .expect("expected fenced code block fact");
    assert_eq!(metadata_str(code_block, "language"), Some("rust"));

    let inline_link = facts_with_pattern(&results, "markdown.inline_link.v1")
        .into_iter()
        .find(|fact| metadata_str(fact, "label") == Some("the API"))
        .expect("expected inline link fact");
    assert_eq!(
        metadata_str(inline_link, "destination"),
        Some("https://api.example.com/workers")
    );

    let link_def = facts_with_pattern(&results, "markdown.link_definition.v1")
        .into_iter()
        .next()
        .expect("expected link definition fact");
    assert_eq!(metadata_str(link_def, "label"), Some("ext-ref"));
    assert_eq!(
        metadata_str(link_def, "destination"),
        Some("https://example.com/external")
    );

    let table = facts_with_pattern(&results, "markdown.table.v1")
        .into_iter()
        .next()
        .expect("expected table fact");
    assert!(metadata_u64(table, "row_count").unwrap_or(0) >= 2);

    assert!(
        results
            .structural_facts
            .iter()
            .all(|fact| fact.end_byte > fact.start_byte)
    );
}

#[test]
fn markdown_inline_link_fallback_ignores_fenced_code() {
    let source = r#"# Links

See [real link](https://example.com/real).

```markdown
[not a document link](https://example.com/code)
```
"#;

    let results = extract(source);
    let labels = facts_with_pattern(&results, "markdown.inline_link.v1")
        .iter()
        .filter_map(|fact| metadata_str(fact, "label"))
        .collect::<BTreeSet<_>>();

    assert_eq!(labels, BTreeSet::from(["real link"]));
}
