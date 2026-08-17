#![cfg(feature = "test-store-resolution-contract")]

use julie_extract_artifact::store::{
    ManifestEntry, ManifestStore, ResolutionBaseReader, ResolutionBaseWriter,
    ResolutionIdentifierRow, ResolutionPendingRow, ResolutionScratchReader, StoreConnectionFactory,
    StoreLayout, ensure_resolution_scope_feature, stream_resolution_diff,
};
use julie_extract_cli::resolution::{
    CandidateLookup, CandidateSymbol, EdgeOrigin, ReferenceKind, TierOutcome, TypeFact,
    UnresolvedEdge, WorkspaceCandidateIndex, resolve_one, resolve_with_candidate_lookup,
    run_resolution_session,
};
use julie_extract_cli::resolution_session::{
    ResolutionPassRequest, ResolutionPhase, ResolutionPhaseChunk, ResolutionSession,
    ResolutionWorklists, ResolutionWriteBatch, SemanticIdentifierId, SemanticPendingRelationshipId,
    SemanticSymbolId, SemanticVersionId,
};
use julie_extract_cli::store::resolution_session::{
    CandidateQueryFamily, PropagationCoverageTelemetry, StoreManifestIdentity,
    StoreResolutionError, StoreScratchResolutionSession,
};
use julie_extractors::SymbolKind;
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

const NOW: &str = "2026-08-08T12:00:00Z";

#[test]
fn store_resolution_source_uses_only_bounded_ports() {
    let source = include_str!("../src/store/resolution_session.rs");
    for forbidden in [
        "WorkspaceCandidateIndex",
        "IdentifierLocator",
        "CurrentResolutionOverlay",
    ] {
        assert!(
            !source.contains(forbidden),
            "Store adapter contains forbidden {forbidden}"
        );
    }
    assert!(source.contains("LIMIT ?"));
    assert!(source.contains("open_reader()"));
    assert!(source.contains("build_store_delta_scope"));
    assert!(source.contains("PriorOverlayReader"));
    assert_eq!(source.matches("ATTACH DATABASE ?1 AS prior_").count(), 2);
    assert_eq!(source.matches("DETACH DATABASE prior_").count(), 2);
    assert!(!source.contains("Incremental(String)"));
    assert!(!source.contains("IdentifierTotalityMissing"));
}

#[test]
fn exact_children_batch_count_releases_parent_index_hint_but_fetch_keeps_it() {
    let source = include_str!("../src/store/resolution_session.rs");
    let method_start = source
        .find("    fn prime_exact_children_keys(\n")
        .expect("exact-child batch method should exist");
    let method_end = source[method_start..]
        .find("\n    fn candidate_page(")
        .map(|offset| method_start + offset)
        .expect("exact-child batch method should end");
    let method = &source[method_start..method_end];
    let count_start = method
        .find("        let count_sql = format!(")
        .expect("exact-child count SQL should exist");
    let fetch_start = method
        .find("            let fetch_sql = format!(")
        .expect("exact-child fetch SQL should exist");
    let count = &method[count_start..fetch_start];
    let fetch_end = method[fetch_start..]
        .find("            );\n            let mut bind")
        .map(|offset| fetch_start + offset)
        .expect("exact-child fetch SQL should end");
    let fetch = &method[fetch_start..fetch_end];
    assert!(
        !count.contains("INDEXED BY idx_read_symbols_parent"),
        "COUNT must not force the parent-only index"
    );
    assert!(
        fetch.contains("INDEXED BY idx_read_symbols_parent\n"),
        "FETCH must retain the parent-only index hint"
    );
    assert!(
        !fetch.contains("INDEXED BY idx_read_symbols_parent_name"),
        "FETCH must not use the parent/name index hint"
    );
}

#[test]
fn exact_children_batch_count_plans_parent_name_and_survives_missing_index() {
    let temp = TempDir::new().unwrap();
    let layout = StoreLayout::create(temp.path().join("family"), "family-a", "2.30.0").unwrap();
    let factory = StoreConnectionFactory::new(layout, "family-a", "2.30.0");
    let mut connection = factory.open_writer().unwrap();
    ManifestStore::new(&mut connection)
        .ensure_view("view-a", "/repo")
        .unwrap();
    assert_eq!(
        connection
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        2
    );
    let version = insert_version(&connection, "src/lib.rs", "rust", true);
    connection
        .execute(
            "INSERT INTO symbols(version_id,symbol_id,path,language,name,kind,
             start_line,start_column,end_line,end_column,start_byte,end_byte,is_test,test_container,test_lifecycle)
             VALUES (?1,'parent','src/lib.rs','rust','parent','function',1,1,1,5,0,4,0,0,0)",
            [version],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO symbols(version_id,symbol_id,path,language,name,kind,parent_symbol_id,
             start_line,start_column,end_line,end_column,start_byte,end_byte,is_test,test_container,test_lifecycle)
             VALUES (?1,'child-000','src/lib.rs','rust','ChildName','function','parent',1,1,1,5,0,4,0,0,0)",
            [version],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at)
             VALUES ('view-a',1,'manifest-a','request-a',?1)",
            [NOW],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO manifest_entries(view_id,generation,path,language,version_id,status,observed_content_hash,indexed_at)
             VALUES ('view-a',1,'src/lib.rs','rust',?1,'indexed','hash-src/lib.rs',?2)",
            params![version, NOW],
        )
        .unwrap();

    let count_sql = |batch_size: usize| {
        let values = (0..batch_size)
            .map(|ordinal| format!("({ordinal},?,?,?)"))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "WITH wanted(ordinal,version_id,parent_symbol_id,name) AS (VALUES {values})
             SELECT wanted.ordinal,COUNT(s.symbol_id)
             FROM wanted
             LEFT JOIN symbols AS s
               ON s.version_id=wanted.version_id
              AND s.parent_symbol_id=wanted.parent_symbol_id
              AND s.name=wanted.name
              AND EXISTS (
                SELECT 1 FROM manifest_entries AS me
                WHERE me.view_id=? AND me.generation=?
                  AND me.status IN ('indexed','failed_preserved')
                  AND me.version_id=s.version_id
              )
             GROUP BY wanted.ordinal
             ORDER BY wanted.ordinal"
        )
    };
    let bind = |batch_size: usize| {
        let mut bind: Vec<rusqlite::types::Value> = Vec::with_capacity(batch_size * 3 + 2);
        for _ in 0..batch_size {
            bind.push(version.into());
            bind.push("parent".to_string().into());
            bind.push("ChildName".to_string().into());
        }
        bind.push("view-a".to_string().into());
        bind.push(1_i64.into());
        bind
    };
    let explain = |sql: &str, bind: Vec<rusqlite::types::Value>| {
        connection
            .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
            .unwrap()
            .query_map(rusqlite::params_from_iter(bind), |row| {
                row.get::<_, String>(3)
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
            .join("\n")
    };
    let execute = |sql: &str, bind: Vec<rusqlite::types::Value>| {
        connection
            .prepare(sql)
            .unwrap()
            .query_map(rusqlite::params_from_iter(bind), |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    };

    for batch_size in [1, 300] {
        let sql = count_sql(batch_size);
        let plan = explain(&sql, bind(batch_size));
        assert!(
            plan.contains("idx_read_symbols_parent_name"),
            "batch size {batch_size} did not use the parent/name index. Plan:\n{plan}"
        );
        let rows = execute(&sql, bind(batch_size));
        assert_eq!(rows.len(), batch_size);
        assert!(rows.iter().all(|(_, count)| *count == 1));
    }

    connection
        .execute("DROP INDEX idx_read_symbols_parent_name", [])
        .unwrap();
    for batch_size in [1, 300] {
        let sql = count_sql(batch_size);
        let rows = execute(&sql, bind(batch_size));
        assert_eq!(rows.len(), batch_size);
        assert!(rows.iter().all(|(_, count)| *count == 1));
    }
}

#[test]
fn scoped_delta_finalizer_has_no_full_base_or_effective_walk() {
    let source = include_str!("../src/store/resolution_session.rs");
    let start = source
        .find("    fn finish_scoped_delta_inner")
        .expect("scoped finalizer should exist");
    let helper_start = source
        .find("const SCOPED_TARGET_BATCH")
        .expect("scoped helper region should exist");
    let end = source[helper_start..]
        .find("impl CandidateLookup for StoreScratchResolutionSession")
        .map(|offset| helper_start + offset)
        .expect("scoped helper region should end before candidate lookup");
    let finalizer = &source[start..end];
    for forbidden in [
        "ResolutionBaseReader::open",
        "ResolutionBaseReader",
        "PRAGMA integrity_check",
        "integrity_check",
        "ScopedBaseIdentifierCursor",
        "ScopedBasePendingCursor",
        "ScopedEffectiveIdentifierCursor",
        "ScopedEffectivePendingCursor",
        "validate_effective_identifier_totality",
    ] {
        assert!(
            !finalizer.contains(forbidden),
            "scoped finalizer retains unbounded path {forbidden}"
        );
    }
}

#[test]
fn store_resolution_rejects_windows_above_sqlite_parameter_budget() {
    let temp = TempDir::new().unwrap();
    let layout = StoreLayout::create(temp.path().join("family"), "family-a", "2.30.0").unwrap();
    let error = StoreScratchResolutionSession::new(
        StoreConnectionFactory::new(layout, "family-a", "2.30.0"),
        StoreManifestIdentity {
            family_id: "family-a".to_string(),
            view_id: "view-a".to_string(),
            generation: 1,
            manifest_hash: "manifest-a".to_string(),
        },
        temp.path().join("exact.db"),
        301,
        6,
    )
    .unwrap_err();
    assert!(matches!(
        error,
        StoreResolutionError::InvalidWindowSize {
            requested: 301,
            maximum: 300
        }
    ));
}

#[test]
fn store_children_named_preserve_binary_order_and_early_stop() {
    let temp = TempDir::new().unwrap();
    let layout = StoreLayout::create(temp.path().join("family"), "family-a", "2.30.0").unwrap();
    let factory = StoreConnectionFactory::new(layout, "family-a", "2.30.0");
    let mut connection = factory.open_writer().unwrap();
    ManifestStore::new(&mut connection)
        .ensure_view("view-a", "/repo")
        .unwrap();
    let version = insert_version(&connection, "src/lib.rs", "rust", true);
    connection
        .execute(
            "INSERT INTO symbols(version_id,symbol_id,path,language,name,kind,
             start_line,start_column,end_line,end_column,start_byte,end_byte,is_test,test_container,test_lifecycle)
             VALUES (?1,'scope','src/lib.rs','rust','scope','function',1,1,20,1,0,200,0,0,0)",
            [version],
        )
        .unwrap();
    for symbol_id in ["z-child", "a-child", "b-child"] {
        connection
            .execute(
                "INSERT INTO symbols(version_id,symbol_id,path,language,name,kind,parent_symbol_id,
                 start_line,start_column,end_line,end_column,start_byte,end_byte,is_test,test_container,test_lifecycle)
                 VALUES (?1,?2,'src/lib.rs','rust','target','function','scope',2,1,2,10,10,20,0,0,0)",
                params![version, symbol_id],
            )
            .unwrap();
    }
    connection
        .execute(
            "INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at)
             VALUES ('view-a',1,'manifest-a','request-a',?1)",
            [NOW],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO manifest_entries(view_id,generation,path,language,version_id,status,observed_content_hash,indexed_at)
             VALUES ('view-a',1,'src/lib.rs','rust',?1,'indexed','hash-src/lib.rs',?2)",
            params![version, NOW],
        )
        .unwrap();
    drop(connection);

    let session = StoreScratchResolutionSession::new(
        factory,
        StoreManifestIdentity {
            family_id: "family-a".to_string(),
            view_id: "view-a".to_string(),
            generation: 1,
            manifest_hash: "manifest-a".to_string(),
        },
        temp.path().join("exact.db"),
        2,
        6,
    )
    .unwrap();
    let mut delivered = Vec::new();
    session
        .visit_children_named(&version.to_string(), "scope", "target", |_, hit| {
            delivered.push(hit.symbol.symbol_id);
            Ok(delivered.len() < 2)
        })
        .unwrap();
    assert_eq!(delivered, ["a-child", "b-child"]);
    let telemetry = session.candidate_query_telemetry(CandidateQueryFamily::ChildrenNamed);
    assert_eq!(telemetry.executions, 0);
    let mini = session.candidate_query_telemetry(CandidateQueryFamily::VersionMiniIndex);
    assert_eq!(mini.executions, 1);
    assert!(mini.rows_read >= 4);
}

#[test]
fn store_children_named_reuses_complete_scalar_positive_and_empty_keys() {
    let temp = TempDir::new().unwrap();
    let layout = StoreLayout::create(temp.path().join("family"), "family-a", "2.30.0").unwrap();
    let factory = StoreConnectionFactory::new(layout, "family-a", "2.30.0");
    let mut connection = factory.open_writer().unwrap();
    ManifestStore::new(&mut connection)
        .ensure_view("view-a", "/repo")
        .unwrap();
    let version = insert_version(&connection, "src/lib.rs", "rust", true);
    connection
        .execute(
            "INSERT INTO symbols(version_id,symbol_id,path,language,name,kind,
             start_line,start_column,end_line,end_column,start_byte,end_byte,is_test,test_container,test_lifecycle)
             VALUES (?1,'scope','src/lib.rs','rust','scope','function',1,1,20,1,0,200,0,0,0)",
            [version],
        )
        .unwrap();
    for symbol_id in ["b-child", "a-child"] {
        connection
            .execute(
                "INSERT INTO symbols(version_id,symbol_id,path,language,name,kind,parent_symbol_id,
                 start_line,start_column,end_line,end_column,start_byte,end_byte,is_test,test_container,test_lifecycle)
                 VALUES (?1,?2,'src/lib.rs','rust','target','function','scope',2,1,2,10,10,20,0,0,0)",
                params![version, symbol_id],
            )
            .unwrap();
    }
    connection
        .execute(
            "INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at)
             VALUES ('view-a',1,'manifest-a','request-a',?1)",
            [NOW],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO manifest_entries(view_id,generation,path,language,version_id,status,observed_content_hash,indexed_at)
             VALUES ('view-a',1,'src/lib.rs','rust',?1,'indexed','hash-src/lib.rs',?2)",
            params![version, NOW],
        )
        .unwrap();
    drop(connection);

    let session = StoreScratchResolutionSession::new(
        factory,
        StoreManifestIdentity {
            family_id: "family-a".to_string(),
            view_id: "view-a".to_string(),
            generation: 1,
            manifest_hash: "manifest-a".to_string(),
        },
        temp.path().join("exact.db"),
        6,
        6,
    )
    .unwrap();
    let mut stopped = Vec::new();
    session
        .visit_children_named(&version.to_string(), "scope", "target", |_, hit| {
            let keep_going = !stopped.is_empty();
            stopped.push(hit.symbol.symbol_id);
            Ok(keep_going)
        })
        .unwrap();
    assert_eq!(stopped, ["a-child"]);
    let mut first_positive = Vec::new();
    session
        .visit_children_named(&version.to_string(), "scope", "target", |_, hit| {
            first_positive.push(hit.symbol.symbol_id);
            Ok(true)
        })
        .unwrap();
    let mut second_positive = Vec::new();
    session
        .visit_children_named(&version.to_string(), "scope", "target", |_, hit| {
            second_positive.push(hit.symbol.symbol_id);
            Ok(true)
        })
        .unwrap();
    let mut first_empty = Vec::new();
    session
        .visit_children_named(&version.to_string(), "scope", "missing", |_, hit| {
            first_empty.push(hit.symbol.symbol_id);
            Ok(true)
        })
        .unwrap();
    let mut second_empty = Vec::new();
    session
        .visit_children_named(&version.to_string(), "scope", "missing", |_, hit| {
            second_empty.push(hit.symbol.symbol_id);
            Ok(true)
        })
        .unwrap();

    assert_eq!(first_positive, ["a-child", "b-child"]);
    assert_eq!(second_positive, ["a-child", "b-child"]);
    assert!(first_empty.is_empty());
    assert!(second_empty.is_empty());
    let telemetry = session.candidate_query_telemetry(CandidateQueryFamily::ChildrenNamed);
    assert_eq!(telemetry.executions, 0);
    let mini = session.candidate_query_telemetry(CandidateQueryFamily::VersionMiniIndex);
    assert_eq!(mini.executions, 1);
}

#[test]
fn store_children_named_unknown_kind_does_not_truncate_cached_scalar_pages() {
    let temp = TempDir::new().unwrap();
    let layout = StoreLayout::create(temp.path().join("family"), "family-a", "2.30.0").unwrap();
    let factory = StoreConnectionFactory::new(layout, "family-a", "2.30.0");
    let mut connection = factory.open_writer().unwrap();
    ManifestStore::new(&mut connection)
        .ensure_view("view-a", "/repo")
        .unwrap();
    let version = insert_version(&connection, "src/lib.rs", "rust", true);
    connection
        .execute(
            "INSERT INTO symbols(version_id,symbol_id,path,language,name,kind,
             start_line,start_column,end_line,end_column,start_byte,end_byte,is_test,test_container,test_lifecycle)
             VALUES (?1,'scope','src/lib.rs','rust','scope','function',1,1,20,1,0,200,0,0,0)",
            [version],
        )
        .unwrap();
    for (symbol_id, kind) in [
        ("a-unknown", "not-a-symbol-kind"),
        ("b-known", "function"),
        ("c-known", "function"),
    ] {
        connection
            .execute(
                "INSERT INTO symbols(version_id,symbol_id,path,language,name,kind,parent_symbol_id,
                 start_line,start_column,end_line,end_column,start_byte,end_byte,is_test,test_container,test_lifecycle)
                 VALUES (?1,?2,'src/lib.rs','rust','target',?3,'scope',2,1,2,10,10,20,0,0,0)",
                params![version, symbol_id, kind],
            )
            .unwrap();
    }
    connection
        .execute(
            "INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at)
             VALUES ('view-a',1,'manifest-a','request-a',?1)",
            [NOW],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO manifest_entries(view_id,generation,path,language,version_id,status,observed_content_hash,indexed_at)
             VALUES ('view-a',1,'src/lib.rs','rust',?1,'indexed','hash-src/lib.rs',?2)",
            params![version, NOW],
        )
        .unwrap();
    drop(connection);

    let session = StoreScratchResolutionSession::new(
        factory,
        StoreManifestIdentity {
            family_id: "family-a".to_string(),
            view_id: "view-a".to_string(),
            generation: 1,
            manifest_hash: "manifest-a".to_string(),
        },
        temp.path().join("exact.db"),
        2,
        6,
    )
    .unwrap();
    let mut first = Vec::new();
    session
        .visit_children_named(&version.to_string(), "scope", "target", |_, hit| {
            first.push(hit.symbol.symbol_id);
            Ok(true)
        })
        .unwrap();
    let mut second = Vec::new();
    session
        .visit_children_named(&version.to_string(), "scope", "target", |_, hit| {
            second.push(hit.symbol.symbol_id);
            Ok(true)
        })
        .unwrap();

    assert_eq!(first, ["b-known", "c-known"]);
    assert_eq!(second, ["b-known", "c-known"]);
    let telemetry = session.candidate_query_telemetry(CandidateQueryFamily::ChildrenNamed);
    assert_eq!(telemetry.executions, 0);
    let mini = session.candidate_query_telemetry(CandidateQueryFamily::VersionMiniIndex);
    assert_eq!(mini.executions, 1);
}

