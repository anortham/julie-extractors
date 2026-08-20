use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::string::FromUtf8Error;

use julie_extract_artifact::model::{
    ArtifactComplexityMetric, ArtifactFile, ArtifactIdentifier, ArtifactLiteral,
    ArtifactParseDiagnostic, ArtifactPendingRelationship, ArtifactRelationship,
    ArtifactSourceRegion, ArtifactStructuralFact, ArtifactSymbol, ArtifactSymbolAnnotation,
    ArtifactTypeArgument, ArtifactTypeArgumentUsage, ArtifactTypeFact, FileStatus,
    ReferenceSiteProvenance,
};
use julie_extractors::base::{
    ComplexityMetric, NormalizedSpan, StructuralFact, StructuredPendingRelationship,
};
use julie_extractors::language_policy::classify_literals_by_carrier;
use julie_extractors::{
    ExtractionLevel, ExtractionResults, Literal, ParseDiagnosticKind, PendingRelationship,
    SourceRegion, TypeArgument, TypeArgumentUsage, TypeInfo, detect_language_for_source,
    extract_canonical_at,
};
use serde::Serialize;
use serde_json::Value;

use crate::paths::FileTarget;

/// Build the bounded extraction pool, retrying once with one worker if needed.
pub(crate) fn select_extraction_pool<P, E>(
    requested_jobs: usize,
    mut build: impl FnMut(usize) -> Result<P, E>,
) -> Result<P, E> {
    build(requested_jobs).or_else(|error| {
        if requested_jobs == 1 {
            Err(error)
        } else {
            build(1)
        }
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExtractFileErrorKind {
    Read,
    Extract,
    Serialize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExtractFileError {
    pub kind: ExtractFileErrorKind,
    pub path: String,
    pub root_relative_path: String,
    pub message: String,
    pub content_hash: Option<String>,
    pub content_bytes: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SourceSnapshot {
    pub content: String,
    pub content_hash: String,
    pub content_bytes: i64,
    pub line_count: Option<i64>,
}

#[derive(Debug)]
enum SourceDecodeError {
    Utf8(FromUtf8Error),
    Utf16 {
        encoding: &'static str,
        message: String,
    },
}

pub(crate) fn extract_artifact_file(
    root: &Path,
    target: &FileTarget,
    language: String,
    indexed_at: String,
    level: ExtractionLevel,
) -> Result<ArtifactFile, ExtractFileError> {
    let snapshot = read_source_snapshot(target)?;
    extract_artifact_file_from_snapshot_at(root, target, language, indexed_at, snapshot, level)
}

pub(crate) fn read_source_snapshot(
    target: &FileTarget,
) -> Result<SourceSnapshot, ExtractFileError> {
    let bytes = fs::read(&target.absolute_path).map_err(|error| ExtractFileError {
        kind: ExtractFileErrorKind::Read,
        path: target.absolute_path.display().to_string(),
        root_relative_path: target.root_relative_path.clone(),
        message: format!("source file could not be read: {error}"),
        content_hash: None,
        content_bytes: None,
    })?;
    let content_hash = content_hash_bytes(&bytes);
    let content_bytes = bytes.len() as i64;
    let content = decode_source_content(bytes)
        .map_err(|error| decode_error(target, error, &content_hash, content_bytes))?;

    Ok(SourceSnapshot {
        content_hash,
        content_bytes,
        line_count: Some(line_count(&content)),
        content,
    })
}

#[allow(dead_code)]
pub(crate) fn read_source_identity(target: &FileTarget) -> Result<(String, u64), ExtractFileError> {
    let bytes = fs::read(&target.absolute_path).map_err(|error| ExtractFileError {
        kind: ExtractFileErrorKind::Read,
        path: target.absolute_path.display().to_string(),
        root_relative_path: target.root_relative_path.clone(),
        message: format!("source file could not be read: {error}"),
        content_hash: None,
        content_bytes: None,
    })?;
    Ok((content_hash_bytes(&bytes), bytes.len() as u64))
}

fn decode_source_content(bytes: Vec<u8>) -> Result<String, SourceDecodeError> {
    if let Some(content_bytes) = bytes.strip_prefix(&[0xff, 0xfe]) {
        return decode_utf16_content("UTF-16LE", content_bytes, u16::from_le_bytes);
    }

    if let Some(content_bytes) = bytes.strip_prefix(&[0xfe, 0xff]) {
        return decode_utf16_content("UTF-16BE", content_bytes, u16::from_be_bytes);
    }

    String::from_utf8(bytes).map_err(SourceDecodeError::Utf8)
}

fn decode_utf16_content(
    encoding: &'static str,
    bytes: &[u8],
    decode_unit: fn([u8; 2]) -> u16,
) -> Result<String, SourceDecodeError> {
    let (chunks, remainder) = bytes.as_chunks::<2>();
    if !remainder.is_empty() {
        return Err(SourceDecodeError::Utf16 {
            encoding,
            message: "odd byte length after UTF-16 byte order mark".to_string(),
        });
    }

    let units = chunks.iter().copied().map(decode_unit).collect::<Vec<_>>();
    String::from_utf16(&units).map_err(|error| SourceDecodeError::Utf16 {
        encoding,
        message: error.to_string(),
    })
}

#[cfg(test)]
pub(crate) fn extract_artifact_file_from_snapshot(
    root: &Path,
    target: &FileTarget,
    language: String,
    indexed_at: String,
    snapshot: SourceSnapshot,
) -> Result<ArtifactFile, ExtractFileError> {
    extract_artifact_file_from_snapshot_at(
        root,
        target,
        language,
        indexed_at,
        snapshot,
        ExtractionLevel::Full,
    )
}

pub(crate) fn extract_artifact_file_from_snapshot_at(
    root: &Path,
    target: &FileTarget,
    language: String,
    indexed_at: String,
    snapshot: SourceSnapshot,
    level: ExtractionLevel,
) -> Result<ArtifactFile, ExtractFileError> {
    let language = detect_language_for_source(&target.root_relative_path, &snapshot.content)
        .unwrap_or(language.as_str())
        .to_string();
    let mut results = catch_extraction_panic(target, &snapshot, || {
        extract_canonical_at(&target.root_relative_path, &snapshot.content, root, level)
    })?;
    classify_literals_by_carrier(&mut results.literals);

    map_results(target, language, indexed_at, &snapshot, results)
}

/// Build the `Extract`-kind error for a file, carrying the snapshot's hash/byte
/// context so a failed extraction still produces a faithful `FailedPreserved` row.
fn extract_error(
    target: &FileTarget,
    snapshot: &SourceSnapshot,
    message: String,
) -> ExtractFileError {
    ExtractFileError {
        kind: ExtractFileErrorKind::Extract,
        path: target.absolute_path.display().to_string(),
        root_relative_path: target.root_relative_path.clone(),
        message,
        content_hash: Some(snapshot.content_hash.clone()),
        content_bytes: Some(snapshot.content_bytes),
    }
}

/// Run a single file's extraction, converting both a returned error AND a panic
/// into an `ExtractFileError`. A panic in any extractor (e.g. an unguarded byte
/// slice) must degrade to one `FailedPreserved` row, never abort a whole-tree scan.
fn catch_extraction_panic<F, E>(
    target: &FileTarget,
    snapshot: &SourceSnapshot,
    extract: F,
) -> Result<ExtractionResults, ExtractFileError>
where
    F: FnOnce() -> Result<ExtractionResults, E>,
    E: std::fmt::Display,
{
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(extract)) {
        Ok(Ok(results)) => Ok(results),
        Ok(Err(error)) => Err(extract_error(target, snapshot, error.to_string())),
        Err(panic) => Err(extract_error(
            target,
            snapshot,
            format!("extractor panicked: {}", panic_message(panic)),
        )),
    }
}

fn panic_message(panic: Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = panic.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = panic.downcast_ref::<String>() {
        message.clone()
    } else {
        "unknown panic".to_string()
    }
}

pub(crate) fn failed_artifact_file(
    target: &FileTarget,
    language: String,
    indexed_at: String,
    error: &ExtractFileError,
) -> ArtifactFile {
    let path = target.root_relative_path.clone();
    let content_hash = error.content_hash.clone().unwrap_or_else(|| {
        content_hash(&format!("{}:{}", error.root_relative_path, error.message))
    });
    let content_bytes = error.content_bytes.unwrap_or(0);
    ArtifactFile {
        file_id: stable_id("file", [&path]),
        path,
        language,
        content_hash,
        content_bytes,
        line_count: None,
        indexed_at,
        status: FileStatus::FailedPreserved,
        metadata_json: None,
        symbols: Vec::new(),
        symbol_annotations: Vec::new(),
        identifiers: Vec::new(),
        relationships: Vec::new(),
        pending_relationships: Vec::new(),
        type_facts: Vec::new(),
        type_argument_usages: Vec::new(),
        type_arguments: Vec::new(),
        literals: Vec::new(),
        source_regions: Vec::new(),
        structural_facts: Vec::new(),
        complexity_metrics: Vec::new(),
        parse_diagnostics: vec![failure_parse_diagnostic(target, error, content_bytes)],
    }
}

pub(crate) fn unchanged_artifact_file(
    target: &FileTarget,
    language: String,
    indexed_at: String,
    snapshot: &SourceSnapshot,
) -> ArtifactFile {
    let path = target.root_relative_path.clone();
    ArtifactFile {
        file_id: stable_id("file", [&path]),
        path,
        language,
        content_hash: snapshot.content_hash.clone(),
        content_bytes: snapshot.content_bytes,
        line_count: snapshot.line_count,
        indexed_at,
        status: FileStatus::Indexed,
        metadata_json: None,
        symbols: Vec::new(),
        symbol_annotations: Vec::new(),
        identifiers: Vec::new(),
        relationships: Vec::new(),
        pending_relationships: Vec::new(),
        type_facts: Vec::new(),
        type_argument_usages: Vec::new(),
        type_arguments: Vec::new(),
        literals: Vec::new(),
        source_regions: Vec::new(),
        structural_facts: Vec::new(),
        complexity_metrics: Vec::new(),
        parse_diagnostics: Vec::new(),
    }
}

fn map_results(
    target: &FileTarget,
    language: String,
    indexed_at: String,
    snapshot: &SourceSnapshot,
    results: ExtractionResults,
) -> Result<ArtifactFile, ExtractFileError> {
    let path = target.root_relative_path.clone();
    let file_id = stable_id("file", [&path]);
    let mut symbols = Vec::with_capacity(results.symbols.len());
    let mut symbol_annotations = Vec::new();

    for symbol in &results.symbols {
        symbols.push(ArtifactSymbol {
            symbol_id: symbol.id.clone(),
            name: symbol.name.clone(),
            kind: symbol.kind.to_string(),
            signature: symbol.signature.clone(),
            doc_comment: symbol.doc_comment.clone(),
            visibility: symbol
                .visibility
                .as_ref()
                .map(|visibility| visibility.as_storage_str().to_string()),
            parent_symbol_id: symbol.parent_id.clone(),
            start_line: i64::from(symbol.start_line),
            start_column: i64::from(symbol.start_column),
            end_line: i64::from(symbol.end_line),
            end_column: i64::from(symbol.end_column),
            start_byte: i64::from(symbol.start_byte),
            end_byte: i64::from(symbol.end_byte),
            body_start_line: symbol
                .body_span
                .as_ref()
                .map(|span| i64::from(span.start_line)),
            body_start_column: symbol
                .body_span
                .as_ref()
                .map(|span| i64::from(span.start_column)),
            body_end_line: symbol
                .body_span
                .as_ref()
                .map(|span| i64::from(span.end_line)),
            body_end_column: symbol
                .body_span
                .as_ref()
                .map(|span| i64::from(span.end_column)),
            body_start_byte: symbol
                .body_span
                .as_ref()
                .map(|span| i64::from(span.start_byte)),
            body_end_byte: symbol
                .body_span
                .as_ref()
                .map(|span| i64::from(span.end_byte)),
            body_hash: symbol.body_hash.clone(),
            semantic_group: symbol.semantic_group.clone(),
            confidence: symbol.confidence.map(f64::from),
            content_type: symbol.content_type.clone(),
            is_test: metadata_flag(&symbol.metadata, "is_test"),
            test_container: metadata_flag(&symbol.metadata, "test_container"),
            test_lifecycle: metadata_flag(&symbol.metadata, "test_lifecycle"),
            metadata_json: optional_json(&symbol.metadata, target)?,
        });

        for (index, annotation) in symbol.annotations.iter().enumerate() {
            let index = index.to_string();
            symbol_annotations.push(ArtifactSymbolAnnotation {
                annotation_id: stable_id(
                    "symbol_annotation",
                    [
                        symbol.id.as_str(),
                        index.as_str(),
                        annotation.annotation_key.as_str(),
                        annotation.raw_text.as_deref().unwrap_or(""),
                    ],
                ),
                symbol_id: symbol.id.clone(),
                annotation: annotation.annotation.clone(),
                annotation_key: annotation.annotation_key.clone(),
                raw_text: annotation.raw_text.clone(),
                carrier: annotation.carrier.clone(),
                metadata_json: None,
            });
        }
    }

    let mut type_infos = results.types.values().collect::<Vec<_>>();
    type_infos.sort_by(|left, right| left.symbol_id.cmp(&right.symbol_id));

    let identifiers = dedupe_by_id(
        map_identifiers(&results, target, &snapshot.content, &file_id)?,
        |identifier| identifier.identifier_id.as_str(),
    );
    let relationships = dedupe_by_id(
        map_relationships(&results, target, &file_id)?,
        |relationship| relationship.relationship_id.as_str(),
    );
    let pending_relationships = dedupe_by_id(
        map_pending_relationships(&results, target, &file_id)?,
        |pending| pending.pending_relationship_id.as_str(),
    );
    let type_facts = dedupe_by_id(map_type_facts(type_infos, target)?, |type_fact| {
        type_fact.type_fact_id.as_str()
    });
    let type_argument_usages = dedupe_by_id(
        map_type_argument_usages(&results.type_argument_usages),
        |usage| usage.usage_id.as_str(),
    );
    let type_arguments = dedupe_by_id(
        map_type_arguments(&results.type_argument_usages),
        |type_argument| type_argument.type_argument_id.as_str(),
    );
    let literals = dedupe_by_id(map_literals(&results.literals), |literal| {
        literal.literal_id.as_str()
    });
    let source_regions = dedupe_by_id(
        map_source_regions(&results.source_regions, target)?,
        |region| region.source_region_id.as_str(),
    );
    let structural_facts = dedupe_by_id(
        map_structural_facts(&results.structural_facts, target)?,
        |fact| fact.structural_fact_id.as_str(),
    );
    let complexity_metrics = dedupe_by_id(
        map_complexity_metrics(&results.complexity_metrics, target)?,
        |metric| metric.complexity_metric_id.as_str(),
    );
    let parse_diagnostics = dedupe_by_id(map_parse_diagnostics(&results, target), |diagnostic| {
        diagnostic.diagnostic_id.as_str()
    });

    Ok(ArtifactFile {
        file_id,
        path,
        language,
        content_hash: snapshot.content_hash.clone(),
        content_bytes: snapshot.content_bytes,
        line_count: snapshot.line_count,
        indexed_at,
        status: FileStatus::Indexed,
        metadata_json: None,
        symbols: dedupe_by_id(symbols, |symbol| symbol.symbol_id.as_str()),
        symbol_annotations: dedupe_by_id(symbol_annotations, |annotation| {
            annotation.annotation_id.as_str()
        }),
        identifiers,
        relationships,
        pending_relationships,
        type_facts,
        type_argument_usages,
        type_arguments,
        literals,
        source_regions,
        structural_facts,
        complexity_metrics,
        parse_diagnostics,
    })
}

fn dedupe_by_id<T>(rows: Vec<T>, mut key: impl FnMut(&T) -> &str) -> Vec<T> {
    let mut seen = HashSet::new();
    rows.into_iter()
        .filter(|row| seen.insert(key(row).to_string()))
        .collect()
}

fn map_identifiers(
    results: &ExtractionResults,
    target: &FileTarget,
    source: &str,
    file_id: &str,
) -> Result<Vec<ArtifactIdentifier>, ExtractFileError> {
    results
        .identifiers
        .iter()
        .map(|identifier| {
            let mut metadata = serde_json::Map::new();
            let identifier_kind = identifier.kind.to_string();
            let source_receiver = matches!(identifier_kind.as_str(), "call" | "member_access")
                .then(|| receiver_before_identifier(source, identifier.start_byte))
                .flatten();
            if let Some(receiver) = source_receiver {
                metadata.insert("receiver".to_string(), serde_json::Value::String(receiver));
                if let Some(qualifier) =
                    receiver_qualifier_before_identifier(source, identifier.start_byte)
                {
                    metadata.insert(
                        "receiver_qualifier".to_string(),
                        serde_json::Value::String(qualifier),
                    );
                }
            }
            Ok(ArtifactIdentifier {
                identifier_id: identifier.id.clone(),
                reference_site_id: exact_reference_site_id(
                    file_id,
                    identifier.start_byte,
                    identifier.end_byte,
                ),
                name: identifier.name.clone(),
                kind: identifier.kind.to_string(),
                containing_symbol_id: identifier.containing_symbol_id.clone(),
                start_line: i64::from(identifier.start_line),
                start_column: i64::from(identifier.start_column),
                end_line: i64::from(identifier.end_line),
                end_column: i64::from(identifier.end_column),
                start_byte: i64::from(identifier.start_byte),
                end_byte: i64::from(identifier.end_byte),
                site_is_exact: true,
                site_provenance: ReferenceSiteProvenance::TargetToken,
                confidence: f64::from(identifier.confidence),
                code_context: None,
                metadata_json: (!metadata.is_empty())
                    .then(|| serde_json::Value::Object(metadata).to_string()),
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| serialization_error(target, error))
}

/// The member-access token immediately before `at`, with the byte offset it starts
/// at, so the caller can keep walking the same chain leftward.
fn receiver_token_before(source: &str, at: usize) -> Option<(String, usize)> {
    let bytes = source.as_bytes();
    let mut cursor = at.min(bytes.len());
    while cursor > 0 && bytes[cursor - 1].is_ascii_whitespace() {
        cursor -= 1;
    }
    let separator_width =
        if cursor >= 2 && matches!(&bytes[cursor - 2..cursor], b"::" | b"->" | b"?.") {
            2
        } else if cursor >= 1 && bytes[cursor - 1] == b'.' {
            1
        } else {
            return None;
        };
    cursor -= separator_width;
    while cursor > 0 && bytes[cursor - 1].is_ascii_whitespace() {
        cursor -= 1;
    }
    let end = cursor;
    while cursor > 0
        && (bytes[cursor - 1].is_ascii_alphanumeric()
            || matches!(bytes[cursor - 1], b'_' | b'$' | b'@'))
    {
        cursor -= 1;
    }
    (cursor < end).then(|| (source[cursor..end].to_string(), cursor))
}

fn receiver_before_identifier(source: &str, start_byte: u32) -> Option<String> {
    let at = usize::try_from(start_byte).ok()?;
    receiver_token_before(source, at).map(|(token, _)| token)
}

/// The dotted qualification standing in front of the receiver token:
/// `Some.Namespace.Fixture.Create()` yields `Some.Namespace` for `Create`. A
/// resolver needs it to tell a fully-qualified reference to a workspace type from
/// a foreign one that merely shares the type's simple name.
fn receiver_qualifier_before_identifier(source: &str, start_byte: u32) -> Option<String> {
    let at = usize::try_from(start_byte).ok()?;
    let (_, mut cursor) = receiver_token_before(source, at)?;
    let mut segments = Vec::new();
    while let Some((token, start)) = receiver_token_before(source, cursor) {
        segments.push(token);
        cursor = start;
    }
    if segments.is_empty() {
        return None;
    }
    segments.reverse();
    Some(segments.join("."))
}

fn map_relationships(
    results: &ExtractionResults,
    target: &FileTarget,
    file_id: &str,
) -> Result<Vec<ArtifactRelationship>, ExtractFileError> {
    results
        .relationships
        .iter()
        .map(|relationship| {
            let span = relationship
                .span
                .as_ref()
                .filter(|_| relationship.reference_site_is_exact);
            Ok(ArtifactRelationship {
                reference_site_id: reference_site_id(file_id, span, &relationship.id),
                relationship_id: span.map_or_else(
                    || relationship.id.clone(),
                    |span| {
                        stable_id(
                            "relationship",
                            [
                                relationship.from_symbol_id.clone(),
                                relationship.to_symbol_id.clone(),
                                relationship.kind.to_string(),
                                span.start_line.to_string(),
                                span.start_column.to_string(),
                                span.end_line.to_string(),
                                span.end_column.to_string(),
                                span.start_byte.to_string(),
                                span.end_byte.to_string(),
                            ],
                        )
                    },
                ),
                from_symbol_id: relationship.from_symbol_id.clone(),
                to_symbol_id: relationship.to_symbol_id.clone(),
                kind: relationship.kind.to_string(),
                start_line: Some(i64::from(
                    span.map_or(relationship.line_number, |span| span.start_line),
                )),
                start_column: span.map(|span| i64::from(span.start_column)),
                end_line: span.map(|span| i64::from(span.end_line)),
                end_column: span.map(|span| i64::from(span.end_column)),
                start_byte: span.map(|span| i64::from(span.start_byte)),
                end_byte: span.map(|span| i64::from(span.end_byte)),
                site_is_exact: span.is_some(),
                site_provenance: if span.is_some() {
                    ReferenceSiteProvenance::TargetToken
                } else {
                    ReferenceSiteProvenance::Spanless
                },
                confidence: f64::from(relationship.confidence),
                metadata_json: optional_json(&relationship.metadata, target)?,
            })
        })
        .collect()
}

fn map_pending_relationships(
    results: &ExtractionResults,
    target: &FileTarget,
    file_id: &str,
) -> Result<Vec<ArtifactPendingRelationship>, ExtractFileError> {
    if !results.structured_pending_relationships.is_empty() {
        return results
            .structured_pending_relationships
            .iter()
            .map(|pending| map_structured_pending(pending, target, file_id))
            .collect();
    }

    results
        .pending_relationships
        .iter()
        .map(|pending| map_legacy_pending(pending, file_id))
        .collect()
}

fn map_structured_pending(
    pending: &StructuredPendingRelationship,
    target: &FileTarget,
    file_id: &str,
) -> Result<ArtifactPendingRelationship, ExtractFileError> {
    let namespace_json = json_string(&pending.target.namespace_path, target)
        .map_err(|error| serialization_error(target, error))?;
    let span = pending
        .span
        .as_ref()
        .filter(|_| pending.reference_site_is_exact);
    let pending_relationship_id = pending_id(
        pending.pending.from_symbol_id.as_str(),
        pending.target.display_name.as_str(),
        pending.pending.kind.to_string().as_str(),
        pending.pending.line_number,
        span,
    );
    Ok(ArtifactPendingRelationship {
        reference_site_id: reference_site_id(file_id, span, &pending_relationship_id),
        pending_relationship_id,
        from_symbol_id: pending.pending.from_symbol_id.clone(),
        caller_scope_symbol_id: pending.caller_scope_symbol_id.clone(),
        kind: pending.pending.kind.to_string(),
        target_display_name: pending.target.display_name.clone(),
        target_terminal_name: pending.target.terminal_name.clone(),
        target_receiver: pending.target.receiver.clone(),
        target_namespace_json: namespace_json,
        target_import_context: pending.target.import_context.clone(),
        start_line: span.map_or_else(
            || i64::from(pending.pending.line_number),
            |span| i64::from(span.start_line),
        ),
        start_column: span.map(|span| i64::from(span.start_column)),
        end_line: span.map(|span| i64::from(span.end_line)),
        end_column: span.map(|span| i64::from(span.end_column)),
        start_byte: span.map(|span| i64::from(span.start_byte)),
        end_byte: span.map(|span| i64::from(span.end_byte)),
        site_is_exact: span.is_some(),
        site_provenance: if span.is_some() {
            ReferenceSiteProvenance::TargetToken
        } else {
            ReferenceSiteProvenance::Spanless
        },
        confidence: f64::from(pending.pending.confidence),
        metadata_json: None,
    })
}

fn map_legacy_pending(
    pending: &PendingRelationship,
    file_id: &str,
) -> Result<ArtifactPendingRelationship, ExtractFileError> {
    let pending_relationship_id = pending_id(
        pending.from_symbol_id.as_str(),
        pending.callee_name.as_str(),
        pending.kind.to_string().as_str(),
        pending.line_number,
        None,
    );
    Ok(ArtifactPendingRelationship {
        reference_site_id: reference_site_id(file_id, None, &pending_relationship_id),
        pending_relationship_id,
        from_symbol_id: pending.from_symbol_id.clone(),
        caller_scope_symbol_id: None,
        kind: pending.kind.to_string(),
        target_display_name: pending.callee_name.clone(),
        target_terminal_name: pending.callee_name.clone(),
        target_receiver: None,
        target_namespace_json: "[]".to_string(),
        target_import_context: None,
        start_line: i64::from(pending.line_number),
        start_column: None,
        end_line: None,
        end_column: None,
        start_byte: None,
        end_byte: None,
        site_is_exact: false,
        site_provenance: ReferenceSiteProvenance::Spanless,
        confidence: f64::from(pending.confidence),
        metadata_json: None,
    })
}

fn map_type_facts(
    type_infos: Vec<&TypeInfo>,
    target: &FileTarget,
) -> Result<Vec<ArtifactTypeFact>, ExtractFileError> {
    type_infos
        .into_iter()
        .map(|type_info| {
            Ok(ArtifactTypeFact {
                type_fact_id: stable_id("type_fact", [type_info.symbol_id.as_str()]),
                symbol_id: type_info.symbol_id.clone(),
                resolved_type: type_info.resolved_type.clone(),
                generic_params_json: optional_json(&type_info.generic_params, target)?,
                constraints_json: optional_json(&type_info.constraints, target)?,
                is_inferred: type_info.is_inferred,
                metadata_json: optional_json(&type_info.metadata, target)?,
            })
        })
        .collect()
}

fn map_type_argument_usages(usages: &[TypeArgumentUsage]) -> Vec<ArtifactTypeArgumentUsage> {
    usages
        .iter()
        .map(|usage| ArtifactTypeArgumentUsage {
            usage_id: type_argument_usage_id(usage),
            identifier_id: usage.identifier_id.clone(),
            metadata_json: None,
        })
        .collect()
}

fn map_type_arguments(usages: &[TypeArgumentUsage]) -> Vec<ArtifactTypeArgument> {
    let mut rows = Vec::new();
    for usage in usages {
        let usage_id = type_argument_usage_id(usage);
        push_type_arguments(&usage_id, None, Vec::new(), &usage.arguments, &mut rows);
    }
    rows
}

fn push_type_arguments(
    usage_id: &str,
    parent_type_argument_id: Option<String>,
    ordinal_path: Vec<String>,
    arguments: &[TypeArgument],
    rows: &mut Vec<ArtifactTypeArgument>,
) {
    for argument in arguments {
        let ordinal = argument.ordinal.to_string();
        let mut child_ordinal_path = ordinal_path.clone();
        child_ordinal_path.push(ordinal.clone());
        let path = child_ordinal_path.join(".");
        let parent_id = parent_type_argument_id.clone().unwrap_or_default();
        let type_argument_id = stable_id(
            "type_argument",
            [
                usage_id,
                parent_id.as_str(),
                path.as_str(),
                argument.type_name.as_str(),
            ],
        );
        rows.push(ArtifactTypeArgument {
            type_argument_id: type_argument_id.clone(),
            usage_id: usage_id.to_string(),
            parent_type_argument_id: parent_type_argument_id.clone(),
            ordinal: i64::from(argument.ordinal),
            type_name: argument.type_name.clone(),
        });
        push_type_arguments(
            usage_id,
            Some(type_argument_id),
            child_ordinal_path,
            &argument.children,
            rows,
        );
    }
}

fn map_literals(literals: &[Literal]) -> Vec<ArtifactLiteral> {
    literals
        .iter()
        .map(|literal| ArtifactLiteral {
            literal_id: literal.id.clone(),
            literal_text: literal.literal_text.clone(),
            kind: literal.kind.as_str().to_string(),
            carrier: literal.carrier.clone(),
            arg_position: i64::from(literal.arg_position),
            containing_symbol_id: literal.containing_symbol_id.clone(),
            start_line: i64::from(literal.start_line),
            start_column: i64::from(literal.start_column),
            end_line: i64::from(literal.end_line),
            end_column: i64::from(literal.end_column),
            start_byte: i64::from(literal.start_byte),
            end_byte: i64::from(literal.end_byte),
            confidence: f64::from(literal.confidence),
            metadata_json: None,
        })
        .collect()
}

fn map_source_regions(
    regions: &[SourceRegion],
    target: &FileTarget,
) -> Result<Vec<ArtifactSourceRegion>, ExtractFileError> {
    regions
        .iter()
        .map(|region| {
            Ok(ArtifactSourceRegion {
                source_region_id: region.id.clone(),
                kind: region.kind.as_str().to_string(),
                containing_symbol_id: region.containing_symbol_id.clone(),
                start_line: i64::from(region.start_line),
                start_column: i64::from(region.start_column),
                end_line: i64::from(region.end_line),
                end_column: i64::from(region.end_column),
                start_byte: i64::from(region.start_byte),
                end_byte: i64::from(region.end_byte),
                metadata_json: optional_json(&region.metadata, target)?,
            })
        })
        .collect()
}

fn map_structural_facts(
    facts: &[StructuralFact],
    target: &FileTarget,
) -> Result<Vec<ArtifactStructuralFact>, ExtractFileError> {
    facts
        .iter()
        .map(|fact| {
            Ok(ArtifactStructuralFact {
                structural_fact_id: fact.id.clone(),
                pattern_id: fact.pattern_id.clone(),
                capture_name: fact.capture_name.clone(),
                node_kind: fact.node_kind.clone(),
                containing_symbol_id: fact.containing_symbol_id.clone(),
                start_line: i64::from(fact.start_line),
                start_column: i64::from(fact.start_column),
                end_line: i64::from(fact.end_line),
                end_column: i64::from(fact.end_column),
                start_byte: i64::from(fact.start_byte),
                end_byte: i64::from(fact.end_byte),
                confidence: f64::from(fact.confidence),
                metadata_json: optional_json(&fact.metadata, target)?,
            })
        })
        .collect()
}

fn map_complexity_metrics(
    metrics: &[ComplexityMetric],
    target: &FileTarget,
) -> Result<Vec<ArtifactComplexityMetric>, ExtractFileError> {
    metrics
        .iter()
        .map(|metric| {
            Ok(ArtifactComplexityMetric {
                complexity_metric_id: metric.id.clone(),
                scope: metric.scope.clone(),
                symbol_id: metric.symbol_id.clone(),
                algorithm_id: metric.algorithm_id.clone(),
                covered_lines: i64::from(metric.covered_lines),
                covered_bytes: i64::from(metric.covered_bytes),
                decision_count: i64::from(metric.decision_count),
                loop_count: i64::from(metric.loop_count),
                max_nesting_depth: i64::from(metric.max_nesting_depth),
                parameter_count: metric.parameter_count.map(i64::from),
                start_line: i64::from(metric.start_line),
                start_column: i64::from(metric.start_column),
                end_line: i64::from(metric.end_line),
                end_column: i64::from(metric.end_column),
                start_byte: i64::from(metric.start_byte),
                end_byte: i64::from(metric.end_byte),
                metadata_json: optional_json(&metric.metadata, target)?,
            })
        })
        .collect()
}

fn map_parse_diagnostics(
    results: &ExtractionResults,
    target: &FileTarget,
) -> Vec<ArtifactParseDiagnostic> {
    results
        .parse_diagnostics
        .iter()
        .map(|diagnostic| {
            let kind = match diagnostic.kind {
                ParseDiagnosticKind::Error => "error",
                ParseDiagnosticKind::Missing => "missing",
                ParseDiagnosticKind::DepthTruncated => "depth_truncated",
            };
            let mut identity = vec![
                target.root_relative_path.clone(),
                kind.to_string(),
                diagnostic.start_line.to_string(),
                diagnostic.start_column.to_string(),
                diagnostic.end_line.to_string(),
                diagnostic.end_column.to_string(),
            ];
            // Only extend the identity when a message is present: an extractor
            // diagnostic can share a span with a tree one, and the two must not
            // collide on the primary key.
            if let Some(message) = &diagnostic.message {
                identity.push(message.clone());
            }

            ArtifactParseDiagnostic {
                diagnostic_id: stable_id("parse_diagnostic", identity),
                kind: kind.to_string(),
                message: diagnostic.message.clone(),
                start_line: i64::from(diagnostic.start_line),
                start_column: i64::from(diagnostic.start_column),
                end_line: i64::from(diagnostic.end_line),
                end_column: i64::from(diagnostic.end_column),
                start_byte: i64::from(diagnostic.start_byte),
                end_byte: i64::from(diagnostic.end_byte),
                metadata_json: None,
            }
        })
        .collect()
}

fn optional_json<T: Serialize>(
    value: &Option<T>,
    target: &FileTarget,
) -> Result<Option<String>, ExtractFileError> {
    value
        .as_ref()
        .map(|value| json_string(value, target).map_err(|error| serialization_error(target, error)))
        .transpose()
}

/// Serialize one artifact `metadata_json` cell in canonical (sorted-key) form.
///
/// Row metadata reaches this chokepoint from `HashMap`-backed extractor maps whose
/// iteration order reseeds per process, so serializing them directly makes two scans
/// of the same tree disagree byte-for-byte on rows that carry identical facts.
/// Routing through `serde_json::Value` sorts the keys because `serde_json::Map` is
/// BTreeMap-backed while the `preserve_order` feature stays off — the workspace uses
/// resolver 2, so tree-sitter's build-dependency copy of that feature does not unify
/// into this graph. Downstream row-equivalence proofs compare this text directly.
fn json_string<T: Serialize>(value: &T, _target: &FileTarget) -> Result<String, serde_json::Error> {
    serde_json::to_string(&serde_json::to_value(value)?)
}

fn metadata_flag(metadata: &Option<std::collections::HashMap<String, Value>>, key: &str) -> bool {
    metadata
        .as_ref()
        .and_then(|metadata| metadata.get(key))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn serialization_error(target: &FileTarget, error: serde_json::Error) -> ExtractFileError {
    ExtractFileError {
        kind: ExtractFileErrorKind::Serialize,
        path: target.absolute_path.display().to_string(),
        root_relative_path: target.root_relative_path.clone(),
        message: format!("extraction row metadata could not be serialized: {error}"),
        content_hash: None,
        content_bytes: None,
    }
}

fn exact_reference_site_id(file_id: &str, start_byte: u32, end_byte: u32) -> String {
    stable_id(
        "reference_site",
        [
            file_id.to_string(),
            start_byte.to_string(),
            end_byte.to_string(),
        ],
    )
}

fn reference_site_id(
    file_id: &str,
    span: Option<&NormalizedSpan>,
    row_specific_id: &str,
) -> String {
    span.map_or_else(
        || stable_id("reference_site_spanless", [file_id, row_specific_id]),
        |span| exact_reference_site_id(file_id, span.start_byte, span.end_byte),
    )
}

fn pending_id(
    from_symbol_id: &str,
    target_name: &str,
    kind: &str,
    line_number: u32,
    span: Option<&NormalizedSpan>,
) -> String {
    // Spanless rows keep the historical (from, name, kind, line) identity so
    // their dedup behavior is unchanged. When a call-site span is present, fold
    // in the occurrence's start_byte/start_column so two same-name calls on one
    // line become distinct rows.
    match span {
        Some(span) => stable_id(
            "pending_relationship",
            [
                from_symbol_id,
                target_name,
                kind,
                line_number.to_string().as_str(),
                span.start_byte.to_string().as_str(),
                span.start_column.to_string().as_str(),
            ],
        ),
        None => stable_id(
            "pending_relationship",
            [
                from_symbol_id,
                target_name,
                kind,
                line_number.to_string().as_str(),
            ],
        ),
    }
}

fn type_argument_usage_id(usage: &TypeArgumentUsage) -> String {
    stable_id("type_argument_usage", [usage.identifier_id.as_str()])
}

fn content_hash(content: &str) -> String {
    content_hash_bytes(content.as_bytes())
}

fn content_hash_bytes(content: &[u8]) -> String {
    format!("blake3:{}", blake3::hash(content).to_hex())
}

fn line_count(content: &str) -> i64 {
    if content.is_empty() {
        0
    } else {
        content.lines().count() as i64
    }
}

fn stable_id<I, S>(prefix: &str, parts: I) -> String
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut hasher = blake3::Hasher::new();
    for part in parts {
        hasher.update(part.as_ref().as_bytes());
        hasher.update(b"\x1f");
    }
    let hex = hasher.finalize().to_hex().to_string();
    format!("{prefix}-{}", &hex[..32])
}

fn decode_error(
    target: &FileTarget,
    error: SourceDecodeError,
    content_hash: &str,
    content_bytes: i64,
) -> ExtractFileError {
    match error {
        SourceDecodeError::Utf8(error) => utf8_error(target, error, content_hash, content_bytes),
        SourceDecodeError::Utf16 { encoding, message } => ExtractFileError {
            kind: ExtractFileErrorKind::Read,
            path: target.absolute_path.display().to_string(),
            root_relative_path: target.root_relative_path.clone(),
            message: format!("source file could not be read as {encoding}: {message}"),
            content_hash: Some(content_hash.to_string()),
            content_bytes: Some(content_bytes),
        },
    }
}

fn utf8_error(
    target: &FileTarget,
    error: FromUtf8Error,
    content_hash: &str,
    content_bytes: i64,
) -> ExtractFileError {
    ExtractFileError {
        kind: ExtractFileErrorKind::Read,
        path: target.absolute_path.display().to_string(),
        root_relative_path: target.root_relative_path.clone(),
        message: format!("source file could not be read as UTF-8: {error}"),
        content_hash: Some(content_hash.to_string()),
        content_bytes: Some(content_bytes),
    }
}

fn failure_parse_diagnostic(
    target: &FileTarget,
    error: &ExtractFileError,
    content_bytes: i64,
) -> ArtifactParseDiagnostic {
    ArtifactParseDiagnostic {
        diagnostic_id: stable_id(
            "parse_diagnostic",
            [
                target.root_relative_path.as_str(),
                "error",
                "1",
                "0",
                error.message.as_str(),
            ],
        ),
        kind: "error".to_string(),
        message: Some(error.message.clone()),
        start_line: 1,
        start_column: 0,
        end_line: 1,
        end_column: 0,
        start_byte: 0,
        end_byte: content_bytes,
        metadata_json: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receiver_context_is_stable_across_common_member_separators() {
        for (source, start, expected) in [
            ("service.run()", 8, "service"),
            ("service :: run()", 11, "service"),
            ("service->run()", 9, "service"),
            ("service?.run()", 9, "service"),
            ("$service->run()", 10, "$service"),
            ("@service.run()", 9, "@service"),
        ] {
            assert_eq!(
                receiver_before_identifier(source, start),
                Some(expected.to_string())
            );
        }
        assert_eq!(receiver_before_identifier("run()", 0), None);
        for (source, start) in [
            ("foo<Bar>::baz()", 10),
            ("value - member", 8),
            ("value > member", 8),
            ("value ? member", 8),
        ] {
            assert_eq!(receiver_before_identifier(source, start), None);
        }
    }
    use std::sync::Mutex;

    static PANIC_HOOK_LOCK: Mutex<()> = Mutex::new(());

    fn sample_target() -> FileTarget {
        FileTarget {
            absolute_path: std::path::PathBuf::from("/tmp/test/x.rs"),
            root_relative_path: "x.rs".to_string(),
        }
    }

    fn sample_snapshot() -> SourceSnapshot {
        SourceSnapshot {
            content: String::new(),
            content_hash: "blake3:deadbeef".to_string(),
            content_bytes: 0,
            line_count: Some(0),
        }
    }

    fn span_at(start_byte: u32, start_column: u32) -> NormalizedSpan {
        NormalizedSpan {
            start_line: 5,
            start_column,
            end_line: 5,
            end_column: start_column + 3,
            start_byte,
            end_byte: start_byte + 3,
        }
    }

    #[test]
    fn exact_reference_site_id_depends_only_on_file_and_byte_span() {
        let span = span_at(40, 11);

        assert_eq!(
            reference_site_id("file-a", Some(&span), "identifier-a"),
            reference_site_id("file-a", Some(&span), "relationship-b")
        );
    }

    #[test]
    fn reference_site_id_keeps_same_line_occurrences_and_spanless_rows_distinct() {
        assert_ne!(
            reference_site_id("file-a", Some(&span_at(40, 11)), "identifier-a"),
            reference_site_id("file-a", Some(&span_at(48, 19)), "identifier-b")
        );
        assert_ne!(
            reference_site_id("file-a", None, "relationship-a"),
            reference_site_id("file-a", None, "relationship-b")
        );
    }

    /// Invariant: a spanless pending row keeps the historical
    /// (from, name, kind, line) identity, so two spanless occurrences with the
    /// same key still dedup to one id (no regression for legacy/spanless rows).
    #[test]
    fn pending_id_without_span_is_stable_and_dedups() {
        let a = pending_id("from#1", "bar", "calls", 5, None);
        let b = pending_id("from#1", "bar", "calls", 5, None);
        assert_eq!(a, b, "spanless ids for the same key must be identical");
    }

    /// Invariant: adding a span never collides with the spanless id, and two
    /// same-line occurrences with different byte offsets get distinct ids —
    /// the property the occurrence-distinct row test relies on.
    #[test]
    fn pending_id_with_distinct_spans_is_occurrence_distinct() {
        let spanless = pending_id("from#1", "bar", "calls", 5, None);
        let first = pending_id("from#1", "bar", "calls", 5, Some(&span_at(40, 11)));
        let second = pending_id("from#1", "bar", "calls", 5, Some(&span_at(48, 19)));

        assert_ne!(
            spanless, first,
            "spanned id must not collide with the spanless id"
        );
        assert_ne!(
            first, second,
            "two same-line occurrences must produce distinct ids"
        );

        // Same span twice is deterministic (stable id).
        let first_again = pending_id("from#1", "bar", "calls", 5, Some(&span_at(40, 11)));
        assert_eq!(first, first_again, "same span must yield the same id");
    }

    fn utf16le_bom_bytes(content: &str) -> Vec<u8> {
        let mut bytes = vec![0xff, 0xfe];
        for unit in content.encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn read_source_snapshot_decodes_utf16le_bom_source() {
        let temp_dir = tempfile::tempdir().expect("temp dir");
        let path = temp_dir.path().join("dbo.SqlCommandType.sql");
        let source = "MERGE dbo.SqlCommandType;\nSELECT N'run';\n";
        let bytes = utf16le_bom_bytes(source);
        fs::write(&path, &bytes).expect("write UTF-16LE fixture");

        let target = FileTarget {
            absolute_path: path,
            root_relative_path: "dbo.SqlCommandType.sql".to_string(),
        };

        let snapshot = read_source_snapshot(&target).expect("UTF-16LE source should decode");

        assert_eq!(snapshot.content, source);
        assert_eq!(snapshot.content_bytes, bytes.len() as i64);
        assert_eq!(snapshot.content_hash, content_hash_bytes(&bytes));
        assert_eq!(snapshot.line_count, Some(2));
    }

    #[test]
    fn catch_extraction_panic_converts_panic_into_failed_extract_error() {
        let _hook_lock = PANIC_HOOK_LOCK.lock().unwrap();
        // Suppress the default panic hook so the deliberately-induced panic does not
        // print a backtrace line to the test output.
        let previous_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let result = catch_extraction_panic(
            &sample_target(),
            &sample_snapshot(),
            || -> Result<ExtractionResults, String> { panic!("boom in extractor") },
        );
        std::panic::set_hook(previous_hook);

        let error = result.expect_err("a panicking extractor must produce an error, not unwind");
        assert_eq!(error.kind, ExtractFileErrorKind::Extract);
        assert!(
            error.message.contains("boom in extractor"),
            "panic payload should be surfaced, got: {}",
            error.message
        );
        assert_eq!(error.content_hash.as_deref(), Some("blake3:deadbeef"));
        assert_eq!(error.root_relative_path, "x.rs");
    }

    #[test]
    fn catch_extraction_panic_maps_returned_error() {
        let error = catch_extraction_panic(&sample_target(), &sample_snapshot(), || {
            Err::<ExtractionResults, String>("parse exploded".to_string())
        })
        .expect_err("a returned Err must map to an ExtractFileError");
        assert_eq!(error.kind, ExtractFileErrorKind::Extract);
        assert!(error.message.contains("parse exploded"));
    }

    #[test]
    fn catch_extraction_panic_passes_through_success() {
        let results = catch_extraction_panic(&sample_target(), &sample_snapshot(), || {
            Ok::<ExtractionResults, String>(ExtractionResults::empty())
        })
        .expect("successful extraction must pass through unchanged");
        assert!(results.symbols.is_empty());
    }

    #[test]
    fn map_results_dedupes_structural_facts_by_id_before_artifact_write() {
        let mut results = ExtractionResults::empty();
        let fact = StructuralFact {
            id: "structural-fact:duplicate".to_string(),
            file_path: "x.rs".to_string(),
            language: "rust".to_string(),
            pattern_id: "review.duplicate.v1".to_string(),
            capture_name: "first".to_string(),
            node_kind: "identifier".to_string(),
            containing_symbol_id: None,
            start_line: 1,
            start_column: 0,
            end_line: 1,
            end_column: 1,
            start_byte: 0,
            end_byte: 1,
            confidence: 1.0,
            metadata: None,
        };
        results.structural_facts.push(fact.clone());

        let mut duplicate = fact;
        duplicate.capture_name = "second".to_string();
        results.structural_facts.push(duplicate);

        let artifact = map_results(
            &sample_target(),
            "rust".to_string(),
            "2026-07-04T00:00:00Z".to_string(),
            &sample_snapshot(),
            results,
        )
        .expect("mapping duplicate structural fact ids should not fail");

        assert_eq!(
            artifact.structural_facts.len(),
            1,
            "CLI artifact mapping must dedupe structural facts before writer insertion"
        );
        assert_eq!(
            artifact.structural_facts[0].structural_fact_id,
            "structural-fact:duplicate"
        );
    }
}
