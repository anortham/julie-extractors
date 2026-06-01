use std::fs;
use std::path::Path;

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
}

pub(crate) fn extract_artifact_file(
    root: &Path,
    target: &FileTarget,
    language: String,
    indexed_at: String,
) -> Result<ArtifactFile, ExtractFileError> {
    let content = fs::read_to_string(&target.absolute_path).map_err(|error| ExtractFileError {
        kind: ExtractFileErrorKind::Read,
        path: target.absolute_path.display().to_string(),
        root_relative_path: target.root_relative_path.clone(),
        message: format!("source file could not be read as UTF-8: {error}"),
    })?;

    let mut results =
        extract_canonical(&target.root_relative_path, &content, root).map_err(|error| {
            ExtractFileError {
                kind: ExtractFileErrorKind::Extract,
                path: target.absolute_path.display().to_string(),
                root_relative_path: target.root_relative_path.clone(),
                message: error.to_string(),
            }
        })?;
    classify_literals_by_carrier(&mut results.literals);

    map_results(target, language, indexed_at, &content, results)
}

fn map_results(
    target: &FileTarget,
    language: String,
    indexed_at: String,
    content: &str,
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

    Ok(ArtifactFile {
        file_id,
        path,
        language,
        content_hash: content_hash(content),
        content_bytes: content.len() as i64,
        line_count: Some(line_count(content)),
        indexed_at,
        status: FileStatus::Indexed,
        metadata_json: None,
        symbols,
        symbol_annotations,
        identifiers: map_identifiers(&results, target)?,
        relationships: map_relationships(&results, target)?,
        pending_relationships: map_pending_relationships(&results, target)?,
        type_facts: map_type_facts(type_infos, target)?,
        type_argument_usages: map_type_argument_usages(&results.type_argument_usages),
        type_arguments: map_type_arguments(&results.type_argument_usages),
        literals: map_literals(&results.literals),
        parse_diagnostics: map_parse_diagnostics(&results, target),
    })
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

fn serialization_error(target: &FileTarget, error: serde_json::Error) -> ExtractFileError {
    ExtractFileError {
        kind: ExtractFileErrorKind::Serialize,
        path: target.absolute_path.display().to_string(),
        root_relative_path: target.root_relative_path.clone(),
        message: format!("extraction row metadata could not be serialized: {error}"),
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
    format!("blake3:{}", blake3::hash(content.as_bytes()).to_hex())
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