#[test]
fn store_version_mini_index_serves_file_local_lookups() {
    let temp = TempDir::new().unwrap();
    let layout = StoreLayout::create(temp.path().join("family"), "family-a", "2.30.0").unwrap();
    let factory = StoreConnectionFactory::new(layout, "family-a", "2.30.0");
    let mut connection = factory.open_writer().unwrap();
    ManifestStore::new(&mut connection)
        .ensure_view("view-a", "/repo")
        .unwrap();
    let version = insert_version(&connection, "src/lib.rs", "rust", true);
    connection
        .execute(
            "INSERT INTO symbols(version_id,symbol_id,path,language,name,kind,parent_symbol_id,
             start_line,start_column,end_line,end_column,start_byte,end_byte,is_test,test_container,test_lifecycle)
             VALUES (?1,'scope','src/lib.rs','rust','scope','function',NULL,1,1,20,1,0,200,0,0,0)",
            [version],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO symbols(version_id,symbol_id,path,language,name,kind,parent_symbol_id,
             start_line,start_column,end_line,end_column,start_byte,end_byte,is_test,test_container,test_lifecycle)
             VALUES (?1,'child','src/lib.rs','rust','target','function','scope',2,1,2,10,10,20,0,0,0)",
            [version],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO type_facts(version_id,type_fact_id,symbol_id,language,resolved_type,is_inferred)
             VALUES (?1,'fact-child','child','rust','TypeC',0)",
            [version],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at)
             VALUES ('view-a',1,'manifest-a','request-a',?1)",
            [NOW],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO manifest_entries(view_id,generation,path,language,version_id,status,observed_content_hash,indexed_at)
             VALUES ('view-a',1,'src/lib.rs','rust',?1,'indexed','hash-src/lib.rs',?2)",
            params![version, NOW],
        )
        .unwrap();
    drop(connection);

    let session = StoreScratchResolutionSession::new(
        factory,
        StoreManifestIdentity {
            family_id: "family-a".to_string(),
            view_id: "view-a".to_string(),
            generation: 1,
            manifest_hash: "manifest-a".to_string(),
        },
        temp.path().join("exact.db"),
        8,
        6,
    )
    .unwrap();
    let source_key = version.to_string();
    let by_id = session.symbol_by_id(&source_key, "child").unwrap().unwrap();
    assert_eq!(by_id.symbol.name, "target");
    let mut top_level = Vec::new();
    session
        .visit_top_level_named(&source_key, "scope", |_, hit| {
            top_level.push(hit.symbol.symbol_id);
            Ok(true)
        })
        .unwrap();
    assert_eq!(top_level, ["scope"]);
    let mut children = Vec::new();
    session
        .visit_children_named(&source_key, "scope", "target", |_, hit| {
            children.push(hit.symbol.symbol_id);
            Ok(true)
        })
        .unwrap();
    assert_eq!(children, ["child"]);
    let mut facts = Vec::new();
    session
        .visit_type_facts(
            &SemanticSymbolId {
                version: SemanticVersionId::Store(version),
                local_id: "child".to_string(),
            },
            |_, fact| {
                facts.push(fact.resolved_type);
                Ok(true)
            },
        )
        .unwrap();
    assert_eq!(facts, ["TypeC"]);
    let mut filtered = Vec::new();
    session
        .visit_filtered_by_name(
            "target",
            "rust",
            &[SymbolKind::Function],
            Some(&source_key),
            |_, hit| {
                filtered.push(hit.symbol.symbol_id);
                Ok(true)
            },
        )
        .unwrap();
    assert_eq!(filtered, ["child"]);
    let summary = session
        .filtered_name_summary(
            "target",
            "rust",
            &[SymbolKind::Function],
            Some(&source_key),
            0.9,
        )
        .unwrap();
    assert_eq!(summary.exact_count, 1);

    assert_eq!(
        session
            .candidate_query_telemetry(CandidateQueryFamily::SymbolById)
            .executions,
        0
    );
    assert_eq!(
        session
            .candidate_query_telemetry(CandidateQueryFamily::TopLevelNamed)
            .executions,
        0
    );
    assert_eq!(
        session
            .candidate_query_telemetry(CandidateQueryFamily::ChildrenNamed)
            .executions,
        0
    );
    assert_eq!(
        session
            .candidate_query_telemetry(CandidateQueryFamily::TypeFacts)
            .executions,
        0
    );
    assert_eq!(
        session
            .candidate_query_telemetry(CandidateQueryFamily::FilteredByName)
            .executions,
        0
    );
    assert_eq!(
        session
            .candidate_query_telemetry(CandidateQueryFamily::FilteredNameSummary)
            .executions,
        0
    );
    assert_eq!(
        session
            .candidate_query_telemetry(CandidateQueryFamily::VersionMiniIndex)
            .executions,
        1
    );
}

#[test]
fn store_version_mini_index_too_large_stays_on_sql() {
    let temp = TempDir::new().unwrap();
    let layout = StoreLayout::create(temp.path().join("family"), "family-a", "2.30.0").unwrap();
    let factory = StoreConnectionFactory::new(layout, "family-a", "2.30.0");
    let mut connection = factory.open_writer().unwrap();
    ManifestStore::new(&mut connection)
        .ensure_view("view-a", "/repo")
        .unwrap();
    let version = insert_version(&connection, "src/lib.rs", "rust", true);
    for index in 0..=2048 {
        connection
            .execute(
                "INSERT INTO symbols(version_id,symbol_id,path,language,name,kind,
                 start_line,start_column,end_line,end_column,start_byte,end_byte,is_test,test_container,test_lifecycle)
                 VALUES (?1,?2,'src/lib.rs','rust',?2,'function',1,1,1,1,0,1,0,0,0)",
                params![version, format!("sym-{index:04}")],
            )
            .unwrap();
    }
    connection
        .execute(
            "INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at)
             VALUES ('view-a',1,'manifest-a','request-a',?1)",
            [NOW],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO manifest_entries(view_id,generation,path,language,version_id,status,observed_content_hash,indexed_at)
             VALUES ('view-a',1,'src/lib.rs','rust',?1,'indexed','hash-src/lib.rs',?2)",
            params![version, NOW],
        )
        .unwrap();
    drop(connection);

    let session = StoreScratchResolutionSession::new(
        factory,
        StoreManifestIdentity {
            family_id: "family-a".to_string(),
            view_id: "view-a".to_string(),
            generation: 1,
            manifest_hash: "manifest-a".to_string(),
        },
        temp.path().join("exact.db"),
        8,
        6,
    )
    .unwrap();
    let hit = session
        .symbol_by_id(&version.to_string(), "sym-0000")
        .unwrap()
        .unwrap();
    assert_eq!(hit.symbol.name, "sym-0000");
    let mini = session.candidate_query_telemetry(CandidateQueryFamily::VersionMiniIndex);
    assert_eq!(mini.executions, 1);
    assert_eq!(mini.rows_read, 0);
    let by_id = session.candidate_query_telemetry(CandidateQueryFamily::SymbolById);
    assert_eq!(by_id.executions, 1);
    assert_eq!(by_id.rows_read, 1);
}

#[test]
fn store_filtered_name_pass_cache_reuses_complete_workspace_scan() {
    let temp = TempDir::new().unwrap();
    let layout = StoreLayout::create(temp.path().join("family"), "family-a", "2.30.0").unwrap();
    let factory = StoreConnectionFactory::new(layout, "family-a", "2.30.0");
    let mut connection = factory.open_writer().unwrap();
    ManifestStore::new(&mut connection)
        .ensure_view("view-a", "/repo")
        .unwrap();
    let version = insert_version(&connection, "src/lib.rs", "rust", true);
    for symbol_id in ["a-hit", "b-hit"] {
        connection
            .execute(
                "INSERT INTO symbols(version_id,symbol_id,path,language,name,kind,
                 start_line,start_column,end_line,end_column,start_byte,end_byte,is_test,test_container,test_lifecycle)
                 VALUES (?1,?2,'src/lib.rs','rust','target','function',1,1,1,1,0,1,0,0,0)",
                params![version, symbol_id],
            )
            .unwrap();
    }
    connection
        .execute(
            "INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at)
             VALUES ('view-a',1,'manifest-a','request-a',?1)",
            [NOW],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO manifest_entries(view_id,generation,path,language,version_id,status,observed_content_hash,indexed_at)
             VALUES ('view-a',1,'src/lib.rs','rust',?1,'indexed','hash-src/lib.rs',?2)",
            params![version, NOW],
        )
        .unwrap();
    drop(connection);

    let session = StoreScratchResolutionSession::new(
        factory,
        StoreManifestIdentity {
            family_id: "family-a".to_string(),
            view_id: "view-a".to_string(),
            generation: 1,
            manifest_hash: "manifest-a".to_string(),
        },
        temp.path().join("exact.db"),
        8,
        6,
    )
    .unwrap();
    let mut first = Vec::new();
    session
        .visit_filtered_by_name("target", "rust", &[SymbolKind::Function], None, |_, hit| {
            first.push(hit.symbol.symbol_id);
            Ok(true)
        })
        .unwrap();
    let mut second = Vec::new();
    session
        .visit_filtered_by_name("target", "rust", &[SymbolKind::Function], None, |_, hit| {
            second.push(hit.symbol.symbol_id);
            Ok(true)
        })
        .unwrap();
    assert_eq!(first, ["a-hit", "b-hit"]);
    assert_eq!(second, ["a-hit", "b-hit"]);
    let visit = session.candidate_query_telemetry(CandidateQueryFamily::FilteredByName);
    assert_eq!(visit.executions, 1);
    let summary = session
        .filtered_name_summary("target", "rust", &[SymbolKind::Function], None, 0.9)
        .unwrap();
    assert_eq!(summary.exact_count, 2);
    assert_eq!(
        session
            .candidate_query_telemetry(CandidateQueryFamily::FilteredNameSummary)
            .executions,
        0
    );
}

#[test]
fn store_filtered_name_pass_cache_rejects_lists_over_hit_cap() {
    let temp = TempDir::new().unwrap();
    let layout = StoreLayout::create(temp.path().join("family"), "family-a", "2.30.0").unwrap();
    let factory = StoreConnectionFactory::new(layout, "family-a", "2.30.0");
    let mut connection = factory.open_writer().unwrap();
    ManifestStore::new(&mut connection)
        .ensure_view("view-a", "/repo")
        .unwrap();
    let version = insert_version(&connection, "src/lib.rs", "rust", true);
    for index in 0..33 {
        connection
            .execute(
                "INSERT INTO symbols(version_id,symbol_id,path,language,name,kind,
                 start_line,start_column,end_line,end_column,start_byte,end_byte,is_test,test_container,test_lifecycle)
                 VALUES (?1,?2,'src/lib.rs','rust','target','function',1,1,1,1,0,1,0,0,0)",
                params![version, format!("hit-{index:02}")],
            )
            .unwrap();
    }
    connection
        .execute(
            "INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at)
             VALUES ('view-a',1,'manifest-a','request-a',?1)",
            [NOW],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO manifest_entries(view_id,generation,path,language,version_id,status,observed_content_hash,indexed_at)
             VALUES ('view-a',1,'src/lib.rs','rust',?1,'indexed','hash-src/lib.rs',?2)",
            params![version, NOW],
        )
        .unwrap();
    drop(connection);

    let session = StoreScratchResolutionSession::new(
        factory,
        StoreManifestIdentity {
            family_id: "family-a".to_string(),
            view_id: "view-a".to_string(),
            generation: 1,
            manifest_hash: "manifest-a".to_string(),
        },
        temp.path().join("exact.db"),
        40,
        6,
    )
    .unwrap();
    let mut first = 0usize;
    session
        .visit_filtered_by_name("target", "rust", &[SymbolKind::Function], None, |_, _| {
            first += 1;
            Ok(true)
        })
        .unwrap();
    let mut second = 0usize;
    session
        .visit_filtered_by_name("target", "rust", &[SymbolKind::Function], None, |_, _| {
            second += 1;
            Ok(true)
        })
        .unwrap();
    assert_eq!(first, 33);
    assert_eq!(second, 33);
    let visit = session.candidate_query_telemetry(CandidateQueryFamily::FilteredByName);
    assert_eq!(visit.executions, 2);
}

#[test]
fn store_filtered_name_pass_cache_does_not_store_early_stop() {
    let temp = TempDir::new().unwrap();
    let layout = StoreLayout::create(temp.path().join("family"), "family-a", "2.30.0").unwrap();
    let factory = StoreConnectionFactory::new(layout, "family-a", "2.30.0");
    let mut connection = factory.open_writer().unwrap();
    ManifestStore::new(&mut connection)
        .ensure_view("view-a", "/repo")
        .unwrap();
    let version = insert_version(&connection, "src/lib.rs", "rust", true);
    for symbol_id in ["a-hit", "b-hit"] {
        connection
            .execute(
                "INSERT INTO symbols(version_id,symbol_id,path,language,name,kind,
                 start_line,start_column,end_line,end_column,start_byte,end_byte,is_test,test_container,test_lifecycle)
                 VALUES (?1,?2,'src/lib.rs','rust','target','function',1,1,1,1,0,1,0,0,0)",
                params![version, symbol_id],
            )
            .unwrap();
    }
    connection
        .execute(
            "INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at)
             VALUES ('view-a',1,'manifest-a','request-a',?1)",
            [NOW],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO manifest_entries(view_id,generation,path,language,version_id,status,observed_content_hash,indexed_at)
             VALUES ('view-a',1,'src/lib.rs','rust',?1,'indexed','hash-src/lib.rs',?2)",
            params![version, NOW],
        )
        .unwrap();
    drop(connection);

    let session = StoreScratchResolutionSession::new(
        factory,
        StoreManifestIdentity {
            family_id: "family-a".to_string(),
            view_id: "view-a".to_string(),
            generation: 1,
            manifest_hash: "manifest-a".to_string(),
        },
        temp.path().join("exact.db"),
        8,
        6,
    )
    .unwrap();
    let mut first = Vec::new();
    session
        .visit_filtered_by_name("target", "rust", &[SymbolKind::Function], None, |_, hit| {
            first.push(hit.symbol.symbol_id);
            Ok(false)
        })
        .unwrap();
    let mut second = Vec::new();
    session
        .visit_filtered_by_name("target", "rust", &[SymbolKind::Function], None, |_, hit| {
            second.push(hit.symbol.symbol_id);
            Ok(true)
        })
        .unwrap();
    assert_eq!(first, ["a-hit"]);
    assert_eq!(second, ["a-hit", "b-hit"]);
    let visit = session.candidate_query_telemetry(CandidateQueryFamily::FilteredByName);
    assert_eq!(visit.executions, 2);
}

#[test]
fn store_mini_index_keeps_excess_type_facts_on_sql() {
    let temp = TempDir::new().unwrap();
    let layout = StoreLayout::create(temp.path().join("family"), "family-a", "2.30.0").unwrap();
    let factory = StoreConnectionFactory::new(layout, "family-a", "2.30.0");
    let mut connection = factory.open_writer().unwrap();
    ManifestStore::new(&mut connection)
        .ensure_view("view-a", "/repo")
        .unwrap();
    let version = insert_version(&connection, "src/lib.rs", "rust", true);
    connection
        .execute(
            "INSERT INTO symbols(version_id,symbol_id,path,language,name,kind,
             start_line,start_column,end_line,end_column,start_byte,end_byte,is_test,test_container,test_lifecycle)
             VALUES (?1,'zeta','src/lib.rs','rust','zeta','variable',1,1,1,1,0,1,0,0,0)",
            [version],
        )
        .unwrap();
    for index in 0..=4096 {
        connection
            .execute(
                "INSERT INTO type_facts(version_id,type_fact_id,symbol_id,language,resolved_type,is_inferred)
                 VALUES (?1,?2,'zeta','rust','TypeC',0)",
                params![version, format!("fact-{index:04}")],
            )
            .unwrap();
    }
    connection
        .execute(
            "INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at)
             VALUES ('view-a',1,'manifest-a','request-a',?1)",
            [NOW],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO manifest_entries(view_id,generation,path,language,version_id,status,observed_content_hash,indexed_at)
             VALUES ('view-a',1,'src/lib.rs','rust',?1,'indexed','hash-src/lib.rs',?2)",
            params![version, NOW],
        )
        .unwrap();
    drop(connection);

    let session = StoreScratchResolutionSession::new(
        factory,
        StoreManifestIdentity {
            family_id: "family-a".to_string(),
            view_id: "view-a".to_string(),
            generation: 1,
            manifest_hash: "manifest-a".to_string(),
        },
        temp.path().join("exact.db"),
        8,
        6,
    )
    .unwrap();
    assert!(
        session
            .symbol_by_id(&version.to_string(), "zeta")
            .unwrap()
            .is_some()
    );
    let mut facts = 0usize;
    session
        .visit_type_facts(
            &SemanticSymbolId {
                version: SemanticVersionId::Store(version),
                local_id: "zeta".to_string(),
            },
            |_, _| {
                facts += 1;
                Ok(true)
            },
        )
        .unwrap();
    assert_eq!(facts, 4097);
    assert_eq!(
        session
            .candidate_query_telemetry(CandidateQueryFamily::SymbolById)
            .executions,
        0
    );
    assert!(
        session
            .candidate_query_telemetry(CandidateQueryFamily::TypeFacts)
            .executions
            > 0
    );
}

