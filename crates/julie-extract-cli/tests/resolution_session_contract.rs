#![cfg(feature = "test-store-resolution-contract")]

use std::collections::BTreeSet;
use std::path::Path;

use julie_extract_artifact::resolution_store::{read_resolution_metadata, resolution_report};
use julie_extract_cli::resolution::run_resolution_session;
use julie_extract_cli::resolution_session::{
    LegacyResolutionSession, ResolutionCorpusIdentity, ResolutionPassRequest, ResolutionPhase,
    ResolutionSession, ResolutionWorklists, ResolutionWrite, ResolutionWriteBatch,
    SemanticIdentifierId, SemanticSymbolId, SemanticVersionId, SessionRelationship,
    SessionResolutionState,
};
use rusqlite::Connection;
use serde_json::{Value, json};
use tempfile::TempDir;

fn fixture_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/store-resolution/legacy-v3")
}

fn scan_fixture() -> (TempDir, std::path::PathBuf) {
    let temp = TempDir::new().expect("temporary fixture root");
    let root = temp.path().join("repo");
    copy_dir(&fixture_root(), &root);
    std::fs::create_dir_all(root.join("failed-preserved")).expect("failed-preserved path exists");
    std::fs::write(root.join("failed-preserved/broken.rs"), [0xff, 0xfe, 0xfd])
        .expect("failed-preserved fixture is written");

    let db = temp.path().join("symbols.db");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "scan",
            "--root",
            root.to_str().expect("fixture root is utf8"),
            "--db",
            db.to_str().expect("database path is utf8"),
            "--json",
        ])
        .output()
        .expect("julie-extract starts");
    assert_eq!(
        output.status.code(),
        Some(1),
        "the fixture intentionally carries one failed-preserved path\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    (temp, db)
}

fn copy_dir(source: &Path, target: &Path) {
    std::fs::create_dir_all(target).expect("fixture destination exists");
    for entry in std::fs::read_dir(source).expect("fixture source exists") {
        let entry = entry.expect("fixture entry readable");
        if entry.file_name() == "expected.semantic.json" {
            continue;
        }
        let destination = target.join(entry.file_name());
        if entry.file_type().expect("fixture entry type").is_dir() {
            copy_dir(&entry.path(), &destination);
        } else {
            std::fs::copy(entry.path(), destination).expect("fixture file copied");
        }
    }
}

#[test]
fn legacy_resolution_oracle_is_pinned() {
    let (_first_temp, first_db) = scan_fixture();
    let (_second_temp, second_db) = scan_fixture();
    let first = semantic_dump(&first_db);
    let second = semantic_dump(&second_db);
    assert_eq!(first, second, "fresh v3 artifacts diverged semantically");

    let value: Value = serde_json::from_str(&first).expect("semantic dump is JSON");
    assert_vacuous_surfaces_are_populated(&value);

    let expected_path = fixture_root().join("expected.semantic.json");
    let expected = std::fs::read_to_string(expected_path).expect("pinned oracle exists");
    assert_eq!(first, expected, "legacy semantic oracle changed");
}

#[test]
fn legacy_phase_windows_are_bounded_and_output_invariant() {
    let (_small_temp, small_db) = scan_fixture();
    let (_large_temp, large_db) = scan_fixture();

    let (small, small_peak) = rerun_resolution_with_window(&small_db, 1);
    let (large, large_peak) = rerun_resolution_with_window(&large_db, 7);

    assert_eq!(small, large);
    assert_eq!(small_peak, 1);
    assert!(large_peak <= 7);
    assert!(large_peak > 1, "fixture does not exercise multi-row chunks");
}

#[test]
fn legacy_ambiguity_persists_complete_candidate_count() {
    let temp = TempDir::new().expect("temporary fixture root");
    let root = temp.path().join("repo");
    std::fs::create_dir(&root).expect("fixture root exists");
    for (path, source) in [
        ("a.rs", "pub fn collision() {}\n"),
        ("b.rs", "pub fn collision() {}\n"),
        ("c.rs", "pub fn collision() {}\n"),
        ("use.rs", "pub fn use_it() { collision(); }\n"),
    ] {
        std::fs::write(root.join(path), source).expect("fixture source written");
    }
    let db = temp.path().join("symbols.db");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "scan",
            "--root",
            root.to_str().expect("fixture root is utf8"),
            "--db",
            db.to_str().expect("database path is utf8"),
            "--json",
        ])
        .output()
        .expect("julie-extract starts");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let connection = Connection::open(db).expect("artifact opens");
    let row: (String, i64) = connection
        .query_row(
            "SELECT ir.outcome,ir.candidates
             FROM identifier_resolutions AS ir
             JOIN identifiers AS i ON i.identifier_id=ir.identifier_id
             WHERE i.name='collision'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("ambiguous identifier row exists");
    assert_eq!(row, ("ambiguous".to_string(), 3));
}

