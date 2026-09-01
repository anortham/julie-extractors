use std::collections::HashMap;

use serde_json::{Number, Value};

use super::source_regions::{SourceRegionLine, source_region_lines};
use super::span::NormalizedSpan;
use super::types::{SourceRegion, SourceRegionKind, StructuralFact, stable_location_id};

const PATTERN_ID: &str = "code.marker.v1";
const CAPTURE_NAME: &str = "marker";
const QUERY_FAMILY: &str = "marker";
const MARKERS: [&str; 5] = ["TODO", "FIXME", "HACK", "XXX", "RAZORBACK"];

pub fn collect_marker_structural_facts(
    content: &str,
    source_regions: &[SourceRegion],
) -> Vec<StructuralFact> {
    source_regions
        .iter()
        .filter(|region| {
            matches!(
                region.kind,
                SourceRegionKind::Comment | SourceRegionKind::DocComment
            )
        })
        .flat_map(|region| {
            source_region_lines(content, region)
                .into_iter()
                .filter_map(|line| marker_fact_for_line(content, region, line))
        })
        .collect()
}

fn marker_fact_for_line(
    content: &str,
    region: &SourceRegion,
    line: SourceRegionLine<'_>,
) -> Option<StructuralFact> {
    let semantic = semantic_line(line.text)?;
    let marker = marker_prefix(semantic.text)?;
    let end_offset = semantic.text.trim_end().len();
    let start_byte = line.start_byte + semantic.start_offset;
    let end_byte = line.start_byte + semantic.start_offset + end_offset;
    let span = NormalizedSpan::from_content_range(content, start_byte, end_byte)?;

    let mut metadata = HashMap::from([
        (
            "pattern_version".to_string(),
            Value::Number(Number::from(1)),
        ),
        (
            "query_family".to_string(),
            Value::String(QUERY_FAMILY.to_string()),
        ),
        ("marker".to_string(), Value::String(marker.name.to_string())),
        (
            "source_region_kind".to_string(),
            Value::String(region.kind.as_str().to_string()),
        ),
    ]);
    if let Some(owner) = marker.owner {
        metadata.insert("owner".to_string(), Value::String(owner.to_string()));
    }
    if let Some(description) = marker.description {
        metadata.insert(
            "description".to_string(),
            Value::String(description.to_string()),
        );
    }

    Some(StructuralFact {
        id: stable_location_id(
            &region.file_path,
            &format!("{PATTERN_ID}:{CAPTURE_NAME}"),
            span,
        ),
        file_path: region.file_path.clone(),
        language: region.language.clone(),
        pattern_id: PATTERN_ID.to_string(),
        capture_name: CAPTURE_NAME.to_string(),
        node_kind: region.kind.as_str().to_string(),
        containing_symbol_id: region.containing_symbol_id.clone(),
        start_line: span.start_line,
        start_column: span.start_column,
        end_line: span.end_line,
        end_column: span.end_column,
        start_byte: span.start_byte,
        end_byte: span.end_byte,
        confidence: 1.0,
        metadata: Some(metadata),
    })
}

struct SemanticLine<'a> {
    text: &'a str,
    start_offset: usize,
}

fn semantic_line(line: &str) -> Option<SemanticLine<'_>> {
    let leading_trimmed = line.trim_start();
    let mut start_offset = line.len() - leading_trimmed.len();
    let mut text = leading_trimmed;

    for decoration in [
        "<!--", "/**", "/*!", "///", "//!", "'''", "---", "@*", "<#", "%%%", "/*", "//", "##",
        "#'", "--", "%%", "'", "#", "%", "*",
    ] {
        if let Some(rest) = text.strip_prefix(decoration) {
            text = rest;
            start_offset += decoration.len();
            break;
        }
    }

    let trimmed = text.trim_start();
    start_offset += text.len() - trimmed.len();
    text = trimmed;

    for decoration in ["-->", "*/", "*@", "#>"] {
        let trimmed_end = text.trim_end();
        if let Some(rest) = trimmed_end.strip_suffix(decoration) {
            text = rest.trim_end();
            break;
        }
    }

    (!text.is_empty()).then_some(SemanticLine { text, start_offset })
}

struct Marker<'a> {
    name: &'static str,
    owner: Option<&'a str>,
    description: Option<&'a str>,
}

fn marker_prefix(text: &str) -> Option<Marker<'_>> {
    let marker = MARKERS.iter().copied().find(|candidate| {
        text.get(..candidate.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(candidate))
    })?;
    let mut rest = text.get(marker.len()..)?;
    if !rest.is_empty()
        && !rest.starts_with(char::is_whitespace)
        && !rest.starts_with('(')
        && !rest.starts_with(':')
        && !rest.starts_with('-')
    {
        return None;
    }

    let owner = rest
        .strip_prefix('(')
        .and_then(|owner_text| owner_text.split_once(')'))
        .and_then(|(owner, after_owner)| {
            let owner = owner.trim();
            if owner.is_empty() {
                return None;
            }
            rest = after_owner;
            Some(owner)
        });

    rest = rest.trim_start();
    rest = rest
        .strip_prefix(':')
        .or_else(|| rest.strip_prefix('-'))
        .unwrap_or(rest)
        .trim_start();
    let description = (!rest.is_empty()).then_some(rest.trim_end());

    Some(Marker {
        name: marker,
        owner,
        description,
    })
}