#[cfg(unix)]
#[test]
fn store_resolution_refuses_symlinked_scratch_parent_without_outside_file() {
    use std::os::unix::fs::symlink;

    let temp = TempDir::new().unwrap();
    let layout = StoreLayout::create(temp.path().join("family"), "family-a", "2.30.0").unwrap();
    let factory = StoreConnectionFactory::new(layout, "family-a", "2.30.0");
    let mut connection = factory.open_writer().unwrap();
    ManifestStore::new(&mut connection)
        .ensure_view("view-a", "/repo")
        .unwrap();
    let version = insert_version(&connection, "src/lib.rs", "rust", true);
    connection.execute("INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at) VALUES ('view-a',1,'manifest-a','request-a',?1)", [NOW]).unwrap();
    connection.execute("INSERT INTO manifest_entries(view_id,generation,path,language,version_id,status,observed_content_hash,indexed_at) VALUES ('view-a',1,'src/lib.rs','rust',?1,'indexed','hash',?2)", params![version, NOW]).unwrap();
    drop(connection);

    let outside = temp.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    let redirected = temp.path().join("redirected");
    symlink(&outside, &redirected).unwrap();
    let error = StoreScratchResolutionSession::new(
        factory,
        StoreManifestIdentity {
            family_id: "family-a".to_string(),
            view_id: "view-a".to_string(),
            generation: 1,
            manifest_hash: "manifest-a".to_string(),
        },
        redirected.join("exact.db"),
        7,
        6,
    )
    .unwrap_err();

    assert!(matches!(
        error,
        StoreResolutionError::Artifact(
            julie_extract_artifact::store::ResolutionValidationError::SymlinkPath { .. }
        )
    ));
    assert!(!outside.join("exact.db.work").exists());
}

#[test]
fn manifest_windows_scope_versions_and_preserve_failed_path_facts() {
    let temp = TempDir::new().unwrap();
    let exact_path = temp.path().join("exact.db");
    let layout = StoreLayout::create(temp.path().join("family"), "family-a", "2.30.0").unwrap();
    let factory = StoreConnectionFactory::new(layout, "family-a", "2.30.0");
    let mut connection = factory.open_writer().unwrap();
    ManifestStore::new(&mut connection)
        .ensure_view("view-a", "/repo")
        .unwrap();
    let indexed = insert_version(&connection, "src/indexed.rs", "rust", true);
    let preserved = insert_version(&connection, "src/preserved.ts", "typescript", true);
    let retained = insert_version(&connection, "src/retained.rs", "rust", true);
    connection
        .execute(
            "INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at)
             VALUES ('view-a',1,'manifest-a','request-a',?1)",
            [NOW],
        )
        .unwrap();
    for (path, language, version, status, hash, error) in [
        (
            "src/indexed.rs",
            "rust",
            Some(indexed),
            "indexed",
            "hash-indexed",
            None,
        ),
        (
            "src/preserved.ts",
            "typescript",
            Some(preserved),
            "failed_preserved",
            "hash-preserved",
            Some("parse"),
        ),
        (
            "src/failed.py",
            "python",
            None,
            "failed",
            "hash-failed",
            Some("parse"),
        ),
    ] {
        connection
            .execute(
                "INSERT INTO manifest_entries
                 (view_id,generation,path,language,version_id,status,observed_content_hash,indexed_at,error_class,error_json)
                 VALUES ('view-a',1,?1,?2,?3,?4,?5,?6,?7,?8)",
                params![path, language, version, status, hash, NOW, error, error.map(|_| "{}")],
            )
            .unwrap();
    }
    drop(connection);

    let session = StoreScratchResolutionSession::new(
        factory,
        StoreManifestIdentity {
            family_id: "family-a".to_string(),
            view_id: "view-a".to_string(),
            generation: 1,
            manifest_hash: "manifest-a".to_string(),
        },
        &exact_path,
        2,
        6,
    )
    .unwrap();
    let first = session.manifest_window(None).unwrap();
    let second = session
        .manifest_window(first.last().map(|entry| entry.path.as_str()))
        .unwrap();
    let entries = first.into_iter().chain(second).collect::<Vec<_>>();

    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].path, "src/failed.py");
    assert_eq!(entries[0].language, None);
    assert_eq!(entries[0].version_id, None);
    assert_eq!(entries[1].path, "src/indexed.rs");
    assert_eq!(entries[1].language.as_deref(), Some("rust"));
    assert_eq!(entries[2].path, "src/preserved.ts");
    assert_eq!(entries[2].language.as_deref(), Some("typescript"));
    assert!(
        !entries
            .iter()
            .any(|entry| entry.version_id == Some(retained))
    );
    assert_eq!(
        session.extraction_versions_window(None).unwrap(),
        vec![indexed, preserved]
    );
}

#[test]
fn incomplete_l2_refuses_before_creating_exact_output() {
    let temp = TempDir::new().unwrap();
    let exact_path = temp.path().join("exact.db");
    let layout = StoreLayout::create(temp.path().join("family"), "family-a", "2.30.0").unwrap();
    let factory = StoreConnectionFactory::new(layout, "family-a", "2.30.0");
    let mut connection = factory.open_writer().unwrap();
    ManifestStore::new(&mut connection)
        .ensure_view("view-a", "/repo")
        .unwrap();
    let version = insert_version(&connection, "src/lib.rs", "rust", false);
    connection.execute("INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at) VALUES ('view-a',1,'manifest-a','request-a',?1)", [NOW]).unwrap();
    connection.execute("INSERT INTO manifest_entries(view_id,generation,path,language,version_id,status,observed_content_hash,indexed_at) VALUES ('view-a',1,'src/lib.rs','rust',?1,'indexed','hash',?2)", params![version, NOW]).unwrap();
    drop(connection);

    let error = StoreScratchResolutionSession::new(
        factory,
        StoreManifestIdentity {
            family_id: "family-a".to_string(),
            view_id: "view-a".to_string(),
            generation: 1,
            manifest_hash: "manifest-a".to_string(),
        },
        &exact_path,
        2,
        6,
    )
    .unwrap_err();

    assert_eq!(error.code(), "resolution_input_incomplete");
    assert!(!exact_path.exists());
}

#[test]
fn high_collision_store_lookup_matches_legacy_and_caps_ambiguity_evidence() {
    let temp = TempDir::new().unwrap();
    let layout = StoreLayout::create(temp.path().join("family"), "family-a", "2.30.0").unwrap();
    let factory = StoreConnectionFactory::new(layout, "family-a", "2.30.0");
    let mut connection = factory.open_writer().unwrap();
    ManifestStore::new(&mut connection)
        .ensure_view("view-a", "/repo")
        .unwrap();
    let version = insert_version(&connection, "src/lib.rs", "rust", true);
    connection.execute("INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at) VALUES ('view-a',1,'manifest-a','request-a',?1)", [NOW]).unwrap();
    connection.execute("INSERT INTO manifest_entries(view_id,generation,path,language,version_id,status,observed_content_hash,indexed_at) VALUES ('view-a',1,'src/lib.rs','rust',?1,'indexed','hash',?2)", params![version, NOW]).unwrap();
    let transaction = connection.transaction().unwrap();
    {
        let mut insert = transaction
            .prepare(
                "INSERT INTO symbols(
               version_id,symbol_id,path,language,name,kind,
               start_line,start_column,end_line,end_column,start_byte,end_byte,
               is_test,test_container,test_lifecycle
             ) VALUES (?1,?2,'src/lib.rs','rust','collision','function',1,1,1,1,0,1,0,0,0)",
            )
            .unwrap();
        for index in 0..10_000 {
            insert
                .execute(params![version, format!("symbol-{index:05}")])
                .unwrap();
        }
    }
    transaction
        .execute(
            "INSERT INTO reference_sites(version_id,reference_site_id,path,language,
         start_line,start_column,end_line,end_column,start_byte,end_byte,is_exact,provenance,level)
         VALUES (?1,'collision-site','src/lib.rs','rust',2,1,2,10,2,11,1,'target_token',2)",
            [version],
        )
        .unwrap();
    transaction.execute(
        "INSERT INTO identifiers(version_id,identifier_id,reference_site_id,path,language,name,kind,
         start_line,start_column,end_line,end_column,start_byte,end_byte,confidence)
         VALUES (?1,'collision-use','collision-site','src/lib.rs','rust','collision','call',2,1,2,10,2,11,1.0)",
        [version],
    ).unwrap();
    transaction.commit().unwrap();
    drop(connection);

    let exact_path = temp.path().join("exact.db");
    let mut session = StoreScratchResolutionSession::new(
        factory,
        StoreManifestIdentity {
            family_id: "family-a".to_string(),
            view_id: "view-a".to_string(),
            generation: 1,
            manifest_hash: "manifest-a".to_string(),
        },
        &exact_path,
        300,
        6,
    )
    .unwrap();
    let edge = UnresolvedEdge {
        origin: EdgeOrigin::Identifier,
        kind: ReferenceKind::Call,
        language: "rust".to_string(),
        file_id: version.to_string(),
        terminal_name: "collision".to_string(),
        receiver: None,
        caller_scope_symbol_id: None,
        import_context: None,
        receiver_qualifier: None,
        source_confidence: 1.0,
    };
    let legacy = WorkspaceCandidateIndex::build(
        (0..10_000)
            .map(|index| CandidateSymbol {
                symbol_id: format!("symbol-{index:05}"),
                file_id: version.to_string(),
                language: "rust".to_string(),
                name: "collision".to_string(),
                kind: SymbolKind::Function,
                parent_symbol_id: None,
                visibility: None,
                signature: None,
                is_static: None,
            })
            .collect(),
        vec![],
        vec![],
    );

    let legacy_outcome = resolve_one(&edge, &legacy);
    let store_outcome = resolve_with_candidate_lookup(&session, &edge).unwrap();
    assert!(
        matches!(legacy_outcome, TierOutcome::Ambiguous { ref candidates, exact_count: 10_000 } if candidates.len() == 2)
    );
    assert!(
        matches!(store_outcome, TierOutcome::Ambiguous { ref candidates, exact_count: 10_000 } if candidates.len() == 2)
    );
    assert!(session.max_store_read_page() <= 300);
    assert_eq!(session.max_store_read_page(), 2);
    run_resolution_session(&mut session, true, true).unwrap();
    session.finish_exact().unwrap();
    let rows = ResolutionBaseReader::open(&exact_path)
        .unwrap()
        .identifiers()
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].candidates, Some(10_000));
}

#[test]
fn third_same_name_receiver_contributes_type_fact_in_legacy_and_store() {
    let temp = TempDir::new().unwrap();
    let layout = StoreLayout::create(temp.path().join("family"), "family-a", "2.30.0").unwrap();
    let factory = StoreConnectionFactory::new(layout, "family-a", "2.30.0");
    let mut connection = factory.open_writer().unwrap();
    ManifestStore::new(&mut connection)
        .ensure_view("view-a", "/repo")
        .unwrap();
    let version = insert_version(&connection, "src/lib.rs", "rust", true);
    connection.execute("INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at) VALUES ('view-a',1,'manifest-a','request-a',?1)", [NOW]).unwrap();
    connection.execute("INSERT INTO manifest_entries(view_id,generation,path,language,version_id,status,observed_content_hash,indexed_at) VALUES ('view-a',1,'src/lib.rs','rust',?1,'indexed','hash',?2)", params![version, NOW]).unwrap();
    for (symbol_id, name, kind, parent) in [
        ("alpha", "recv", "variable", None),
        ("mid", "recv", "variable", None),
        ("zeta", "recv", "variable", None),
        ("type-c", "TypeC", "class", None),
        ("member", "hit", "method", Some("type-c")),
    ] {
        connection.execute(
            "INSERT INTO symbols(version_id,symbol_id,path,language,name,kind,parent_symbol_id,
             start_line,start_column,end_line,end_column,start_byte,end_byte,is_test,test_container,test_lifecycle)
             VALUES (?1,?2,'src/lib.rs','rust',?3,?4,?5,1,1,1,1,0,1,0,0,0)",
            params![version, symbol_id, name, kind, parent],
        ).unwrap();
    }
    for fact_id in ["fact-zeta-a", "fact-zeta-b"] {
        connection.execute(
            "INSERT INTO type_facts(version_id,type_fact_id,symbol_id,language,resolved_type,is_inferred)
             VALUES (?1,?2,'zeta','rust','TypeC',0)",
            params![version, fact_id],
        ).unwrap();
    }
    drop(connection);

    let session = StoreScratchResolutionSession::new(
        factory,
        StoreManifestIdentity {
            family_id: "family-a".to_string(),
            view_id: "view-a".to_string(),
            generation: 1,
            manifest_hash: "manifest-a".to_string(),
        },
        temp.path().join("exact.db"),
        2,
        6,
    )
    .unwrap();
    let edge = UnresolvedEdge {
        origin: EdgeOrigin::Identifier,
        kind: ReferenceKind::Call,
        language: "rust".to_string(),
        file_id: version.to_string(),
        terminal_name: "hit".to_string(),
        receiver: Some("recv".to_string()),
        caller_scope_symbol_id: None,
        import_context: None,
        receiver_qualifier: None,
        source_confidence: 1.0,
    };
    let legacy_symbols = [
        ("alpha", "recv", SymbolKind::Variable, None),
        ("mid", "recv", SymbolKind::Variable, None),
        ("zeta", "recv", SymbolKind::Variable, None),
        ("type-c", "TypeC", SymbolKind::Class, None),
        ("member", "hit", SymbolKind::Method, Some("type-c")),
    ]
    .into_iter()
    .map(|(symbol_id, name, kind, parent)| CandidateSymbol {
        symbol_id: symbol_id.to_string(),
        file_id: version.to_string(),
        language: "rust".to_string(),
        name: name.to_string(),
        kind,
        parent_symbol_id: parent.map(str::to_string),
        visibility: None,
        signature: None,
        is_static: None,
    })
    .collect();
    let legacy = WorkspaceCandidateIndex::build(
        legacy_symbols,
        vec![
            TypeFact {
                symbol_id: "zeta".to_string(),
                resolved_type: "TypeC".to_string(),
                is_inferred: false,
            },
            TypeFact {
                symbol_id: "zeta".to_string(),
                resolved_type: "TypeC".to_string(),
                is_inferred: false,
            },
        ],
        vec![],
    );

    for outcome in [
        resolve_one(&edge, &legacy),
        resolve_with_candidate_lookup(&session, &edge).unwrap(),
    ] {
        assert!(matches!(
            outcome,
            TierOutcome::Resolved { target_symbol_id, .. } if target_symbol_id.local_id == "member"
        ));
    }
    assert!(session.max_store_read_page() <= 2);
}

#[test]
fn store_phase_windows_freeze_membership_and_emit_identical_exact_bases() {
    let temp = TempDir::new().unwrap();
    let layout = StoreLayout::create(temp.path().join("family"), "family-a", "2.30.0").unwrap();
    let factory = StoreConnectionFactory::new(layout, "family-a", "2.30.0");
    let mut connection = factory.open_writer().unwrap();
    ManifestStore::new(&mut connection)
        .ensure_view("view-a", "/repo")
        .unwrap();
    let version = insert_version(&connection, "src/lib.rs", "rust", true);
    connection.execute("INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at) VALUES ('view-a',1,'manifest-a','request-a',?1)", [NOW]).unwrap();
    connection.execute("INSERT INTO manifest_entries(view_id,generation,path,language,version_id,status,observed_content_hash,indexed_at) VALUES ('view-a',1,'src/lib.rs','rust',?1,'indexed','hash',?2)", params![version, NOW]).unwrap();
    insert_resolution_fixture(&mut connection, version, 23);
    drop(connection);

    let identity = StoreManifestIdentity {
        family_id: "family-a".to_string(),
        view_id: "view-a".to_string(),
        generation: 1,
        manifest_hash: "manifest-a".to_string(),
    };
    let mut small = StoreScratchResolutionSession::new(
        factory.clone(),
        identity.clone(),
        temp.path().join("small.db"),
        1,
        6,
    )
    .unwrap();
    assert_eq!(
        small.scratch_pragma_values().unwrap(),
        (4096, 2, "wal".to_string(), 2, 1, 1, 8000)
    );
    run_resolution_session(&mut small, true, true).unwrap();
    assert_eq!(small.max_emitted_chunk_size(), 1);
    assert_eq!(small.max_store_read_page(), 1);
    assert!(small.max_candidate_cache_entries() <= 3);
    let small_reader_opens = small.phase_reader_opens();
    assert_eq!(small_reader_opens, 26);
    let small_identity = small.finish_exact().unwrap();

    let mut large = StoreScratchResolutionSession::new(
        factory.clone(),
        identity.clone(),
        temp.path().join("large.db"),
        7,
        6,
    )
    .unwrap();
    run_resolution_session(&mut large, true, true).unwrap();
    assert!(large.max_emitted_chunk_size() <= 7);
    assert!(large.max_emitted_chunk_size() > 1);
    assert!(large.max_store_read_page() <= 7);
    assert!(large.max_candidate_cache_entries() <= 21);
    let large_reader_opens = large.phase_reader_opens();
    assert_eq!(large_reader_opens, 7);
    let large_identity = large.finish_exact().unwrap();
    assert!(small_reader_opens > large_reader_opens * 3);

    let small = ResolutionBaseReader::open(small_identity.path).unwrap();
    let large = ResolutionBaseReader::open(large_identity.path).unwrap();
    assert_eq!(small.identifiers().unwrap(), large.identifiers().unwrap());
    assert_eq!(small.pending().unwrap(), large.pending().unwrap());
    assert_eq!(small.identifiers().unwrap().len(), 23);
    assert_eq!(small.pending().unwrap().len(), 23);

    let mut corrupt =
        StoreScratchResolutionSession::new(factory, identity, temp.path().join("corrupt.db"), 7, 6)
            .unwrap();
    corrupt
        .inject_frozen_phase_key_for_test(ResolutionPhase::Pending, version, "missing-pending")
        .unwrap();
    let error = corrupt
        .next_phase_chunk(&ResolutionWorklists {
            phase: ResolutionPhase::Pending,
            ..ResolutionWorklists::default()
        })
        .unwrap_err();
    assert!(matches!(
        error,
        StoreResolutionError::PhaseHydrationMismatch {
            phase: "pending",
            expected: 1,
            actual: 0
        }
    ));
}

#[test]
fn visible_version_roots_commit_once_per_bounded_store_page() {
    let temp = TempDir::new().unwrap();
    let layout = StoreLayout::create(temp.path().join("family"), "family-a", "2.30.0").unwrap();
    let factory = StoreConnectionFactory::new(layout, "family-a", "2.30.0");
    let mut connection = factory.open_writer().unwrap();
    ManifestStore::new(&mut connection)
        .ensure_view("view-a", "/repo")
        .unwrap();
    connection.execute("INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at) VALUES ('view-a',1,'manifest-a','request-a',?1)", [NOW]).unwrap();
    let mut versions = Vec::new();
    for index in 0..5 {
        let path = format!("src/file-{index}.rs");
        let version = insert_version(&connection, &path, "rust", true);
        connection.execute("INSERT INTO manifest_entries(view_id,generation,path,language,version_id,status,observed_content_hash,indexed_at) VALUES ('view-a',1,?1,'rust',?2,'indexed',?3,?4)", params![path, version, format!("hash-{index}"), NOW]).unwrap();
        versions.push(version);
    }
    drop(connection);

    let mut session = StoreScratchResolutionSession::new(
        factory,
        StoreManifestIdentity {
            family_id: "family-a".to_string(),
            view_id: "view-a".to_string(),
            generation: 1,
            manifest_hash: "manifest-a".to_string(),
        },
        temp.path().join("exact.db"),
        2,
        6,
    )
    .unwrap();
    session
        .open_resolution_pass(&ResolutionPassRequest::full())
        .unwrap();

    assert_eq!(session.visible_root_batches(), 3);
    assert!(session.max_store_read_page() <= 2);
    let exact = session.finish_exact().unwrap();
    assert_eq!(
        ResolutionBaseReader::open(exact.path)
            .unwrap()
            .source_versions()
            .unwrap(),
        versions
    );
}

