use crate::base::body::{body_hash, infer_body_span_from_span};
use crate::base::{BaseExtractor, NormalizedSpan, Symbol, SymbolKind, SymbolOptions, Visibility};
use serde_json::Value;
use std::collections::HashMap;

/// A symbol over the host byte range `start..end`, with 0-based columns and a
/// body span inferred from its braces.
#[allow(clippy::too_many_arguments)]
pub(super) fn create_symbol_manual(
    base: &BaseExtractor,
    name: &str,
    kind: SymbolKind,
    start: usize,
    end: usize,
    signature: Option<String>,
    documentation: Option<String>,
    metadata: Option<HashMap<String, Value>>,
) -> Option<Symbol> {
    let options = SymbolOptions {
        signature,
        doc_comment: documentation,
        visibility: Some(Visibility::Public),
        parent_id: None,
        metadata,
        annotations: Vec::new(),
    };
    let span = NormalizedSpan::from_content_range(&base.content, start, end.max(start))?;
    let body_span = infer_body_span_from_span(&base.content, span);
    let body_hash = body_span.and_then(|span| body_hash(&base.content, span, &base.language));

    Some(Symbol {
        id: base.generate_id_for_span(name, &span),
        name: name.to_string(),
        kind,
        language: base.language.clone(),
        file_path: base.file_path.clone(),
        start_line: span.start_line,
        start_column: span.start_column,
        end_line: span.end_line,
        end_column: span.end_column,
        start_byte: span.start_byte,
        end_byte: span.end_byte,
        body_span,
        body_hash,
        signature: options.signature,
        doc_comment: options.doc_comment,
        visibility: options.visibility,
        parent_id: options.parent_id,
        metadata: Some(options.metadata.unwrap_or_default()),
        annotations: options.annotations,
        semantic_group: None,
        confidence: None,
        content_type: None,
    })
}
