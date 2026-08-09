use std::error::Error;
use std::fmt;

use rusqlite::Connection;

/// Physical SQLite catalog version shared by `store.db` and `coord.db`.
pub const STORE_SQLITE_SCHEMA_VERSION: i64 = 2;
/// Initial generation-format epoch for the versioned store.
pub const STORE_FORMAT_EPOCH: i64 = 1;

/// A typed refusal or SQLite failure while creating a store catalog.
#[derive(Debug)]
pub enum StoreSchemaError {
    NewerSchema {
        database: &'static str,
        found: i64,
        supported: i64,
    },
    OlderSchema {
        database: &'static str,
        found: i64,
        supported: i64,
    },
    Sqlite(rusqlite::Error),
}

impl fmt::Display for StoreSchemaError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NewerSchema {
                database,
                found,
                supported,
            } => write!(
                formatter,
                "{database} schema version {found} is newer than supported version {supported}"
            ),
            Self::OlderSchema {
                database,
                found,
                supported,
            } => write!(
                formatter,
                "{database} schema version {found} requires migration to version {supported}"
            ),
            Self::Sqlite(error) => error.fmt(formatter),
        }
    }
}

impl Error for StoreSchemaError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Sqlite(error) => Some(error),
            Self::NewerSchema { .. } | Self::OlderSchema { .. } => None,
        }
    }
}

impl From<rusqlite::Error> for StoreSchemaError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

/// Creates or validates the independently versioned `store.db` catalog.
pub fn create_store_schema(conn: &Connection) -> Result<(), StoreSchemaError> {
    create_schema(conn, "store.db", STORE_SCHEMA_SQL)
}

/// Creates or validates the independently versioned `coord.db` catalog.
pub fn create_coordinator_schema(conn: &Connection) -> Result<(), StoreSchemaError> {
    create_schema(conn, "coord.db", COORDINATOR_SCHEMA_SQL)
}

fn create_schema(
    conn: &Connection,
    database: &'static str,
    schema: &str,
) -> Result<(), StoreSchemaError> {
    let found = conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))?;
    if found > STORE_SQLITE_SCHEMA_VERSION {
        return Err(StoreSchemaError::NewerSchema {
            database,
            found,
            supported: STORE_SQLITE_SCHEMA_VERSION,
        });
    }
    if found != 0 && found < STORE_SQLITE_SCHEMA_VERSION {
        return Err(StoreSchemaError::OlderSchema {
            database,
            found,
            supported: STORE_SQLITE_SCHEMA_VERSION,
        });
    }

    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    conn.execute_batch(schema)?;
    Ok(())
}

const STORE_SCHEMA_SQL: &str = r#"
BEGIN IMMEDIATE;

CREATE TABLE IF NOT EXISTS store_meta (
  key TEXT PRIMARY KEY CHECK (length(key) > 0),
  value TEXT NOT NULL
) STRICT;

INSERT OR IGNORE INTO store_meta (key, value) VALUES
  ('store_sqlite_schema_version', '2'),
  ('store_format_epoch', '1'),
  ('retention_window_days', '7'),
  ('retention_byte_target', '1.20'),
  ('retention_byte_ceiling', '1.25'),
  ('retention_path_cap', '24'),
  ('generation_state', 'serving');

CREATE TRIGGER IF NOT EXISTS trg_store_meta_generation_state_insert
BEFORE INSERT ON store_meta
WHEN NEW.key = 'generation_state' AND NEW.value NOT IN ('serving', 'retired')
BEGIN
  SELECT RAISE(ABORT, 'invalid generation state');
END;

CREATE TRIGGER IF NOT EXISTS trg_store_meta_generation_state_update
BEFORE UPDATE OF value ON store_meta
WHEN NEW.key = 'generation_state' AND NEW.value NOT IN ('serving', 'retired')
BEGIN
  SELECT RAISE(ABORT, 'invalid generation state');
END;

CREATE TRIGGER IF NOT EXISTS trg_store_meta_generation_state_delete
BEFORE DELETE ON store_meta
WHEN OLD.key = 'generation_state'
BEGIN
  SELECT RAISE(ABORT, 'generation state is required');
END;

CREATE TABLE IF NOT EXISTS file_versions (
  version_id INTEGER PRIMARY KEY AUTOINCREMENT,
  path TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  extraction_epoch INTEGER NOT NULL,
  language TEXT NOT NULL,
  content_bytes INTEGER NOT NULL CHECK (content_bytes >= 0),
  line_count INTEGER CHECK (line_count IS NULL OR line_count >= 0),
  metadata_json TEXT,
  complete_l1 INTEGER CHECK (complete_l1 IS NULL OR complete_l1 > 0),
  complete_l2 INTEGER CHECK (complete_l2 IS NULL OR complete_l2 > 0),
  complete_l3 INTEGER CHECK (complete_l3 IS NULL OR complete_l3 > 0),
  CHECK (complete_l2 IS NULL OR complete_l1 IS NOT NULL),
  CHECK (complete_l3 IS NULL OR complete_l2 IS NOT NULL)
) STRICT;

CREATE TABLE IF NOT EXISTS parser_inventory (
  extraction_epoch INTEGER NOT NULL,
  language TEXT NOT NULL,
  parser_package TEXT NOT NULL,
  parser_version TEXT,
  grammar_version TEXT,
  source TEXT,
  metadata_json TEXT,
  PRIMARY KEY (extraction_epoch, language, parser_package)
) STRICT;

CREATE TABLE IF NOT EXISTS language_capabilities (
  extraction_epoch INTEGER NOT NULL,
  language TEXT NOT NULL,
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
  kind_coverage_json TEXT NOT NULL,
  PRIMARY KEY (extraction_epoch, language)
) STRICT;

CREATE TABLE IF NOT EXISTS language_capability_fixtures (
  extraction_epoch INTEGER NOT NULL,
  language TEXT NOT NULL,
  fixture_name TEXT NOT NULL,
  source_path TEXT NOT NULL,
  expected_path TEXT NOT NULL,
  PRIMARY KEY (extraction_epoch, language, fixture_name),
  FOREIGN KEY (extraction_epoch, language)
    REFERENCES language_capabilities(extraction_epoch, language) ON DELETE CASCADE
) STRICT;

CREATE TABLE IF NOT EXISTS language_capability_gaps (
  extraction_epoch INTEGER NOT NULL,
  gap_id TEXT NOT NULL,
  language TEXT NOT NULL,
  capability TEXT NOT NULL,
  status TEXT NOT NULL CHECK (status IN ('open', 'exception')),
  reason TEXT NOT NULL,
  required_closure TEXT NOT NULL,
  evidence_json TEXT NOT NULL,
  PRIMARY KEY (extraction_epoch, gap_id),
  FOREIGN KEY (extraction_epoch, language)
    REFERENCES language_capabilities(extraction_epoch, language) ON DELETE CASCADE
) STRICT;