#[test]
fn propagation_pending_scratch_precedence_without_prior() {
    let temp = TempDir::new().unwrap();
    let layout = StoreLayout::create(temp.path().join("family"), "family-a", "2.30.0").unwrap();
    let factory = StoreConnectionFactory::new(layout, "family-a", "2.30.0");
    let mut connection = factory.open_writer().unwrap();
    ManifestStore::new(&mut connection)
        .ensure_view("view-a", "/repo")
        .unwrap();
    let version = insert_version(&connection, "src/lib.rs", "rust", true);
    insert_named_symbol(&connection, version, "caller", "caller", "src/lib.rs");
    insert_named_symbol(&connection, version, "target", "Target", "src/lib.rs");
    insert_named_identifier(&connection, version, "resolved-use", "Target", "src/lib.rs");
    insert_named_identifier(&connection, version, "demoted-use", "Demoted", "src/lib.rs");
    for (pending_id, site_id, name) in [
        ("pending-resolved", "site-resolved-use", "Target"),
        ("pending-demoted", "site-demoted-use", "Demoted"),
    ] {
        connection
            .execute(
                "INSERT INTO pending_relationships
                 (version_id,pending_relationship_id,reference_site_id,from_symbol_id,path,kind,
                  target_display_name,target_terminal_name,target_namespace_json,start_line,start_column,
                  end_line,end_column,start_byte,end_byte,confidence)
                 VALUES (?1,?2,?3,'caller','src/lib.rs','calls',?4,?4,'[]',2,1,2,5,5,9,1.0)",
                params![version, pending_id, site_id, name],
            )
            .unwrap();
    }
    connection
        .execute(
            "INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at)
             VALUES ('view-a',1,'manifest-a','request-a',?1)",
            [NOW],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO manifest_entries
             (view_id,generation,path,language,version_id,status,observed_content_hash,indexed_at)
             VALUES ('view-a',1,'src/lib.rs','rust',?1,'indexed','hash',?2)",
            params![version, NOW],
        )
        .unwrap();
    drop(connection);

    let mut session = StoreScratchResolutionSession::new(
        factory,
        StoreManifestIdentity {
            family_id: "family-a".to_string(),
            view_id: "view-a".to_string(),
            generation: 1,
            manifest_hash: "manifest-a".to_string(),
        },
        temp.path().join("exact.db"),
        9,
        6,
    )
    .unwrap();
    session
        .open_resolution_pass(&ResolutionPassRequest::full())
        .unwrap();
    let mut writes = ResolutionWriteBatch::default();
    writes.record_pending_resolution(
        SemanticPendingRelationshipId {
            version: SemanticVersionId::Store(version),
            local_id: "pending-resolved".to_string(),
        },
        SemanticSymbolId {
            version: SemanticVersionId::Store(version),
            local_id: "target".to_string(),
        },
        1,
        1.0,
        "test",
        1,
    );
    writes.demote_pending(SemanticPendingRelationshipId {
        version: SemanticVersionId::Store(version),
        local_id: "pending-demoted".to_string(),
    });
    session.flush(writes).unwrap();

    let coverage = session
        .propagation_is_covered_batch(&[
            SemanticIdentifierId {
                version: SemanticVersionId::Store(version),
                local_id: "resolved-use".to_string(),
            },
            SemanticIdentifierId {
                version: SemanticVersionId::Store(version),
                local_id: "demoted-use".to_string(),
            },
        ])
        .unwrap();
    assert_eq!(
        coverage
            .into_iter()
            .map(|identifier| identifier.local_id)
            .collect::<HashSet<_>>(),
        HashSet::from(["resolved-use".to_string()])
    );
}

#[test]
fn propagation_pending_batch_does_not_multiply_same_name_candidates_before_window() {
    let temp = TempDir::new().unwrap();
    let layout = StoreLayout::create(temp.path().join("family"), "family-a", "2.30.0").unwrap();
    let factory = StoreConnectionFactory::new(layout, "family-a", "2.30.0");
    let mut connection = factory.open_writer().unwrap();
    ManifestStore::new(&mut connection)
        .ensure_view("view-a", "/repo")
        .unwrap();
    let version = insert_version(&connection, "src/lib.rs", "rust", true);
    insert_named_symbol(&connection, version, "caller", "caller", "src/lib.rs");
    insert_named_symbol(&connection, version, "target", "Target", "src/lib.rs");
    for index in 0..8 {
        insert_named_identifier(
            &connection,
            version,
            &format!("requested-{index:02}"),
            "Target",
            "src/lib.rs",
        );
    }
    insert_named_identifier(
        &connection,
        version,
        "requested-outside",
        "Target",
        "src/lib.rs",
    );
    connection
        .execute(
            "UPDATE identifiers
             SET start_line=99,end_line=99,start_byte=200,end_byte=204
             WHERE version_id=?1 AND identifier_id='requested-outside'",
            [version],
        )
        .unwrap();
    for pending_id in (0..7)
        .map(|index| format!("pending-{index:02}"))
        .chain(["pending-z".to_string()])
    {
        connection
            .execute(
                "INSERT INTO pending_relationships
                 (version_id,pending_relationship_id,reference_site_id,from_symbol_id,path,kind,
                  target_display_name,target_terminal_name,target_namespace_json,start_line,start_column,
                  end_line,end_column,start_byte,end_byte,confidence)
                 VALUES (?1,?2,'site-requested-00','caller','src/lib.rs','calls','Target','Target','[]',
                         2,1,2,5,0,100,1.0)",
                params![version, pending_id],
            )
            .unwrap();
    }
    connection
        .execute(
            "INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at)
             VALUES ('view-a',1,'manifest-a','request-a',?1)",
            [NOW],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO manifest_entries
             (view_id,generation,path,language,version_id,status,observed_content_hash,indexed_at)
             VALUES ('view-a',1,'src/lib.rs','rust',?1,'indexed','hash',?2)",
            params![version, NOW],
        )
        .unwrap();
    drop(connection);

    let mut session = StoreScratchResolutionSession::new(
        factory,
        StoreManifestIdentity {
            family_id: "family-a".to_string(),
            view_id: "view-a".to_string(),
            generation: 1,
            manifest_hash: "manifest-a".to_string(),
        },
        temp.path().join("exact.db"),
        9,
        6,
    )
    .unwrap();
    session
        .open_resolution_pass(&ResolutionPassRequest::full())
        .unwrap();
    let mut writes = ResolutionWriteBatch::default();
    writes.record_pending_resolution(
        SemanticPendingRelationshipId {
            version: SemanticVersionId::Store(version),
            local_id: "pending-z".to_string(),
        },
        SemanticSymbolId {
            version: SemanticVersionId::Store(version),
            local_id: "target".to_string(),
        },
        1,
        1.0,
        "test",
        1,
    );
    session.flush(writes).unwrap();

    let identifiers = (0..8)
        .map(|index| SemanticIdentifierId {
            version: SemanticVersionId::Store(version),
            local_id: format!("requested-{index:02}"),
        })
        .chain([SemanticIdentifierId {
            version: SemanticVersionId::Store(version),
            local_id: "requested-outside".to_string(),
        }])
        .collect::<Vec<_>>();
    let coverage = session.propagation_is_covered_batch(&identifiers).unwrap();
    assert_eq!(coverage.len(), 8);
    assert_eq!(
        session
            .propagation_coverage_telemetry()
            .pending_query_executions,
        1
    );
    assert_eq!(
        session
            .propagation_coverage_telemetry()
            .pending_candidate_rows_read,
        8
    );
    assert!(session.max_store_read_page() <= 9);
}

#[test]
fn propagation_materialized_batch_uses_zero_for_null_relationship_start_line() {
    let temp = TempDir::new().unwrap();
    let layout = StoreLayout::create(temp.path().join("family"), "family-a", "2.30.0").unwrap();
    let factory = StoreConnectionFactory::new(layout, "family-a", "2.30.0");
    let mut connection = factory.open_writer().unwrap();
    ManifestStore::new(&mut connection)
        .ensure_view("view-a", "/repo")
        .unwrap();
    let version = insert_version(&connection, "src/lib.rs", "rust", true);
    insert_named_symbol(&connection, version, "caller", "caller", "src/lib.rs");
    insert_named_symbol(
        &connection,
        version,
        "zero-target",
        "ZeroTarget",
        "src/lib.rs",
    );
    insert_named_identifier(&connection, version, "zero-use", "ZeroTarget", "src/lib.rs");
    connection
        .execute(
            "UPDATE identifiers
             SET start_line=0,end_line=0
             WHERE version_id=?1 AND identifier_id='zero-use'",
            [version],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO reference_sites
             (version_id,reference_site_id,path,language,start_line,start_column,end_line,end_column,
              start_byte,end_byte,is_exact,provenance,level)
             VALUES (?1,'site-zero-rel','src/lib.rs','rust',NULL,NULL,NULL,NULL,NULL,NULL,0,'spanless',2)",
            [version],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO relationships
             (version_id,relationship_id,reference_site_id,from_symbol_id,to_symbol_id,path,kind,
              start_line,start_column,end_line,end_column,start_byte,end_byte,confidence)
             VALUES (?1,'zero-rel','site-zero-rel','caller','zero-target','src/lib.rs','calls',
                     NULL,NULL,NULL,NULL,NULL,NULL,1.0)",
            [version],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at)
             VALUES ('view-a',1,'manifest-a','request-a',?1)",
            [NOW],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO manifest_entries
             (view_id,generation,path,language,version_id,status,observed_content_hash,indexed_at)
             VALUES ('view-a',1,'src/lib.rs','rust',?1,'indexed','hash',?2)",
            params![version, NOW],
        )
        .unwrap();
    drop(connection);

    let mut session = StoreScratchResolutionSession::new(
        factory,
        StoreManifestIdentity {
            family_id: "family-a".to_string(),
            view_id: "view-a".to_string(),
            generation: 1,
            manifest_hash: "manifest-a".to_string(),
        },
        temp.path().join("exact.db"),
        8,
        6,
    )
    .unwrap();
    session
        .open_resolution_pass(&ResolutionPassRequest::full())
        .unwrap();
    let coverage = session
        .propagation_is_covered_batch(&[SemanticIdentifierId {
            version: SemanticVersionId::Store(version),
            local_id: "zero-use".to_string(),
        }])
        .unwrap();
    assert_eq!(
        coverage,
        HashSet::from([SemanticIdentifierId {
            version: SemanticVersionId::Store(version),
            local_id: "zero-use".to_string(),
        }])
    );
}

#[test]
fn propagation_materialized_coverage_applies_locator_rules() {
    let temp = TempDir::new().unwrap();
    let layout = StoreLayout::create(temp.path().join("family"), "family-a", "2.30.0").unwrap();
    let factory = StoreConnectionFactory::new(layout, "family-a", "2.30.0");
    let mut connection = factory.open_writer().unwrap();
    ManifestStore::new(&mut connection)
        .ensure_view("view-a", "/repo")
        .unwrap();
    let version = insert_version(&connection, "src/lib.rs", "rust", true);
    insert_named_symbol(&connection, version, "caller", "caller", "src/lib.rs");
    for (symbol_id, name) in [
        ("byte-target", "ByteTarget"),
        ("line-target", "LineTarget"),
        ("unsupported-target", "Unsupported"),
        ("ambiguous-target", "Ambiguous"),
    ] {
        insert_named_symbol(&connection, version, symbol_id, name, "src/lib.rs");
    }
    for (identifier_id, name) in [
        ("byte-use", "ByteTarget"),
        ("line-use", "LineTarget"),
        ("unsupported-use", "Unsupported"),
        ("ambiguous-use", "Ambiguous"),
        ("ambiguous-other", "Ambiguous"),
    ] {
        insert_named_identifier(&connection, version, identifier_id, name, "src/lib.rs");
    }
    for (identifier_id, line, start_byte, end_byte) in [
        ("byte-use", 2, 10, 14),
        ("line-use", 3, 20, 24),
        ("unsupported-use", 4, 30, 34),
        ("ambiguous-use", 5, 40, 44),
        ("ambiguous-other", 5, 40, 44),
    ] {
        connection
            .execute(
                "UPDATE identifiers
                 SET start_line=?1,end_line=?1,start_byte=?2,end_byte=?3
                 WHERE version_id=?4 AND identifier_id=?5",
                params![line, start_byte, end_byte, version, identifier_id],
            )
            .unwrap();
    }
    for (relationship_id, site_id, target_symbol_id, kind, line, start_byte, end_byte) in [
        (
            "byte-rel",
            "site-byte-rel",
            "byte-target",
            "calls",
            2_i64,
            Some(10_i64),
            Some(14_i64),
        ),
        (
            "line-rel",
            "site-line-rel",
            "line-target",
            "calls",
            3,
            None,
            None,
        ),
        (
            "unsupported-rel",
            "site-unsupported-rel",
            "unsupported-target",
            "macro",
            4,
            Some(30),
            Some(34),
        ),
        (
            "ambiguous-rel",
            "site-ambiguous-rel",
            "ambiguous-target",
            "calls",
            5,
            Some(40),
            Some(44),
        ),
    ] {
        if let (Some(start_byte), Some(end_byte)) = (start_byte, end_byte) {
            connection
                .execute(
                    "INSERT INTO reference_sites
                     (version_id,reference_site_id,path,language,start_line,start_column,end_line,end_column,
                      start_byte,end_byte,is_exact,provenance,level)
                     VALUES (?1,?2,'src/lib.rs','rust',?3,1,?3,5,?4,?5,1,'target_token',2)",
                    params![version, site_id, line, start_byte, end_byte],
                )
                .unwrap();
        } else {
            connection
                .execute(
                    "INSERT INTO reference_sites
                     (version_id,reference_site_id,path,language,start_line,start_column,end_line,end_column,
                      start_byte,end_byte,is_exact,provenance,level)
                     VALUES (?1,?2,'src/lib.rs','rust',NULL,NULL,NULL,NULL,NULL,NULL,0,'spanless',2)",
                    params![version, site_id],
                )
                .unwrap();
        }
        connection
            .execute(
                "INSERT INTO relationships
                 (version_id,relationship_id,reference_site_id,from_symbol_id,to_symbol_id,path,kind,
                  start_line,start_column,end_line,end_column,start_byte,end_byte,confidence)
                 VALUES (?1,?2,?3,'caller',?4,'src/lib.rs',?5,?6,1,?6,5,?7,?8,1.0)",
                params![
                    version,
                    relationship_id,
                    site_id,
                    target_symbol_id,
                    kind,
                    line,
                    start_byte,
                    end_byte
                ],
            )
            .unwrap();
    }
    connection
        .execute(
            "INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at)
             VALUES ('view-a',1,'manifest-a','request-a',?1)",
            [NOW],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO manifest_entries
             (view_id,generation,path,language,version_id,status,observed_content_hash,indexed_at)
             VALUES ('view-a',1,'src/lib.rs','rust',?1,'indexed','hash',?2)",
            params![version, NOW],
        )
        .unwrap();
    drop(connection);

    let mut session = StoreScratchResolutionSession::new(
        factory,
        StoreManifestIdentity {
            family_id: "family-a".to_string(),
            view_id: "view-a".to_string(),
            generation: 1,
            manifest_hash: "manifest-a".to_string(),
        },
        temp.path().join("exact.db"),
        8,
        6,
    )
    .unwrap();
    session
        .open_resolution_pass(&ResolutionPassRequest::full())
        .unwrap();
    let coverage = session
        .propagation_is_covered_batch(
            &["byte-use", "line-use", "unsupported-use", "ambiguous-use"]
                .into_iter()
                .map(|local_id| SemanticIdentifierId {
                    version: SemanticVersionId::Store(version),
                    local_id: local_id.to_string(),
                })
                .collect::<Vec<_>>(),
        )
        .unwrap();
    assert_eq!(
        coverage
            .into_iter()
            .map(|identifier| identifier.local_id)
            .collect::<HashSet<_>>(),
        HashSet::from(["byte-use".to_string(), "line-use".to_string()])
    );
    assert_eq!(
        session
            .propagation_coverage_telemetry()
            .materialized_candidate_rows_read,
        3
    );
}

