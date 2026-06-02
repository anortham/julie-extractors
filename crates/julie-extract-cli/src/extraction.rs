use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::string::FromUtf8Error;

use julie_extract_artifact::model::{
    ArtifactFile, ArtifactIdentifier, ArtifactLiteral, ArtifactParseDiagnostic,
    ArtifactPendingRelationship, ArtifactRelationship, ArtifactSymbol, ArtifactSymbolAnnotation,
    ArtifactTypeArgument, ArtifactTypeArgumentUsage, ArtifactTypeFact, FileStatus,
};
use julie_extractors::base::StructuredPendingRelationship;
use julie_extractors::language_policy::classify_literals_by_carrier;
use julie_extractors::{
    ExtractionResults, Literal, ParseDiagnosticKind, PendingRelationship, TypeArgument,
    TypeArgumentUsage, TypeInfo, extract_canonical,
};
use serde::Serialize;
use serde_json::Value;

use crate::paths::FileTarget;

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

pub(crate) fn extract_artifact_file(
    root: &Path,
    target: &FileTarget,
    language: String,
    indexed_at: String,
) -> Result<ArtifactFile, ExtractFileError> {
    let snapshot = read_source_snapshot(target)?;
    extract_artifact_file_from_snapshot(root, target, language, indexed_at, snapshot)
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
    let content = String::from_utf8(bytes)
        .map_err(|error| utf8_error(target, error, &content_hash, content_bytes))?;

    Ok(SourceSnapshot {
        content_hash,
        content_bytes,
        line_count: Some(line_count(&content)),
        content,
    })
}

pub(crate) fn extract_artifact_file_from_snapshot(
    root: &Path,
    target: &FileTarget,
    language: String,
    indexed_at: String,
    snapshot: SourceSnapshot,
) -> Result<ArtifactFile, ExtractFileError> {
    let mut results = catch_extraction_panic(target, &snapshot, || {
        extract_canonical(&target.root_relative_path, &snapshot.content, root)
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

    let identifiers = dedupe_by_id(map_identifiers(&results, target)?, |identifier| {
        identifier.identifier_id.as_str()
    });
    let relationships = dedupe_by_id(map_relationships(&results, target)?, |relationship| {
        relationship.relationship_id.as_str()
    });
    let pending_relationships =
        dedupe_by_id(map_pending_relationships(&results, target)?, |pending| {
            pending.pending_relationship_id.as_str()
        });
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
) -> Result<Vec<ArtifactIdentifier>, ExtractFileError> {
    results
        .identifiers
        .iter()
        .map(|identifier| {
            Ok(ArtifactIdentifier {
                identifier_id: identifier.id.clone(),
                name: identifier.name.clone(),
                kind: identifier.kind.to_string(),
                containing_symbol_id: identifier.containing_symbol_id.clone(),
                target_symbol_id: identifier.target_symbol_id.clone(),
                start_line: i64::from(identifier.start_line),
                start_column: i64::from(identifier.start_column),
                end_line: i64::from(identifier.end_line),
                end_column: i64::from(identifier.end_column),
                start_byte: i64::from(identifier.start_byte),
                end_byte: i64::from(identifier.end_byte),
                confidence: f64::from(identifier.confidence),
                code_context: identifier.code_context.clone(),
                metadata_json: None,
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| serialization_error(target, error))
}

fn map_relationships(
    results: &ExtractionResults,
    target: &FileTarget,
) -> Result<Vec<ArtifactRelationship>, ExtractFileError> {
    results
        .relationships
        .iter()
        .map(|relationship| {
            Ok(ArtifactRelationship {
                relationship_id: relationship.id.clone(),
                from_symbol_id: relationship.from_symbol_id.clone(),
                to_symbol_id: relationship.to_symbol_id.clone(),
                kind: relationship.kind.to_string(),
                start_line: Some(i64::from(relationship.line_number)),
                start_column: None,
                end_line: None,
                end_column: None,
                start_byte: None,
                end_byte: None,
                confidence: f64::from(relationship.confidence),
                metadata_json: optional_json(&relationship.metadata, target)?,
            })
        })
        .collect()
}

fn map_pending_relationships(
    results: &ExtractionResults,
    target: &FileTarget,
) -> Result<Vec<ArtifactPendingRelationship>, ExtractFileError> {
    if !results.structured_pending_relationships.is_empty() {
        return results
            .structured_pending_relationships
            .iter()
            .map(|pending| map_structured_pending(pending, target))
            .collect();
    }

    results
        .pending_relationships
        .iter()
        .map(|pending| map_legacy_pending(pending, target))
        .collect()
}

fn map_structured_pending(
    pending: &StructuredPendingRelationship,
    target: &FileTarget,
) -> Result<ArtifactPendingRelationship, ExtractFileError> {
    let namespace_json = json_string(&pending.target.namespace_path, target)
        .map_err(|error| serialization_error(target, error))?;
    Ok(ArtifactPendingRelationship {
        pending_relationship_id: pending_id(
            pending.pending.from_symbol_id.as_str(),
            pending.target.display_name.as_str(),
            pending.pending.kind.to_string().as_str(),
            pending.pending.line_number,
        ),
        from_symbol_id: pending.pending.from_symbol_id.clone(),
        caller_scope_symbol_id: pending.caller_scope_symbol_id.clone(),
        kind: pending.pending.kind.to_string(),
        target_display_name: pending.target.display_name.clone(),
        target_terminal_name: pending.target.terminal_name.clone(),
        target_receiver: pending.target.receiver.clone(),
        target_namespace_json: namespace_json,
        target_import_context: pending.target.import_context.clone(),
        start_line: i64::from(pending.pending.line_number),
        start_column: None,
        end_line: None,
        end_column: None,
        start_byte: None,
        end_byte: None,
        confidence: f64::from(pending.pending.confidence),
        metadata_json: None,
    })
}

fn map_legacy_pending(
    pending: &PendingRelationship,
    _target: &FileTarget,
) -> Result<ArtifactPendingRelationship, ExtractFileError> {
    Ok(ArtifactPendingRelationship {
        pending_relationship_id: pending_id(
            pending.from_symbol_id.as_str(),
            pending.callee_name.as_str(),
            pending.kind.to_string().as_str(),
            pending.line_number,
        ),
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
            };
            ArtifactParseDiagnostic {
                diagnostic_id: stable_id(
                    "parse_diagnostic",
                    [
                        target.root_relative_path.as_str(),
                        kind,
                        diagnostic.start_line.to_string().as_str(),
                        diagnostic.start_column.to_string().as_str(),
                        diagnostic.end_line.to_string().as_str(),
                        diagnostic.end_column.to_string().as_str(),
                    ],
                ),
                kind: kind.to_string(),
                message: None,
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

fn json_string<T: Serialize>(value: &T, _target: &FileTarget) -> Result<String, serde_json::Error> {
    serde_json::to_string(value)
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

fn pending_id(from_symbol_id: &str, target_name: &str, kind: &str, line_number: u32) -> String {
    stable_id(
        "pending_relationship",
        [
            from_symbol_id,
            target_name,
            kind,
            line_number.to_string().as_str(),
        ],
    )
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

    #[test]
    fn catch_extraction_panic_converts_panic_into_failed_extract_error() {
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
}
