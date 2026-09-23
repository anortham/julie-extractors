//! Relationships for SQL schema objects such as views and triggers.

use crate::base::{BaseExtractor, Relationship, RelationshipKind, Symbol, SymbolKind};
use regex::Regex;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::LazyLock;
use tree_sitter::Node;

// Best-effort fallback regex used only when the SQL parser produces an ERROR
// node. Captures the FIRST `FROM <identifier>` after `AS`, so a view that
// references a CTE (`WITH cte AS (SELECT ... FROM x) SELECT ... FROM y`) or
// nests subqueries (`SELECT * FROM (SELECT ... FROM x) sub`) will bind to the
// inner table (`x`) instead of the outer one (`y`). The AST path is exact and
// handles those cases correctly; this fallback exists to recover something
// useful when the dialect-specific syntax fails to parse.
static CREATE_VIEW_SOURCE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?is)\bCREATE\s+(?:OR\s+REPLACE\s+)?(?:TEMP(?:ORARY)?\s+)?(?:RECURSIVE\s+)?VIEW\s+(?:IF\s+NOT\s+EXISTS\s+)?([a-zA-Z_][a-zA-Z0-9_\.]*)\s*(?:\([^)]*\)\s*)?AS\s+.*?\bFROM\s+([a-zA-Z_][a-zA-Z0-9_\.]*)",
    )
    .unwrap()
});

static CREATE_TRIGGER_TARGET_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?is)\bCREATE\s+(?:OR\s+REPLACE\s+)?(?:DEFINER\s*=\s*[a-zA-Z_][a-zA-Z0-9_\.]*\s+)?(?:CONSTRAINT\s+)?(?:TEMP(?:ORARY)?\s+)?TRIGGER\s+(?:IF\s+NOT\s+EXISTS\s+)?([a-zA-Z_][a-zA-Z0-9_\.]*)\s+(?:BEFORE|AFTER|INSTEAD\s+OF)\s+(?:INSERT|UPDATE|DELETE)(?:\s+OR\s+(?:INSERT|UPDATE|DELETE))*\s+ON\s+([a-zA-Z_][a-zA-Z0-9_\.]*)",
    )
    .unwrap()
});

/// T-SQL names the table before the timing: `CREATE TRIGGER dbo.t ON
/// dbo.Orders AFTER INSERT`.
static CREATE_TSQL_TRIGGER_TARGET_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?is)\bCREATE\s+(?:OR\s+ALTER\s+)?TRIGGER\s+([\w\.\[\]"]+)\s+ON\s+([\w\.\[\]"]+)\s+(?:AFTER|FOR|INSTEAD\s+OF)\b"#,
    )
    .unwrap()
});

pub(super) fn extract_error_relationships(
    base: &mut BaseExtractor,
    node: Node,
    symbols: &[Symbol],
    relationships: &mut Vec<Relationship>,
) {
    let error_text = base.get_node_text(&node);

    for captures in CREATE_VIEW_SOURCE_RE.captures_iter(&error_text) {
        let Some(view_match) = captures.get(1) else {
            continue;
        };
        let Some(table_match) = captures.get(2) else {
            continue;
        };
        let view_name = unqualified_name(view_match.as_str());
        let table_name = unqualified_name(table_match.as_str());
        let Some(view_symbol) =
            symbol_by_name_and_metadata(symbols, &view_name, SymbolKind::Interface, "isView")
        else {
            continue;
        };
        let Some(table_symbol) = symbols
            .iter()
            .find(|s| s.name == table_name && s.kind == SymbolKind::Class)
        else {
            continue;
        };

        push_table_relationship_at_byte(
            base,
            relationships,
            view_symbol,
            table_symbol,
            node.start_byte() + table_match.start(),
            "view_source",
            &table_name,
        );
    }

    for captures in CREATE_TRIGGER_TARGET_RE
        .captures_iter(&error_text)
        .chain(CREATE_TSQL_TRIGGER_TARGET_RE.captures_iter(&error_text))
    {
        let Some(trigger_match) = captures.get(1) else {
            continue;
        };
        let Some(table_match) = captures.get(2) else {
            continue;
        };
        let trigger_name = unqualified_name(trigger_match.as_str());
        let table_name = unqualified_name(table_match.as_str());
        let Some(trigger_symbol) =
            symbol_by_name_and_metadata(symbols, &trigger_name, SymbolKind::Method, "isTrigger")
        else {
            continue;
        };
        let Some(table_symbol) = symbols
            .iter()
            .find(|s| s.name == table_name && s.kind == SymbolKind::Class)
        else {
            continue;
        };

        push_table_relationship_at_byte(
            base,
            relationships,
            trigger_symbol,
            table_symbol,
            node.start_byte() + table_match.start(),
            "trigger_target",
            &table_name,
        );
    }
}

fn symbol_by_name_and_metadata<'a>(
    symbols: &'a [Symbol],
    name: &str,
    kind: SymbolKind,
    metadata_key: &str,
) -> Option<&'a Symbol> {
    symbols.iter().find(|symbol| {
        symbol.name == name
            && symbol.kind == kind
            && symbol
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get(metadata_key))
                .and_then(Value::as_bool)
                .unwrap_or(false)
    })
}

fn unqualified_name(name: &str) -> String {
    super::helpers::normalize_sql_identifier(name.rsplit('.').next().unwrap_or(name))
}

fn push_table_relationship_at_byte(
    base: &BaseExtractor,
    relationships: &mut Vec<Relationship>,
    source_symbol: &Symbol,
    target_symbol: &Symbol,
    relationship_byte: usize,
    relationship_type: &str,
    target_table_name: &str,
) {
    push_table_relationship_at_line(
        base,
        relationships,
        source_symbol,
        target_symbol,
        line_number_for_byte(base, relationship_byte),
        relationship_byte,
        relationship_type,
        target_table_name,
    );
}

#[allow(clippy::too_many_arguments)]
fn push_table_relationship_at_line(
    base: &BaseExtractor,
    relationships: &mut Vec<Relationship>,
    source_symbol: &Symbol,
    target_symbol: &Symbol,
    line_number: u32,
    relationship_byte: usize,
    relationship_type: &str,
    target_table_name: &str,
) {
    let mut metadata = HashMap::new();
    metadata.insert(
        "relationshipType".to_string(),
        Value::String(relationship_type.to_string()),
    );
    metadata.insert(
        "targetTable".to_string(),
        Value::String(target_table_name.to_string()),
    );
    metadata.insert("isExternal".to_string(), Value::Bool(false));

    relationships.push(Relationship {
        id: format!(
            "{}_{}_{:?}_{}_{}",
            source_symbol.id,
            target_symbol.id,
            RelationshipKind::References,
            relationship_type,
            relationship_byte
        ),
        from_symbol_id: source_symbol.id.clone(),
        to_symbol_id: target_symbol.id.clone(),
        kind: RelationshipKind::References,
        file_path: base.file_path.clone(),
        line_number,
        span: crate::base::NormalizedSpan::from_content_range(
            &base.content,
            relationship_byte,
            relationship_byte + target_table_name.len(),
        ),
        reference_site_is_exact: false,
        confidence: 0.95,
        metadata: Some(metadata),
    });
}

fn line_number_for_byte(base: &BaseExtractor, byte_offset: usize) -> u32 {
    base.content
        .as_bytes()
        .iter()
        .take(byte_offset)
        .filter(|byte| **byte == b'\n')
        .count() as u32
        + 1
}