#[test]
fn propagation_materialized_batch_reads_only_requested_locator_candidates() {
    let temp = TempDir::new().unwrap();
    let layout = StoreLayout::create(temp.path().join("family"), "family-a", "2.30.0").unwrap();
    let factory = StoreConnectionFactory::new(layout, "family-a", "2.30.0");
    let mut connection = factory.open_writer().unwrap();
    ManifestStore::new(&mut connection)
        .ensure_view("view-a", "/repo")
        .unwrap();
    let version = insert_version(&connection, "src/lib.rs", "rust", true);
    insert_named_symbol(&connection, version, "caller", "caller", "src/lib.rs");
    insert_named_symbol(
        &connection,
        version,
        "wanted-target",
        "Wanted",
        "src/lib.rs",
    );
    insert_named_identifier(&connection, version, "wanted-use", "Wanted", "src/lib.rs");
    connection
        .execute(
            "UPDATE identifiers
             SET start_line=2,end_line=2,start_byte=10,end_byte=14
             WHERE version_id=?1 AND identifier_id='wanted-use'",
            [version],
        )
        .unwrap();
    for index in 0..32 {
        let symbol_id = format!("common-target-{index:02}");
        let identifier_id = format!("common-use-{index:02}");
        insert_named_symbol(&connection, version, &symbol_id, "Common", "src/lib.rs");
        insert_named_identifier(&connection, version, &identifier_id, "Common", "src/lib.rs");
        let site_id = format!("site-common-rel-{index:02}");
        let relationship_id = format!("common-rel-{index:02}");
        connection
            .execute(
                "INSERT INTO reference_sites
                 (version_id,reference_site_id,path,language,start_line,start_column,end_line,end_column,
                  start_byte,end_byte,is_exact,provenance,level)
                 VALUES (?1,?2,'src/lib.rs','rust',2,1,2,5,5,9,1,'target_token',2)",
                params![version, site_id],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO relationships
                 (version_id,relationship_id,reference_site_id,from_symbol_id,to_symbol_id,path,kind,
                  start_line,start_column,end_line,end_column,start_byte,end_byte,confidence)
                 VALUES (?1,?2,?3,'caller',?4,'src/lib.rs','calls',2,1,2,5,5,9,1.0)",
                params![version, relationship_id, site_id, symbol_id],
            )
            .unwrap();
    }
    connection
        .execute(
            "INSERT INTO reference_sites
             (version_id,reference_site_id,path,language,start_line,start_column,end_line,end_column,
              start_byte,end_byte,is_exact,provenance,level)
             VALUES (?1,'site-wanted-rel','src/lib.rs','rust',2,1,2,5,10,14,1,'target_token',2)",
            [version],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO relationships
             (version_id,relationship_id,reference_site_id,from_symbol_id,to_symbol_id,path,kind,
              start_line,start_column,end_line,end_column,start_byte,end_byte,confidence)
             VALUES (?1,'wanted-rel','site-wanted-rel','caller','wanted-target','src/lib.rs',
                     'calls',2,1,2,5,10,14,1.0)",
            [version],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at)
             VALUES ('view-a',1,'manifest-a','request-a',?1)",
            [NOW],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO manifest_entries
             (view_id,generation,path,language,version_id,status,observed_content_hash,indexed_at)
             VALUES ('view-a',1,'src/lib.rs','rust',?1,'indexed','hash',?2)",
            params![version, NOW],
        )
        .unwrap();
    drop(connection);

    let mut session = StoreScratchResolutionSession::new(
        factory,
        StoreManifestIdentity {
            family_id: "family-a".to_string(),
            view_id: "view-a".to_string(),
            generation: 1,
            manifest_hash: "manifest-a".to_string(),
        },
        temp.path().join("exact.db"),
        64,
        6,
    )
    .unwrap();
    session
        .open_resolution_pass(&ResolutionPassRequest::full())
        .unwrap();
    let coverage = session
        .propagation_is_covered_batch(&[SemanticIdentifierId {
            version: SemanticVersionId::Store(version),
            local_id: "wanted-use".to_string(),
        }])
        .unwrap();
    assert_eq!(
        coverage
            .into_iter()
            .map(|identifier| identifier.local_id)
            .collect::<HashSet<_>>(),
        HashSet::from(["wanted-use".to_string()])
    );
    assert_eq!(
        session
            .propagation_coverage_telemetry()
            .materialized_candidate_rows_read,
        1
    );
}