fn rerun_resolution_with_window(db: &Path, window_size: usize) -> (Value, usize) {
    let mut connection = Connection::open(db).expect("artifact opens");
    let transaction = connection.transaction().expect("resolution transaction");
    transaction
        .execute_batch(
            "DELETE FROM pending_resolutions;
             DELETE FROM identifier_resolutions;
             DELETE FROM artifact_metadata WHERE key LIKE 'reference_resolution_%';",
        )
        .expect("prior overlay clears");
    let scope = julie_extract_artifact::writer::ResolutionScopeInput {
        is_full_scan: true,
        whole_corpus: true,
        ..Default::default()
    };
    let mut session =
        LegacyResolutionSession::new(&transaction, &scope, 0.7).with_window_size(window_size);
    run_resolution_session(&mut session, true, true).expect("windowed resolution succeeds");
    let peak = session.max_emitted_chunk_size();
    drop(session);
    transaction.commit().expect("windowed overlay commits");
    let connection = Connection::open(db).expect("artifact reopens");
    (
        json!({
            "pending": pending_resolution_rows(&connection),
            "identifiers": identifier_resolution_rows(&connection),
        }),
        peak,
    )
}

fn assert_vacuous_surfaces_are_populated(value: &Value) {
    assert!(
        value["metadata"].is_object(),
        "resolution metadata is missing"
    );
    assert!(
        value["report_rows"]
            .as_array()
            .is_some_and(|rows| !rows.is_empty()),
        "aggregate resolution report rows are empty"
    );
    assert!(
        value["pending_resolutions"]
            .as_array()
            .is_some_and(|rows| !rows.is_empty()),
        "pending_resolutions overlay is empty"
    );
    assert!(
        value["identifier_resolutions"]
            .as_array()
            .is_some_and(|rows| !rows.is_empty()),
        "identifier_resolutions overlay is empty"
    );
    assert!(
        value["collisions"]
            .as_array()
            .is_some_and(|rows| !rows.is_empty()),
        "same-name collision surface is empty"
    );
    assert!(
        value["imports"]
            .as_array()
            .is_some_and(|rows| !rows.is_empty()),
        "module-path surface is empty"
    );
    let module_paths = value["module_path_existence"]
        .as_array()
        .expect("module path rows");
    assert!(
        module_paths.iter().any(|row| row["exists"] == true)
            && module_paths.iter().any(|row| row["exists"] == false),
        "module path fixture must include existing and failed path lookups"
    );
    assert!(
        value["files"]
            .as_array()
            .is_some_and(|rows| { rows.iter().any(|row| row["status"] == "failed_preserved") }),
        "failed-preserved path surface is empty"
    );

    let outcomes = value["identifier_resolutions"]
        .as_array()
        .expect("identifier resolution rows")
        .iter()
        .filter_map(|row| row["outcome"].as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        outcomes,
        BTreeSet::from(["ambiguous", "missing", "no_context", "resolved"]),
        "the oracle must cover every identifier outcome"
    );

    let pending = value["pending_resolutions"]
        .as_array()
        .expect("pending resolution rows");
    assert!(
        pending.iter().any(|row| row["target"].is_string()),
        "resolved pending relationship is missing"
    );
    assert!(
        value["pending_relationships"]
            .as_array()
            .is_some_and(|rows| rows.iter().any(|row| row["target"].is_null())),
        "unresolved pending relationship is missing"
    );
}

