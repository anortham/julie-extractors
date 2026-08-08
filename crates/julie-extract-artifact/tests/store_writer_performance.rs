use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use julie_extract_artifact::model::{
    ArtifactCapabilityFlags, ArtifactCapabilitySnapshot, ArtifactFile, ArtifactIdentifier,
    ArtifactLanguageCapabilityFixtureRow, ArtifactLanguageCapabilityGapRow,
    ArtifactLanguageCapabilityRow, ArtifactParserInventoryRow, ArtifactRelationship,
    ArtifactSymbol, CapabilityGapStatus, FileStatus, ReferenceSiteProvenance,
};
use julie_extract_artifact::store::{
    StoreConnectionFactory, StoreFileVersion, StoreLayout, StoreLevel, StoreWriteRequest,
    StoreWriter,
};

const CREATED_AT: &str = "2026-08-07T12:00:00Z";

#[test]
fn statement_sets_are_prepared_once_per_level_transaction() {
    let small_store = TestStore::new("prepare-once-small");
    let mut small_writer = small_store.writer();
    small_writer.stage_capability_snapshot(1, dense_capability_snapshot(1));
    let small_version = StoreFileVersion::try_from_artifact_file(1, &dense_file(1)).unwrap();
    let small_l1 = small_writer
        .write_level(&request("request-small-l1"), &small_version, StoreLevel::L1)
        .unwrap();

    let store = TestStore::new("prepare-once-large");
    let mut writer = store.writer();
    writer.stage_capability_snapshot(1, dense_capability_snapshot(500));
    let version = StoreFileVersion::try_from_artifact_file(1, &dense_file(500)).unwrap();

    let l1 = writer
        .write_level(&request("request-l1"), &version, StoreLevel::L1)
        .unwrap();
    let l2 = writer
        .write_level(&request("request-l2"), &version, StoreLevel::L2)
        .unwrap();
    let l3 = writer
        .write_level(&request("request-l3"), &version, StoreLevel::L3)
        .unwrap();

    assert_eq!(small_l1.statement_preparations, 21);
    assert_eq!(l1.statement_preparations, 21);
    assert_eq!(l1.statement_preparations, small_l1.statement_preparations);
    assert_eq!(l2.statement_preparations, 2);
    assert_eq!(l3.statement_preparations, 5);
    assert_eq!(l1.counts.symbols, 500);
    assert_eq!(l2.counts.identifiers, 500);
    assert_eq!(table_count(writer.connection(), "store_log"), 3);
}

#[test]
fn symbols_and_full_files_have_identical_l1_projections_across_languages() {
    for (path, language, hash) in [
        ("src/lib.rs", "rust", "blake3:rust"),
        ("src/Program.cs", "csharp", "blake3:csharp"),
    ] {
        let mut full = dense_file(2);
        full.path = path.to_string();
        full.language = language.to_string();
        full.content_hash = hash.to_string();
        full.file_id = format!("file-{language}");
        full.identifiers[0].start_line = 1;
        full.identifiers[0].start_column = 0;
        full.identifiers[0].end_line = 1;
        full.identifiers[0].end_column = 1;
        full.identifiers[0].start_byte = 0;
        full.identifiers[0].end_byte = 1;
        let mut symbols = full.clone();
        symbols.identifiers.clear();

        let symbols = StoreFileVersion::try_from_artifact_file(1, &symbols).unwrap();
        let full = StoreFileVersion::try_from_artifact_file(1, &full).unwrap();

        assert!(symbols.l1_projection_equals(&full));

        let symbols_store = TestStore::new(&format!("symbols-{language}"));
        let mut symbols_writer = symbols_store.writer();
        symbols_writer.stage_capability_snapshot(1, capability_snapshot(language));
        let symbols_result = symbols_writer
            .write_level(
                &request(&format!("request-symbols-{language}")),
                &symbols,
                StoreLevel::L1,
            )
            .unwrap();

        let full_store = TestStore::new(&format!("full-{language}"));
        let mut full_writer = full_store.writer();
        full_writer.stage_capability_snapshot(1, capability_snapshot(language));
        let full_result = full_writer
            .write_level(
                &request(&format!("request-full-{language}")),
                &full,
                StoreLevel::L1,
            )
            .unwrap();

        for table in [
            "file_versions",
            "symbols",
            "symbol_annotations",
            "reference_sites",
            "relationships",
            "pending_relationships",
            "type_facts",
            "complexity_metrics",
            "parse_diagnostics",
        ] {
            assert_eq!(
                version_rows(
                    symbols_writer.connection(),
                    table,
                    symbols_result.version_id
                ),
                version_rows(full_writer.connection(), table, full_result.version_id),
                "{language} {table}"
            );
        }
    }
}

fn dense_capability_snapshot(rows: usize) -> ArtifactCapabilitySnapshot {
    let parser_inventory = (0..rows)
        .map(|index| ArtifactParserInventoryRow {
            language: format!("language-{index}"),
            parser_package: format!("parser-{index}"),
            parser_version: Some(format!("1.0.{index}")),
            grammar_version: Some(index.to_string()),
            source: Some("built-in".to_string()),
            metadata: Some(serde_json::json!({"index": index})),
        })
        .collect::<Vec<_>>();
    let languages = (0..rows)
        .map(|index| capability_language(&format!("language-{index}"), &format!("parser-{index}")))
        .collect::<Vec<_>>();
    ArtifactCapabilitySnapshot {
        parser_inventory,
        languages,
    }
}

fn capability_snapshot(language: &str) -> ArtifactCapabilitySnapshot {
    let parser_package = format!("parser-{language}");
    ArtifactCapabilitySnapshot {
        parser_inventory: vec![ArtifactParserInventoryRow {
            language: language.to_string(),
            parser_package: parser_package.clone(),
            parser_version: Some("1.0.0".to_string()),
            grammar_version: Some("1".to_string()),
            source: Some("built-in".to_string()),
            metadata: None,
        }],
        languages: vec![capability_language(language, &parser_package)],
    }
}