#[test]
fn scoped_store_resolution_reuses_predecessor_and_carries_unselected_sibling() {
    let temp = TempDir::new().unwrap();
    let layout = StoreLayout::create(temp.path().join("family"), "family-a", "2.30.0").unwrap();
    let factory = StoreConnectionFactory::new(layout.clone(), "family-a", "2.30.0");
    let mut connection = factory.open_writer().unwrap();
    ensure_resolution_scope_feature(&connection).unwrap();
    ManifestStore::new(&mut connection)
        .ensure_view("view-a", "/repo")
        .unwrap();
    let old_target = insert_version(&connection, "src/target.rs", "rust", true);
    let new_target = insert_version(&connection, "src/target-new.rs", "rust", true);
    connection
        .execute(
            "UPDATE file_versions SET path='src/target.rs' WHERE version_id=?1",
            [new_target],
        )
        .unwrap();
    let user = insert_version(&connection, "src/user.rs", "rust", true);
    let untouched = insert_version(&connection, "src/untouched.rs", "rust", true);
    insert_named_symbol(&connection, old_target, "old-foo", "Foo", "src/target.rs");
    insert_named_symbol(&connection, new_target, "new-foo", "Foo", "src/target.rs");
    insert_named_symbol(&connection, user, "caller", "caller", "src/user.rs");
    insert_named_symbol(
        &connection,
        user,
        "sibling-target",
        "NeverDefined",
        "src/user.rs",
    );
    insert_named_symbol(
        &connection,
        untouched,
        "untouched-target",
        "UntouchedTarget",
        "src/untouched.rs",
    );
    insert_named_symbol(
        &connection,
        untouched,
        "untouched-caller",
        "untouched_caller",
        "src/untouched.rs",
    );
    connection
        .execute(
            "INSERT INTO reference_sites
             (version_id,reference_site_id,path,language,start_line,start_column,end_line,end_column,
              start_byte,end_byte,is_exact,provenance,level)
             VALUES (?1,'site-untouched-pending','src/untouched.rs','rust',2,1,2,5,5,9,1,
                     'target_token',2)",
            [untouched],
        )
        .unwrap();
    insert_named_identifier(
        &connection,
        old_target,
        "removed-use",
        "NeverDefined",
        "src/target.rs",
    );
    insert_named_identifier(&connection, user, "foo-use", "Foo", "src/user.rs");
    insert_named_identifier(
        &connection,
        user,
        "sibling-use",
        "NeverDefined",
        "src/user.rs",
    );
    for index in 0..3 {
        insert_named_identifier(
            &connection,
            untouched,
            &format!("padding-use-{index}"),
            &format!("Padding{index}"),
            "src/untouched.rs",
        );
    }
    connection
        .execute(
            "UPDATE identifiers
             SET start_line=3,end_line=3,start_byte=15,end_byte=19,
                 metadata_json='{\"receiver\":\"receiverOnly\"}'
             WHERE version_id=?1 AND identifier_id='sibling-use'",
            [user],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE identifiers
             SET name='ScratchOnly',start_line=3,end_line=3,start_byte=15,end_byte=19
             WHERE version_id=?1 AND identifier_id='padding-use-0'",
            [untouched],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO pending_relationships
             (version_id,pending_relationship_id,reference_site_id,from_symbol_id,path,kind,
              target_display_name,target_terminal_name,target_namespace_json,start_line,start_column,
              end_line,end_column,start_byte,end_byte,confidence)
             VALUES (?1,?2,'site-foo-use','caller','src/user.rs','calls','Foo','Foo','[]',2,1,2,5,5,9,1.0)",
            params![user, "a-unresolved"],
        )
        .unwrap();
    for pending_relationship_id in ["delta-replace", "delta-tombstone", "scratch-authority"] {
        connection
            .execute(
                "INSERT INTO pending_relationships
                 (version_id,pending_relationship_id,reference_site_id,from_symbol_id,path,kind,
                  target_display_name,target_terminal_name,target_namespace_json,start_line,start_column,
                  end_line,end_column,start_byte,end_byte,confidence)
                 VALUES (?1,?2,'site-untouched-pending','untouched-caller','src/untouched.rs','calls',
                         'NeverPending','NeverPending','[]',3,1,3,5,15,19,1.0)",
                params![untouched, pending_relationship_id],
            )
            .unwrap();
    }
    connection
        .execute(
            "UPDATE pending_relationships
             SET target_display_name='ScratchOnly',target_terminal_name='ScratchOnly'
             WHERE version_id=?1 AND pending_relationship_id='scratch-authority'",
            [untouched],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO pending_relationships
             (version_id,pending_relationship_id,reference_site_id,from_symbol_id,path,kind,
              target_display_name,target_terminal_name,target_namespace_json,start_line,start_column,
              end_line,end_column,start_byte,end_byte,confidence)
             VALUES (?1,?2,'site-foo-use','caller','src/user.rs','calls','Foo','Foo','[]',2,1,2,5,5,9,1.0)",
            params![user, "z-resolved"],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO relationships
             (version_id,relationship_id,reference_site_id,from_symbol_id,to_symbol_id,path,kind,
              start_line,start_column,end_line,end_column,start_byte,end_byte,confidence)
             VALUES (?1,'sibling-relationship','site-sibling-use','caller','sibling-target',
                     'src/user.rs','calls',3,1,3,5,15,19,1.0)",
            [user],
        )
        .unwrap();
    let first_entries = [
        manifest_entry(&connection, old_target),
        manifest_entry(&connection, user),
        manifest_entry(&connection, untouched),
    ];
    let first = publish_manifest(&mut connection, None, first_entries, "request-first");
    let base_path = layout.bases_dir().join("base-a.db");
    let mut base = ResolutionBaseWriter::new(&base_path, &first.manifest_hash, 6).unwrap();
    base.push_source_version(old_target).unwrap();
    base.push_source_version(user).unwrap();
    base.push_source_version(untouched).unwrap();
    base.push_identifier_resolution(ResolutionIdentifierRow {
        version_id: old_target,
        identifier_id: "removed-use".to_string(),
        target_version_id: None,
        target_symbol_id: None,
        tier: None,
        confidence: None,
        method: Some("removed-version".to_string()),
        outcome: "missing".to_string(),
        candidates: Some(0),
    })
    .unwrap();
    base.push_pending_resolution(ResolutionPendingRow {
        version_id: user,
        pending_relationship_id: "z-resolved".to_string(),
        target_version_id: old_target,
        target_symbol_id: "old-foo".to_string(),
        tier: 4,
        confidence: 1.0,
        method: "base-pending".to_string(),
    })
    .unwrap();
    for (pending_relationship_id, method) in [
        ("delta-replace", "base-delta-replace"),
        ("delta-tombstone", "base-delta-tombstone"),
        ("scratch-authority", "base-scratch-authority"),
    ] {
        base.push_pending_resolution(ResolutionPendingRow {
            version_id: untouched,
            pending_relationship_id: pending_relationship_id.to_string(),
            target_version_id: untouched,
            target_symbol_id: "untouched-target".to_string(),
            tier: 1,
            confidence: 0.9,
            method: method.to_string(),
        })
        .unwrap();
    }
    base.push_identifier_resolution(ResolutionIdentifierRow {
        version_id: user,
        identifier_id: "foo-use".to_string(),
        target_version_id: Some(old_target),
        target_symbol_id: Some("old-foo".to_string()),
        tier: Some(4),
        confidence: Some(1.0),
        method: Some("base-foo".to_string()),
        outcome: "resolved".to_string(),
        candidates: Some(1),
    })
    .unwrap();
    base.push_identifier_resolution(ResolutionIdentifierRow {
        version_id: user,
        identifier_id: "sibling-use".to_string(),
        target_version_id: Some(user),
        target_symbol_id: Some("sibling-target".to_string()),
        tier: Some(1),
        confidence: Some(0.95),
        method: Some("base-sibling".to_string()),
        outcome: "resolved".to_string(),
        candidates: None,
    })
    .unwrap();
    for index in 0..3 {
        base.push_identifier_resolution(ResolutionIdentifierRow {
            version_id: untouched,
            identifier_id: format!("padding-use-{index}"),
            target_version_id: None,
            target_symbol_id: None,
            tier: None,
            confidence: None,
            method: None,
            outcome: "missing".to_string(),
            candidates: Some(0),
        })
        .unwrap();
    }
    let base_identity = base.finish_with_target_lookup(|_, _| Ok(true)).unwrap();
    connection
        .execute(
            "INSERT INTO resolution_bases
         (base_id,manifest_hash,resolver_output_epoch,state,relative_path,identifier_count,
          pending_count,file_bytes,file_sha256,request_id,created_at,updated_at)
         VALUES ('base-a',?1,6,'ready','bases/base-a.db',6,4,?2,?3,'request-base',?4,?4)",
            params![
                first.manifest_hash,
                i64::try_from(base_identity.file_bytes).unwrap(),
                base_identity.file_sha256,
                NOW
            ],
        )
        .unwrap();
    for version_id in [old_target, user, untouched] {
        connection
            .execute(
                "INSERT INTO resolution_base_versions(base_id,version_id) VALUES ('base-a',?1)",
                [version_id],
            )
            .unwrap();
    }
    connection
        .execute(
            "INSERT INTO resolution_deltas
         (view_id,delta_generation,base_id,manifest_generation,manifest_hash,resolver_output_epoch,
          identifier_replacements,pending_replacements,pending_tombstones,exact_gap_rows,
          exact_gap_files,exact_gap_json,request_id,created_at)
         VALUES ('view-a',1,'base-a',?1,?2,6,0,2,1,0,0,'[]','request-base',?3)",
            params![
                i64::try_from(first.generation).unwrap(),
                first.manifest_hash,
                NOW
            ],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO resolution_pending_deltas
             (view_id,delta_generation,version_id,pending_relationship_id,operation,
              target_version_id,target_symbol_id,tier,confidence,method)
             VALUES ('view-a',1,?1,'delta-replace','replace',?1,'untouched-target',1,0.95,
                     'delta-replace')",
            [untouched],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO resolution_pending_deltas
             (view_id,delta_generation,version_id,pending_relationship_id,operation,
              target_version_id,target_symbol_id,tier,confidence,method)
             VALUES ('view-a',1,?1,'scratch-authority','replace',?1,'untouched-target',1,0.95,
                     'delta-shadowed')",
            [untouched],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO resolution_pending_deltas
             (view_id,delta_generation,version_id,pending_relationship_id,operation,
              target_version_id,target_symbol_id,tier,confidence,method)
             VALUES ('view-a',1,?1,'delta-tombstone','tombstone',NULL,NULL,NULL,NULL,NULL)",
            [untouched],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE views SET resolution_state='exact',resolution_base_id='base-a',
         resolution_delta_generation=1,resolution_exact_at=?1 WHERE view_id='view-a'",
            [i64::try_from(first.generation).unwrap()],
        )
        .unwrap();
    let second_entries = [
        manifest_entry(&connection, new_target),
        manifest_entry(&connection, user),
        manifest_entry(&connection, untouched),
    ];
    let second = publish_manifest(
        &mut connection,
        Some(i64::try_from(first.generation).unwrap()),
        second_entries,
        "request-second",
    );
    drop(connection);

    let probe_identity = StoreManifestIdentity {
        family_id: "family-a".to_string(),
        view_id: "view-a".to_string(),
        generation: i64::try_from(second.generation).unwrap(),
        manifest_hash: second.manifest_hash.clone(),
    };
    let mut selected_probe = StoreScratchResolutionSession::new(
        factory.clone(),
        probe_identity.clone(),
        temp.path().join("selected-probe.db"),
        8,
        6,
    )
    .unwrap();
    assert!(selected_probe.prior_resolution_state().unwrap().is_some());
    selected_probe
        .open_resolution_pass(&ResolutionPassRequest { full: false })
        .unwrap();
    let prior_coverage = selected_probe
        .propagation_is_covered_batch(&[SemanticIdentifierId {
            version: SemanticVersionId::Store(untouched),
            local_id: "padding-use-0".to_string(),
        }])
        .unwrap();
    assert_eq!(prior_coverage.len(), 1);
    let coverage = selected_probe
        .propagation_is_covered_batch(&[
            SemanticIdentifierId {
                version: SemanticVersionId::Store(user),
                local_id: "foo-use".to_string(),
            },
            SemanticIdentifierId {
                version: SemanticVersionId::Store(user),
                local_id: "sibling-use".to_string(),
            },
        ])
        .unwrap();
    assert_eq!(
        coverage
            .into_iter()
            .map(|identifier| identifier.local_id)
            .collect::<HashSet<_>>(),
        HashSet::from(["foo-use".to_string(), "sibling-use".to_string()])
    );
    assert_eq!(
        selected_probe
            .candidate_query_telemetry(CandidateQueryFamily::LocateIdentifier)
            .executions,
        0,
        "Store batch coverage must not execute scalar identifier locators"
    );
    assert_eq!(
        selected_probe.propagation_coverage_telemetry(),
        PropagationCoverageTelemetry {
            reader_opens: 2,
            pending_query_executions: 2,
            pending_candidate_rows_read: 3,
            materialized_query_executions: 1,
            materialized_candidate_rows_read: 1,
        }
    );
    let mut writes = ResolutionWriteBatch::default();
    writes.demote_identifier(SemanticIdentifierId {
        version: SemanticVersionId::Store(user),
        local_id: "foo-use".to_string(),
    });
    writes.demote_pending(SemanticPendingRelationshipId {
        version: SemanticVersionId::Store(untouched),
        local_id: "scratch-authority".to_string(),
    });
    selected_probe.flush(writes).unwrap();
    let demoted_coverage = selected_probe
        .propagation_is_covered_batch(&[SemanticIdentifierId {
            version: SemanticVersionId::Store(untouched),
            local_id: "padding-use-0".to_string(),
        }])
        .unwrap();
    assert!(demoted_coverage.is_empty());
    let selected_worklists = ResolutionWorklists {
        effective_full: false,
        selected_versions: vec![SemanticVersionId::Store(user)],
        phase: ResolutionPhase::ResolvedIdentifiers,
        ..ResolutionWorklists::default()
    };
    let mut selected_ids = Vec::new();
    while let Some(ResolutionPhaseChunk::ResolvedIdentifiers(rows)) = selected_probe
        .next_phase_chunk(&selected_worklists)
        .unwrap()
    {
        selected_ids.extend(rows.into_iter().map(|row| row.identifier.identifier_id));
    }
    assert_eq!(selected_ids, ["sibling-use"]);

    let mut hydration_probe = StoreScratchResolutionSession::new(
        factory.clone(),
        StoreManifestIdentity {
            family_id: "family-a".to_string(),
            view_id: "view-a".to_string(),
            generation: i64::try_from(second.generation).unwrap(),
            manifest_hash: second.manifest_hash.clone(),
        },
        temp.path().join("hydration-probe.db"),
        1,
        6,
    )
    .unwrap();
    assert!(hydration_probe.prior_resolution_state().unwrap().is_some());
    hydration_probe
        .open_resolution_pass(&ResolutionPassRequest { full: false })
        .unwrap();
    let hydration_worklists = ResolutionWorklists {
        effective_full: false,
        selected_versions: vec![SemanticVersionId::Store(user)],
        phase: ResolutionPhase::ResolvedIdentifiers,
        ..ResolutionWorklists::default()
    };
    let first_hydrated = hydration_probe
        .next_phase_chunk(&hydration_worklists)
        .unwrap()
        .unwrap();
    assert!(matches!(
        first_hydrated,
        ResolutionPhaseChunk::ResolvedIdentifiers(rows)
            if rows[0].identifier.identifier_id == "foo-use"
    ));
    let mut writes = ResolutionWriteBatch::default();
    writes.demote_identifier(SemanticIdentifierId {
        version: SemanticVersionId::Store(user),
        local_id: "sibling-use".to_string(),
    });
    hydration_probe.flush(writes).unwrap();
    assert!(
        hydration_probe
            .next_phase_chunk(&hydration_worklists)
            .unwrap()
            .is_none()
    );

    let mut name_probe = StoreScratchResolutionSession::new(
        factory.clone(),
        probe_identity,
        temp.path().join("name-probe.db"),
        1,
        6,
    )
    .unwrap();
    assert!(name_probe.prior_resolution_state().unwrap().is_some());
    name_probe
        .open_resolution_pass(&ResolutionPassRequest { full: false })
        .unwrap();
    let name_worklists = ResolutionWorklists {
        effective_full: false,
        recheck_names: vec!["NeverDefined".to_string()],
        phase: ResolutionPhase::ResolvedIdentifiers,
        ..ResolutionWorklists::default()
    };
    let mut name_ids = Vec::new();
    while let Some(ResolutionPhaseChunk::ResolvedIdentifiers(rows)) =
        name_probe.next_phase_chunk(&name_worklists).unwrap()
    {
        name_ids.extend(rows.into_iter().map(|row| row.identifier.identifier_id));
    }
    assert_eq!(name_ids, ["sibling-use"]);

    let forced_path = temp.path().join("forced.db");
    let mut forced = StoreScratchResolutionSession::new(
        factory.clone(),
        StoreManifestIdentity {
            family_id: "family-a".to_string(),
            view_id: "view-a".to_string(),
            generation: i64::try_from(second.generation).unwrap(),
            manifest_hash: second.manifest_hash.clone(),
        },
        &forced_path,
        1,
        6,
    )
    .unwrap();
    let (_, forced_report) = run_resolution_session(&mut forced, true, true).unwrap();
    assert!(forced_report.rows.is_some());
    forced.finish_exact().unwrap();
    let forced_rows = ResolutionBaseReader::open(&forced_path)
        .unwrap()
        .identifiers()
        .unwrap();
    assert_eq!(forced_rows.len(), 5);
    assert_ne!(
        forced_rows
            .iter()
            .find(|row| row.identifier_id == "sibling-use")
            .and_then(|row| row.method.as_deref()),
        Some("base-sibling")
    );

    let exact_path = temp.path().join("exact.db");
    let mut session = StoreScratchResolutionSession::new(
        factory,
        StoreManifestIdentity {
            family_id: "family-a".to_string(),
            view_id: "view-a".to_string(),
            generation: i64::try_from(second.generation).unwrap(),
            manifest_hash: second.manifest_hash.clone(),
        },
        &exact_path,
        1,
        6,
    )
    .unwrap();
    let (_, report) = run_resolution_session(&mut session, false, true).unwrap();
    assert_eq!(report.status.as_str(), "partial");
    assert!(report.rows.is_none());
    let mut pending_writes = ResolutionWriteBatch::default();
    pending_writes.record_pending_resolution(
        SemanticPendingRelationshipId {
            version: SemanticVersionId::Store(untouched),
            local_id: "scratch-authority".to_string(),
        },
        SemanticSymbolId {
            version: SemanticVersionId::Store(untouched),
            local_id: "untouched-target".to_string(),
        },
        1,
        0.99,
        "scratch-authority",
        6,
    );
    session.flush(pending_writes).unwrap();
    session.finish_exact().unwrap();
    let rows = ResolutionBaseReader::open(&exact_path)
        .unwrap()
        .identifiers()
        .unwrap();
    assert_eq!(rows.len(), 5);
    let foo = rows
        .iter()
        .find(|row| row.identifier_id == "foo-use")
        .unwrap();
    assert_eq!(foo.target_version_id, Some(new_target));
    let sibling = rows
        .iter()
        .find(|row| row.identifier_id == "sibling-use")
        .unwrap();
    assert_eq!(sibling.target_version_id, Some(user));
    assert_eq!(sibling.method.as_deref(), Some("base-sibling"));
    assert!(!rows.iter().any(|row| row.identifier_id == "removed-use"));
    let pending = ResolutionBaseReader::open(&exact_path)
        .unwrap()
        .pending()
        .unwrap();
    assert!(
        pending.iter().any(|row| {
            row.pending_relationship_id == "delta-replace" && row.method == "delta-replace"
        }),
        "{pending:?}"
    );
    assert!(
        !pending
            .iter()
            .any(|row| row.pending_relationship_id == "delta-tombstone")
    );
    assert!(pending.iter().any(|row| {
        row.pending_relationship_id == "scratch-authority" && row.method == "scratch-authority"
    }));

    let frozen_path = temp.path().join("frozen-state.db");
    let mut frozen = StoreScratchResolutionSession::new(
        StoreConnectionFactory::new(layout.clone(), "family-a", "2.30.0"),
        StoreManifestIdentity {
            family_id: "family-a".to_string(),
            view_id: "view-a".to_string(),
            generation: i64::try_from(second.generation).unwrap(),
            manifest_hash: second.manifest_hash.clone(),
        },
        &frozen_path,
        1,
        6,
    )
    .unwrap();
    run_resolution_session(&mut frozen, false, true).unwrap();
    Connection::open(layout.store_db())
        .unwrap()
        .execute(
            "UPDATE resolution_deltas SET pending_replacements=1
             WHERE view_id='view-a' AND delta_generation=1",
            [],
        )
        .unwrap();
    assert!(matches!(
        frozen.finish_exact().unwrap_err(),
        StoreResolutionError::Artifact(
            julie_extract_artifact::store::ResolutionValidationError::InvalidMetadata { .. }
        )
    ));
    assert!(!frozen_path.exists());

    let connection = Connection::open(layout.store_db()).unwrap();
    connection
        .execute(
            "UPDATE resolution_deltas SET identifier_replacements=1,pending_replacements=2
         WHERE view_id='view-a' AND delta_generation=1",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO resolution_identifier_deltas
         (view_id,delta_generation,version_id,identifier_id,target_version_id,target_symbol_id,
          tier,confidence,method,outcome,candidates)
         VALUES ('view-a',1,?1,'sibling-use',?2,'old-foo',4,1.0,'stale','resolved',1)",
            params![user, old_target],
        )
        .unwrap();
    drop(connection);
    let stale_path = temp.path().join("stale.db");
    let mut stale = StoreScratchResolutionSession::new(
        StoreConnectionFactory::new(layout.clone(), "family-a", "2.30.0"),
        StoreManifestIdentity {
            family_id: "family-a".to_string(),
            view_id: "view-a".to_string(),
            generation: i64::try_from(second.generation).unwrap(),
            manifest_hash: second.manifest_hash.clone(),
        },
        &stale_path,
        1,
        6,
    )
    .unwrap();
    run_resolution_session(&mut stale, false, true).unwrap();
    assert!(matches!(
        stale.finish_exact().unwrap_err(),
        StoreResolutionError::Artifact(
            julie_extract_artifact::store::ResolutionValidationError::TargetMissing { .. }
        )
    ));
    assert!(!stale_path.exists());

    let mut connection = Connection::open(layout.store_db()).unwrap();
    connection
        .execute(
            "DELETE FROM resolution_identifier_deltas
             WHERE view_id='view-a' AND delta_generation=1",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO resolution_identifier_deltas
             (view_id,delta_generation,version_id,identifier_id,target_version_id,target_symbol_id,
              tier,confidence,method,outcome,candidates)
             VALUES ('view-a',1,?1,'foo-use',?2,'new-foo',4,1.0,'cumulative','resolved',1)",
            params![user, new_target],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE resolution_deltas
             SET manifest_generation=?1,manifest_hash=?2,identifier_replacements=1,
                 pending_replacements=2
             WHERE view_id='view-a' AND delta_generation=1",
            params![
                i64::try_from(second.generation).unwrap(),
                second.manifest_hash
            ],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE views SET resolution_state='exact',resolution_base_id='base-a',
             resolution_delta_generation=1,resolution_exact_at=?1 WHERE view_id='view-a'",
            [i64::try_from(second.generation).unwrap()],
        )
        .unwrap();
    let latest_target = insert_version(&connection, "src/target-latest.rs", "rust", true);
    connection
        .execute(
            "UPDATE file_versions SET path='src/target.rs' WHERE version_id=?1",
            [latest_target],
        )
        .unwrap();
    insert_named_symbol(
        &connection,
        latest_target,
        "latest-foo",
        "Foo",
        "src/target.rs",
    );
    let third_entries = [
        manifest_entry(&connection, latest_target),
        manifest_entry(&connection, user),
        manifest_entry(&connection, untouched),
    ];
    let third = publish_manifest(
        &mut connection,
        Some(i64::try_from(second.generation).unwrap()),
        third_entries,
        "request-third",
    );
    let (base_manifest_hash, delta_manifest_hash): (String, String) = connection
        .query_row(
            "SELECT base.manifest_hash,delta.manifest_hash
             FROM resolution_bases AS base
             JOIN resolution_deltas AS delta ON delta.base_id=base.base_id
             WHERE base.base_id='base-a' AND delta.view_id='view-a' AND delta.delta_generation=1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(base_manifest_hash, first.manifest_hash);
    assert_ne!(base_manifest_hash, second.manifest_hash);
    assert_eq!(delta_manifest_hash, second.manifest_hash);
    drop(connection);

    let cumulative_path = temp.path().join("cumulative.db");
    let mut cumulative = StoreScratchResolutionSession::new(
        StoreConnectionFactory::new(layout, "family-a", "2.30.0"),
        StoreManifestIdentity {
            family_id: "family-a".to_string(),
            view_id: "view-a".to_string(),
            generation: i64::try_from(third.generation).unwrap(),
            manifest_hash: third.manifest_hash,
        },
        &cumulative_path,
        1,
        6,
    )
    .unwrap();
    let (_, cumulative_report) = run_resolution_session(&mut cumulative, false, true).unwrap();
    assert_eq!(cumulative_report.status.as_str(), "partial");
    assert!(cumulative_report.rows.is_none());
    cumulative.finish_exact().unwrap();
    let cumulative_reader = ResolutionBaseReader::open(&cumulative_path).unwrap();
    let cumulative_identifiers = cumulative_reader.identifiers().unwrap();
    assert_eq!(cumulative_identifiers.len(), 5);
    assert!(cumulative_identifiers.iter().any(|row| {
        row.identifier_id == "foo-use" && row.target_version_id == Some(latest_target)
    }));
    assert!(
        cumulative_identifiers.iter().any(|row| {
            row.identifier_id == "sibling-use" && row.target_version_id == Some(user)
        })
    );
    let cumulative_pending = cumulative_reader.pending().unwrap();
    assert!(cumulative_pending.iter().any(|row| {
        row.pending_relationship_id == "delta-replace" && row.method == "delta-replace"
    }));
    assert!(cumulative_pending.iter().any(|row| {
        row.pending_relationship_id == "scratch-authority" && row.method == "delta-shadowed"
    }));
    assert!(
        !cumulative_pending
            .iter()
            .any(|row| row.pending_relationship_id == "delta-tombstone")
    );
}

#[test]
fn scoped_delta_finalizer_matches_full_diff_and_emits_pending_tombstones() {
    let temp = TempDir::new().unwrap();
    let layout = StoreLayout::create(temp.path().join("family"), "family-a", "2.30.0").unwrap();
    let factory = StoreConnectionFactory::new(layout.clone(), "family-a", "2.30.0");
    let mut connection = factory.open_writer().unwrap();
    ensure_resolution_scope_feature(&connection).unwrap();
    ManifestStore::new(&mut connection)
        .ensure_view("view-a", "/repo")
        .unwrap();

    let old_version = insert_version(&connection, "src/changed-old.rs", "rust", true);
    let new_version = insert_version(&connection, "src/changed-new.rs", "rust", true);
    connection
        .execute(
            "UPDATE file_versions SET path='src/changed.rs' WHERE version_id=?1",
            [old_version],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE file_versions SET path='src/changed.rs' WHERE version_id=?1",
            [new_version],
        )
        .unwrap();
    let untouched_version = insert_version(&connection, "src/untouched.rs", "rust", true);
    let cold_version = insert_version(&connection, "src/cold.rs", "rust", true);
    insert_named_symbol(
        &connection,
        old_version,
        "old-target",
        "OldTarget",
        "src/changed.rs",
    );
    insert_named_symbol(
        &connection,
        new_version,
        "new-target",
        "NewTarget",
        "src/changed.rs",
    );
    insert_named_symbol(
        &connection,
        untouched_version,
        "stable-target",
        "StableTarget",
        "src/untouched.rs",
    );
    insert_named_symbol(
        &connection,
        cold_version,
        "cold-target",
        "ColdTarget",
        "src/cold.rs",
    );
    insert_named_symbol(
        &connection,
        old_version,
        "caller",
        "caller",
        "src/changed.rs",
    );
    insert_named_symbol(
        &connection,
        new_version,
        "caller",
        "caller",
        "src/changed.rs",
    );
    insert_named_symbol(
        &connection,
        untouched_version,
        "caller",
        "caller",
        "src/untouched.rs",
    );
    insert_named_symbol(&connection, cold_version, "caller", "caller", "src/cold.rs");
    insert_named_identifier(
        &connection,
        old_version,
        "old-use",
        "OldTarget",
        "src/changed.rs",
    );
    insert_named_identifier(
        &connection,
        new_version,
        "new-use",
        "NewTarget",
        "src/changed.rs",
    );
    insert_named_identifier(
        &connection,
        untouched_version,
        "stable-use",
        "StableTarget",
        "src/untouched.rs",
    );
    for index in 0..256 {
        insert_named_identifier(
            &connection,
            untouched_version,
            &format!("stable-zpadding-{index:03}"),
            "StableTarget",
            "src/untouched.rs",
        );
    }
    insert_named_identifier(
        &connection,
        cold_version,
        "cold-use",
        "ColdTarget",
        "src/cold.rs",
    );
    for index in 0..512 {
        insert_named_identifier(
            &connection,
            cold_version,
            &format!("cold-zpadding-{index:03}"),
            "ColdTarget",
            "src/cold.rs",
        );
    }
    insert_named_pending(
        &connection,
        old_version,
        "old-pending",
        "OldTarget",
        "src/changed.rs",
    );
    insert_named_pending(
        &connection,
        new_version,
        "new-pending",
        "NewTarget",
        "src/changed.rs",
    );
    insert_named_pending(
        &connection,
        untouched_version,
        "stable-pending",
        "StableTarget",
        "src/untouched.rs",
    );
    insert_named_pending(
        &connection,
        untouched_version,
        "stable-carried-pending",
        "StableTarget",
        "src/untouched.rs",
    );
    insert_named_pending(
        &connection,
        cold_version,
        "cold-pending",
        "ColdTarget",
        "src/cold.rs",
    );

    let first_entries = [
        manifest_entry(&connection, old_version),
        manifest_entry(&connection, untouched_version),
        manifest_entry(&connection, cold_version),
    ];
    let first = publish_manifest(&mut connection, None, first_entries, "request-first");
    let base_path = layout.bases_dir().join("base-a.db");
    let mut base = ResolutionBaseWriter::new(&base_path, &first.manifest_hash, 6).unwrap();
    base.push_source_version(old_version).unwrap();
    base.push_source_version(untouched_version).unwrap();
    base.push_source_version(cold_version).unwrap();
    base.push_identifier_resolution(ResolutionIdentifierRow {
        version_id: old_version,
        identifier_id: "old-use".to_string(),
        target_version_id: Some(old_version),
        target_symbol_id: Some("old-target".to_string()),
        tier: Some(4),
        confidence: Some(0.55),
        method: Some("base-old".to_string()),
        outcome: "resolved".to_string(),
        candidates: Some(1),
    })
    .unwrap();
    base.push_identifier_resolution(ResolutionIdentifierRow {
        version_id: untouched_version,
        identifier_id: "stable-use".to_string(),
        target_version_id: Some(untouched_version),
        target_symbol_id: Some("stable-target".to_string()),
        tier: Some(4),
        confidence: Some(0.55),
        method: Some("tier4_global".to_string()),
        outcome: "resolved".to_string(),
        candidates: None,
    })
    .unwrap();
    for index in 0..256 {
        base.push_identifier_resolution(ResolutionIdentifierRow {
            version_id: untouched_version,
            identifier_id: format!("stable-zpadding-{index:03}"),
            target_version_id: Some(untouched_version),
            target_symbol_id: Some("stable-target".to_string()),
            tier: Some(4),
            confidence: Some(0.55),
            method: Some(if index == 0 {
                "base-zpadding".to_string()
            } else {
                "tier4_global".to_string()
            }),
            outcome: "resolved".to_string(),
            candidates: None,
        })
        .unwrap();
    }
    base.push_identifier_resolution(ResolutionIdentifierRow {
        version_id: cold_version,
        identifier_id: "cold-use".to_string(),
        target_version_id: Some(cold_version),
        target_symbol_id: Some("cold-target".to_string()),
        tier: Some(4),
        confidence: Some(0.55),
        method: Some("tier4_global".to_string()),
        outcome: "resolved".to_string(),
        candidates: None,
    })
    .unwrap();
    for index in 0..512 {
        base.push_identifier_resolution(ResolutionIdentifierRow {
            version_id: cold_version,
            identifier_id: format!("cold-zpadding-{index:03}"),
            target_version_id: Some(cold_version),
            target_symbol_id: Some("cold-target".to_string()),
            tier: Some(4),
            confidence: Some(0.55),
            method: Some("tier4_global".to_string()),
            outcome: "resolved".to_string(),
            candidates: None,
        })
        .unwrap();
    }
    base.push_pending_resolution(ResolutionPendingRow {
        version_id: old_version,
        pending_relationship_id: "old-pending".to_string(),
        target_version_id: old_version,
        target_symbol_id: "old-target".to_string(),
        tier: 4,
        confidence: 0.55,
        method: "base-old".to_string(),
    })
    .unwrap();
    base.push_pending_resolution(ResolutionPendingRow {
        version_id: untouched_version,
        pending_relationship_id: "stable-carried-pending".to_string(),
        target_version_id: untouched_version,
        target_symbol_id: "stable-target".to_string(),
        tier: 4,
        confidence: 0.55,
        method: "tier4_global".to_string(),
    })
    .unwrap();
    base.push_pending_resolution(ResolutionPendingRow {
        version_id: untouched_version,
        pending_relationship_id: "stable-pending".to_string(),
        target_version_id: untouched_version,
        target_symbol_id: "stable-target".to_string(),
        tier: 4,
        confidence: 0.55,
        method: "tier4_global".to_string(),
    })
    .unwrap();
    base.push_pending_resolution(ResolutionPendingRow {
        version_id: cold_version,
        pending_relationship_id: "cold-pending".to_string(),
        target_version_id: cold_version,
        target_symbol_id: "cold-target".to_string(),
        tier: 4,
        confidence: 0.55,
        method: "tier4_global".to_string(),
    })
    .unwrap();
    let base_identity = base.finish_with_target_lookup(|_, _| Ok(true)).unwrap();
    connection
        .execute(
            "INSERT INTO resolution_bases
             (base_id,manifest_hash,resolver_output_epoch,state,relative_path,identifier_count,
              pending_count,file_bytes,file_sha256,request_id,created_at,updated_at)
             VALUES ('base-a',?1,6,'ready','bases/base-a.db',771,4,?2,?3,'request-base',?4,?4)",
            params![
                first.manifest_hash,
                i64::try_from(base_identity.file_bytes).unwrap(),
                base_identity.file_sha256,
                NOW
            ],
        )
        .unwrap();
    for version_id in [old_version, untouched_version, cold_version] {
        connection
            .execute(
                "INSERT INTO resolution_base_versions(base_id,version_id) VALUES ('base-a',?1)",
                [version_id],
            )
            .unwrap();
    }
    let prior_gap_json = format!(
        r#"{{"files":[{untouched_version}],"rows":[{{"kind":"replaced","local_id":"stable-use","table":"identifier","version_id":{untouched_version}}},{{"kind":"replaced","local_id":"stable-zpadding-000","table":"identifier","version_id":{untouched_version}}},{{"kind":"replaced","local_id":"stable-pending","table":"pending","version_id":{untouched_version}}},{{"kind":"replaced","local_id":"stable-carried-pending","table":"pending","version_id":{untouched_version}}}]}}"#
    );
    let valid_gap_json = prior_gap_json.clone();
    connection
        .execute(
            "INSERT INTO resolution_deltas
             (view_id,delta_generation,base_id,manifest_generation,manifest_hash,resolver_output_epoch,
              identifier_replacements,pending_replacements,pending_tombstones,exact_gap_rows,
              exact_gap_files,exact_gap_json,request_id,created_at)
             VALUES ('view-a',1,'base-a',?1,?2,6,2,2,0,4,1,?3,'request-base',?4)",
            params![
                i64::try_from(first.generation).unwrap(),
                first.manifest_hash,
                prior_gap_json,
                NOW
            ],
        )
        .unwrap();
    for identifier_id in ["stable-use", "stable-zpadding-000"] {
        connection
            .execute(
                "INSERT INTO resolution_identifier_deltas
                 (view_id,delta_generation,version_id,identifier_id,target_version_id,target_symbol_id,
                  tier,confidence,method,outcome,candidates)
                 VALUES ('view-a',1,?1,?2,?1,'stable-target',4,0.55,?3,'resolved',NULL)",
                params![untouched_version, identifier_id, "tier4_global"],
            )
            .unwrap();
    }
    connection
        .execute(
            "INSERT INTO resolution_pending_deltas
             (view_id,delta_generation,version_id,pending_relationship_id,operation,
              target_version_id,target_symbol_id,tier,confidence,method)
             VALUES ('view-a',1,?1,'stable-pending','replace',?1,'stable-target',4,0.8,
                     'prior-pending')",
            [untouched_version],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO resolution_pending_deltas
             (view_id,delta_generation,version_id,pending_relationship_id,operation,
              target_version_id,target_symbol_id,tier,confidence,method)
             VALUES ('view-a',1,?1,'stable-carried-pending','replace',?1,'stable-target',4,0.8,
                     'prior-carried-pending')",
            [untouched_version],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE views SET resolution_state='exact',resolution_base_id='base-a',
             resolution_delta_generation=1,resolution_exact_at=?1 WHERE view_id='view-a'",
            [i64::try_from(first.generation).unwrap()],
        )
        .unwrap();
    let second_entries = [
        manifest_entry(&connection, new_version),
        manifest_entry(&connection, untouched_version),
        manifest_entry(&connection, cold_version),
    ];
    let second = publish_manifest(
        &mut connection,
        Some(i64::try_from(first.generation).unwrap()),
        second_entries,
        "request-second",
    );
    drop(connection);

    let current_identity = StoreManifestIdentity {
        family_id: "family-a".to_string(),
        view_id: "view-a".to_string(),
        generation: i64::try_from(second.generation).unwrap(),
        manifest_hash: second.manifest_hash.clone(),
    };
    let forced_path = temp.path().join("forced-exact.db");
    let mut forced = StoreScratchResolutionSession::new(
        factory.clone(),
        current_identity.clone(),
        &forced_path,
        2,
        6,
    )
    .unwrap();
    run_resolution_session(&mut forced, true, true).unwrap();
    let mut touched_over_prior = ResolutionWriteBatch::default();
    touched_over_prior.record_identifier_outcome(
        SemanticIdentifierId {
            version: SemanticVersionId::Store(untouched_version),
            local_id: "stable-use".to_string(),
        },
        julie_extract_artifact::resolution_store::Outcome::Resolved,
        Some(SemanticSymbolId {
            version: SemanticVersionId::Store(untouched_version),
            local_id: "stable-target".to_string(),
        }),
        Some(4),
        Some(0.99),
        Some("touched-over-prior"),
        Some(1),
        6,
    );
    touched_over_prior.record_pending_resolution(
        SemanticPendingRelationshipId {
            version: SemanticVersionId::Store(untouched_version),
            local_id: "stable-pending".to_string(),
        },
        SemanticSymbolId {
            version: SemanticVersionId::Store(untouched_version),
            local_id: "stable-target".to_string(),
        },
        4,
        0.99,
        "touched-pending",
        6,
    );
    let mut forced_overrides = touched_over_prior.clone();
    forced_overrides.record_pending_resolution(
        SemanticPendingRelationshipId {
            version: SemanticVersionId::Store(untouched_version),
            local_id: "stable-carried-pending".to_string(),
        },
        SemanticSymbolId {
            version: SemanticVersionId::Store(untouched_version),
            local_id: "stable-target".to_string(),
        },
        4,
        0.8,
        "prior-carried-pending",
        6,
    );
    forced.flush(forced_overrides).unwrap();
    forced.finish_exact().unwrap();

    let base_reader = ResolutionBaseReader::open(&base_path).unwrap();
    let forced_reader = ResolutionBaseReader::open(&forced_path).unwrap();
    let oracle_path = temp.path().join("oracle-delta.db");
    let mut oracle_gaps = Vec::new();
    let oracle_result =
        stream_resolution_diff(&base_reader, &forced_reader, &oracle_path, 2, |gap| {
            oracle_gaps.push(gap);
            Ok(())
        })
        .unwrap();
    let oracle = ResolutionScratchReader::open(&oracle_path).unwrap();
    let oracle_identifiers = oracle.identifier_replacements().unwrap();
    let oracle_pending = oracle.pending_replacements().unwrap();
    let oracle_tombstones = oracle.pending_tombstones().unwrap();
    assert!(
        oracle_identifiers
            .iter()
            .any(|row| { row.version_id == new_version && row.identifier_id == "new-use" })
    );
    assert!(oracle_identifiers.iter().any(|row| {
        row.version_id == untouched_version
            && row.identifier_id == "stable-zpadding-000"
            && row.method.as_deref() == Some("tier4_global")
    }));
    assert!(oracle_identifiers.iter().any(|row| {
        row.version_id == untouched_version
            && row.identifier_id == "stable-use"
            && row.method.as_deref() == Some("touched-over-prior")
    }));
    assert!(oracle_pending.iter().any(|row| {
        row.version_id == untouched_version
            && row.pending_relationship_id == "stable-pending"
            && row.method == "touched-pending"
    }));
    assert!(oracle_pending.iter().any(|row| {
        row.version_id == untouched_version
            && row.pending_relationship_id == "stable-carried-pending"
            && row.method == "prior-carried-pending"
    }));
    assert!(oracle_tombstones.iter().any(|row| {
        row.version_id == old_version && row.pending_relationship_id == "old-pending"
    }));

    let direct_exact_path = temp.path().join("direct-exact.db");
    let direct_delta_path = temp.path().join("direct-delta.db");
    let mut direct =
        StoreScratchResolutionSession::new(factory, current_identity, &direct_exact_path, 2, 6)
            .unwrap();
    run_resolution_session(&mut direct, false, true).unwrap();
    direct.flush(touched_over_prior).unwrap();
    let mut direct_gaps = Vec::new();
    let (_, finalization_telemetry) = direct
        .finish_scoped_delta_observing(&direct_delta_path, |gap| {
            direct_gaps.push(gap);
            Ok(())
        })
        .unwrap();
    assert!(finalization_telemetry.current_identifier_queries <= 2);
    assert!(finalization_telemetry.current_identifier_rows <= 4);
    assert!(finalization_telemetry.current_pending_queries <= 2);
    assert!(finalization_telemetry.current_pending_rows <= 4);
    assert!(finalization_telemetry.prior_identifier_queries <= 2);
    assert!(finalization_telemetry.prior_identifier_rows <= 4);
    assert!(finalization_telemetry.prior_pending_queries <= 2);
    assert!(finalization_telemetry.prior_pending_rows <= 4);
    assert!(finalization_telemetry.base_identifier_queries <= 4);
    assert!(finalization_telemetry.base_identifier_rows <= 4);
    assert!(finalization_telemetry.base_pending_queries <= 4);
    assert!(finalization_telemetry.base_pending_rows <= 4);
    assert_eq!(finalization_telemetry.base_keyed_reader_opens, 1);
    assert_eq!(finalization_telemetry.base_identifier_target_queries, 1);
    assert_eq!(finalization_telemetry.base_pending_target_queries, 1);
    assert_eq!(finalization_telemetry.base_identifier_target_rows, 1);
    assert_eq!(finalization_telemetry.base_pending_target_rows, 1);
    assert_eq!(finalization_telemetry.target_validation_queries, 1);
    assert!(finalization_telemetry.target_validation_targets >= 2);
    assert!(finalization_telemetry.target_validation_targets <= 8);
    let direct_reader = ResolutionScratchReader::open(&direct_delta_path).unwrap();
    assert!(!direct_exact_path.exists());
    assert_eq!(
        direct_reader.identifier_replacements().unwrap(),
        oracle_identifiers
    );
    assert_eq!(
        direct_reader.pending_replacements().unwrap(),
        oracle_pending
    );
    assert_eq!(
        direct_reader.pending_tombstones().unwrap(),
        oracle_tombstones
    );
    assert_eq!(direct_gaps, oracle_gaps);
    assert_eq!(oracle_result.gaps as usize, direct_gaps.len());
    drop(base_reader);
    drop(forced_reader);

    let duplicate_gap_json = format!(
        r#"{{"files":[{untouched_version}],"rows":[{{"kind":"replaced","local_id":"stable-use","table":"identifier","version_id":{untouched_version}}},{{"kind":"replaced","local_id":"stable-use","table":"identifier","version_id":{untouched_version}}},{{"kind":"replaced","local_id":"stable-zpadding-000","table":"identifier","version_id":{untouched_version}}},{{"kind":"replaced","local_id":"stable-pending","table":"pending","version_id":{untouched_version}}}]}}"#
    );
    let corruption_connection = Connection::open(layout.store_db()).unwrap();
    corruption_connection
        .execute(
            "UPDATE resolution_deltas
             SET exact_gap_rows=4,exact_gap_files=1,exact_gap_json=?1
             WHERE view_id='view-a' AND delta_generation=1",
            [&duplicate_gap_json],
        )
        .unwrap();
    let duplicate_exact_path = temp.path().join("duplicate-gap-exact.db");
    let duplicate_delta_path = temp.path().join("duplicate-gap-delta.db");
    let mut duplicate = StoreScratchResolutionSession::new(
        StoreConnectionFactory::new(layout.clone(), "family-a", "2.30.0"),
        StoreManifestIdentity {
            family_id: "family-a".to_string(),
            view_id: "view-a".to_string(),
            generation: i64::try_from(second.generation).unwrap(),
            manifest_hash: second.manifest_hash.clone(),
        },
        &duplicate_exact_path,
        2,
        6,
    )
    .unwrap();
    run_resolution_session(&mut duplicate, false, true).unwrap();
    assert!(matches!(
        duplicate
            .finish_scoped_delta(&duplicate_delta_path, |_| Ok(()))
            .unwrap_err(),
        StoreResolutionError::Artifact(
            julie_extract_artifact::store::ResolutionValidationError::InvalidMetadata {
                key,
                ..
            }
        ) if key == "exact_gap_json"
    ));
    corruption_connection
        .execute(
            "UPDATE resolution_deltas
             SET exact_gap_rows=99,exact_gap_files=1,exact_gap_json=?1
             WHERE view_id='view-a' AND delta_generation=1",
            [&valid_gap_json],
        )
        .unwrap();
    let count_exact_path = temp.path().join("count-gap-exact.db");
    let count_delta_path = temp.path().join("count-gap-delta.db");
    let mut count_mismatch = StoreScratchResolutionSession::new(
        StoreConnectionFactory::new(layout.clone(), "family-a", "2.30.0"),
        StoreManifestIdentity {
            family_id: "family-a".to_string(),
            view_id: "view-a".to_string(),
            generation: i64::try_from(second.generation).unwrap(),
            manifest_hash: second.manifest_hash.clone(),
        },
        &count_exact_path,
        2,
        6,
    )
    .unwrap();
    run_resolution_session(&mut count_mismatch, false, true).unwrap();
    assert!(matches!(
        count_mismatch
            .finish_scoped_delta(&count_delta_path, |_| Ok(()))
            .unwrap_err(),
        StoreResolutionError::Artifact(
            julie_extract_artifact::store::ResolutionValidationError::InvalidMetadata {
                key,
                ..
            }
        ) if key == "exact_gap_rows"
    ));
    corruption_connection
        .execute(
            "UPDATE resolution_deltas
             SET exact_gap_rows=4,exact_gap_files=1,exact_gap_json=?1
             WHERE view_id='view-a' AND delta_generation=1",
            [&valid_gap_json],
        )
        .unwrap();
    drop(corruption_connection);

    let set_base_target = |target_version_id: i64, target_symbol_id: &str, method: &str| {
        let base_connection = Connection::open(&base_path).unwrap();
        base_connection
            .execute(
                "UPDATE identifier_resolutions
                 SET target_version_id=?1,target_symbol_id=?2,method=?3
                 WHERE version_id=?4 AND identifier_id='stable-zpadding-001'",
                params![
                    target_version_id,
                    target_symbol_id,
                    method,
                    untouched_version
                ],
            )
            .unwrap();
        base_connection
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
            .unwrap();
        drop(base_connection);
        let reader = ResolutionBaseReader::open(&base_path).unwrap();
        let identity = reader.file_identity();
        let catalog = Connection::open(layout.store_db()).unwrap();
        catalog
            .execute(
                "UPDATE resolution_bases
                 SET file_bytes=?1,file_sha256=?2 WHERE base_id='base-a'",
                params![
                    i64::try_from(identity.file_bytes).unwrap(),
                    identity.file_sha256
                ],
            )
            .unwrap();
    };
    set_base_target(old_version, "old-target", "stale-implicit-base");
    let implicit_exact_path = temp.path().join("stale-implicit-exact.db");
    let implicit_delta_path = temp.path().join("stale-implicit-delta.db");
    let mut implicit = StoreScratchResolutionSession::new(
        StoreConnectionFactory::new(layout.clone(), "family-a", "2.30.0"),
        StoreManifestIdentity {
            family_id: "family-a".to_string(),
            view_id: "view-a".to_string(),
            generation: i64::try_from(second.generation).unwrap(),
            manifest_hash: second.manifest_hash.clone(),
        },
        &implicit_exact_path,
        2,
        6,
    )
    .unwrap();
    run_resolution_session(&mut implicit, false, true).unwrap();
    assert!(matches!(
        implicit
            .finish_scoped_delta(&implicit_delta_path, |_| Ok(()))
            .unwrap_err(),
        StoreResolutionError::Artifact(
            julie_extract_artifact::store::ResolutionValidationError::TargetMissing {
                version_id,
                symbol_id
            }
        ) if version_id == old_version && symbol_id == "old-target"
    ));
    set_base_target(untouched_version, "stable-target", "tier4_global");

    let stale_connection = Connection::open(layout.store_db()).unwrap();
    stale_connection
        .execute(
            "UPDATE resolution_pending_deltas
             SET target_version_id=?1,target_symbol_id='old-target',method='stale-carried-pending'
             WHERE view_id='view-a' AND delta_generation=1
               AND version_id=?2 AND pending_relationship_id='stable-carried-pending'",
            params![old_version, untouched_version],
        )
        .unwrap();
    let stale_pending_exact_path = temp.path().join("stale-pending-exact.db");
    let stale_pending_delta_path = temp.path().join("stale-pending-delta.db");
    let mut stale_pending = StoreScratchResolutionSession::new(
        StoreConnectionFactory::new(layout.clone(), "family-a", "2.30.0"),
        StoreManifestIdentity {
            family_id: "family-a".to_string(),
            view_id: "view-a".to_string(),
            generation: i64::try_from(second.generation).unwrap(),
            manifest_hash: second.manifest_hash.clone(),
        },
        &stale_pending_exact_path,
        2,
        6,
    )
    .unwrap();
    run_resolution_session(&mut stale_pending, false, true).unwrap();
    assert!(matches!(
        stale_pending
            .finish_scoped_delta(&stale_pending_delta_path, |_| Ok(()))
            .unwrap_err(),
        StoreResolutionError::Artifact(
            julie_extract_artifact::store::ResolutionValidationError::TargetMissing {
                version_id,
                symbol_id
            }
        ) if version_id == old_version && symbol_id == "old-target"
    ));
    stale_connection
        .execute(
            "UPDATE resolution_pending_deltas
             SET target_version_id=?1,target_symbol_id='stable-target',method='prior-carried-pending'
             WHERE view_id='view-a' AND delta_generation=1
               AND version_id=?2 AND pending_relationship_id='stable-carried-pending'",
            params![untouched_version, untouched_version],
        )
        .unwrap();
    stale_connection
        .execute(
            "UPDATE resolution_identifier_deltas
             SET target_version_id=?1,target_symbol_id='old-target',method='stale-carried'
             WHERE view_id='view-a' AND delta_generation=1
               AND version_id=?2 AND identifier_id='stable-use'",
            params![old_version, untouched_version],
        )
        .unwrap();
    let stale_exact_path = temp.path().join("stale-carried-exact.db");
    let stale_delta_path = temp.path().join("stale-carried-delta.db");
    let mut stale = StoreScratchResolutionSession::new(
        StoreConnectionFactory::new(layout.clone(), "family-a", "2.30.0"),
        StoreManifestIdentity {
            family_id: "family-a".to_string(),
            view_id: "view-a".to_string(),
            generation: i64::try_from(second.generation).unwrap(),
            manifest_hash: second.manifest_hash.clone(),
        },
        &stale_exact_path,
        2,
        6,
    )
    .unwrap();
    run_resolution_session(&mut stale, false, true).unwrap();
    assert!(matches!(
        stale
            .finish_scoped_delta(&stale_delta_path, |_| Ok(()))
            .unwrap_err(),
        StoreResolutionError::Artifact(
            julie_extract_artifact::store::ResolutionValidationError::TargetMissing {
                version_id,
                symbol_id
            }
        ) if version_id == old_version && symbol_id == "old-target"
    ));
    stale_connection
        .execute(
            "UPDATE resolution_identifier_deltas
             SET target_version_id=?1,target_symbol_id='stable-target',method='tier4_global'
             WHERE view_id='view-a' AND delta_generation=1
               AND version_id=?2 AND identifier_id='stable-use'",
            params![untouched_version, untouched_version],
        )
        .unwrap();

    let assert_scratch_clean = |path: &Path| {
        for suffix in ["", ".work", ".work-wal", ".work-shm", "-wal", "-shm"] {
            let candidate = if suffix.is_empty() {
                path.to_path_buf()
            } else {
                Path::new(&format!("{}{}", path.display(), suffix)).to_path_buf()
            };
            assert!(
                !candidate.exists(),
                "scratch artifact remains: {}",
                candidate.display()
            );
        }
    };

    let totality_path = temp.path().join("direct-totality-exact.db");
    let totality_delta_path = temp.path().join("direct-totality-delta.db");
    let mut totality = StoreScratchResolutionSession::new(
        StoreConnectionFactory::new(layout.clone(), "family-a", "2.30.0"),
        StoreManifestIdentity {
            family_id: "family-a".to_string(),
            view_id: "view-a".to_string(),
            generation: i64::try_from(second.generation).unwrap(),
            manifest_hash: second.manifest_hash.clone(),
        },
        &totality_path,
        2,
        6,
    )
    .unwrap();
    run_resolution_session(&mut totality, false, true).unwrap();
    totality
        .remove_identifier_resolution_without_touch_for_test(new_version, "new-use")
        .unwrap();
    let error = totality
        .finish_scoped_delta(&totality_delta_path, |_| Ok(()))
        .unwrap_err();
    assert!(matches!(
        error,
        StoreResolutionError::Artifact(
            julie_extract_artifact::store::ResolutionValidationError::IdentifierTotalityViolation {
                version_id,
                identifier_id
            }
        ) if version_id == new_version && identifier_id == "new-use"
    ));
    assert_scratch_clean(&totality_path);
    assert_scratch_clean(&totality_delta_path);

    let target_path = temp.path().join("direct-target-exact.db");
    let target_delta_path = temp.path().join("direct-target-delta.db");
    let mut target = StoreScratchResolutionSession::new(
        StoreConnectionFactory::new(layout, "family-a", "2.30.0"),
        StoreManifestIdentity {
            family_id: "family-a".to_string(),
            view_id: "view-a".to_string(),
            generation: i64::try_from(second.generation).unwrap(),
            manifest_hash: second.manifest_hash,
        },
        &target_path,
        2,
        6,
    )
    .unwrap();
    run_resolution_session(&mut target, false, true).unwrap();
    let mut stale_target = ResolutionWriteBatch::default();
    stale_target.record_identifier_outcome(
        SemanticIdentifierId {
            version: SemanticVersionId::Store(new_version),
            local_id: "new-use".to_string(),
        },
        julie_extract_artifact::resolution_store::Outcome::Resolved,
        Some(SemanticSymbolId {
            version: SemanticVersionId::Store(new_version),
            local_id: "stale-target".to_string(),
        }),
        Some(4),
        Some(1.0),
        Some("stale-target"),
        Some(1),
        6,
    );
    target.flush(stale_target).unwrap();
    let error = target
        .finish_scoped_delta(&target_delta_path, |_| Ok(()))
        .unwrap_err();
    assert!(matches!(
        error,
        StoreResolutionError::Artifact(
            julie_extract_artifact::store::ResolutionValidationError::TargetMissing {
                version_id,
                symbol_id
            }
        ) if version_id == new_version && symbol_id == "stale-target"
    ));
    assert_scratch_clean(&target_path);
    assert_scratch_clean(&target_delta_path);
}

#[test]
fn pinned_fixture_store_session_matches_legacy_semantics() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("repo");
    copy_fixture(&fixture_root(), &root);
    std::fs::create_dir_all(root.join("failed-preserved")).unwrap();
    std::fs::write(root.join("failed-preserved/broken.rs"), [0xff, 0xfe, 0xfd]).unwrap();
    let legacy_db = temp.path().join("legacy.db");
    let legacy = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "scan",
            "--root",
            root.to_str().unwrap(),
            "--db",
            legacy_db.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(
        legacy.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&legacy.stderr)
    );

    let store_root = temp.path().join("store");
    let store = Command::new(env!("CARGO_BIN_EXE_julie-extract"))
        .args([
            "store",
            "import",
            "--store",
            store_root.to_str().unwrap(),
            "--family",
            "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11",
            "--root",
            root.to_str().unwrap(),
            "--view",
            "view-parity",
            "--level",
            "full",
            "--request-id",
            "request-parity",
            "--idempotency-key",
            "idem-parity",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(
        store.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&store.stderr)
    );
    let layout = StoreLayout::open(&store_root).unwrap();
    let connection = Connection::open(layout.store_db()).unwrap();
    let (generation, manifest_hash): (i64, String) = connection
        .query_row(
            "SELECT v.current_generation,m.manifest_hash FROM views AS v
         JOIN manifests AS m ON m.view_id=v.view_id AND m.generation=v.current_generation
         WHERE v.view_id='view-parity'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    drop(connection);
    let factory = StoreConnectionFactory::new(
        layout.clone(),
        "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11",
        env!("CARGO_PKG_VERSION"),
    );
    let exact_path = temp.path().join("store-exact.db");
    let mut session = StoreScratchResolutionSession::new(
        factory,
        StoreManifestIdentity {
            family_id: "9f8c2c9a-3b92-4f38-9b0d-0e2b8c7a4d11".to_string(),
            view_id: "view-parity".to_string(),
            generation,
            manifest_hash,
        },
        &exact_path,
        3,
        6,
    )
    .unwrap();
    run_resolution_session(&mut session, true, true).unwrap();
    session.finish_exact().unwrap();

    assert_eq!(
        store_semantics(layout.store_db(), &exact_path),
        legacy_semantics(&legacy_db)
    );
}

fn fixture_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/store-resolution/legacy-v3")
}

