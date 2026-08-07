use rusqlite::Connection;

pub const SQLITE_SCHEMA_VERSION: i64 = 6;
pub const EXTRACT_CONTRACT_VERSION: i64 = 4;

pub fn create_schema(conn: &Connection) -> rusqlite::Result<()> {
    drop_superseded_reference_site_guard(conn)?;
    drop_retired_secondary_indexes(conn)?;
    conn.execute_batch(SCHEMA_TABLES_SQL)?;
    create_secondary_indexes(conn)
}

/// Retired 2026-08-03: a two-repo consumer audit plus `EXPLAIN QUERY PLAN`
/// showed no query in julie-extractors or Miller ever selects these three, and
/// together they were ~11% of a dotnet/runtime-scale artifact plus their share
/// of bulk-load index-build time. (Two further audit candidates were RETAINED:
/// `idx_identifiers_reference_site` and `idx_reference_sites_containing_symbol`
/// back FK cascade/SET NULL child searches that no query plan surfaces.) Dropping here (idempotent, no schema-cookie
/// churn when absent) lets an existing artifact shed them on its next open;
/// the freed pages are reused rather than returned until the artifact is
/// rebuilt or vacuumed.
fn drop_retired_secondary_indexes(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "DROP INDEX IF EXISTS idx_reference_sites_span;
         DROP INDEX IF EXISTS idx_identifiers_path;
         DROP INDEX IF EXISTS idx_identifiers_file_line_name;",
    )
}

/// The secondary indexes only. Split out of [`create_schema`] so the
/// fresh-artifact bulk load can build them once at the end of the write instead
/// of maintaining every one of them per inserted row. Idempotent.
pub fn create_secondary_indexes(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(SCHEMA_INDEXES_SQL)
}

/// Drops every index [`create_secondary_indexes`] creates, leaving the implicit
/// PRIMARY KEY and UNIQUE indexes SQLite owns. Read from `sqlite_master` rather
/// than a second hand-maintained list, so an index added to the DDL can never be
/// silently left behind during a bulk load.
///
/// Dropping ALL of them is only safe because the bulk load disables foreign-key
/// enforcement for the insert passes and validates the whole artifact once before
/// commit — otherwise the deferred-foreign-key parent-side searches these indexes
/// back would each degrade to a full table scan. See `begin_bulk_load`.
pub fn drop_secondary_indexes(conn: &Connection) -> rusqlite::Result<()> {
    let names = {
        let mut statement = conn.prepare(
            "SELECT name FROM sqlite_master
             WHERE type = 'index' AND sql IS NOT NULL
             ORDER BY name",
        )?;
        let names = statement.query_map([], |row| row.get::<_, String>(0))?;
        names.collect::<rusqlite::Result<Vec<_>>>()?
    };
    for name in names {
        conn.execute_batch(&format!("DROP INDEX IF EXISTS \"{name}\""))?;
    }
    Ok(())
}

/// The guard originally raised ABORT, so one payload disagreement between
/// extraction passes rolled back the entire import. `CREATE TRIGGER IF NOT
/// EXISTS` would leave that fatal version in place on artifacts written before
/// the demotion, so the superseded trigger is dropped once — the conditional
/// keeps every later open from churning the SQLite schema cookie.
fn drop_superseded_reference_site_guard(conn: &Connection) -> rusqlite::Result<()> {
    let superseded: bool = conn.query_row(
        "SELECT EXISTS (
           SELECT 1 FROM sqlite_master
           WHERE type = 'trigger'
             AND name = 'reference_sites_identity_guard'
             AND sql LIKE '%RAISE(ABORT%'
         )",
        [],
        |row| row.get(0),
    )?;
    if superseded {
        conn.execute_batch("DROP TRIGGER reference_sites_identity_guard")?;
    }
    Ok(())
}

