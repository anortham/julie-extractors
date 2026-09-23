//! Erlang carries documentation on two channels: EDoc `%%` comment blocks and
//! the OTP 27 `-doc` / `-moduledoc` attributes. Both resolve to the same
//! `doc_comment` field; attributes also surface as annotation markers.

use tree_sitter::Node;

use super::ErlangExtractor;
use super::helpers::{
    doc_macro_name, doc_macro_string, preceding_attributes, wild_attribute_name,
    wild_attribute_string,
};
use crate::base::AnnotationMarker;
use crate::base::extractor::select_doc_comment_block;
use crate::base::normalize_annotations;

const DOC_ATTRIBUTE: &str = "doc";

/// `create_symbol` falls back to any comment above the node when given no
/// doc; Erlang computes its own doc, including "none", so it is kept as is.
pub(super) fn keep_doc(
    mut symbol: crate::base::Symbol,
    doc: Option<String>,
) -> crate::base::Symbol {
    symbol.doc_comment = doc;
    symbol
}

/// Documentation for a declaration. An explicit `-doc` attribute in the run
/// above it wins. Otherwise the EDoc `%%` block directly above the
/// declaration, or directly above its `-spec`/`-doc` attribute run (the rebar3
/// template layout), is used.
pub(super) fn doc_for(extractor: &ErlangExtractor, node: &Node) -> Option<String> {
    let attributes = preceding_attributes(&extractor.base, node);
    doc_attribute_text(extractor, &attributes)
        .or_else(|| comment_doc_above(extractor, node))
        .or_else(|| {
            attributes
                .last()
                .and_then(|top| comment_doc_above(extractor, top))
        })
}

fn doc_attribute_text(extractor: &ErlangExtractor, attributes: &[Node]) -> Option<String> {
    attributes.iter().find_map(|attribute| {
        if doc_macro_name(&extractor.base, attribute).is_some() {
            return doc_macro_string(&extractor.base, attribute);
        }
        (wild_attribute_name(&extractor.base, attribute).as_deref() == Some(DOC_ATTRIBUTE))
            .then(|| wild_attribute_string(&extractor.base, attribute))
            .flatten()
    })
}

/// The `%%` comment block that touches `node`: a blank line ends the block,
/// separator lines (`%%`, `%%====`, `%%----`) and `%CopyrightBegin%` ...
/// `%CopyrightEnd%` license blocks are not documentation.
pub(super) fn comment_doc_above(extractor: &ErlangExtractor, node: &Node) -> Option<String> {
    let mut blocks = Vec::new();
    let mut next_row = node.start_position().row;
    let mut current = node.prev_named_sibling();
    while let Some(sibling) = current {
        if sibling.kind() != "comment" {
            break;
        }
        let text = extractor.base.get_node_text(&sibling);
        let text = text.trim_end();
        let last_row = sibling.start_position().row + text.lines().count().saturating_sub(1);
        if next_row > last_row + 1 {
            break;
        }
        blocks.push(text.to_string());
        next_row = sibling.start_position().row;
        current = sibling.prev_named_sibling();
    }
    blocks.reverse();

    let mut in_license = false;
    let mut kept: Vec<&str> = Vec::new();
    for line in blocks.iter().flat_map(|block| block.lines()) {
        if line.trim_start().starts_with("%%!") {
            continue;
        }
        let content = line.trim().trim_start_matches('%').trim();
        if content.contains("%CopyrightBegin%") {
            in_license = true;
        }
        let noise = in_license || content.chars().all(|ch| "=-*_#+~%".contains(ch));
        if content.contains("%CopyrightEnd%") {
            in_license = false;
        }
        if !noise {
            kept.push(line.trim());
        }
    }
    let nearest_first: Vec<String> = kept.iter().rev().map(|line| line.to_string()).collect();
    select_doc_comment_block(&extractor.base.language, &nearest_first)
}

/// The module's doc: an EDoc `@doc` block above `-module`, else `-moduledoc`,
/// else any other comment block above `-module`.
pub(super) fn module_doc_for(
    extractor: &ErlangExtractor,
    node: &Node,
    module_doc: Option<String>,
) -> Option<String> {
    let comment = comment_doc_above(extractor, node);
    match comment {
        Some(comment) if comment.contains("@doc") => Some(comment),
        comment => module_doc.or(comment),
    }
}

pub(super) fn module_doc_text(extractor: &ErlangExtractor, node: &Node) -> Option<String> {
    if doc_macro_name(&extractor.base, node).is_some() {
        return doc_macro_string(&extractor.base, node);
    }
    wild_attribute_string(&extractor.base, node)
}

/// `-spec`, `-doc`, and other attributes attached to the declaration below
/// them become annotation markers keyed by the attribute name (`spec`, `doc`).
pub(super) fn annotations_for(extractor: &ErlangExtractor, node: &Node) -> Vec<AnnotationMarker> {
    let mut raw_texts: Vec<String> = preceding_attributes(&extractor.base, node)
        .iter()
        .map(|attribute| annotation_text(extractor, attribute))
        .collect();
    raw_texts.reverse();
    normalize_annotations(&raw_texts, "erlang")
}

fn annotation_text(extractor: &ErlangExtractor, node: &Node) -> String {
    let text = extractor.base.get_node_text(node);
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let text = collapsed
        .trim_start_matches('-')
        .trim_end_matches('.')
        .trim();
    match text.strip_prefix("?DOC") {
        Some(rest) => format!("doc{rest}"),
        None => text.to_string(),
    }
}
