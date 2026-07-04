#![cfg(feature = "test-perf")]
//! Feature-gated writer/export performance harness for `julie-extract-artifact`.
//!
//! Kept out of the default suite by the test-tier convention so the fast
//! default gate stays cheap. Run on demand with:
//!
//! ```text
//! cargo test -p julie-extract-artifact --features test-perf \
//!     --test writer_perf -- --nocapture
//! ```
//!
//! The harness drives `ArtifactWriter` and `export_jsonl` directly with a
//! synthetic corpus, so it isolates the writer/export path (where the
//! deferred P2 perf items live) without paying for extraction. It reports
//! wall-clock throughput for the scan and export phases plus
//! `EXPLAIN QUERY PLAN` evidence that the export-order indexes are used.
//!
//! Corpus sizing is overridable through env vars so the same gate can be run
//! at a larger volume for tighter evidence without editing source.

use std::io::sink;
use std::time::{Duration, Instant};

use julie_extract_artifact::jsonl::export_jsonl;
use julie_extract_artifact::metadata::ArtifactMetadata;
use julie_extract_artifact::model::{
    ArtifactComplexityMetric, ArtifactFile, ArtifactSourceRegion, ArtifactStructuralFact,
    ArtifactSymbol, FileStatus, RevisionInput, WriteMode, WriteOperation,
};
use julie_extract_artifact::writer::ArtifactWriter;
use rusqlite::{Connection, OpenFlags};

/// `mmap_size` applied to the read-only export connection. Mirrors the value
/// used by the CLI reader path so the export phase reflects real behavior.
const READER_MMAP_SIZE_BYTES: i64 = 1024 * 1024 * 1024;

