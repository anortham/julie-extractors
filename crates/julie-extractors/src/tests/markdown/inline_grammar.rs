use std::collections::BTreeSet;
use std::path::Path;

use crate::base::{Symbol, SymbolKind};
use crate::tests::helpers::{facts_with_pattern, metadata_str};

fn extract(source: &str) -> crate::ExtractionResults {
    crate::pipeline::extract_canonical("guide.md", source, Path::new("/repo"))
        .expect("canonical Markdown extraction should succeed")
}

fn meta<'a>(symbol: &'a Symbol, key: &str) -> Option<&'a str> {
    symbol.metadata.as_ref()?.get(key)?.as_str()
}

fn by_kind<'a>(results: &'a crate::ExtractionResults, kind: &str) -> Vec<&'a Symbol> {
    results
        .symbols
        .iter()
        .filter(|symbol| meta(symbol, "markdown_kind") == Some(kind))
        .collect()
}

fn names(symbols: &[&Symbol]) -> BTreeSet<String> {
    symbols.iter().map(|symbol| symbol.name.clone()).collect()
}

#[test]
fn code_blocks_code_spans_html_and_escapes_emit_no_link_rows() {
    let source = "# Regex Guide\n\nUse `[^a-z]` and call `handlers[0](event)`.\n\n```python\nPATTERN = re.compile(r\"[^0-9]+\")\nresult = callbacks[idx](payload)\n```\n\n    indented = table[key](arg)\n\n<!-- TODO: restore [old docs](https://old.example.com) -->\n\nEscaped \\[not a link](nope).\n\nA [real link](https://example.com/real).\n";
    let results = extract(source);

    assert_eq!(
        names(&by_kind(&results, "inline_link")),
        BTreeSet::from(["real link".to_string()])
    );
    assert!(by_kind(&results, "footnote_reference").is_empty());
    let fact_labels = facts_with_pattern(&results, "markdown.inline_link.v1")
        .iter()
        .filter_map(|fact| metadata_str(fact, "label"))
        .collect::<BTreeSet<_>>();
    assert_eq!(fact_labels, BTreeSet::from(["real link"]));
    let literals = results
        .literals
        .iter()
        .map(|literal| literal.literal_text.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(literals, BTreeSet::from(["https://example.com/real"]));
}

#[test]
fn inline_links_use_grammar_destinations_titles_and_labels() {
    let source = "Read the [API reference](https://example.com/api \"API Reference\") first.\n\nSee [Lisp](https://en.wikipedia.org/wiki/Lisp_(programming_language)).\n\nClick [![Build](https://ci.example.com/badge.svg)](https://ci.example.com/job) now.\n";
    let results = extract(source);
    let links = by_kind(&results, "inline_link");

    let api = links
        .iter()
        .find(|s| s.name == "API reference")
        .expect("API link");
    assert_eq!(meta(api, "destination"), Some("https://example.com/api"));
    assert_eq!(meta(api, "title"), Some("API Reference"));
    assert_eq!(api.doc_comment.as_deref(), Some("API Reference"));
    let lisp = links.iter().find(|s| s.name == "Lisp").expect("Lisp link");
    assert_eq!(
        meta(lisp, "destination"),
        Some("https://en.wikipedia.org/wiki/Lisp_(programming_language)")
    );
    let badge = links
        .iter()
        .find(|s| s.name == "Build")
        .expect("badge link");
    assert_eq!(
        meta(badge, "destination"),
        Some("https://ci.example.com/job")
    );

    let api_fact = facts_with_pattern(&results, "markdown.inline_link.v1")
        .into_iter()
        .find(|fact| metadata_str(fact, "label") == Some("API reference"))
        .expect("API fact");
    assert_eq!(
        metadata_str(api_fact, "destination"),
        Some("https://example.com/api")
    );
    assert_eq!(metadata_str(api_fact, "title"), Some("API Reference"));
}

#[test]
fn images_and_autolinks_become_import_symbols() {
    let source = "![Architecture diagram](docs/images/arch.png)\n\nClick [![Build](https://ci.example.com/badge.svg)](https://ci.example.com/job).\n\nVisit <https://example.com/autolink> or email <team@example.com>.\n";
    let results = extract(source);

    let images = by_kind(&results, "image");
    let destinations = images
        .iter()
        .filter_map(|image| meta(image, "destination"))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        destinations,
        BTreeSet::from(["docs/images/arch.png", "https://ci.example.com/badge.svg"])
    );
    assert!(images.iter().all(|image| image.kind == SymbolKind::Import));
    assert_eq!(
        names(&by_kind(&results, "autolink")),
        BTreeSet::from([
            "https://example.com/autolink".to_string(),
            "team@example.com".to_string()
        ])
    );
}