CREATE TABLE IF NOT EXISTS symbols (
  version_id INTEGER NOT NULL,
  symbol_id TEXT NOT NULL,
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
  PRIMARY KEY (version_id, symbol_id),
  FOREIGN KEY (version_id) REFERENCES file_versions(version_id) ON DELETE CASCADE,
  FOREIGN KEY (version_id, parent_symbol_id)
    REFERENCES symbols(version_id, symbol_id)
    ON DELETE NO ACTION DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE IF NOT EXISTS symbol_annotations (
  version_id INTEGER NOT NULL,
  annotation_id TEXT NOT NULL,
  symbol_id TEXT NOT NULL,
  annotation TEXT NOT NULL,
  annotation_key TEXT NOT NULL,
  raw_text TEXT,
  carrier TEXT,
  metadata_json TEXT,
  PRIMARY KEY (version_id, annotation_id),
  FOREIGN KEY (version_id) REFERENCES file_versions(version_id) ON DELETE CASCADE,
  FOREIGN KEY (version_id, symbol_id)
    REFERENCES symbols(version_id, symbol_id)
    ON DELETE NO ACTION DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE IF NOT EXISTS reference_sites (
  version_id INTEGER NOT NULL,
  reference_site_id TEXT NOT NULL,
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
  level INTEGER NOT NULL,
  PRIMARY KEY (version_id, reference_site_id),
  FOREIGN KEY (version_id) REFERENCES file_versions(version_id) ON DELETE CASCADE,
  FOREIGN KEY (version_id, containing_symbol_id)
    REFERENCES symbols(version_id, symbol_id)
    ON DELETE NO ACTION DEFERRABLE INITIALLY DEFERRED,
  CHECK (length(reference_site_id) > 0),
  CHECK (is_exact IN (0, 1)),
  CHECK (level IN (1, 2)),
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
) STRICT;

CREATE TABLE IF NOT EXISTS identifiers (
  version_id INTEGER NOT NULL,
  identifier_id TEXT NOT NULL,
  reference_site_id TEXT NOT NULL,
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
  PRIMARY KEY (version_id, identifier_id),
  FOREIGN KEY (version_id) REFERENCES file_versions(version_id) ON DELETE CASCADE,
  FOREIGN KEY (version_id, reference_site_id)
    REFERENCES reference_sites(version_id, reference_site_id)
    ON DELETE NO ACTION DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (version_id, containing_symbol_id)
    REFERENCES symbols(version_id, symbol_id)
    ON DELETE NO ACTION DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE IF NOT EXISTS relationships (
  version_id INTEGER NOT NULL,
  relationship_id TEXT NOT NULL,
  reference_site_id TEXT NOT NULL,
  from_symbol_id TEXT NOT NULL,
  to_symbol_id TEXT NOT NULL,
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
  PRIMARY KEY (version_id, relationship_id),
  FOREIGN KEY (version_id) REFERENCES file_versions(version_id) ON DELETE CASCADE,
  FOREIGN KEY (version_id, reference_site_id)
    REFERENCES reference_sites(version_id, reference_site_id)
    ON DELETE NO ACTION DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (version_id, from_symbol_id)
    REFERENCES symbols(version_id, symbol_id)
    ON DELETE NO ACTION DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (version_id, to_symbol_id)
    REFERENCES symbols(version_id, symbol_id)
    ON DELETE NO ACTION DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE IF NOT EXISTS pending_relationships (
  version_id INTEGER NOT NULL,
  pending_relationship_id TEXT NOT NULL,
  reference_site_id TEXT NOT NULL,
  from_symbol_id TEXT NOT NULL,
  caller_scope_symbol_id TEXT,
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
  PRIMARY KEY (version_id, pending_relationship_id),
  FOREIGN KEY (version_id) REFERENCES file_versions(version_id) ON DELETE CASCADE,
  FOREIGN KEY (version_id, reference_site_id)
    REFERENCES reference_sites(version_id, reference_site_id)
    ON DELETE NO ACTION DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (version_id, from_symbol_id)
    REFERENCES symbols(version_id, symbol_id)
    ON DELETE NO ACTION DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (version_id, caller_scope_symbol_id)
    REFERENCES symbols(version_id, symbol_id)
    ON DELETE NO ACTION DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE IF NOT EXISTS type_facts (
  version_id INTEGER NOT NULL,
  type_fact_id TEXT NOT NULL,
  symbol_id TEXT NOT NULL,
  language TEXT NOT NULL,
  resolved_type TEXT NOT NULL,
  generic_params_json TEXT,
  constraints_json TEXT,
  is_inferred INTEGER NOT NULL,
  metadata_json TEXT,
  PRIMARY KEY (version_id, type_fact_id),
  FOREIGN KEY (version_id) REFERENCES file_versions(version_id) ON DELETE CASCADE,
  FOREIGN KEY (version_id, symbol_id)
    REFERENCES symbols(version_id, symbol_id)
    ON DELETE NO ACTION DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE IF NOT EXISTS type_argument_usages (
  version_id INTEGER NOT NULL,
  usage_id TEXT NOT NULL,
  identifier_id TEXT NOT NULL,
  path TEXT NOT NULL,
  language TEXT NOT NULL,
  metadata_json TEXT,
  PRIMARY KEY (version_id, usage_id),
  FOREIGN KEY (version_id) REFERENCES file_versions(version_id) ON DELETE CASCADE,
  FOREIGN KEY (version_id, identifier_id)
    REFERENCES identifiers(version_id, identifier_id)
    ON DELETE NO ACTION DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE IF NOT EXISTS type_arguments (
  version_id INTEGER NOT NULL,
  type_argument_id TEXT NOT NULL,
  usage_id TEXT NOT NULL,
  parent_type_argument_id TEXT,
  ordinal INTEGER NOT NULL,
  type_name TEXT NOT NULL,
  PRIMARY KEY (version_id, type_argument_id),
  FOREIGN KEY (version_id) REFERENCES file_versions(version_id) ON DELETE CASCADE,
  FOREIGN KEY (version_id, usage_id)
    REFERENCES type_argument_usages(version_id, usage_id)
    ON DELETE NO ACTION DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (version_id, parent_type_argument_id)
    REFERENCES type_arguments(version_id, type_argument_id)
    ON DELETE NO ACTION DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE IF NOT EXISTS literals (
  version_id INTEGER NOT NULL,
  literal_id TEXT NOT NULL,
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
  PRIMARY KEY (version_id, literal_id),
  FOREIGN KEY (version_id) REFERENCES file_versions(version_id) ON DELETE CASCADE,
  FOREIGN KEY (version_id, containing_symbol_id)
    REFERENCES symbols(version_id, symbol_id)
    ON DELETE NO ACTION DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE IF NOT EXISTS source_regions (
  version_id INTEGER NOT NULL,
  source_region_id TEXT NOT NULL,
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
  PRIMARY KEY (version_id, source_region_id),
  FOREIGN KEY (version_id) REFERENCES file_versions(version_id) ON DELETE CASCADE,
  FOREIGN KEY (version_id, containing_symbol_id)
    REFERENCES symbols(version_id, symbol_id)
    ON DELETE NO ACTION DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE IF NOT EXISTS structural_facts (
  version_id INTEGER NOT NULL,
  structural_fact_id TEXT NOT NULL,
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
  PRIMARY KEY (version_id, structural_fact_id),
  FOREIGN KEY (version_id) REFERENCES file_versions(version_id) ON DELETE CASCADE,
  FOREIGN KEY (version_id, containing_symbol_id)
    REFERENCES symbols(version_id, symbol_id)
    ON DELETE NO ACTION DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE IF NOT EXISTS complexity_metrics (
  version_id INTEGER NOT NULL,
  complexity_metric_id TEXT NOT NULL,
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
  PRIMARY KEY (version_id, complexity_metric_id),
  FOREIGN KEY (version_id) REFERENCES file_versions(version_id) ON DELETE CASCADE,
  FOREIGN KEY (version_id, symbol_id)
    REFERENCES symbols(version_id, symbol_id)
    ON DELETE NO ACTION DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE IF NOT EXISTS parse_diagnostics (
  version_id INTEGER NOT NULL,
  diagnostic_id TEXT NOT NULL,
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
  PRIMARY KEY (version_id, diagnostic_id),
  FOREIGN KEY (version_id) REFERENCES file_versions(version_id) ON DELETE CASCADE
) STRICT;

CREATE TRIGGER IF NOT EXISTS reference_sites_identity_guard
BEFORE INSERT ON reference_sites
WHEN EXISTS (
  SELECT 1
  FROM reference_sites AS existing
  WHERE existing.version_id = NEW.version_id
    AND existing.reference_site_id = NEW.reference_site_id
    AND (
      existing.path IS NOT NEW.path
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
      OR existing.level IS NOT NEW.level
    )
)
BEGIN
  SELECT RAISE(IGNORE);
END;

CREATE TABLE IF NOT EXISTS views (
  view_id TEXT PRIMARY KEY CHECK (length(view_id) > 0),
  root TEXT NOT NULL CHECK (length(root) > 0),
  current_generation INTEGER,
  resolution_state TEXT NOT NULL DEFAULT 'unbound',
  resolution_base_id TEXT,
  resolution_delta_generation INTEGER,
  resolution_exact_at INTEGER,
  created_at TEXT NOT NULL CHECK (
    length(created_at) BETWEEN 20 AND 30
      AND substr(created_at, 5, 1) = '-'
      AND substr(created_at, 8, 1) = '-'
      AND substr(created_at, 11, 1) = 'T'
      AND substr(created_at, 14, 1) = ':'
      AND substr(created_at, 17, 1) = ':'
      AND substr(created_at, -1, 1) = 'Z'
      AND strftime('%Y-%m-%dT%H:%M:%S', created_at) = substr(created_at, 1, 19)
      AND (
        length(created_at) = 20
        OR (
          substr(created_at, 20, 1) = '.'
          AND length(created_at) >= 22
          AND substr(created_at, 21, length(created_at) - 21) NOT GLOB '*[^0-9]*'
        )
      )
  ),
  updated_at TEXT NOT NULL CHECK (
    length(updated_at) BETWEEN 20 AND 30
      AND substr(updated_at, 5, 1) = '-'
      AND substr(updated_at, 8, 1) = '-'
      AND substr(updated_at, 11, 1) = 'T'
      AND substr(updated_at, 14, 1) = ':'
      AND substr(updated_at, 17, 1) = ':'
      AND substr(updated_at, -1, 1) = 'Z'
      AND strftime('%Y-%m-%dT%H:%M:%S', updated_at) = substr(updated_at, 1, 19)
      AND (
        length(updated_at) = 20
        OR (
          substr(updated_at, 20, 1) = '.'
          AND length(updated_at) >= 22
          AND substr(updated_at, 21, length(updated_at) - 21) NOT GLOB '*[^0-9]*'
        )
      )
  ),
  CHECK (
    (resolution_state = 'unbound'
      AND resolution_base_id IS NULL
      AND resolution_delta_generation IS NULL
      AND resolution_exact_at IS NULL)
    OR
    (resolution_state = 'converging'
      AND current_generation IS NOT NULL
      AND resolution_base_id IS NOT NULL
      AND resolution_delta_generation IS NOT NULL
      AND resolution_exact_at IS NULL)
    OR
    (resolution_state = 'exact'
      AND current_generation IS NOT NULL
      AND resolution_base_id IS NOT NULL
      AND resolution_delta_generation IS NOT NULL
      AND resolution_exact_at = current_generation)
  ),
  FOREIGN KEY (view_id, current_generation)
    REFERENCES manifests(view_id, generation)
    ON DELETE NO ACTION DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (resolution_base_id) REFERENCES resolution_bases(base_id)
    ON DELETE NO ACTION DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (view_id, resolution_delta_generation)
    REFERENCES resolution_deltas(view_id, delta_generation)
    ON DELETE NO ACTION DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE IF NOT EXISTS manifests (
  view_id TEXT NOT NULL,
  generation INTEGER NOT NULL CHECK (generation > 0),
  manifest_hash TEXT NOT NULL CHECK (length(manifest_hash) > 0),
  request_id TEXT NOT NULL CHECK (length(request_id) > 0),
  created_at TEXT NOT NULL CHECK (
    length(created_at) BETWEEN 20 AND 30
      AND substr(created_at, 5, 1) = '-'
      AND substr(created_at, 8, 1) = '-'
      AND substr(created_at, 11, 1) = 'T'
      AND substr(created_at, 14, 1) = ':'
      AND substr(created_at, 17, 1) = ':'
      AND substr(created_at, -1, 1) = 'Z'
      AND strftime('%Y-%m-%dT%H:%M:%S', created_at) = substr(created_at, 1, 19)
      AND (
        length(created_at) = 20
        OR (
          substr(created_at, 20, 1) = '.'
          AND length(created_at) >= 22
          AND substr(created_at, 21, length(created_at) - 21) NOT GLOB '*[^0-9]*'
        )
      )
  ),
  PRIMARY KEY (view_id, generation),
  FOREIGN KEY (view_id) REFERENCES views(view_id)
    ON DELETE NO ACTION DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE IF NOT EXISTS manifest_entries (
  view_id TEXT NOT NULL,
  generation INTEGER NOT NULL,
  path TEXT NOT NULL,
  language TEXT NOT NULL CHECK (length(language) > 0),
  version_id INTEGER,
  status TEXT NOT NULL CHECK (status IN ('indexed', 'failed_preserved', 'failed')),
  observed_content_hash TEXT NOT NULL,
  indexed_at TEXT NOT NULL CHECK (
    length(indexed_at) BETWEEN 20 AND 30
      AND substr(indexed_at, 5, 1) = '-'
      AND substr(indexed_at, 8, 1) = '-'
      AND substr(indexed_at, 11, 1) = 'T'
      AND substr(indexed_at, 14, 1) = ':'
      AND substr(indexed_at, 17, 1) = ':'
      AND substr(indexed_at, -1, 1) = 'Z'
      AND strftime('%Y-%m-%dT%H:%M:%S', indexed_at) = substr(indexed_at, 1, 19)
      AND (
        length(indexed_at) = 20
        OR (
          substr(indexed_at, 20, 1) = '.'
          AND length(indexed_at) >= 22
          AND substr(indexed_at, 21, length(indexed_at) - 21) NOT GLOB '*[^0-9]*'
        )
      )
  ),
  error_class TEXT,
  error_json TEXT,
  PRIMARY KEY (view_id, generation, path),
  FOREIGN KEY (view_id, generation)
    REFERENCES manifests(view_id, generation)
    ON DELETE NO ACTION DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (version_id) REFERENCES file_versions(version_id) ON DELETE RESTRICT,
  CHECK (
    (status = 'indexed'
      AND version_id IS NOT NULL
      AND error_class IS NULL
      AND error_json IS NULL)
    OR
    (status = 'failed_preserved'
      AND version_id IS NOT NULL
      AND error_class IS NOT NULL
      AND error_json IS NOT NULL)
    OR
    (status = 'failed'
      AND version_id IS NULL
      AND error_class IS NOT NULL
      AND error_json IS NOT NULL)
  )
) STRICT;

CREATE TABLE IF NOT EXISTS resolution_bases (
  base_id TEXT PRIMARY KEY CHECK (length(base_id) > 0),
  manifest_hash TEXT NOT NULL CHECK (length(manifest_hash) > 0),
  resolver_output_epoch INTEGER NOT NULL CHECK (resolver_output_epoch > 0),
  state TEXT NOT NULL CHECK (state IN ('building', 'ready')),
  relative_path TEXT NOT NULL CHECK (length(relative_path) > 0),
  identifier_count INTEGER NOT NULL CHECK (identifier_count >= 0),
  pending_count INTEGER NOT NULL CHECK (pending_count >= 0),
  file_bytes INTEGER,
  file_sha256 TEXT,
  request_id TEXT NOT NULL CHECK (length(request_id) > 0),
  created_at TEXT NOT NULL CHECK (
    length(created_at) BETWEEN 20 AND 30
      AND substr(created_at, 5, 1) = '-'
      AND substr(created_at, 8, 1) = '-'
      AND substr(created_at, 11, 1) = 'T'
      AND substr(created_at, 14, 1) = ':'
      AND substr(created_at, 17, 1) = ':'
      AND substr(created_at, -1, 1) = 'Z'
      AND strftime('%Y-%m-%dT%H:%M:%S', created_at) = substr(created_at, 1, 19)
      AND (
        length(created_at) = 20
        OR (
          substr(created_at, 20, 1) = '.'
          AND length(created_at) >= 22
          AND substr(created_at, 21, length(created_at) - 21) NOT GLOB '*[^0-9]*'
        )
      )
  ),
  updated_at TEXT NOT NULL CHECK (
    length(updated_at) BETWEEN 20 AND 30
      AND substr(updated_at, 5, 1) = '-'
      AND substr(updated_at, 8, 1) = '-'
      AND substr(updated_at, 11, 1) = 'T'
      AND substr(updated_at, 14, 1) = ':'
      AND substr(updated_at, 17, 1) = ':'
      AND substr(updated_at, -1, 1) = 'Z'
      AND strftime('%Y-%m-%dT%H:%M:%S', updated_at) = substr(updated_at, 1, 19)
      AND (
        length(updated_at) = 20
        OR (
          substr(updated_at, 20, 1) = '.'
          AND length(updated_at) >= 22
          AND substr(updated_at, 21, length(updated_at) - 21) NOT GLOB '*[^0-9]*'
        )
      )
  ),
  CHECK (
    (state = 'building' AND file_bytes IS NULL AND file_sha256 IS NULL)
    OR
    (state = 'ready' AND file_bytes > 0 AND file_sha256 IS NOT NULL AND length(file_sha256) > 0)
  )
) STRICT;

CREATE TABLE IF NOT EXISTS resolution_base_versions (
  base_id TEXT NOT NULL,
  version_id INTEGER NOT NULL,
  PRIMARY KEY (base_id, version_id),
  FOREIGN KEY (base_id) REFERENCES resolution_bases(base_id) ON DELETE CASCADE,
  FOREIGN KEY (version_id) REFERENCES file_versions(version_id) ON DELETE RESTRICT
) STRICT;

CREATE TABLE IF NOT EXISTS resolution_deltas (
  view_id TEXT NOT NULL,
  delta_generation INTEGER NOT NULL CHECK (delta_generation > 0),
  base_id TEXT NOT NULL,
  manifest_generation INTEGER NOT NULL CHECK (manifest_generation > 0),
  manifest_hash TEXT NOT NULL CHECK (length(manifest_hash) > 0),
  resolver_output_epoch INTEGER NOT NULL CHECK (resolver_output_epoch > 0),
  identifier_replacements INTEGER NOT NULL CHECK (identifier_replacements >= 0),
  pending_replacements INTEGER NOT NULL CHECK (pending_replacements >= 0),
  pending_tombstones INTEGER NOT NULL CHECK (pending_tombstones >= 0),
  exact_gap_rows INTEGER NOT NULL CHECK (exact_gap_rows >= 0),
  exact_gap_files INTEGER NOT NULL CHECK (exact_gap_files >= 0),
  exact_gap_json TEXT NOT NULL CHECK (length(exact_gap_json) > 0),
  request_id TEXT NOT NULL CHECK (length(request_id) > 0),
  created_at TEXT NOT NULL CHECK (
    length(created_at) BETWEEN 20 AND 30
      AND substr(created_at, 5, 1) = '-'
      AND substr(created_at, 8, 1) = '-'
      AND substr(created_at, 11, 1) = 'T'
      AND substr(created_at, 14, 1) = ':'
      AND substr(created_at, 17, 1) = ':'
      AND substr(created_at, -1, 1) = 'Z'
      AND strftime('%Y-%m-%dT%H:%M:%S', created_at) = substr(created_at, 1, 19)
      AND (
        length(created_at) = 20
        OR (
          substr(created_at, 20, 1) = '.'
          AND length(created_at) >= 22
          AND substr(created_at, 21, length(created_at) - 21) NOT GLOB '*[^0-9]*'
        )
      )
  ),
  PRIMARY KEY (view_id, delta_generation),
  FOREIGN KEY (view_id) REFERENCES views(view_id)
    ON DELETE NO ACTION DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (base_id) REFERENCES resolution_bases(base_id)
    ON DELETE NO ACTION DEFERRABLE INITIALLY DEFERRED,
  FOREIGN KEY (view_id, manifest_generation)
    REFERENCES manifests(view_id, generation)
    ON DELETE NO ACTION DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE IF NOT EXISTS resolution_identifier_deltas (
  view_id TEXT NOT NULL,
  delta_generation INTEGER NOT NULL,
  version_id INTEGER NOT NULL,
  identifier_id TEXT NOT NULL CHECK (length(identifier_id) > 0),
  target_version_id INTEGER,
  target_symbol_id TEXT,
  tier INTEGER,
  confidence REAL,
  method TEXT,
  outcome TEXT NOT NULL CHECK (length(outcome) > 0),
  candidates INTEGER CHECK (candidates IS NULL OR candidates >= 0),
  PRIMARY KEY (view_id, delta_generation, version_id, identifier_id),
  FOREIGN KEY (view_id, delta_generation)
    REFERENCES resolution_deltas(view_id, delta_generation) ON DELETE CASCADE,
  FOREIGN KEY (version_id) REFERENCES file_versions(version_id) ON DELETE RESTRICT,
  CHECK (
    (outcome = 'resolved'
      AND target_version_id IS NOT NULL
      AND target_symbol_id IS NOT NULL
      AND tier IS NOT NULL
      AND confidence IS NOT NULL
      AND method IS NOT NULL)
    OR
    (outcome <> 'resolved'
      AND target_version_id IS NULL
      AND target_symbol_id IS NULL)
  )
) STRICT;

CREATE TABLE IF NOT EXISTS resolution_pending_deltas (
  view_id TEXT NOT NULL,
  delta_generation INTEGER NOT NULL,
  version_id INTEGER NOT NULL,
  pending_relationship_id TEXT NOT NULL CHECK (length(pending_relationship_id) > 0),
  operation TEXT NOT NULL CHECK (operation IN ('replace', 'tombstone')),
  target_version_id INTEGER,
  target_symbol_id TEXT,
  tier INTEGER,
  confidence REAL,
  method TEXT,
  PRIMARY KEY (view_id, delta_generation, version_id, pending_relationship_id),
  FOREIGN KEY (view_id, delta_generation)
    REFERENCES resolution_deltas(view_id, delta_generation) ON DELETE CASCADE,
  FOREIGN KEY (version_id) REFERENCES file_versions(version_id) ON DELETE RESTRICT,
  CHECK (
    (operation = 'replace'
      AND target_version_id IS NOT NULL
      AND target_symbol_id IS NOT NULL
      AND tier IS NOT NULL
      AND confidence IS NOT NULL
      AND method IS NOT NULL)
    OR
    (operation = 'tombstone'
      AND target_version_id IS NULL
      AND target_symbol_id IS NULL
      AND tier IS NULL
      AND confidence IS NULL
      AND method IS NULL)
  )
) STRICT;

CREATE TABLE IF NOT EXISTS resolution_pins (
  pin_id TEXT PRIMARY KEY CHECK (length(pin_id) > 0),
  owner_kind TEXT NOT NULL CHECK (owner_kind IN ('reader', 'resolve')),
  owner_id TEXT NOT NULL CHECK (length(owner_id) > 0),
  view_id TEXT NOT NULL,
  manifest_generation INTEGER NOT NULL CHECK (manifest_generation > 0),
  base_id TEXT NOT NULL,
  delta_generation INTEGER,
  expires_at TEXT NOT NULL CHECK (
    length(expires_at) BETWEEN 20 AND 30
      AND substr(expires_at, 5, 1) = '-'
      AND substr(expires_at, 8, 1) = '-'
      AND substr(expires_at, 11, 1) = 'T'
      AND substr(expires_at, 14, 1) = ':'
      AND substr(expires_at, 17, 1) = ':'
      AND substr(expires_at, -1, 1) = 'Z'
      AND strftime('%Y-%m-%dT%H:%M:%S', expires_at) = substr(expires_at, 1, 19)
      AND (
        length(expires_at) = 20
        OR (
          substr(expires_at, 20, 1) = '.'
          AND length(expires_at) >= 22
          AND substr(expires_at, 21, length(expires_at) - 21) NOT GLOB '*[^0-9]*'
        )
      )
  ),
  created_at TEXT NOT NULL CHECK (
    length(created_at) BETWEEN 20 AND 30
      AND substr(created_at, 5, 1) = '-'
      AND substr(created_at, 8, 1) = '-'
      AND substr(created_at, 11, 1) = 'T'
      AND substr(created_at, 14, 1) = ':'
      AND substr(created_at, 17, 1) = ':'
      AND substr(created_at, -1, 1) = 'Z'
      AND strftime('%Y-%m-%dT%H:%M:%S', created_at) = substr(created_at, 1, 19)
      AND (
        length(created_at) = 20
        OR (
          substr(created_at, 20, 1) = '.'
          AND length(created_at) >= 22
          AND substr(created_at, 21, length(created_at) - 21) NOT GLOB '*[^0-9]*'
        )
      )
  ),
  FOREIGN KEY (view_id, manifest_generation)
    REFERENCES manifests(view_id, generation) ON DELETE NO ACTION,
  FOREIGN KEY (base_id) REFERENCES resolution_bases(base_id) ON DELETE NO ACTION,
  FOREIGN KEY (view_id, delta_generation)
    REFERENCES resolution_deltas(view_id, delta_generation) ON DELETE NO ACTION
) STRICT;

CREATE TABLE IF NOT EXISTS store_log (
  sequence INTEGER PRIMARY KEY AUTOINCREMENT,
  request_id TEXT NOT NULL CHECK (length(request_id) > 0),
  event_kind TEXT NOT NULL CHECK (length(event_kind) > 0),
  view_id TEXT,
  generation INTEGER,
  version_id INTEGER,
  level INTEGER CHECK (level IS NULL OR level IN (1, 2, 3)),
  terminal INTEGER NOT NULL DEFAULT 0 CHECK (terminal IN (0, 1)),
  payload_json TEXT NOT NULL,
  created_at TEXT NOT NULL CHECK (
    length(created_at) BETWEEN 20 AND 30
      AND substr(created_at, 5, 1) = '-'
      AND substr(created_at, 8, 1) = '-'
      AND substr(created_at, 11, 1) = 'T'
      AND substr(created_at, 14, 1) = ':'
      AND substr(created_at, 17, 1) = ':'
      AND substr(created_at, -1, 1) = 'Z'
      AND strftime('%Y-%m-%dT%H:%M:%S', created_at) = substr(created_at, 1, 19)
      AND (
        length(created_at) = 20
        OR (
          substr(created_at, 20, 1) = '.'
          AND length(created_at) >= 22
          AND substr(created_at, 21, length(created_at) - 21) NOT GLOB '*[^0-9]*'
        )
      )
  )
) STRICT;

CREATE TABLE IF NOT EXISTS request_chunks (
  request_id TEXT NOT NULL CHECK (length(request_id) > 0),
  chunk_index INTEGER NOT NULL CHECK (chunk_index >= 0),
  store_log_sequence INTEGER NOT NULL,
  level INTEGER CHECK (level IS NULL OR level IN (1, 2, 3)),
  payload_json TEXT NOT NULL,
  created_at TEXT NOT NULL CHECK (
    length(created_at) BETWEEN 20 AND 30
      AND substr(created_at, 5, 1) = '-'
      AND substr(created_at, 8, 1) = '-'
      AND substr(created_at, 11, 1) = 'T'
      AND substr(created_at, 14, 1) = ':'
      AND substr(created_at, 17, 1) = ':'
      AND substr(created_at, -1, 1) = 'Z'
      AND strftime('%Y-%m-%dT%H:%M:%S', created_at) = substr(created_at, 1, 19)
      AND (
        length(created_at) = 20
        OR (
          substr(created_at, 20, 1) = '.'
          AND length(created_at) >= 22
          AND substr(created_at, 21, length(created_at) - 21) NOT GLOB '*[^0-9]*'
        )
      )
  ),
  PRIMARY KEY (request_id, chunk_index)
) STRICT;

CREATE UNIQUE INDEX IF NOT EXISTS uidx_read_file_versions_identity
  ON file_versions(path, content_hash, extraction_epoch);
CREATE INDEX IF NOT EXISTS idx_read_language_capability_gaps_language
  ON language_capability_gaps(extraction_epoch, language);
CREATE INDEX IF NOT EXISTS idx_gc_symbols_path ON symbols(version_id, path);
CREATE INDEX IF NOT EXISTS idx_gc_symbols_is_test ON symbols(version_id, is_test);
CREATE INDEX IF NOT EXISTS idx_gc_symbols_test_container ON symbols(version_id, test_container);
CREATE INDEX IF NOT EXISTS idx_gc_symbols_test_lifecycle ON symbols(version_id, test_lifecycle);
CREATE INDEX IF NOT EXISTS idx_read_symbols_name_kind ON symbols(name, kind, version_id);
CREATE INDEX IF NOT EXISTS idx_read_symbols_parent ON symbols(parent_symbol_id, version_id);
CREATE INDEX IF NOT EXISTS idx_gc_symbol_annotations_symbol
  ON symbol_annotations(version_id, symbol_id);
CREATE INDEX IF NOT EXISTS idx_read_reference_sites_containing_symbol
  ON reference_sites(containing_symbol_id, version_id);
CREATE INDEX IF NOT EXISTS idx_read_identifiers_name_kind
  ON identifiers(name, kind, version_id);
CREATE INDEX IF NOT EXISTS idx_read_identifiers_containing
  ON identifiers(containing_symbol_id, version_id);
CREATE INDEX IF NOT EXISTS idx_read_identifiers_locator_line
  ON identifiers(version_id, name, start_line, identifier_id);
CREATE INDEX IF NOT EXISTS idx_read_identifiers_locator_span
  ON identifiers(version_id, name, start_byte, end_byte, identifier_id);
CREATE INDEX IF NOT EXISTS idx_read_identifiers_reference_site
  ON identifiers(reference_site_id, version_id);
CREATE INDEX IF NOT EXISTS idx_read_relationships_from
  ON relationships(from_symbol_id, version_id);
CREATE INDEX IF NOT EXISTS idx_read_relationships_to
  ON relationships(to_symbol_id, version_id);
CREATE INDEX IF NOT EXISTS idx_read_relationships_kind
  ON relationships(kind, version_id);
CREATE INDEX IF NOT EXISTS idx_read_relationships_reference_site
  ON relationships(reference_site_id, version_id);
CREATE INDEX IF NOT EXISTS idx_read_pending_terminal
  ON pending_relationships(target_terminal_name, version_id);
CREATE INDEX IF NOT EXISTS idx_read_pending_from
  ON pending_relationships(from_symbol_id, version_id);
CREATE INDEX IF NOT EXISTS idx_read_pending_caller_scope
  ON pending_relationships(caller_scope_symbol_id, version_id);
CREATE INDEX IF NOT EXISTS idx_read_pending_reference_site
  ON pending_relationships(reference_site_id, version_id);
CREATE INDEX IF NOT EXISTS idx_read_type_argument_usages_identifier
  ON type_argument_usages(identifier_id, version_id);
CREATE INDEX IF NOT EXISTS idx_gc_type_arguments_usage
  ON type_arguments(version_id, usage_id);
CREATE INDEX IF NOT EXISTS idx_gc_type_arguments_parent
  ON type_arguments(version_id, parent_type_argument_id);
CREATE INDEX IF NOT EXISTS idx_read_literals_containing_symbol
  ON literals(containing_symbol_id, version_id);
CREATE INDEX IF NOT EXISTS idx_gc_source_regions_file_span
  ON source_regions(version_id, start_byte, end_byte);
CREATE INDEX IF NOT EXISTS idx_gc_source_regions_export_order
  ON source_regions(version_id, path, start_byte, end_byte, kind, source_region_id);
CREATE INDEX IF NOT EXISTS idx_read_source_regions_kind
  ON source_regions(kind, version_id, start_byte);
CREATE INDEX IF NOT EXISTS idx_read_source_regions_symbol
  ON source_regions(containing_symbol_id, version_id);
CREATE INDEX IF NOT EXISTS idx_gc_structural_facts_file_span
  ON structural_facts(version_id, start_byte, end_byte);
CREATE INDEX IF NOT EXISTS idx_gc_structural_facts_export_order
  ON structural_facts(version_id, path, start_byte, end_byte, pattern_id, capture_name, structural_fact_id);
CREATE INDEX IF NOT EXISTS idx_read_structural_facts_pattern_language_path
  ON structural_facts(pattern_id, language, path, version_id);
CREATE INDEX IF NOT EXISTS idx_read_structural_facts_symbol
  ON structural_facts(containing_symbol_id, version_id);
CREATE INDEX IF NOT EXISTS idx_gc_complexity_metrics_file_scope
  ON complexity_metrics(version_id, scope, start_byte);
CREATE INDEX IF NOT EXISTS idx_gc_complexity_metrics_export_order
  ON complexity_metrics(version_id, path, start_byte, end_byte, scope, symbol_id, complexity_metric_id);
CREATE INDEX IF NOT EXISTS idx_read_complexity_metrics_scope_language
  ON complexity_metrics(scope, language, path, version_id);
CREATE INDEX IF NOT EXISTS idx_read_complexity_metrics_symbol
  ON complexity_metrics(symbol_id, version_id);
CREATE INDEX IF NOT EXISTS idx_gc_diagnostics_path
  ON parse_diagnostics(version_id, path);
CREATE UNIQUE INDEX IF NOT EXISTS uidx_read_manifests_hash
  ON manifests(view_id, manifest_hash);
CREATE INDEX IF NOT EXISTS idx_read_manifest_entries_version
  ON manifest_entries(version_id, view_id, generation);
CREATE UNIQUE INDEX IF NOT EXISTS uidx_read_resolution_bases_identity
  ON resolution_bases(manifest_hash, resolver_output_epoch);
CREATE INDEX IF NOT EXISTS idx_read_resolution_base_versions_version
  ON resolution_base_versions(version_id, base_id);
CREATE INDEX IF NOT EXISTS idx_read_resolution_deltas_base
  ON resolution_deltas(base_id, view_id, delta_generation);
CREATE INDEX IF NOT EXISTS idx_read_resolution_identifier_deltas_target
  ON resolution_identifier_deltas(target_version_id, target_symbol_id, view_id, delta_generation);
CREATE INDEX IF NOT EXISTS idx_gc_resolution_identifier_deltas_version
  ON resolution_identifier_deltas(version_id, view_id, delta_generation, identifier_id);
CREATE INDEX IF NOT EXISTS idx_read_resolution_pending_deltas_target
  ON resolution_pending_deltas(target_version_id, target_symbol_id, view_id, delta_generation);
CREATE INDEX IF NOT EXISTS idx_gc_resolution_pending_deltas_version
  ON resolution_pending_deltas(version_id, view_id, delta_generation, pending_relationship_id);
CREATE INDEX IF NOT EXISTS idx_read_resolution_pins_owner_expiry
  ON resolution_pins(owner_kind, owner_id, expires_at, pin_id);
CREATE INDEX IF NOT EXISTS idx_read_resolution_pins_bound
  ON resolution_pins(view_id, manifest_generation, base_id, delta_generation);
CREATE UNIQUE INDEX IF NOT EXISTS uidx_read_store_log_terminal_request
  ON store_log(request_id) WHERE terminal = 1;
CREATE INDEX IF NOT EXISTS idx_read_store_log_request
  ON store_log(request_id, sequence);
CREATE UNIQUE INDEX IF NOT EXISTS uidx_read_request_chunks_log_sequence
  ON request_chunks(store_log_sequence);

CREATE TRIGGER IF NOT EXISTS trg_resolution_delta_ready_base_insert
BEFORE INSERT ON resolution_deltas
WHEN NOT EXISTS (
  SELECT 1 FROM resolution_bases AS base
  WHERE base.base_id = NEW.base_id
    AND base.state = 'ready'
    AND base.resolver_output_epoch = NEW.resolver_output_epoch
)
BEGIN
  SELECT RAISE(ABORT, 'resolution delta requires a ready base at the same resolver epoch');
END;

CREATE TRIGGER IF NOT EXISTS trg_resolution_delta_ready_base_update
BEFORE UPDATE OF base_id, resolver_output_epoch ON resolution_deltas
WHEN NOT EXISTS (
  SELECT 1 FROM resolution_bases AS base
  WHERE base.base_id = NEW.base_id
    AND base.state = 'ready'
    AND base.resolver_output_epoch = NEW.resolver_output_epoch
)
BEGIN
  SELECT RAISE(ABORT, 'resolution delta requires a ready base at the same resolver epoch');
END;

CREATE TRIGGER IF NOT EXISTS trg_resolution_delta_manifest_insert
BEFORE INSERT ON resolution_deltas
WHEN NOT EXISTS (
  SELECT 1 FROM manifests AS manifest
  WHERE manifest.view_id = NEW.view_id
    AND manifest.generation = NEW.manifest_generation
    AND manifest.manifest_hash = NEW.manifest_hash
)
BEGIN
  SELECT RAISE(ABORT, 'resolution delta manifest identity is incoherent');
END;

CREATE TRIGGER IF NOT EXISTS trg_resolution_delta_manifest_update
BEFORE UPDATE OF view_id, manifest_generation, manifest_hash ON resolution_deltas
WHEN NOT EXISTS (
  SELECT 1 FROM manifests AS manifest
  WHERE manifest.view_id = NEW.view_id
    AND manifest.generation = NEW.manifest_generation
    AND manifest.manifest_hash = NEW.manifest_hash
)
BEGIN
  SELECT RAISE(ABORT, 'resolution delta manifest identity is incoherent');
END;

CREATE TRIGGER IF NOT EXISTS trg_resolution_base_referenced_ready_update
BEFORE UPDATE OF state, resolver_output_epoch ON resolution_bases
WHEN EXISTS (
  SELECT 1 FROM resolution_deltas AS delta
  WHERE delta.base_id = OLD.base_id
    AND (NEW.state <> 'ready' OR NEW.resolver_output_epoch <> delta.resolver_output_epoch)
)
BEGIN
  SELECT RAISE(ABORT, 'referenced resolution base must remain ready at its published epoch');
END;

CREATE TRIGGER IF NOT EXISTS trg_resolution_pin_tuple_insert
BEFORE INSERT ON resolution_pins
WHEN NEW.delta_generation IS NOT NULL AND NOT EXISTS (
  SELECT 1 FROM resolution_deltas AS delta
  WHERE delta.view_id = NEW.view_id
    AND delta.delta_generation = NEW.delta_generation
    AND delta.base_id = NEW.base_id
    AND delta.manifest_generation = NEW.manifest_generation
)
BEGIN
  SELECT RAISE(ABORT, 'resolution pin delta does not match its bound tuple');
END;

CREATE TRIGGER IF NOT EXISTS trg_resolution_pin_tuple_update
BEFORE UPDATE OF view_id, manifest_generation, base_id, delta_generation ON resolution_pins
WHEN NEW.delta_generation IS NOT NULL AND NOT EXISTS (
  SELECT 1 FROM resolution_deltas AS delta
  WHERE delta.view_id = NEW.view_id
    AND delta.delta_generation = NEW.delta_generation
    AND delta.base_id = NEW.base_id
    AND delta.manifest_generation = NEW.manifest_generation
)
BEGIN
  SELECT RAISE(ABORT, 'resolution pin delta does not match its bound tuple');
END;

CREATE TRIGGER IF NOT EXISTS trg_view_resolution_tuple_insert
BEFORE INSERT ON views
WHEN NEW.resolution_state <> 'unbound' AND NOT EXISTS (
  SELECT 1 FROM resolution_deltas AS delta
  JOIN resolution_bases AS base ON base.base_id = delta.base_id
  WHERE delta.view_id = NEW.view_id
    AND delta.delta_generation = NEW.resolution_delta_generation
    AND delta.base_id = NEW.resolution_base_id
    AND delta.manifest_generation = NEW.current_generation
    AND base.state = 'ready'
)
BEGIN
  SELECT RAISE(ABORT, 'view resolution binding is incoherent');
END;

CREATE TRIGGER IF NOT EXISTS trg_view_resolution_tuple_update
BEFORE UPDATE OF current_generation, resolution_state, resolution_base_id, resolution_delta_generation ON views
WHEN NEW.resolution_state <> 'unbound' AND NOT EXISTS (
  SELECT 1 FROM resolution_deltas AS delta
  JOIN resolution_bases AS base ON base.base_id = delta.base_id
  WHERE delta.view_id = NEW.view_id
    AND delta.delta_generation = NEW.resolution_delta_generation
    AND delta.base_id = NEW.resolution_base_id
    AND delta.manifest_generation = NEW.current_generation
    AND base.state = 'ready'
)
BEGIN
  SELECT RAISE(ABORT, 'view resolution binding is incoherent');
END;

PRAGMA user_version = 2;
COMMIT;
"#;

const COORDINATOR_SCHEMA_SQL: &str = r#"
BEGIN IMMEDIATE;

CREATE TABLE IF NOT EXISTS requests (
  request_id TEXT PRIMARY KEY CHECK (length(request_id) > 0),
  idempotency_key TEXT NOT NULL CHECK (length(idempotency_key) > 0),
  kind TEXT NOT NULL CHECK (kind IN ('import', 'update', 'delete', 'resolve', 'export', 'from_artifact')),
  payload_json TEXT NOT NULL,
  state TEXT NOT NULL CHECK (state IN ('queued', 'claimed', 'committed', 'acknowledged', 'failed')),
  requester_id TEXT NOT NULL CHECK (length(requester_id) > 0),
  requester_deadline INTEGER,
  claim_owner TEXT,
  claim_heartbeat_at INTEGER,
  terminal_log_sequence INTEGER,
  result_json TEXT,
  error_json TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  CHECK (
    (state = 'claimed'
      AND claim_owner IS NOT NULL
      AND length(claim_owner) > 0
      AND claim_heartbeat_at IS NOT NULL)
    OR
    (state <> 'claimed'
      AND claim_owner IS NULL
      AND claim_heartbeat_at IS NULL)
  ),
  CHECK (
    (state IN ('queued', 'claimed')
      AND terminal_log_sequence IS NULL
      AND result_json IS NULL
      AND error_json IS NULL)
    OR
    (state IN ('committed', 'acknowledged')
      AND terminal_log_sequence IS NOT NULL
      AND result_json IS NOT NULL
      AND error_json IS NULL)
    OR
    (state = 'failed'
      AND result_json IS NULL
      AND error_json IS NOT NULL)
  )
) STRICT;

CREATE TABLE IF NOT EXISTS writer_lease (
  resource TEXT PRIMARY KEY CHECK (resource = 'store-writer'),
  holder_id TEXT NOT NULL CHECK (length(holder_id) > 0),
  holder_version TEXT NOT NULL CHECK (length(holder_version) > 0),
  holder_pid INTEGER NOT NULL CHECK (holder_pid > 0),
  heartbeat_at INTEGER NOT NULL,
  expires_at INTEGER NOT NULL,
  fencing_token INTEGER NOT NULL CHECK (fencing_token > 0)
) STRICT;

CREATE TABLE IF NOT EXISTS request_receipts (
  request_id TEXT PRIMARY KEY CHECK (length(request_id) BETWEEN 1 AND 128),
  idempotency_key TEXT NOT NULL UNIQUE CHECK (length(idempotency_key) BETWEEN 1 AND 128),
  kind TEXT NOT NULL CHECK (kind IN ('import', 'update', 'delete', 'resolve', 'export', 'from_artifact')),
  payload_json TEXT NOT NULL CHECK (json_valid(payload_json)),
  terminal_result_json TEXT NOT NULL CHECK (json_valid(terminal_result_json)),
  terminal_generation_name TEXT NOT NULL CHECK (length(terminal_generation_name) BETWEEN 1 AND 128),
  terminal_log_sequence INTEGER NOT NULL UNIQUE CHECK (terminal_log_sequence > 0),
  completed_at INTEGER NOT NULL CHECK (completed_at >= 0)
) STRICT;

CREATE TABLE IF NOT EXISTS consumer_cursors (
  consumer_id TEXT PRIMARY KEY CHECK (length(consumer_id) BETWEEN 1 AND 128),
  generation_name TEXT NOT NULL CHECK (length(generation_name) BETWEEN 1 AND 128),
  store_log_sequence INTEGER NOT NULL CHECK (store_log_sequence >= 0),
  updated_at INTEGER NOT NULL CHECK (updated_at >= 0)
) STRICT;

CREATE TABLE IF NOT EXISTS maintenance_intent (
  resource TEXT PRIMARY KEY CHECK (resource = 'store-maintenance'),
  run_id TEXT NOT NULL UNIQUE CHECK (length(run_id) BETWEEN 1 AND 128),
  action TEXT NOT NULL CHECK (action IN ('gc', 'repair', 'promote', 'rollback')),
  source_generation_name TEXT NOT NULL CHECK (length(source_generation_name) BETWEEN 1 AND 128),
  owner_id TEXT NOT NULL CHECK (length(owner_id) BETWEEN 1 AND 128),
  owner_pid INTEGER NOT NULL CHECK (owner_pid > 0),
  fencing_token INTEGER NOT NULL CHECK (fencing_token > 0),
  heartbeat_at INTEGER NOT NULL CHECK (heartbeat_at >= 0),
  expires_at INTEGER NOT NULL CHECK (expires_at > heartbeat_at),
  started_at INTEGER NOT NULL CHECK (started_at >= 0 AND started_at <= heartbeat_at),
  plan_fingerprint TEXT NOT NULL CHECK (length(plan_fingerprint) > 0),
  source_min_writer_version TEXT NOT NULL CHECK (length(source_min_writer_version) > 0)
) STRICT;

CREATE TABLE IF NOT EXISTS family_allocator_marks (
  allocator_kind TEXT NOT NULL CHECK (
    allocator_kind IN (
      'file_version',
      'store_log',
      'manifest_generation',
      'resolution_delta_generation'
    )
  ),
  scope_id TEXT NOT NULL,
  high_water INTEGER NOT NULL CHECK (high_water >= 0),
  updated_at INTEGER NOT NULL CHECK (updated_at >= 0),
  PRIMARY KEY (allocator_kind, scope_id),
  CHECK (
    (allocator_kind IN ('file_version', 'store_log') AND scope_id = '')
    OR
    (allocator_kind IN ('manifest_generation', 'resolution_delta_generation') AND length(scope_id) > 0)
  )
) STRICT;

CREATE TRIGGER IF NOT EXISTS trg_request_receipts_immutable_update
BEFORE UPDATE ON request_receipts
BEGIN
  SELECT RAISE(ABORT, 'request receipts are immutable');
END;

CREATE TRIGGER IF NOT EXISTS trg_request_receipts_immutable_delete
BEFORE DELETE ON request_receipts
BEGIN
  SELECT RAISE(ABORT, 'request receipts are immutable');
END;

CREATE TRIGGER IF NOT EXISTS trg_consumer_cursors_monotonic
BEFORE UPDATE ON consumer_cursors
WHEN NEW.store_log_sequence < OLD.store_log_sequence OR NEW.updated_at < OLD.updated_at
BEGIN
  SELECT RAISE(ABORT, 'consumer cursor cannot regress');
END;

CREATE TRIGGER IF NOT EXISTS trg_maintenance_intent_coherent_update
BEFORE UPDATE ON maintenance_intent
WHEN NEW.run_id <> OLD.run_id
  OR NEW.action <> OLD.action
  OR NEW.source_generation_name <> OLD.source_generation_name
  OR NEW.started_at <> OLD.started_at
  OR NEW.plan_fingerprint <> OLD.plan_fingerprint
  OR NEW.source_min_writer_version <> OLD.source_min_writer_version
  OR NEW.fencing_token < OLD.fencing_token
  OR NEW.heartbeat_at < OLD.heartbeat_at
  OR (
    (NEW.owner_id <> OLD.owner_id OR NEW.owner_pid <> OLD.owner_pid)
    AND NEW.fencing_token <= OLD.fencing_token
  )
BEGIN
  SELECT RAISE(ABORT, 'maintenance intent cannot regress or change identity');
END;

CREATE TRIGGER IF NOT EXISTS trg_family_allocator_marks_monotonic
BEFORE UPDATE ON family_allocator_marks
WHEN NEW.allocator_kind <> OLD.allocator_kind
  OR NEW.scope_id <> OLD.scope_id
  OR NEW.high_water < OLD.high_water
  OR NEW.updated_at < OLD.updated_at
BEGIN
  SELECT RAISE(ABORT, 'family allocator mark cannot regress');
END;

CREATE UNIQUE INDEX IF NOT EXISTS uidx_read_requests_idempotency_key
  ON requests(idempotency_key);
CREATE INDEX IF NOT EXISTS idx_read_requests_queue
  ON requests(state, created_at, request_id);
CREATE INDEX IF NOT EXISTS idx_read_requests_stale
  ON requests(state, claim_heartbeat_at, request_id);
CREATE UNIQUE INDEX IF NOT EXISTS uidx_coord_one_claimed_resolve
  ON requests(kind) WHERE kind = 'resolve' AND state = 'claimed';

PRAGMA user_version = 2;
COMMIT;
"#;