#[test]
fn writer_scan_and_export_throughput() {
    let file_count = env_usize("JULIE_PERF_FILES", 1_500);
    let temp_dir = unique_temp_dir("writer-perf");
    std::fs::create_dir_all(&temp_dir).unwrap();
    let db_path = temp_dir.join("perf.sqlite");
    let counts = write_corpus(&db_path, file_count);

    let connection =
        Connection::open_with_flags(&db_path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    connection
        .pragma_update(None, "mmap_size", READER_MMAP_SIZE_BYTES)
        .unwrap();
    let export_started = Instant::now();
    let summary = export_jsonl(&connection, sink()).unwrap();
    let export_elapsed = export_started.elapsed();

    let exported_records = summary.total_records;

    println!(
        "writer_perf: files={file_count} symbols={} child_rows={} \
         (structural_facts={} source_regions={} complexity_metrics={})",
        counts.symbols,
        counts.child_rows,
        counts.structural_facts,
        counts.source_regions,
        counts.complexity_metrics,
    );
    println!(
        "writer_perf: write_scan {} ms | WAL sidecar after write = {} bytes | {} child_rows/sec",
        counts.write_elapsed.as_millis(),
        counts.wal_bytes,
        throughput(counts.child_rows, counts.write_elapsed),
    );
    println!(
        "writer_perf: export_jsonl (mmap_size={READER_MMAP_SIZE_BYTES}) {} ms for {exported_records} \
         records ({} records/sec)",
        export_elapsed.as_millis(),
        throughput(exported_records, export_elapsed),
    );

    explain_query_plan(
        &connection,
        "structural_facts",
        "path, start_byte, end_byte, pattern_id, capture_name, structural_fact_id",
    );
    explain_query_plan(
        &connection,
        "source_regions",
        "path, start_byte, end_byte, kind, source_region_id",
    );
    explain_query_plan(
        &connection,
        "complexity_metrics",
        "path, start_byte, end_byte, scope, symbol_id, complexity_metric_id",
    );

    // Soft floors so the gate fails loudly if throughput collapses, but it
    // never runs in the default suite and is intentionally generous to avoid
    // flapping on noisy CI hosts.
    assert!(
        counts.write_elapsed < Duration::from_secs(30),
        "write_scan regressed past perf floor: {:?}",
        counts.write_elapsed
    );
    assert!(
        export_elapsed < Duration::from_secs(15),
        "export_jsonl regressed past perf floor: {export_elapsed:?}"
    );
    assert!(exported_records > 0);

    std::fs::remove_dir_all(&temp_dir).unwrap();
}

#[test]
fn export_mmap_size_comparison() {
    // Isolates the reader `mmap_size` pragma (P3 finding) from writer changes:
    // the same quiescent artifact is exported once without mmap and once with
    // it, on fresh read-only connections, so the delta reflects mmap only.
    let file_count = env_usize("JULIE_PERF_FILES", 1_500);
    let temp_dir = unique_temp_dir("writer-perf-mmap");
    std::fs::create_dir_all(&temp_dir).unwrap();
    let db_path = temp_dir.join("perf.sqlite");
    let counts = write_corpus(&db_path, file_count);

    let without_mmap = export_elapsed(&db_path, None);
    let with_mmap = export_elapsed(&db_path, Some(READER_MMAP_SIZE_BYTES));

    println!(
        "writer_perf: export without mmap_size {} ms ({} records/sec)",
        without_mmap.as_millis(),
        throughput(counts.estimated_records, without_mmap),
    );
    println!(
        "writer_perf: export with    mmap_size={} {} ms ({} records/sec)",
        READER_MMAP_SIZE_BYTES,
        with_mmap.as_millis(),
        throughput(counts.estimated_records, with_mmap),
    );
    let delta = without_mmap.saturating_sub(with_mmap);
    println!("writer_perf: mmap_size export delta {delta:?} (negative means mmap was slower)");

    std::fs::remove_dir_all(&temp_dir).unwrap();
}

fn export_elapsed(db_path: &std::path::Path, mmap_size: Option<i64>) -> Duration {
    let connection =
        Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    if let Some(size) = mmap_size {
        connection.pragma_update(None, "mmap_size", size).unwrap();
    }
    let started = Instant::now();
    let _ = export_jsonl(&connection, sink()).unwrap();
    started.elapsed()
}

struct CorpusCounts {
    symbols: usize,
    child_rows: usize,
    structural_facts: usize,
    source_regions: usize,
    complexity_metrics: usize,
    estimated_records: usize,
    write_elapsed: Duration,
    wal_bytes: u64,
}

fn write_corpus(db_path: &std::path::Path, file_count: usize) -> CorpusCounts {
    let files = synthetic_corpus(file_count);
    let symbols = files.iter().map(|f| f.symbols.len()).sum::<usize>();
    let structural_facts = files
        .iter()
        .map(|f| f.structural_facts.len())
        .sum::<usize>();
    let source_regions = files.iter().map(|f| f.source_regions.len()).sum::<usize>();
    let complexity_metrics = files
        .iter()
        .map(|f| f.complexity_metrics.len())
        .sum::<usize>();
    let child_rows = structural_facts + source_regions + complexity_metrics;
    // One JSONL record per row plus the per-file/per-revision/per-artifact
    // framing records emitted by the exporter; close enough for a rate estimate.
    let estimated_records = child_rows + symbols + file_count + 4;

    let mut writer = ArtifactWriter::open_path(db_path, metadata()).unwrap();
    let write_started = Instant::now();
    let result = writer.write_scan(revision(), &files).unwrap();
    let write_elapsed = write_started.elapsed();
    let wal_bytes = std::fs::metadata(format!("{}-wal", db_path.display()))
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    drop(writer);
    assert_eq!(result.files_changed, file_count);

    CorpusCounts {
        symbols,
        child_rows,
        structural_facts,
        source_regions,
        complexity_metrics,
        estimated_records,
        write_elapsed,
        wal_bytes,
    }
}

fn explain_query_plan(connection: &Connection, table: &str, order_by: &str) {
    let sql = format!("SELECT * FROM {table} ORDER BY {order_by}");
    let mut stmt = connection
        .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
        .unwrap();
    let rows: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(3))
        .unwrap()
        .filter_map(Result::ok)
        .collect();
    println!("writer_perf: EXPLAIN QUERY PLAN {table} ORDER BY ->");
    for row in rows {
        println!("writer_perf:   {row}");
    }
}

fn throughput(items: usize, elapsed: Duration) -> u64 {
    let secs = elapsed.as_secs_f64().max(1e-9);
    (items as f64 / secs) as u64
}

fn synthetic_corpus(file_count: usize) -> Vec<ArtifactFile> {
    (0..file_count).map(file_with_rows).collect()
}