const SCHEMA_TABLES_SQL: &str = r#"
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS artifact_metadata (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS parser_inventory (
  language TEXT NOT NULL,
  parser_package TEXT NOT NULL,
  parser_version TEXT,
  grammar_version TEXT,
  source TEXT,
  metadata_json TEXT,
  PRIMARY KEY (language, parser_package)
);

CREATE TABLE IF NOT EXISTS extraction_revisions (
  revision_id INTEGER PRIMARY KEY,
  parent_revision_id INTEGER,
  operation TEXT NOT NULL,
  mode TEXT,
  started_at TEXT NOT NULL,
  completed_at TEXT NOT NULL,
  binary_version TEXT NOT NULL,
  extract_contract_version INTEGER NOT NULL,
  sqlite_schema_version INTEGER NOT NULL,
  input_root TEXT,
  counts_json TEXT NOT NULL,
  FOREIGN KEY (parent_revision_id) REFERENCES extraction_revisions(revision_id)
);

CREATE TABLE IF NOT EXISTS revision_file_changes (
  revision_id INTEGER NOT NULL,
  file_id TEXT NOT NULL,
  path TEXT NOT NULL,
  change_kind TEXT NOT NULL,
  PRIMARY KEY (revision_id, file_id),
  FOREIGN KEY (revision_id) REFERENCES extraction_revisions(revision_id)
);

CREATE TABLE IF NOT EXISTS files (
  file_id TEXT PRIMARY KEY,
  path TEXT NOT NULL UNIQUE,
  language TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  content_bytes INTEGER NOT NULL,
  line_count INTEGER,
  indexed_at TEXT NOT NULL,
  last_revision_id INTEGER NOT NULL,
  status TEXT NOT NULL,
  metadata_json TEXT,
  FOREIGN KEY (last_revision_id) REFERENCES extraction_revisions(revision_id)
);

CREATE TABLE IF NOT EXISTS symbols (
  symbol_id TEXT PRIMARY KEY,
  file_id TEXT NOT NULL,
  path TEXT NOT NULL,
  language TEXT NOT NULL,
  name TEXT NOT NULL,
  kind TEXT NOT NULL,
  signature TEXT,
  doc_comment TEXT,
  visibility TEXT,
  parent_symbol_id TEXT,
  start_line INTEGER NOT NULL,
  start_column INTEGER NOT NULL,
  end_line INTEGER NOT NULL,
  end_column INTEGER NOT NULL,
  start_byte INTEGER NOT NULL,
  end_byte INTEGER NOT NULL,
  body_start_line INTEGER,
  body_start_column INTEGER,
  body_end_line INTEGER,
  body_end_column INTEGER,
  body_start_byte INTEGER,
  body_end_byte INTEGER,
  body_hash TEXT,
  semantic_group TEXT,
  confidence REAL,
  content_type TEXT,
  is_test INTEGER NOT NULL DEFAULT 0,
  test_container INTEGER NOT NULL DEFAULT 0,
  test_lifecycle INTEGER NOT NULL DEFAULT 0,
  metadata_json TEXT,
  FOREIGN KEY (file_id) REFERENCES files(file_id) ON DELETE CASCADE,
  FOREIGN KEY (parent_symbol_id) REFERENCES symbols(symbol_id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS symbol_annotations (
  annotation_id TEXT PRIMARY KEY,
  symbol_id TEXT NOT NULL,
  annotation TEXT NOT NULL,
  annotation_key TEXT NOT NULL,
  raw_text TEXT,
  carrier TEXT,
  metadata_json TEXT,
  FOREIGN KEY (symbol_id) REFERENCES symbols(symbol_id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS reference_sites (
  reference_site_id TEXT PRIMARY KEY,
  file_id TEXT NOT NULL,
  path TEXT NOT NULL,
  language TEXT NOT NULL,
  containing_symbol_id TEXT,
  start_line INTEGER,
  start_column INTEGER,
  end_line INTEGER,
  end_column INTEGER,
  start_byte INTEGER,
  end_byte INTEGER,
  is_exact INTEGER NOT NULL,
  provenance TEXT NOT NULL,
  FOREIGN KEY (file_id) REFERENCES files(file_id) ON DELETE CASCADE,
  FOREIGN KEY (containing_symbol_id) REFERENCES symbols(symbol_id) ON DELETE SET NULL,
  CHECK (length(reference_site_id) > 0),
  CHECK (is_exact IN (0, 1)),
  CHECK (
    (is_exact = 1
      AND start_line IS NOT NULL
      AND start_column IS NOT NULL
      AND end_line IS NOT NULL
      AND end_column IS NOT NULL
      AND start_byte IS NOT NULL
      AND end_byte IS NOT NULL)
    OR
    (is_exact = 0
      AND start_line IS NULL
      AND start_column IS NULL
      AND end_line IS NULL
      AND end_column IS NULL
      AND start_byte IS NULL
      AND end_byte IS NULL)
  ),
  CHECK (
    (is_exact = 1 AND provenance = 'target_token')
    OR (is_exact = 0 AND provenance = 'spanless')
  )
);

CREATE TABLE IF NOT EXISTS identifiers (
  identifier_id TEXT PRIMARY KEY,
  reference_site_id TEXT NOT NULL,
  file_id TEXT NOT NULL,
  path TEXT NOT NULL,
  language TEXT NOT NULL,
  name TEXT NOT NULL,
  kind TEXT NOT NULL,
  containing_symbol_id TEXT,
  start_line INTEGER NOT NULL,
  start_column INTEGER NOT NULL,
  end_line INTEGER NOT NULL,
  end_column INTEGER NOT NULL,
  start_byte INTEGER NOT NULL,
  end_byte INTEGER NOT NULL,
  confidence REAL NOT NULL,
  code_context TEXT,
  metadata_json TEXT,
  FOREIGN KEY (reference_site_id) REFERENCES reference_sites(reference_site_id) ON DELETE CASCADE,
  FOREIGN KEY (file_id) REFERENCES files(file_id) ON DELETE CASCADE,
  FOREIGN KEY (containing_symbol_id) REFERENCES symbols(symbol_id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS relationships (
  relationship_id TEXT PRIMARY KEY,
  reference_site_id TEXT NOT NULL,
  from_symbol_id TEXT NOT NULL,
  to_symbol_id TEXT NOT NULL,
  file_id TEXT NOT NULL,
  path TEXT NOT NULL,
  kind TEXT NOT NULL,
  start_line INTEGER,
  start_column INTEGER,
  end_line INTEGER,
  end_column INTEGER,
  start_byte INTEGER,
  end_byte INTEGER,
  confidence REAL NOT NULL,
  metadata_json TEXT,
  FOREIGN KEY (reference_site_id) REFERENCES reference_sites(reference_site_id) ON DELETE CASCADE,
  FOREIGN KEY (from_symbol_id) REFERENCES symbols(symbol_id) ON DELETE CASCADE,
  FOREIGN KEY (to_symbol_id) REFERENCES symbols(symbol_id) ON DELETE CASCADE,
  FOREIGN KEY (file_id) REFERENCES files(file_id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS pending_relationships (
  pending_relationship_id TEXT PRIMARY KEY,
  reference_site_id TEXT NOT NULL,
  from_symbol_id TEXT NOT NULL,
  caller_scope_symbol_id TEXT,
  file_id TEXT NOT NULL,
  path TEXT NOT NULL,
  kind TEXT NOT NULL,
  target_display_name TEXT NOT NULL,
  target_terminal_name TEXT NOT NULL,
  target_receiver TEXT,
  target_namespace_json TEXT NOT NULL,
  target_import_context TEXT,
  start_line INTEGER NOT NULL,
  start_column INTEGER,
  end_line INTEGER,
  end_column INTEGER,
  start_byte INTEGER,
  end_byte INTEGER,
  confidence REAL NOT NULL,
  metadata_json TEXT,
  FOREIGN KEY (reference_site_id) REFERENCES reference_sites(reference_site_id) ON DELETE CASCADE,
  FOREIGN KEY (from_symbol_id) REFERENCES symbols(symbol_id) ON DELETE CASCADE,
  FOREIGN KEY (caller_scope_symbol_id) REFERENCES symbols(symbol_id) ON DELETE SET NULL,
  FOREIGN KEY (file_id) REFERENCES files(file_id) ON DELETE CASCADE
);

-- Resolution overlay (schema v4): pending rows are durable facts; resolution is
-- a derived overlay. A pending row is "resolved" iff it has a
-- `pending_resolutions` row. If the target symbol dies, CASCADE removes the
-- resolution and the pending row reverts to unresolved with its context intact.
CREATE TABLE IF NOT EXISTS pending_resolutions (
  pending_relationship_id TEXT PRIMARY KEY
    REFERENCES pending_relationships(pending_relationship_id) ON DELETE CASCADE,
  target_symbol_id TEXT NOT NULL
    REFERENCES symbols(symbol_id) ON DELETE CASCADE,
  tier INTEGER NOT NULL,
  confidence REAL NOT NULL,
  method TEXT NOT NULL,
  resolved_at_revision INTEGER NOT NULL
);

-- Identifier resolution overlay (schema v4). Resolved rows carry a target and
-- CASCADE away when it dies; ambiguous/missing rows have NULL target. The CHECK
-- enforces outcome/target coherence (outcome='resolved' <=> target NOT NULL).
-- Since schema v6 this table is the ONLY place an identifier resolution outcome
-- is stored: `identifiers` carries no denormalized target column, and no
-- resolution provenance is ever written to `identifiers.metadata_json`.
CREATE TABLE IF NOT EXISTS identifier_resolutions (
  identifier_id TEXT PRIMARY KEY
    REFERENCES identifiers(identifier_id) ON DELETE CASCADE,
  target_symbol_id TEXT
    REFERENCES symbols(symbol_id) ON DELETE CASCADE,
  tier INTEGER,
  confidence REAL,
  method TEXT,
  outcome TEXT NOT NULL,
  candidates INTEGER,
  resolved_at_revision INTEGER NOT NULL,
  CHECK ((outcome = 'resolved') = (target_symbol_id IS NOT NULL))
);

-- One source token owns ONE reference site, written once per sharing pass
-- (identifier, relationship, pending). The passes derive the site payload
-- through different code paths, so a disagreement is an extractor bug — but it
-- is a one-column bug on one site, and aborting would roll back the whole
-- single-transaction import. First write wins; the writer counts the divergence
-- and the scan report carries a `reference_site_payload_conflict` warning.
CREATE TRIGGER IF NOT EXISTS reference_sites_identity_guard
BEFORE INSERT ON reference_sites
WHEN EXISTS (
  SELECT 1
  FROM reference_sites AS existing
  WHERE existing.reference_site_id = NEW.reference_site_id
    AND (
      existing.file_id IS NOT NEW.file_id
      OR existing.path IS NOT NEW.path
      OR existing.language IS NOT NEW.language
      OR existing.containing_symbol_id IS NOT NEW.containing_symbol_id
      OR existing.start_line IS NOT NEW.start_line
      OR existing.start_column IS NOT NEW.start_column
      OR existing.end_line IS NOT NEW.end_line
      OR existing.end_column IS NOT NEW.end_column
      OR existing.start_byte IS NOT NEW.start_byte
      OR existing.end_byte IS NOT NEW.end_byte
      OR existing.is_exact IS NOT NEW.is_exact
      OR existing.provenance IS NOT NEW.provenance
    )
)
BEGIN
  SELECT RAISE(IGNORE);
END;

CREATE TABLE IF NOT EXISTS type_facts (
  type_fact_id TEXT PRIMARY KEY,
  symbol_id TEXT NOT NULL,
  language TEXT NOT NULL,
  resolved_type TEXT NOT NULL,
  generic_params_json TEXT,
  constraints_json TEXT,
  is_inferred INTEGER NOT NULL,
  metadata_json TEXT,
  FOREIGN KEY (symbol_id) REFERENCES symbols(symbol_id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS type_argument_usages (
  usage_id TEXT PRIMARY KEY,
  identifier_id TEXT NOT NULL,
  file_id TEXT NOT NULL,
  path TEXT NOT NULL,
  language TEXT NOT NULL,
  metadata_json TEXT,
  FOREIGN KEY (identifier_id) REFERENCES identifiers(identifier_id) ON DELETE CASCADE,
  FOREIGN KEY (file_id) REFERENCES files(file_id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS type_arguments (
  type_argument_id TEXT PRIMARY KEY,
  usage_id TEXT NOT NULL,
  parent_type_argument_id TEXT,
  ordinal INTEGER NOT NULL,
  type_name TEXT NOT NULL,
  FOREIGN KEY (usage_id) REFERENCES type_argument_usages(usage_id) ON DELETE CASCADE,
  FOREIGN KEY (parent_type_argument_id) REFERENCES type_arguments(type_argument_id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS literals (
  literal_id TEXT PRIMARY KEY,
  file_id TEXT NOT NULL,
  path TEXT NOT NULL,
  language TEXT NOT NULL,
  literal_text TEXT NOT NULL,
  kind TEXT NOT NULL,
  carrier TEXT,
  arg_position INTEGER NOT NULL,
  containing_symbol_id TEXT,
  start_line INTEGER NOT NULL,
  start_column INTEGER NOT NULL,
  end_line INTEGER NOT NULL,
  end_column INTEGER NOT NULL,
  start_byte INTEGER NOT NULL,
  end_byte INTEGER NOT NULL,
  confidence REAL NOT NULL,
  metadata_json TEXT,
  FOREIGN KEY (file_id) REFERENCES files(file_id) ON DELETE CASCADE,
  FOREIGN KEY (containing_symbol_id) REFERENCES symbols(symbol_id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS source_regions (
  source_region_id TEXT PRIMARY KEY,
  file_id TEXT NOT NULL,
  path TEXT NOT NULL,
  language TEXT NOT NULL,
  kind TEXT NOT NULL,
  containing_symbol_id TEXT,
  start_line INTEGER NOT NULL,
  start_column INTEGER NOT NULL,
  end_line INTEGER NOT NULL,
  end_column INTEGER NOT NULL,
  start_byte INTEGER NOT NULL,
  end_byte INTEGER NOT NULL,
  metadata_json TEXT,
  FOREIGN KEY (file_id) REFERENCES files(file_id) ON DELETE CASCADE,
  FOREIGN KEY (containing_symbol_id) REFERENCES symbols(symbol_id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS structural_facts (
  structural_fact_id TEXT PRIMARY KEY,
  file_id TEXT NOT NULL,
  path TEXT NOT NULL,
  language TEXT NOT NULL,
  pattern_id TEXT NOT NULL,
  capture_name TEXT NOT NULL,
  node_kind TEXT NOT NULL,
  containing_symbol_id TEXT,
  start_line INTEGER NOT NULL,
  start_column INTEGER NOT NULL,
  end_line INTEGER NOT NULL,
  end_column INTEGER NOT NULL,
  start_byte INTEGER NOT NULL,
  end_byte INTEGER NOT NULL,
  confidence REAL NOT NULL,
  metadata_json TEXT,
  FOREIGN KEY (file_id) REFERENCES files(file_id) ON DELETE CASCADE,
  FOREIGN KEY (containing_symbol_id) REFERENCES symbols(symbol_id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS complexity_metrics (
  complexity_metric_id TEXT PRIMARY KEY,
  file_id TEXT NOT NULL,
  path TEXT NOT NULL,
  language TEXT NOT NULL,
  scope TEXT NOT NULL,
  symbol_id TEXT,
  algorithm_id TEXT NOT NULL,
  covered_lines INTEGER NOT NULL,
  covered_bytes INTEGER NOT NULL,
  decision_count INTEGER NOT NULL,
  loop_count INTEGER NOT NULL,
  max_nesting_depth INTEGER NOT NULL,
  parameter_count INTEGER,
  start_line INTEGER NOT NULL,
  start_column INTEGER NOT NULL,
  end_line INTEGER NOT NULL,
  end_column INTEGER NOT NULL,
  start_byte INTEGER NOT NULL,
  end_byte INTEGER NOT NULL,
  metadata_json TEXT,
  FOREIGN KEY (file_id) REFERENCES files(file_id) ON DELETE CASCADE,
  FOREIGN KEY (symbol_id) REFERENCES symbols(symbol_id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS parse_diagnostics (
  diagnostic_id TEXT PRIMARY KEY,
  file_id TEXT NOT NULL,
  path TEXT NOT NULL,
  language TEXT NOT NULL,
  kind TEXT NOT NULL,
  message TEXT,
  start_line INTEGER NOT NULL,
  start_column INTEGER NOT NULL,
  end_line INTEGER NOT NULL,
  end_column INTEGER NOT NULL,
  start_byte INTEGER NOT NULL,
  end_byte INTEGER NOT NULL,
  metadata_json TEXT,
  FOREIGN KEY (file_id) REFERENCES files(file_id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS language_capabilities (
  language TEXT PRIMARY KEY,
  parser_package TEXT NOT NULL,
  extensions_json TEXT NOT NULL,
  dependency_status TEXT NOT NULL,
  target_symbols INTEGER NOT NULL,
  target_relationships INTEGER NOT NULL,
  target_pending_relationships INTEGER NOT NULL,
  target_identifiers INTEGER NOT NULL,
  target_types INTEGER NOT NULL,
  actual_symbols INTEGER NOT NULL,
  actual_relationships INTEGER NOT NULL,
  actual_pending_relationships INTEGER NOT NULL,
  actual_identifiers INTEGER NOT NULL,
  actual_types INTEGER NOT NULL,
  kind_coverage_json TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS language_capability_fixtures (
  language TEXT NOT NULL,
  fixture_name TEXT NOT NULL,
  source_path TEXT NOT NULL,
  expected_path TEXT NOT NULL,
  PRIMARY KEY (language, fixture_name),
  FOREIGN KEY (language) REFERENCES language_capabilities(language) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS language_capability_gaps (
  gap_id TEXT PRIMARY KEY,
  language TEXT NOT NULL,
  capability TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('open', 'exception')),
  reason TEXT NOT NULL,
  required_closure TEXT NOT NULL,
  evidence_json TEXT NOT NULL,
  FOREIGN KEY (language) REFERENCES language_capabilities(language) ON DELETE CASCADE
);
"#;

const SCHEMA_INDEXES_SQL: &str = r#"
CREATE INDEX IF NOT EXISTS idx_files_path ON files(path);
CREATE INDEX IF NOT EXISTS idx_files_language ON files(language);
CREATE INDEX IF NOT EXISTS idx_symbols_path ON symbols(path);
CREATE INDEX IF NOT EXISTS idx_symbols_file ON symbols(file_id);
CREATE INDEX IF NOT EXISTS idx_symbols_name_kind ON symbols(name, kind);
CREATE INDEX IF NOT EXISTS idx_symbols_parent ON symbols(parent_symbol_id);
CREATE INDEX IF NOT EXISTS idx_symbols_is_test ON symbols(is_test);
CREATE INDEX IF NOT EXISTS idx_symbols_test_container ON symbols(test_container);
CREATE INDEX IF NOT EXISTS idx_symbols_test_lifecycle ON symbols(test_lifecycle);
CREATE INDEX IF NOT EXISTS idx_reference_sites_file ON reference_sites(file_id);
CREATE INDEX IF NOT EXISTS idx_reference_sites_containing_symbol
  ON reference_sites(containing_symbol_id);
CREATE INDEX IF NOT EXISTS idx_identifiers_file ON identifiers(file_id);
CREATE INDEX IF NOT EXISTS idx_identifiers_name_kind ON identifiers(name, kind);
CREATE INDEX IF NOT EXISTS idx_identifiers_containing ON identifiers(containing_symbol_id);
CREATE INDEX IF NOT EXISTS idx_identifiers_reference_site ON identifiers(reference_site_id);
CREATE INDEX IF NOT EXISTS idx_relationships_from ON relationships(from_symbol_id);
CREATE INDEX IF NOT EXISTS idx_relationships_to ON relationships(to_symbol_id);
CREATE INDEX IF NOT EXISTS idx_relationships_kind ON relationships(kind);
CREATE INDEX IF NOT EXISTS idx_relationships_file ON relationships(file_id);
CREATE INDEX IF NOT EXISTS idx_relationships_reference_site ON relationships(reference_site_id);
CREATE INDEX IF NOT EXISTS idx_pending_terminal ON pending_relationships(target_terminal_name);
CREATE INDEX IF NOT EXISTS idx_pending_file ON pending_relationships(file_id);
CREATE INDEX IF NOT EXISTS idx_pending_from ON pending_relationships(from_symbol_id);
CREATE INDEX IF NOT EXISTS idx_pending_caller_scope ON pending_relationships(caller_scope_symbol_id);
CREATE INDEX IF NOT EXISTS idx_pending_reference_site ON pending_relationships(reference_site_id);
CREATE INDEX IF NOT EXISTS idx_type_facts_symbol ON type_facts(symbol_id);
CREATE INDEX IF NOT EXISTS idx_symbol_annotations_symbol ON symbol_annotations(symbol_id);
CREATE INDEX IF NOT EXISTS idx_type_argument_usages_identifier ON type_argument_usages(identifier_id);
CREATE INDEX IF NOT EXISTS idx_type_argument_usages_file ON type_argument_usages(file_id);
CREATE INDEX IF NOT EXISTS idx_type_arguments_usage ON type_arguments(usage_id);
CREATE INDEX IF NOT EXISTS idx_type_arguments_parent ON type_arguments(parent_type_argument_id);
CREATE INDEX IF NOT EXISTS idx_literals_file ON literals(file_id);
CREATE INDEX IF NOT EXISTS idx_literals_containing_symbol ON literals(containing_symbol_id);
CREATE INDEX IF NOT EXISTS idx_source_regions_file_span ON source_regions(file_id, start_byte, end_byte);
CREATE INDEX IF NOT EXISTS idx_source_regions_export_order ON source_regions(path, start_byte, end_byte, kind, source_region_id);
CREATE INDEX IF NOT EXISTS idx_source_regions_kind_file ON source_regions(kind, file_id, start_byte);
CREATE INDEX IF NOT EXISTS idx_source_regions_symbol ON source_regions(containing_symbol_id);
CREATE INDEX IF NOT EXISTS idx_structural_facts_file_span ON structural_facts(file_id, start_byte, end_byte);
CREATE INDEX IF NOT EXISTS idx_structural_facts_export_order ON structural_facts(path, start_byte, end_byte, pattern_id, capture_name, structural_fact_id);
CREATE INDEX IF NOT EXISTS idx_structural_facts_pattern_language_path ON structural_facts(pattern_id, language, path);
CREATE INDEX IF NOT EXISTS idx_structural_facts_symbol ON structural_facts(containing_symbol_id);
CREATE INDEX IF NOT EXISTS idx_complexity_metrics_file_scope ON complexity_metrics(file_id, scope, start_byte);
CREATE INDEX IF NOT EXISTS idx_complexity_metrics_export_order ON complexity_metrics(path, start_byte, end_byte, scope, symbol_id, complexity_metric_id);
CREATE INDEX IF NOT EXISTS idx_complexity_metrics_scope_language ON complexity_metrics(scope, language, path);
CREATE INDEX IF NOT EXISTS idx_complexity_metrics_symbol ON complexity_metrics(symbol_id);
CREATE INDEX IF NOT EXISTS idx_diagnostics_path ON parse_diagnostics(path);
CREATE INDEX IF NOT EXISTS idx_diagnostics_file ON parse_diagnostics(file_id);
CREATE INDEX IF NOT EXISTS idx_pending_resolutions_target ON pending_resolutions(target_symbol_id);
CREATE INDEX IF NOT EXISTS idx_identifier_resolutions_target ON identifier_resolutions(target_symbol_id);
CREATE INDEX IF NOT EXISTS idx_language_capability_gaps_language ON language_capability_gaps(language);
"#;