fn semantic_dump(db: &Path) -> String {
    let connection = Connection::open(db).expect("artifact opens");
    let metadata = read_resolution_metadata(&connection)
        .expect("resolution metadata query")
        .expect("resolution metadata exists");
    let report = resolution_report(&connection).expect("resolution report query");

    let value = json!({
        "metadata": {
            "status": metadata.status.as_str(),
            "version": metadata.version,
            "last_full_revision": metadata.last_full_revision,
        },
        "files": file_rows(&connection),
        "imports": import_rows(&connection),
        "module_path_existence": module_path_rows(&connection),
        "collisions": collision_rows(&connection),
        "pending_relationships": pending_relationship_rows(&connection),
        "pending_resolutions": pending_resolution_rows(&connection),
        "identifier_resolutions": identifier_resolution_rows(&connection),
        "report_rows": report.iter().map(|row| json!({
            "language": row.language,
            "origin": row.origin,
            "raw_kind": row.raw_kind,
            "canonical_kind": row.canonical_kind,
            "tier": row.tier,
            "method": row.method,
            "outcome": row.outcome,
            "span_present": row.span_present,
            "count": row.count,
        })).collect::<Vec<_>>(),
    });
    serde_json::to_string_pretty(&value).expect("semantic dump serializes") + "\n"
}

fn module_path_rows(connection: &Connection) -> Vec<Value> {
    let file_paths = {
        let mut statement = connection
            .prepare("SELECT path FROM files ORDER BY path")
            .expect("module file paths query prepares");
        statement
            .query_map([], |row| row.get::<_, String>(0))
            .expect("module file paths query runs")
            .collect::<Result<BTreeSet<_>, _>>()
            .expect("module file paths decode")
    };
    let mut statement = connection
        .prepare(
            "SELECT path, json_extract(metadata_json, '$.source') \
             FROM symbols WHERE kind = 'import' \
             ORDER BY path, start_byte",
        )
        .expect("module path rows query prepares");
    statement
        .query_map([], |row| {
            let source: Option<String> = row.get(1)?;
            let path: String = row.get(0)?;
            let exists = source.as_deref().is_some_and(|source| {
                let base = Path::new(&path).parent().unwrap_or_else(|| Path::new(""));
                let candidate = base
                    .join(source.trim_start_matches("./"))
                    .to_string_lossy()
                    .replace("\\", "/");
                file_paths.contains(&candidate)
                    || [".ts", ".tsx", ".js", ".jsx"]
                        .iter()
                        .any(|extension| file_paths.contains(&format!("{candidate}{extension}")))
            });
            Ok(json!({
                "path": path,
                "source": source,
                "exists": exists,
            }))
        })
        .expect("module path rows query runs")
        .collect::<Result<Vec<_>, _>>()
        .expect("module path rows decode")
}

fn file_rows(connection: &Connection) -> Vec<Value> {
    let mut statement = connection
        .prepare("SELECT path, language, status FROM files ORDER BY path")
        .expect("file rows query prepares");
    statement
        .query_map([], |row| {
            Ok(json!({
                "path": row.get::<_, String>(0)?,
                "language": row.get::<_, String>(1)?,
                "status": row.get::<_, String>(2)?,
            }))
        })
        .expect("file rows query runs")
        .collect::<Result<Vec<_>, _>>()
        .expect("file rows decode")
}

fn import_rows(connection: &Connection) -> Vec<Value> {
    let mut statement = connection
        .prepare(
            "SELECT path, name, json_extract(metadata_json, '$.source'), \
                    json_extract(metadata_json, '$.importedName') \
             FROM symbols WHERE kind = 'import' ORDER BY path, start_byte, name",
        )
        .expect("import rows query prepares");
    statement
        .query_map([], |row| {
            Ok(json!({
                "path": row.get::<_, String>(0)?,
                "specifier": row.get::<_, String>(1)?,
                "source": row.get::<_, Option<String>>(2)?,
                "imported_name": row.get::<_, Option<String>>(3)?,
            }))
        })
        .expect("import rows query runs")
        .collect::<Result<Vec<_>, _>>()
        .expect("import rows decode")
}

fn collision_rows(connection: &Connection) -> Vec<Value> {
    let mut statement = connection
        .prepare(
            "SELECT language, name, path, kind, start_line, start_byte \
             FROM symbols WHERE (language, name, kind) IN ( \
               SELECT language, name, kind FROM symbols \
               GROUP BY language, name, kind HAVING COUNT(*) > 1 \
             ) ORDER BY language, name, path, start_line, start_byte, kind",
        )
        .expect("collision rows query prepares");
    statement
        .query_map([], |row| {
            Ok(json!({
                "language": row.get::<_, String>(0)?,
                "name": row.get::<_, String>(1)?,
                "path": row.get::<_, String>(2)?,
                "kind": row.get::<_, String>(3)?,
                "start_line": row.get::<_, i64>(4)?,
                "start_byte": row.get::<_, i64>(5)?,
            }))
        })
        .expect("collision rows query runs")
        .collect::<Result<Vec<_>, _>>()
        .expect("collision rows decode")
}