fn file_with_rows(index: usize) -> ArtifactFile {
    let file_id = format!("file-{index}");
    let path = format!("src/file_{index}.rs");
    let symbols: Vec<ArtifactSymbol> = (0..SYMBOLS_PER_FILE)
        .map(|symbol_index| ArtifactSymbol {
            symbol_id: format!("{file_id}-symbol-{symbol_index}"),
            name: format!("symbol_{index}_{symbol_index}"),
            kind: "function".to_string(),
            start_line: (symbol_index + 1) as i64,
            end_line: (symbol_index + 1) as i64,
            start_byte: (symbol_index * 16) as i64,
            end_byte: (symbol_index * 16 + 8) as i64,
            ..ArtifactSymbol::default()
        })
        .collect();

    // Dangling containing_symbol_ids force every requested id into the
    // unresolved path so the symbol-lookup temp-table load is exercised at
    // batch scale. Facts still insert with a NULL containing symbol.
    let structural_fact_count = env_usize("JULIE_PERF_FACTS_PER_FILE", STRUCTURAL_FACTS_PER_FILE);
    let structural_facts: Vec<ArtifactStructuralFact> = (0..structural_fact_count)
        .map(|fact_index| ArtifactStructuralFact {
            structural_fact_id: format!("{file_id}-fact-{fact_index}"),
            pattern_id: "rust.unsafe_block.v1".to_string(),
            capture_name: "unsafe_block".to_string(),
            node_kind: "unsafe_block".to_string(),
            containing_symbol_id: Some(format!("external-symbol-{index}-{fact_index}")),
            start_line: (fact_index + 1) as i64,
            start_column: 0,
            end_line: (fact_index + 2) as i64,
            end_column: 1,
            start_byte: (fact_index * 32) as i64,
            end_byte: (fact_index * 32 + 16) as i64,
            confidence: 1.0,
            metadata_json: Some(format!("{{\"ordinal\":{fact_index}}}")),
        })
        .collect();

    let source_region_count = env_usize("JULIE_PERF_REGIONS_PER_FILE", SOURCE_REGIONS_PER_FILE);
    let source_regions: Vec<ArtifactSourceRegion> = (0..source_region_count)
        .map(|region_index| ArtifactSourceRegion {
            source_region_id: format!("{file_id}-region-{region_index}"),
            kind: "comment".to_string(),
            containing_symbol_id: Some(format!("{file_id}-symbol-0")),
            start_line: (region_index + 1) as i64,
            start_column: 0,
            end_line: (region_index + 1) as i64,
            end_column: 8,
            start_byte: (region_index * 16) as i64,
            end_byte: (region_index * 16 + 8) as i64,
            metadata_json: None,
        })
        .collect();

    let complexity_metric_count =
        env_usize("JULIE_PERF_METRICS_PER_FILE", COMPLEXITY_METRICS_PER_FILE);
    let complexity_metrics: Vec<ArtifactComplexityMetric> = (0..complexity_metric_count)
        .map(|metric_index| ArtifactComplexityMetric {
            complexity_metric_id: format!("{file_id}-complexity-{metric_index}"),
            scope: "symbol".to_string(),
            symbol_id: Some(format!("{file_id}-symbol-0")),
            algorithm_id: "julie-ast-complexity-v1".to_string(),
            covered_lines: 4,
            covered_bytes: 64,
            decision_count: 1,
            loop_count: 1,
            max_nesting_depth: 2,
            parameter_count: Some(2),
            start_line: (metric_index + 1) as i64,
            start_column: 0,
            end_line: (metric_index + 4) as i64,
            end_column: 1,
            start_byte: (metric_index * 32) as i64,
            end_byte: (metric_index * 32 + 64) as i64,
            metadata_json: None,
        })
        .collect();

    ArtifactFile {
        file_id,
        path,
        language: "rust".to_string(),
        content_hash: format!("hash-{index}"),
        content_bytes: 128,
        line_count: Some(64),
        indexed_at: "2026-07-04T16:00:00Z".to_string(),
        status: FileStatus::Indexed,
        metadata_json: None,
        symbols,
        symbol_annotations: Vec::new(),
        identifiers: Vec::new(),
        relationships: Vec::new(),
        pending_relationships: Vec::new(),
        type_facts: Vec::new(),
        type_argument_usages: Vec::new(),
        type_arguments: Vec::new(),
        literals: Vec::new(),
        source_regions,
        structural_facts,
        complexity_metrics,
        parse_diagnostics: Vec::new(),
    }
}

const SYMBOLS_PER_FILE: usize = 4;
const STRUCTURAL_FACTS_PER_FILE: usize = 16;
const SOURCE_REGIONS_PER_FILE: usize = 8;
const COMPLEXITY_METRICS_PER_FILE: usize = 4;

fn metadata() -> ArtifactMetadata {
    ArtifactMetadata {
        artifact_id: "artifact-writer-perf-gate".to_string(),
        root_path: "/repo".to_string(),
        binary_version: "julie-extract 0.1.0".to_string(),
        hash_algorithm: "blake3".to_string(),
        parser_inventory_fingerprint: "sha256:parser".to_string(),
        capability_snapshot_fingerprint: "sha256:cap".to_string(),
        created_at: "2026-07-04T16:00:00Z".to_string(),
        updated_at: "2026-07-04T16:00:00Z".to_string(),
    }
}

fn revision() -> RevisionInput {
    RevisionInput {
        operation: WriteOperation::Scan,
        mode: Some(WriteMode::Incremental),
        started_at: "2026-07-04T16:00:00Z".to_string(),
        completed_at: "2026-07-04T16:00:01Z".to_string(),
        binary_version: "julie-extract 0.1.0".to_string(),
        input_root: Some("/repo".to_string()),
    }
}

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

fn unique_temp_dir(name: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("{name}-{}-{nanos}", std::process::id()))
}