fn copy_fixture(source: &Path, target: &Path) {
    std::fs::create_dir_all(target).unwrap();
    for entry in std::fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        if entry.file_name() == "expected.semantic.json" {
            continue;
        }
        let destination = target.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_fixture(&entry.path(), &destination);
        } else {
            std::fs::copy(entry.path(), destination).unwrap();
        }
    }
}

fn legacy_semantics(path: &Path) -> Value {
    let connection = Connection::open(path).unwrap();
    let identifiers = connection
        .prepare(
            "SELECT f.path,i.identifier_id,target.path,ir.target_symbol_id,ir.tier,ir.confidence,
                ir.method,ir.outcome,ir.candidates
         FROM identifier_resolutions AS ir
         JOIN identifiers AS i ON i.identifier_id=ir.identifier_id
         JOIN files AS f ON f.file_id=i.file_id
         LEFT JOIN symbols AS target ON target.symbol_id=ir.target_symbol_id
         ORDER BY f.path,i.identifier_id",
        )
        .unwrap()
        .query_map([], |row| {
            Ok(json!([
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<i64>>(4)?,
                row.get::<_, Option<f64>>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, Option<i64>>(8)?,
            ]))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let pending = connection
        .prepare(
            "SELECT f.path,pr.pending_relationship_id,target.path,res.target_symbol_id,
                res.tier,res.confidence,res.method
         FROM pending_resolutions AS res
         JOIN pending_relationships AS pr ON pr.pending_relationship_id=res.pending_relationship_id
         JOIN files AS f ON f.file_id=pr.file_id
         JOIN symbols AS target ON target.symbol_id=res.target_symbol_id
         ORDER BY f.path,pr.pending_relationship_id",
        )
        .unwrap()
        .query_map([], |row| {
            Ok(json!([
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, f64>(5)?,
                row.get::<_, String>(6)?,
            ]))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    json!({"identifiers": identifiers, "pending": pending})
}

fn store_semantics(store: impl AsRef<Path>, exact: &Path) -> Value {
    let store = Connection::open(store).unwrap();
    let base = Connection::open(exact).unwrap();
    let path_for = |version_id: i64| -> String {
        store
            .query_row(
                "SELECT path FROM file_versions WHERE version_id=?1",
                [version_id],
                |row| row.get(0),
            )
            .unwrap()
    };
    let identifiers = base.prepare(
        "SELECT version_id,identifier_id,target_version_id,target_symbol_id,tier,confidence,method,outcome,candidates
         FROM identifier_resolutions ORDER BY version_id,identifier_id",
    ).unwrap().query_map([], |row| Ok((
        row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, Option<i64>>(2)?,
        row.get::<_, Option<String>>(3)?, row.get::<_, Option<i64>>(4)?,
        row.get::<_, Option<f64>>(5)?, row.get::<_, Option<String>>(6)?,
        row.get::<_, String>(7)?, row.get::<_, Option<i64>>(8)?,
    ))).unwrap().collect::<Result<Vec<_>, _>>().unwrap().into_iter().map(|row| json!([
        path_for(row.0), row.1, row.2.map(&path_for), row.3, row.4, row.5, row.6, row.7, row.8,
    ])).collect::<Vec<_>>();
    let pending = base.prepare(
        "SELECT version_id,pending_relationship_id,target_version_id,target_symbol_id,tier,confidence,method
         FROM pending_resolutions ORDER BY version_id,pending_relationship_id",
    ).unwrap().query_map([], |row| Ok((
        row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?,
        row.get::<_, String>(3)?, row.get::<_, i64>(4)?, row.get::<_, f64>(5)?, row.get::<_, String>(6)?,
    ))).unwrap().collect::<Result<Vec<_>, _>>().unwrap().into_iter().map(|row| json!([
        path_for(row.0), row.1, path_for(row.2), row.3, row.4, row.5, row.6,
    ])).collect::<Vec<_>>();
    json!({"identifiers": identifiers, "pending": pending})
}

fn insert_resolution_fixture(connection: &mut Connection, version: i64, count: usize) {
    let transaction = connection.transaction().unwrap();
    transaction.execute(
        "INSERT INTO symbols(version_id,symbol_id,path,language,name,kind,
         start_line,start_column,end_line,end_column,start_byte,end_byte,is_test,test_container,test_lifecycle)
         VALUES (?1,'target','src/lib.rs','rust','target','function',1,1,1,10,0,10,0,0,0)",
        [version],
    ).unwrap();
    transaction.execute(
        "INSERT INTO symbols(version_id,symbol_id,path,language,name,kind,
         start_line,start_column,end_line,end_column,start_byte,end_byte,is_test,test_container,test_lifecycle)
         VALUES (?1,'caller','src/lib.rs','rust','caller','function',2,1,200,1,11,10000,0,0,0)",
        [version],
    ).unwrap();
    for index in 0..count {
        let site = format!("site-{index:04}");
        let identifier = format!("identifier-{index:04}");
        let pending = format!("pending-{index:04}");
        let start = 100 + i64::try_from(index).unwrap() * 10;
        transaction.execute(
            "INSERT INTO reference_sites(version_id,reference_site_id,path,language,containing_symbol_id,
             start_line,start_column,end_line,end_column,start_byte,end_byte,is_exact,provenance,level)
             VALUES (?1,?2,'src/lib.rs','rust','caller',?3,1,?3,7,?4,?5,1,'target_token',2)",
            params![version, site, index as i64 + 3, start, start + 6],
        ).unwrap();
        transaction.execute(
            "INSERT INTO identifiers(version_id,identifier_id,reference_site_id,path,language,name,kind,
             containing_symbol_id,start_line,start_column,end_line,end_column,start_byte,end_byte,confidence)
             VALUES (?1,?2,?3,'src/lib.rs','rust','target','call','caller',?4,1,?4,7,?5,?6,1.0)",
            params![version, identifier, site, index as i64 + 3, start, start + 6],
        ).unwrap();
        transaction.execute(
            "INSERT INTO pending_relationships(version_id,pending_relationship_id,reference_site_id,
             from_symbol_id,caller_scope_symbol_id,path,kind,target_display_name,target_terminal_name,
             target_namespace_json,start_line,start_column,end_line,end_column,start_byte,end_byte,confidence)
             VALUES (?1,?2,?3,'caller','caller','src/lib.rs','calls','target','target','[]',?4,1,?4,7,?5,?6,1.0)",
            params![version, pending, site, index as i64 + 3, start, start + 6],
        ).unwrap();
    }
    transaction.commit().unwrap();
}