fn pending_relationship_rows(connection: &Connection) -> Vec<Value> {
    let mut statement = connection
        .prepare(
            "SELECT f.path, f.language, p.kind, p.target_terminal_name, p.start_line, \
                    p.target_receiver, p.target_import_context, p.target_namespace_json, \
                    r.target_symbol_id, ts.path, ts.name, ts.kind, ts.start_line, ts.start_byte \
             FROM pending_relationships p JOIN files f ON f.file_id = p.file_id \
             LEFT JOIN pending_resolutions r ON r.pending_relationship_id = p.pending_relationship_id \
             LEFT JOIN symbols ts ON ts.symbol_id = r.target_symbol_id \
             ORDER BY f.path, p.start_line, p.target_terminal_name, p.kind",
        )
        .expect("pending relationship rows query prepares");
    statement
        .query_map([], |row| {
            Ok(json!({
                "path": row.get::<_, String>(0)?,
                "language": row.get::<_, String>(1)?,
                "kind": row.get::<_, String>(2)?,
                "target_name": row.get::<_, String>(3)?,
                "start_line": row.get::<_, i64>(4)?,
                "receiver": row.get::<_, Option<String>>(5)?,
                "import_context": row.get::<_, Option<String>>(6)?,
                "namespace": row.get::<_, String>(7)?,
                "target": symbol_key(
                    row.get::<_, Option<String>>(9)?,
                    row.get::<_, Option<String>>(10)?,
                    row.get::<_, Option<String>>(11)?,
                    row.get::<_, Option<i64>>(12)?,
                    row.get::<_, Option<i64>>(13)?,
                ),
            }))
        })
        .expect("pending relationship rows query runs")
        .collect::<Result<Vec<_>, _>>()
        .expect("pending relationship rows decode")
}

fn pending_resolution_rows(connection: &Connection) -> Vec<Value> {
    let mut statement = connection
        .prepare(
            "SELECT f.path, f.language, p.kind, p.target_terminal_name, p.start_line, \
                    r.tier, r.confidence, r.method, ts.path, ts.name, ts.kind, \
                    ts.start_line, ts.start_byte \
             FROM pending_resolutions r \
             JOIN pending_relationships p ON p.pending_relationship_id = r.pending_relationship_id \
             JOIN files f ON f.file_id = p.file_id JOIN symbols ts ON ts.symbol_id = r.target_symbol_id \
             ORDER BY f.path, p.start_line, p.target_terminal_name, p.kind",
        )
        .expect("pending resolution rows query prepares");
    statement
        .query_map([], |row| {
            Ok(json!({
                "path": row.get::<_, String>(0)?,
                "language": row.get::<_, String>(1)?,
                "kind": row.get::<_, String>(2)?,
                "target_name": row.get::<_, String>(3)?,
                "start_line": row.get::<_, i64>(4)?,
                "tier": row.get::<_, i64>(5)?,
                "confidence": row.get::<_, f64>(6)?,
                "method": row.get::<_, String>(7)?,
                "target": symbol_key(
                    Some(row.get::<_, String>(8)?),
                    Some(row.get::<_, String>(9)?),
                    Some(row.get::<_, String>(10)?),
                    Some(row.get::<_, i64>(11)?),
                    Some(row.get::<_, i64>(12)?),
                ),
            }))
        })
        .expect("pending resolution rows query runs")
        .collect::<Result<Vec<_>, _>>()
        .expect("pending resolution rows decode")
}

