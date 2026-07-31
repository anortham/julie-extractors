//! Erlang carries documentation on two channels: EDoc `%%` comment blocks and
//! the OTP 27 `-doc` / `-moduledoc` attributes. Both resolve to the same
//! `doc_comment` field; attributes also surface as annotation markers.

use tree_sitter::Node;

use super::ErlangExtractor;
use super::helpers::{preceding_attributes, wild_attribute_name, wild_attribute_string};
use crate::base::AnnotationMarker;
use crate::base::normalize_annotations;

const DOC_ATTRIBUTE: &str = "doc";

pub(super) fn doc_for(extractor: &ErlangExtractor, node: &Node) -> Option<String> {
    extractor
        .base
        .find_doc_comment(node)
        .or_else(|| doc_attribute_text(extractor, node))
}

fn doc_attribute_text(extractor: &ErlangExtractor, node: &Node) -> Option<String> {
    preceding_attributes(&extractor.base, node)
        .into_iter()
        .filter(|attribute| {
            wild_attribute_name(&extractor.base, attribute).as_deref() == Some(DOC_ATTRIBUTE)
        })
        .find_map(|attribute| wild_attribute_string(&extractor.base, &attribute))
}

pub(super) fn module_doc_text(extractor: &ErlangExtractor, node: &Node) -> Option<String> {
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
    collapsed
        .trim_start_matches('-')
        .trim_end_matches('.')
        .trim()
        .to_string()
}