#[test]
fn store_session_rss_child() {
    let Ok(count) = std::env::var("JULIE_STORE_RSS_ROWS") else {
        return;
    };
    let count: usize = count.parse().unwrap();
    let temp = TempDir::new().unwrap();
    let layout = StoreLayout::create(temp.path().join("family"), "family-a", "2.30.0").unwrap();
    let factory = StoreConnectionFactory::new(layout, "family-a", "2.30.0");
    let mut connection = factory.open_writer().unwrap();
    ManifestStore::new(&mut connection)
        .ensure_view("view-a", "/repo")
        .unwrap();
    let version = insert_version(&connection, "src/lib.rs", "rust", true);
    connection.execute("INSERT INTO manifests(view_id,generation,manifest_hash,request_id,created_at) VALUES ('view-a',1,'manifest-rss','request-rss',?1)", [NOW]).unwrap();
    connection.execute("INSERT INTO manifest_entries(view_id,generation,path,language,version_id,status,observed_content_hash,indexed_at) VALUES ('view-a',1,'src/lib.rs','rust',?1,'indexed','hash',?2)", params![version, NOW]).unwrap();
    insert_resolution_fixture(&mut connection, version, count);
    drop(connection);
    let mut session = StoreScratchResolutionSession::new(
        factory,
        StoreManifestIdentity {
            family_id: "family-a".to_string(),
            view_id: "view-a".to_string(),
            generation: 1,
            manifest_hash: "manifest-rss".to_string(),
        },
        temp.path().join("exact.db"),
        32,
        6,
    )
    .unwrap();
    run_resolution_session(&mut session, true, true).unwrap();
    assert!(session.max_store_read_page() <= 32);
    let identity = session.finish_exact().unwrap();
    assert_eq!(identity.counts.identifiers as usize, count);
    assert_eq!(identity.counts.pending as usize, count);
}

#[test]
#[cfg(target_os = "macos")]
fn synthetic_store_session_rss_growth_is_bounded_by_windows() {
    let small = measured_child_rss(1_000);
    let large = measured_child_rss(8_000);
    assert!(
        large <= small + 24 * 1024 * 1024,
        "small={small} large={large}"
    );
}

#[cfg(target_os = "macos")]
fn measured_child_rss(rows: usize) -> u64 {
    let output = Command::new("/usr/bin/time")
        .args([
            "-l",
            std::env::current_exe().unwrap().to_str().unwrap(),
            "store_session_rss_child",
            "--exact",
            "--nocapture",
        ])
        .env("JULIE_STORE_RSS_ROWS", rows.to_string())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stderr)
        .lines()
        .find(|line| line.contains("maximum resident set size"))
        .and_then(|line| line.split_whitespace().next())
        .and_then(|value| value.parse().ok())
        .expect("time -l reports maximum resident set size")
}

fn insert_version(connection: &Connection, path: &str, language: &str, complete_l2: bool) -> i64 {
    connection.execute(
        "INSERT INTO file_versions(path,content_hash,extraction_epoch,language,content_bytes,complete_l1,complete_l2)
         VALUES (?1,?2,1,?3,1,1,?4)",
        params![path, format!("hash-{path}"), language, complete_l2.then_some(1)],
    ).unwrap();
    connection.last_insert_rowid()
}

fn manifest_entry(connection: &Connection, version_id: i64) -> ManifestEntry {
    connection
        .query_row(
            "SELECT path,language,content_hash FROM file_versions WHERE version_id=?1",
            [version_id],
            |row| {
                Ok(ManifestEntry::indexed(
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    version_id,
                    row.get::<_, String>(2)?,
                    NOW,
                ))
            },
        )
        .unwrap()
}

fn publish_manifest(
    connection: &mut Connection,
    expected_generation: Option<i64>,
    entries: impl IntoIterator<Item = ManifestEntry>,
    request_id: &str,
) -> julie_extract_artifact::store::ManifestPublishResult {
    ManifestStore::new(connection)
        .publish(
            "view-a",
            expected_generation.map(|value| value as u64),
            entries,
            request_id,
        )
        .unwrap()
}

fn insert_named_symbol(
    connection: &Connection,
    version_id: i64,
    symbol_id: &str,
    name: &str,
    path: &str,
) {
    connection.execute(
        "INSERT INTO symbols(version_id,symbol_id,path,language,name,kind,
         start_line,start_column,end_line,end_column,start_byte,end_byte,is_test,test_container,test_lifecycle)
         VALUES (?1,?2,?3,'rust',?4,'function',1,1,1,5,0,4,0,0,0)",
        params![version_id, symbol_id, path, name],
    ).unwrap();
}

fn insert_named_identifier(
    connection: &Connection,
    version_id: i64,
    identifier_id: &str,
    name: &str,
    path: &str,
) {
    let site_id = format!("site-{identifier_id}");
    connection
        .execute(
            "INSERT INTO reference_sites(version_id,reference_site_id,path,language,
         start_line,start_column,end_line,end_column,start_byte,end_byte,is_exact,provenance,level)
         VALUES (?1,?2,?3,'rust',2,1,2,5,5,9,1,'target_token',2)",
            params![version_id, site_id, path],
        )
        .unwrap();
    connection.execute(
        "INSERT INTO identifiers(version_id,identifier_id,reference_site_id,path,language,name,kind,
         start_line,start_column,end_line,end_column,start_byte,end_byte,confidence)
         VALUES (?1,?2,?3,?4,'rust',?5,'call',2,1,2,5,5,9,1.0)",
        params![version_id, identifier_id, site_id, path, name],
    ).unwrap();
}

fn insert_named_pending(
    connection: &Connection,
    version_id: i64,
    pending_relationship_id: &str,
    target_name: &str,
    path: &str,
) {
    let site_id = format!("site-pending-{pending_relationship_id}");
    connection
        .execute(
            "INSERT INTO reference_sites(version_id,reference_site_id,path,language,
         start_line,start_column,end_line,end_column,start_byte,end_byte,is_exact,provenance,level)
         VALUES (?1,?2,?3,'rust',2,1,2,5,5,9,1,'target_token',2)",
            params![version_id, site_id, path],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO pending_relationships
             (version_id,pending_relationship_id,reference_site_id,from_symbol_id,
              caller_scope_symbol_id,path,kind,target_display_name,target_terminal_name,
              target_namespace_json,start_line,start_column,end_line,end_column,start_byte,end_byte,
              confidence)
             VALUES (?1,?2,?3,'caller','caller',?4,'calls',?5,?5,'[]',2,1,2,5,5,9,1.0)",
            params![
                version_id,
                pending_relationship_id,
                site_id,
                path,
                target_name
            ],
        )
        .unwrap();
}