#[test]
fn crlf_documents_keep_exact_link_offsets() {
    let source = "# Guide\r\n\r\nLine one.\r\n\r\nLine two.\r\n\r\nSee [the docs](https://example.com/docs) now.\r\n\r\nNote[^n] here.\r\n";
    let results = extract(source);

    let link = by_kind(&results, "inline_link")[0];
    assert_eq!((link.start_byte, link.end_byte), (41, 77));
    assert_eq!(&source[41..77], "[the docs](https://example.com/docs)");
    let footnote = by_kind(&results, "footnote_reference")[0];
    assert_eq!(footnote.start_byte, 90);
    assert_eq!(footnote.name, "n");
}

#[test]
fn footnote_definitions_and_defined_reference_links_are_symbols() {
    let source = "# Notes\n\nCited[^1] and [the spec][spec] but [not defined].\n\n[^1]: The footnote body.\n\n[spec]: https://spec.example.com \"Spec\"\n";
    let results = extract(source);

    let definition = by_kind(&results, "footnote_definition")[0];
    assert_eq!(definition.name, "1");
    assert_eq!(
        definition.doc_comment.as_deref(),
        Some("The footnote body.")
    );
    assert_eq!(
        definition.signature.as_deref(),
        Some("[^1]: The footnote body.")
    );
    assert_eq!(
        names(&by_kind(&results, "footnote_reference")),
        BTreeSet::from(["1".to_string()])
    );
    assert_eq!(
        names(&by_kind(&results, "reference_link")),
        BTreeSet::from(["spec".to_string()])
    );
    assert!(
        results
            .literals
            .iter()
            .any(|literal| literal.literal_text == "https://spec.example.com"
                && literal.carrier.as_deref() == Some("link_definition"))
    );
}

#[test]
fn nested_bracket_link_text_survives_the_inline_grammar_limit() {
    let source = "# Links\n\nSee [API [v2] docs](https://example.com/api) and `[x [y]](z)`.\n";
    let results = extract(source);

    let link = by_kind(&results, "inline_link")[0];
    assert_eq!(link.name, "API [v2] docs");
    assert_eq!(meta(link, "destination"), Some("https://example.com/api"));
    assert_eq!(by_kind(&results, "inline_link").len(), 1);
    let literal = &results.literals[0];
    assert_eq!(literal.literal_text, "https://example.com/api");
    assert_eq!(
        literal.start_byte as usize,
        source.find("https://").unwrap()
    );
}

#[test]
fn only_code_blocks_carry_body_spans() {
    let source = "# Consumer contract (Miller policy v6)\n\n[api]: https://example.com/api\n\n```rust\nuse globset::{Glob, GlobSetBuilder};\nlet mut builder = GlobSetBuilder::new();\n```\n\nSee [the (api)](https://example.com/api) now.\n";
    let results = extract(source);

    let code = results
        .symbols
        .iter()
        .find(|symbol| symbol.name == "rust code block")
        .expect("code block");
    let body = code.body_span.expect("code block body");
    assert_eq!(
        &source[body.start_byte as usize..body.end_byte as usize],
        "use globset::{Glob, GlobSetBuilder};\nlet mut builder = GlobSetBuilder::new();"
    );
    assert!(code.body_hash.is_some());
    for symbol in results.symbols.iter().filter(|symbol| symbol.id != code.id) {
        assert!(
            symbol.body_span.is_none(),
            "{} has a body span",
            symbol.name
        );
    }
}

#[test]
fn anchor_links_in_code_do_not_link_headings() {
    let source = "# Intro\n\nJump to [install](#install).\n\n```md\n[also install](#install)\n```\n\n# Install\n\nRun it.\n";
    let results = extract(source);

    assert_eq!(results.relationships.len(), 1);
    let edge = &results.relationships[0];
    assert_eq!(edge.line_number, 3);
    assert!(edge.reference_site_is_exact);
}