fn capability_language(language: &str, parser_package: &str) -> ArtifactLanguageCapabilityRow {
    ArtifactLanguageCapabilityRow {
        language: language.to_string(),
        parser_package: parser_package.to_string(),
        extensions: vec![format!(".{language}")],
        dependency_status: "available".to_string(),
        target_capabilities: ArtifactCapabilityFlags {
            symbols: true,
            relationships: true,
            pending_relationships: true,
            identifiers: true,
            types: true,
        },
        actual_capabilities: ArtifactCapabilityFlags {
            symbols: true,
            relationships: true,
            pending_relationships: true,
            identifiers: true,
            types: true,
        },
        kind_coverage: serde_json::json!({"function": true}),
        fixtures: vec![ArtifactLanguageCapabilityFixtureRow {
            fixture_name: "basic".to_string(),
            source_path: format!("fixtures/{language}/basic.source"),
            expected_path: format!("fixtures/{language}/basic.json"),
        }],
        gaps: vec![ArtifactLanguageCapabilityGapRow {
            gap_id: format!("gap-{language}"),
            capability: "fixture".to_string(),
            status: CapabilityGapStatus::Exception,
            reason: "fixture".to_string(),
            required_closure: "none".to_string(),
            evidence: serde_json::json!({"accepted": true}),
        }],
    }
}

fn version_rows(
    connection: &rusqlite::Connection,
    table: &str,
    version_id: i64,
) -> Vec<Vec<rusqlite::types::Value>> {
    let columns = table_columns(connection, table)
        .into_iter()
        .filter(|column| {
            column != "version_id" && !column.starts_with("complete_l") && column != "created_at"
        })
        .collect::<Vec<_>>();
    let projection = columns.join(", ");
    let mut statement = connection
        .prepare(&format!(
            "SELECT {projection} FROM {table} WHERE version_id = ?1"
        ))
        .unwrap();
    let mut rows = statement
        .query_map([version_id], |row| {
            (0..columns.len())
                .map(|index| row.get(index))
                .collect::<rusqlite::Result<Vec<_>>>()
        })
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    rows.sort_by_key(|row| format!("{row:?}"));
    rows
}

fn table_columns(connection: &rusqlite::Connection, table: &str) -> Vec<String> {
    connection
        .prepare(&format!("PRAGMA table_info({table})"))
        .unwrap()
        .query_map([], |row| row.get(1))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
}

fn dense_file(rows: usize) -> ArtifactFile {
    let symbols = (0..rows)
        .map(|index| ArtifactSymbol {
            symbol_id: format!("symbol-{index}"),
            name: format!("symbol_{index}"),
            kind: "function".to_string(),
            start_line: index as i64 + 1,
            end_line: index as i64 + 1,
            start_byte: index as i64,
            end_byte: index as i64 + 1,
            ..ArtifactSymbol::default()
        })
        .collect::<Vec<_>>();
    let identifiers = (0..rows)
        .map(|index| ArtifactIdentifier {
            identifier_id: format!("identifier-{index}"),
            reference_site_id: format!("site-{index}"),
            name: format!("symbol_{index}"),
            kind: "call".to_string(),
            containing_symbol_id: Some(format!("symbol-{index}")),
            start_line: index as i64 + 1,
            start_column: 0,
            end_line: index as i64 + 1,
            end_column: 1,
            start_byte: index as i64,
            end_byte: index as i64 + 1,
            site_is_exact: true,
            site_provenance: ReferenceSiteProvenance::TargetToken,
            confidence: 1.0,
            code_context: None,
            metadata_json: None,
        })
        .collect::<Vec<_>>();
    ArtifactFile {
        file_id: "file-rust".to_string(),
        path: "src/lib.rs".to_string(),
        language: "rust".to_string(),
        content_hash: "blake3:dense".to_string(),
        content_bytes: rows as i64,
        line_count: Some(rows as i64),
        indexed_at: CREATED_AT.to_string(),
        status: FileStatus::Indexed,
        metadata_json: Some(r#"{"dense":true}"#.to_string()),
        symbols,
        symbol_annotations: Vec::new(),
        identifiers,
        relationships: vec![ArtifactRelationship {
            relationship_id: "relationship-0".to_string(),
            reference_site_id: "site-0".to_string(),
            from_symbol_id: "symbol-0".to_string(),
            to_symbol_id: "symbol-0".to_string(),
            kind: "calls".to_string(),
            start_line: Some(1),
            start_column: Some(0),
            end_line: Some(1),
            end_column: Some(1),
            start_byte: Some(0),
            end_byte: Some(1),
            site_is_exact: true,
            site_provenance: ReferenceSiteProvenance::TargetToken,
            confidence: 1.0,
            metadata_json: None,
        }],
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

fn request(request_id: &str) -> StoreWriteRequest {
    StoreWriteRequest::routine(request_id, CREATED_AT)
}

fn table_count(connection: &rusqlite::Connection, table: &str) -> i64 {
    connection
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap()
}

struct TestStore {
    path: PathBuf,
    layout: StoreLayout,
}

impl TestStore {
    fn new(name: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "julie-store-writer-performance-{name}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        let layout = StoreLayout::create(&path, "family-performance", "2.30.0").unwrap();
        Self { path, layout }
    }

    fn writer(&self) -> StoreWriter {
        let factory =
            StoreConnectionFactory::new(self.layout.clone(), "family-performance", "2.30.0");
        StoreWriter::open(&factory).unwrap()
    }
}

impl Drop for TestStore {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