fn identifier_resolution_rows(connection: &Connection) -> Vec<Value> {
    let mut statement = connection
        .prepare(
            "SELECT i.path, i.language, i.name, i.kind, i.start_line, i.start_byte, \
                    json_extract(i.metadata_json, '$.receiver'), r.target_symbol_id, \
                    r.tier, r.confidence, r.method, r.outcome, r.candidates, \
                    ts.path, ts.name, ts.kind, ts.start_line, ts.start_byte \
             FROM identifier_resolutions r JOIN identifiers i ON i.identifier_id = r.identifier_id \
             LEFT JOIN symbols ts ON ts.symbol_id = r.target_symbol_id \
             ORDER BY i.path, i.start_byte, i.name, i.kind",
        )
        .expect("identifier resolution rows query prepares");
    statement
        .query_map([], |row| {
            Ok(json!({
                "path": row.get::<_, String>(0)?,
                "language": row.get::<_, String>(1)?,
                "name": row.get::<_, String>(2)?,
                "kind": row.get::<_, String>(3)?,
                "start_line": row.get::<_, i64>(4)?,
                "start_byte": row.get::<_, i64>(5)?,
                "receiver": row.get::<_, Option<String>>(6)?,
                "target": symbol_key(
                    row.get::<_, Option<String>>(13)?,
                    row.get::<_, Option<String>>(14)?,
                    row.get::<_, Option<String>>(15)?,
                    row.get::<_, Option<i64>>(16)?,
                    row.get::<_, Option<i64>>(17)?,
                ),
                "tier": row.get::<_, Option<i64>>(8)?,
                "confidence": row.get::<_, Option<f64>>(9)?,
                "method": row.get::<_, Option<String>>(10)?,
                "outcome": row.get::<_, String>(11)?,
                "candidates": row.get::<_, Option<i64>>(12)?,
            }))
        })
        .expect("identifier resolution rows query runs")
        .collect::<Result<Vec<_>, _>>()
        .expect("identifier resolution rows decode")
}

fn symbol_key(
    path: Option<String>,
    name: Option<String>,
    kind: Option<String>,
    start_line: Option<i64>,
    start_byte: Option<i64>,
) -> Option<String> {
    Some(format!(
        "{}|{}|{}|{}|{}",
        path?, kind?, name?, start_line?, start_byte?
    ))
}

#[derive(Default)]
struct FakeResolutionSession {
    writes: Vec<ResolutionWriteBatch>,
    index: Option<julie_extract_cli::resolution::WorkspaceCandidateIndex>,
    emitted_phases: Vec<ResolutionPhase>,
}

impl ResolutionSession for FakeResolutionSession {
    type Error = std::convert::Infallible;

    fn corpus_identity(&self) -> Result<ResolutionCorpusIdentity, Self::Error> {
        Ok(ResolutionCorpusIdentity::Store {
            family_id: "fake-family".to_string(),
            view_id: "fake-view".to_string(),
            manifest_generation: 1,
            manifest_hash: "fake-manifest".to_string(),
        })
    }

    fn prior_resolution_state(&mut self) -> Result<Option<SessionResolutionState>, Self::Error> {
        Ok(None)
    }

    fn current_revision(&mut self) -> Result<i64, Self::Error> {
        Ok(1)
    }

    fn open_resolution_pass(
        &mut self,
        _request: &ResolutionPassRequest,
    ) -> Result<ResolutionWorklists, Self::Error> {
        self.index = Some(
            julie_extract_cli::resolution::WorkspaceCandidateIndex::build_versioned(
                vec![
                    (
                        SemanticSymbolId {
                            version: SemanticVersionId::Store(10),
                            local_id: "scope".to_string(),
                        },
                        julie_extract_cli::resolution::CandidateSymbol {
                            symbol_id: "scope".to_string(),
                            file_id: "caller".to_string(),
                            language: "rust".to_string(),
                            name: "caller_scope".to_string(),
                            kind: julie_extractors::SymbolKind::Function,
                            parent_symbol_id: None,
                            visibility: None,
                            signature: None,
                            is_static: None,
                        },
                    ),
                    (
                        SemanticSymbolId {
                            version: SemanticVersionId::Store(30),
                            local_id: "scope".to_string(),
                        },
                        julie_extract_cli::resolution::CandidateSymbol {
                            symbol_id: "scope".to_string(),
                            file_id: "other-caller".to_string(),
                            language: "rust".to_string(),
                            name: "other_scope".to_string(),
                            kind: julie_extractors::SymbolKind::Function,
                            parent_symbol_id: None,
                            visibility: None,
                            signature: None,
                            is_static: None,
                        },
                    ),
                    (
                        SemanticSymbolId {
                            version: SemanticVersionId::Store(10),
                            local_id: "target".to_string(),
                        },
                        julie_extract_cli::resolution::CandidateSymbol {
                            symbol_id: "target".to_string(),
                            file_id: "caller".to_string(),
                            language: "rust".to_string(),
                            name: "launch".to_string(),
                            kind: julie_extractors::SymbolKind::Variable,
                            parent_symbol_id: Some("scope".to_string()),
                            visibility: None,
                            signature: None,
                            is_static: None,
                        },
                    ),
                    (
                        SemanticSymbolId {
                            version: SemanticVersionId::Store(30),
                            local_id: "target".to_string(),
                        },
                        julie_extract_cli::resolution::CandidateSymbol {
                            symbol_id: "target".to_string(),
                            file_id: "other-caller".to_string(),
                            language: "rust".to_string(),
                            name: "shadow".to_string(),
                            kind: julie_extractors::SymbolKind::Variable,
                            parent_symbol_id: Some("scope".to_string()),
                            visibility: None,
                            signature: None,
                            is_static: None,
                        },
                    ),
                ],
                vec![],
                vec![],
            ),
        );
        self.emitted_phases.clear();
        Ok(ResolutionWorklists {
            effective_full: true,
            ..ResolutionWorklists::default()
        })
    }

    fn qualify_version(&self, source_key: &str) -> Result<SemanticVersionId, Self::Error> {
        Ok(match source_key {
            "caller" => SemanticVersionId::Store(10),
            "other-caller" => SemanticVersionId::Store(30),
            _ => panic!("unexpected fake source key: {source_key}"),
        })
    }

    fn resolve_edge(
        &mut self,
        edge: &julie_extract_cli::resolution::UnresolvedEdge,
    ) -> Result<julie_extract_cli::resolution::TierOutcome, Self::Error> {
        Ok(julie_extract_cli::resolution::resolve_one(
            edge,
            self.index.as_ref().expect("fake pass is open"),
        ))
    }

    fn target_symbol_name(
        &mut self,
        symbol_id: &SemanticSymbolId,
    ) -> Result<Option<String>, Self::Error> {
        Ok((symbol_id.local_id == "target").then(|| "launch".to_string()))
    }

    fn locate_identifier(
        &self,
        version: &SemanticVersionId,
        name: &str,
        _start_byte: Option<i64>,
        _end_byte: Option<i64>,
        _start_line: i64,
    ) -> Result<Option<String>, Self::Error> {
        Ok(
            (*version == SemanticVersionId::Store(10) && name == "launch")
                .then(|| "relationship-site".to_string()),
        )
    }

    fn identifier_is_covered(
        &mut self,
        _identifier_id: &SemanticIdentifierId,
    ) -> Result<bool, Self::Error> {
        Ok(false)
    }

    fn propagation_is_covered(
        &mut self,
        _identifier_id: &SemanticIdentifierId,
    ) -> Result<bool, Self::Error> {
        Ok(false)
    }

    fn propagation_is_owned(
        &mut self,
        _identifier_id: &SemanticIdentifierId,
    ) -> Result<bool, Self::Error> {
        Ok(false)
    }

    fn next_phase_chunk(
        &mut self,
        worklists: &ResolutionWorklists,
    ) -> Result<Option<julie_extract_cli::resolution_session::ResolutionPhaseChunk>, Self::Error>
    {
        if self.emitted_phases.contains(&worklists.phase) {
            return Ok(None);
        }
        self.emitted_phases.push(worklists.phase);
        let identifiers = if worklists.phase == ResolutionPhase::Identifiers {
            vec![
                julie_extract_artifact::resolution_store::IdentifierWorkItem {
                    identifier_id: "call-site".to_string(),
                    file_id: "caller".to_string(),
                    path: "caller.rs".to_string(),
                    language: "rust".to_string(),
                    name: "launch".to_string(),
                    kind: "variable_ref".to_string(),
                    containing_symbol_id: Some("scope".to_string()),
                    start_line: 1,
                    start_byte: 0,
                    end_byte: 6,
                    receiver: None,
                    receiver_qualifier: None,
                    import_context: None,
                    confidence: 1.0,
                },
            ]
        } else {
            Vec::new()
        };
        let relationships = if worklists.phase == ResolutionPhase::Relationships {
            vec![SessionRelationship {
                target_symbol_id: SemanticSymbolId {
                    version: SemanticVersionId::Store(10),
                    local_id: "target".to_string(),
                },
                source_version_id: SemanticVersionId::Store(10),
                located_identifier_id: None,
                identifier_lookup_complete: false,
                kind: "calls".to_string(),
                start_line: 1,
                start_byte: Some(0),
                end_byte: Some(6),
                confidence: 1.0,
            }]
        } else {
            Vec::new()
        };
        Ok(match worklists.phase {
            ResolutionPhase::Identifiers => Some(
                julie_extract_cli::resolution_session::ResolutionPhaseChunk::Identifiers(
                    identifiers,
                ),
            ),
            ResolutionPhase::Relationships => Some(
                julie_extract_cli::resolution_session::ResolutionPhaseChunk::Relationships(
                    relationships,
                ),
            ),
            _ => None,
        })
    }

    fn flush(
        &mut self,
        writes: ResolutionWriteBatch,
    ) -> Result<julie_extract_artifact::resolution_store::ResolutionCounts, Self::Error> {
        self.writes.push(writes);
        Ok(Default::default())
    }

    fn aggregate_report(
        &mut self,
    ) -> Result<Vec<julie_extract_artifact::resolution_store::ResolutionReportRow>, Self::Error>
    {
        Ok(vec![])
    }
}

#[test]
fn resolver_policy_executes_through_a_session_without_sqlite() {
    let mut session = FakeResolutionSession::default();

    let (counts, _) = run_resolution_session(&mut session, true, true).unwrap();

    assert_eq!(
        session
            .writes
            .into_iter()
            .filter(|batch| !batch.writes.is_empty())
            .collect::<Vec<_>>(),
        vec![
            ResolutionWriteBatch {
                writes: vec![ResolutionWrite::Identifier {
                    identifier_id: SemanticIdentifierId {
                        version: SemanticVersionId::Store(10),
                        local_id: "relationship-site".to_string(),
                    },
                    target_symbol_id: Some(SemanticSymbolId {
                        version: SemanticVersionId::Store(10),
                        local_id: "target".to_string(),
                    }),
                    outcome: julie_extract_artifact::resolution_store::Outcome::Resolved,
                    tier: Some(1),
                    confidence: Some(0.95),
                    method: Some("tier1_local".to_string()),
                    candidates: None,
                    revision: 1,
                }],
            },
            ResolutionWriteBatch {
                writes: vec![ResolutionWrite::Identifier {
                    identifier_id: SemanticIdentifierId {
                        version: SemanticVersionId::Store(10),
                        local_id: "call-site".to_string(),
                    },
                    target_symbol_id: Some(SemanticSymbolId {
                        version: SemanticVersionId::Store(10),
                        local_id: "target".to_string(),
                    }),
                    outcome: julie_extract_artifact::resolution_store::Outcome::Resolved,
                    tier: Some(1),
                    confidence: Some(0.95),
                    method: Some("tier1_local".to_string()),
                    candidates: None,
                    revision: 1,
                }],
            },
        ]
    );
    assert_eq!(counts.identifier_resolutions, 2);
}

#[test]
fn generic_resolution_engine_contains_no_physical_storage_access() {
    let source = include_str!("../src/resolution.rs");
    let start = source
        .find("pub fn run_resolution_session")
        .expect("generic engine starts");
    let end = source[start..]
        .find("pub(crate) fn load_relationship_rows")
        .map(|offset| start + offset)
        .expect("legacy adapter helpers follow the engine");
    let engine = &source[start..end];

    for forbidden in [
        "rusqlite::",
        "Connection",
        "Transaction",
        "\"SELECT ",
        "\"INSERT ",
        "\"UPDATE ",
        "\"DELETE ",
    ] {
        assert!(
            !engine.contains(forbidden),
            "generic engine contains physical storage access: {forbidden}"
        );
    }
}

#[test]
fn generic_resolution_session_contract_exposes_only_bounded_ports() {
    let source = include_str!("../src/resolution_session.rs");
    let start = source
        .find("pub trait ResolutionSession")
        .expect("resolution session contract starts");
    let end = source[start..]
        .find("pub struct LegacyResolutionSession")
        .map(|offset| start + offset)
        .expect("legacy adapter follows the contract");
    let contract = &source[start..end];

    for forbidden in [
        "WorkspaceCandidateIndex",
        "IdentifierLocator",
        "HashSet<SemanticIdentifierId>",
        "CurrentResolutionOverlay",
        "Vec<PendingWorkItem>",
        "Vec<IdentifierWorkItem>",
        "Vec<SessionRelationship>",
    ] {
        assert!(
            !contract.contains(forbidden),
            "generic contract exposes an unbounded concrete collection: {forbidden}"
        );
    }
    assert!(contract.contains("open_resolution_pass"));
    assert!(contract.contains("next_phase_chunk"));
}
