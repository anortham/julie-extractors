use std::error::Error;
use std::fmt;
use std::path::{Component, Path, PathBuf};

use julie_extract_artifact::store::{
    ResolutionBaseReader, ResolutionIdentifierRow, ResolutionPendingRow, ResolutionScopeState,
    ResolutionValidatedBase, StoreLayout,
};
use rusqlite::types::Value;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params, params_from_iter};

const PRIOR_OVERLAY_MAX_FILTER_VALUES: usize = 256;
const PRIOR_OVERLAY_MAX_WINDOW: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PriorOverlayKey {
    pub(crate) version_id: i64,
    pub(crate) local_id: String,
}

impl PriorOverlayKey {
    pub(crate) fn new(version_id: i64, local_id: impl Into<String>) -> Self {
        Self {
            version_id,
            local_id: local_id.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PriorOverlayPage<T> {
    pub(crate) rows: Vec<T>,
    pub(crate) next: Option<PriorOverlayKey>,
}

#[derive(Debug)]
pub(crate) enum PriorOverlayAccess<T> {
    Ready(T),
    FullFallback(PriorOverlayFallback),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PriorOverlayFallback {
    ScopeStateMissing {
        view_id: String,
    },
    ScopeStateChanged {
        view_id: String,
    },
    BaseCatalogMissing {
        base_id: String,
    },
    BaseCatalogIncoherent {
        base_id: String,
        detail: String,
    },
    BaseFileMissing {
        path: PathBuf,
    },
    BaseFileIncoherent {
        path: PathBuf,
        detail: String,
    },
    DeltaMissing {
        view_id: String,
        delta_generation: i64,
    },
    DeltaIncoherent {
        view_id: String,
        delta_generation: i64,
        detail: String,
    },
    DeltaRowCountMismatch {
        table: &'static str,
        expected: i64,
        found: i64,
    },
    SourceRowMissing {
        table: &'static str,
        version_id: i64,
        local_id: String,
    },
    OverlayRowMissing {
        table: &'static str,
        version_id: i64,
        local_id: String,
    },
}

#[derive(Debug)]
pub(crate) enum PriorOverlayError {
    InvalidArgument(&'static str),
    Sqlite(rusqlite::Error),
}

impl fmt::Display for PriorOverlayError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidArgument(argument) => write!(formatter, "invalid {argument}"),
            Self::Sqlite(error) => error.fmt(formatter),
        }
    }
}

impl Error for PriorOverlayError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidArgument(_) => None,
            Self::Sqlite(error) => Some(error),
        }
    }
}

impl From<rusqlite::Error> for PriorOverlayError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

#[derive(Debug)]
pub(crate) struct PriorOverlayReader {
    connection: Connection,
    state: ResolutionScopeState,
}

#[derive(Debug)]
struct BaseCatalogRow {
    manifest_hash: String,
    resolver_output_epoch: i64,
    state: String,
    relative_path: String,
    identifier_count: i64,
    pending_count: i64,
    file_bytes: Option<i64>,
    file_sha256: Option<String>,
}

#[derive(Debug)]
struct DeltaCatalogRow {
    base_id: String,
    manifest_generation: i64,
    manifest_hash: String,
    resolver_output_epoch: i64,
    identifier_replacements: i64,
    pending_replacements: i64,
    pending_tombstones: i64,
}

impl PriorOverlayReader {
    pub(crate) fn open(
        layout: &StoreLayout,
        state: &ResolutionScopeState,
    ) -> Result<PriorOverlayAccess<Self>, PriorOverlayError> {
        Self::open_inner(layout, state, None)
    }

    pub(crate) fn open_with_validated_base(
        layout: &StoreLayout,
        state: &ResolutionScopeState,
        proof: &ResolutionValidatedBase,
    ) -> Result<PriorOverlayAccess<Self>, PriorOverlayError> {
        Self::open_inner(layout, state, Some(proof))
    }

    fn open_inner(
        layout: &StoreLayout,
        state: &ResolutionScopeState,
        proof: Option<&ResolutionValidatedBase>,
    ) -> Result<PriorOverlayAccess<Self>, PriorOverlayError> {
        let connection = Connection::open_with_flags(
            layout.store_db(),
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        connection.execute_batch("PRAGMA foreign_keys=ON; PRAGMA query_only=ON; BEGIN;")?;

        let stored_state = read_scope_state(&connection, &state.view_id)?;
        match stored_state {
            None => {
                return Ok(PriorOverlayAccess::FullFallback(
                    PriorOverlayFallback::ScopeStateMissing {
                        view_id: state.view_id.clone(),
                    },
                ));
            }
            Some(stored) if stored != *state => {
                return Ok(PriorOverlayAccess::FullFallback(
                    PriorOverlayFallback::ScopeStateChanged {
                        view_id: state.view_id.clone(),
                    },
                ));
            }
            Some(_) => {}
        }

        let Some(base_catalog) = read_base_catalog(&connection, &state.base_id)? else {
            return Ok(PriorOverlayAccess::FullFallback(
                PriorOverlayFallback::BaseCatalogMissing {
                    base_id: state.base_id.clone(),
                },
            ));
        };
        if base_catalog.state != "ready"
            || base_catalog.resolver_output_epoch != state.resolver_output_epoch
            || base_catalog.identifier_count < 0
            || base_catalog.pending_count < 0
            || base_catalog.file_bytes.is_none()
            || base_catalog
                .file_sha256
                .as_deref()
                .is_none_or(str::is_empty)
        {
            return Ok(PriorOverlayAccess::FullFallback(
                PriorOverlayFallback::BaseCatalogIncoherent {
                    base_id: state.base_id.clone(),
                    detail: "ready base identity is incomplete".to_string(),
                },
            ));
        }
        let expected_relative_path = format!("bases/{}.db", state.base_id);
        let Some(relative_path) = safe_base_relative_path(&base_catalog.relative_path) else {
            return Ok(PriorOverlayAccess::FullFallback(
                PriorOverlayFallback::BaseCatalogIncoherent {
                    base_id: state.base_id.clone(),
                    detail: format!("unsafe relative path {:?}", base_catalog.relative_path),
                },
            ));
        };
        if base_catalog.relative_path != expected_relative_path {
            return Ok(PriorOverlayAccess::FullFallback(
                PriorOverlayFallback::BaseCatalogIncoherent {
                    base_id: state.base_id.clone(),
                    detail: format!(
                        "relative path {:?} does not match {:?}",
                        base_catalog.relative_path, expected_relative_path
                    ),
                },
            ));
        }
        let base_path = layout.generation_dir().join(relative_path);
        if !base_path.is_file() {
            return Ok(PriorOverlayAccess::FullFallback(
                PriorOverlayFallback::BaseFileMissing { path: base_path },
            ));
        }
        let proof_matches = proof.is_some_and(|proof| {
            proof.matches_catalog_identity(
                &state.base_id,
                &base_catalog.manifest_hash,
                base_catalog.resolver_output_epoch,
                &base_catalog.state,
                &base_catalog.relative_path,
            ) && proof.matches_catalog_file(
                base_catalog.identifier_count,
                base_catalog.pending_count,
                base_catalog.file_bytes,
                base_catalog.file_sha256.as_deref(),
            )
        });
        let base = if proof_matches {
            None
        } else {
            let base = match ResolutionBaseReader::open(&base_path) {
                Ok(base) => base,
                Err(error) => {
                    return Ok(PriorOverlayAccess::FullFallback(
                        PriorOverlayFallback::BaseFileIncoherent {
                            path: base_path,
                            detail: error.to_string(),
                        },
                    ));
                }
            };
            let base_identity = base.file_identity();
            if base_identity.manifest_hash != base_catalog.manifest_hash
                || base_identity.resolver_output_epoch != base_catalog.resolver_output_epoch
                || i64::try_from(base_identity.counts.identifiers).ok()
                    != Some(base_catalog.identifier_count)
                || i64::try_from(base_identity.counts.pending).ok()
                    != Some(base_catalog.pending_count)
                || i64::try_from(base_identity.file_bytes).ok() != base_catalog.file_bytes
                || Some(base_identity.file_sha256.as_str()) != base_catalog.file_sha256.as_deref()
            {
                return Ok(PriorOverlayAccess::FullFallback(
                    PriorOverlayFallback::BaseFileIncoherent {
                        path: base_path,
                        detail: "file identity does not match the ready catalog row".to_string(),
                    },
                ));
            }
            Some(base)
        };

        let Some(delta) = read_delta_catalog(&connection, state)? else {
            return Ok(PriorOverlayAccess::FullFallback(
                PriorOverlayFallback::DeltaMissing {
                    view_id: state.view_id.clone(),
                    delta_generation: state.delta_generation,
                },
            ));
        };
        if delta.base_id != state.base_id
            || delta.manifest_generation != state.predecessor_manifest_generation
            || delta.manifest_hash != state.predecessor_manifest_hash
            || delta.resolver_output_epoch != state.resolver_output_epoch
        {
            return Ok(PriorOverlayAccess::FullFallback(
                PriorOverlayFallback::DeltaIncoherent {
                    view_id: state.view_id.clone(),
                    delta_generation: state.delta_generation,
                    detail: "delta identity does not match the preserved predecessor".to_string(),
                },
            ));
        }
        for (table, operation, expected) in [
            (
                "resolution_identifier_deltas",
                None,
                delta.identifier_replacements,
            ),
            (
                "resolution_pending_deltas",
                Some("replace"),
                delta.pending_replacements,
            ),
            (
                "resolution_pending_deltas",
                Some("tombstone"),
                delta.pending_tombstones,
            ),
        ] {
            let found = delta_row_count(&connection, state, table, operation)?;
            if found != expected {
                return Ok(PriorOverlayAccess::FullFallback(
                    PriorOverlayFallback::DeltaRowCountMismatch {
                        table,
                        expected,
                        found,
                    },
                ));
            }
        }

        connection.execute(
            "ATTACH DATABASE ?1 AS prior_base",
            [base_path.to_string_lossy().as_ref()],
        )?;
        drop(base);

        if let Some(detail) = base_root_set_mismatch(&connection, &state.base_id)? {
            return Ok(PriorOverlayAccess::FullFallback(
                PriorOverlayFallback::BaseCatalogIncoherent {
                    base_id: state.base_id.clone(),
                    detail,
                },
            ));
        }
        if let Some(fallback) = first_incoherent_row(&connection, state)? {
            return Ok(PriorOverlayAccess::FullFallback(fallback));
        }
        Ok(PriorOverlayAccess::Ready(Self {
            connection,
            state: state.clone(),
        }))
    }

    pub(crate) fn identifier(
        &self,
        version_id: i64,
        identifier_id: &str,
    ) -> Result<PriorOverlayAccess<Option<ResolutionIdentifierRow>>, PriorOverlayError> {
        validate_key(version_id, identifier_id)?;
        if !self.identifier_belongs_to_predecessor(version_id, identifier_id)? {
            return Ok(PriorOverlayAccess::Ready(None));
        }
        let row = self
            .connection
            .query_row(
                "SELECT COALESCE(delta.version_id,base.version_id),
                        COALESCE(delta.identifier_id,base.identifier_id),
                        CASE WHEN delta.version_id IS NOT NULL THEN delta.target_version_id ELSE base.target_version_id END,
                        CASE WHEN delta.version_id IS NOT NULL THEN delta.target_symbol_id ELSE base.target_symbol_id END,
                        CASE WHEN delta.version_id IS NOT NULL THEN delta.tier ELSE base.tier END,
                        CASE WHEN delta.version_id IS NOT NULL THEN delta.confidence ELSE base.confidence END,
                        CASE WHEN delta.version_id IS NOT NULL THEN delta.method ELSE base.method END,
                        CASE WHEN delta.version_id IS NOT NULL THEN delta.outcome ELSE base.outcome END,
                        CASE WHEN delta.version_id IS NOT NULL THEN delta.candidates ELSE base.candidates END
                 FROM identifiers AS source
                 LEFT JOIN prior_base.identifier_resolutions AS base
                   ON base.version_id=source.version_id AND base.identifier_id=source.identifier_id
                 LEFT JOIN resolution_identifier_deltas AS delta
                   ON delta.view_id=?1 AND delta.delta_generation=?2
                  AND delta.version_id=source.version_id AND delta.identifier_id=source.identifier_id
                 WHERE source.version_id=?3 AND source.identifier_id=?4",
                params![
                    self.state.view_id,
                    self.state.delta_generation,
                    version_id,
                    identifier_id
                ],
                map_identifier_row,
            )
            .optional()?;
        match row {
            Some(row) => Ok(PriorOverlayAccess::Ready(Some(row))),
            None => Ok(PriorOverlayAccess::FullFallback(
                PriorOverlayFallback::OverlayRowMissing {
                    table: "identifier_resolutions",
                    version_id,
                    local_id: identifier_id.to_string(),
                },
            )),
        }
    }

    pub(crate) fn pending(
        &self,
        version_id: i64,
        pending_relationship_id: &str,
    ) -> Result<PriorOverlayAccess<Option<ResolutionPendingRow>>, PriorOverlayError> {
        validate_key(version_id, pending_relationship_id)?;
        if !self.pending_belongs_to_predecessor(version_id, pending_relationship_id)? {
            return Ok(PriorOverlayAccess::Ready(None));
        }
        let change = self
            .connection
            .query_row(
                "SELECT operation,target_version_id,target_symbol_id,tier,confidence,method
                 FROM resolution_pending_deltas
                 WHERE view_id=?1 AND delta_generation=?2 AND version_id=?3
                   AND pending_relationship_id=?4",
                params![
                    self.state.view_id,
                    self.state.delta_generation,
                    version_id,
                    pending_relationship_id
                ],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<i64>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<i64>>(3)?,
                        row.get::<_, Option<f64>>(4)?,
                        row.get::<_, Option<String>>(5)?,
                    ))
                },
            )
            .optional()?;
        if let Some((operation, target_version, target_symbol, tier, confidence, method)) = change {
            return match operation.as_str() {
                "tombstone" => Ok(PriorOverlayAccess::Ready(None)),
                "replace" => Ok(PriorOverlayAccess::Ready(Some(ResolutionPendingRow {
                    version_id,
                    pending_relationship_id: pending_relationship_id.to_string(),
                    target_version_id: target_version.ok_or(rusqlite::Error::InvalidQuery)?,
                    target_symbol_id: target_symbol.ok_or(rusqlite::Error::InvalidQuery)?,
                    tier: tier.ok_or(rusqlite::Error::InvalidQuery)?,
                    confidence: confidence.ok_or(rusqlite::Error::InvalidQuery)?,
                    method: method.ok_or(rusqlite::Error::InvalidQuery)?,
                }))),
                _ => Err(rusqlite::Error::InvalidQuery.into()),
            };
        }
        let base = self
            .connection
            .query_row(
                "SELECT version_id,pending_relationship_id,target_version_id,target_symbol_id,
                        tier,confidence,method
                 FROM prior_base.pending_resolutions
                 WHERE version_id=?1 AND pending_relationship_id=?2",
                params![version_id, pending_relationship_id],
                map_pending_row,
            )
            .optional()?;
        Ok(PriorOverlayAccess::Ready(base))
    }

    pub(crate) fn identifiers_by_names(
        &self,
        names: &[&str],
        after: Option<&PriorOverlayKey>,
        limit: usize,
    ) -> Result<PriorOverlayAccess<PriorOverlayPage<ResolutionIdentifierRow>>, PriorOverlayError>
    {
        self.identifier_page(Filter::Names(names), after, limit)
    }

    pub(crate) fn identifiers_by_files(
        &self,
        version_ids: &[i64],
        after: Option<&PriorOverlayKey>,
        limit: usize,
    ) -> Result<PriorOverlayAccess<PriorOverlayPage<ResolutionIdentifierRow>>, PriorOverlayError>
    {
        self.identifier_page(Filter::Versions(version_ids), after, limit)
    }

    pub(crate) fn pending_by_names(
        &self,
        names: &[&str],
        after: Option<&PriorOverlayKey>,
        limit: usize,
    ) -> Result<PriorOverlayAccess<PriorOverlayPage<ResolutionPendingRow>>, PriorOverlayError> {
        self.pending_page(Filter::Names(names), after, limit)
    }

    pub(crate) fn pending_by_files(
        &self,
        version_ids: &[i64],
        after: Option<&PriorOverlayKey>,
        limit: usize,
    ) -> Result<PriorOverlayAccess<PriorOverlayPage<ResolutionPendingRow>>, PriorOverlayError> {
        self.pending_page(Filter::Versions(version_ids), after, limit)
    }

    pub(crate) fn pending_by_keys(
        &self,
        keys: &[PriorOverlayKey],
    ) -> Result<PriorOverlayAccess<Vec<ResolutionPendingRow>>, PriorOverlayError> {
        self.pending_keys(keys)
    }

    fn identifier_page(
        &self,
        filter: Filter<'_>,
        after: Option<&PriorOverlayKey>,
        limit: usize,
    ) -> Result<PriorOverlayAccess<PriorOverlayPage<ResolutionIdentifierRow>>, PriorOverlayError>
    {
        let (predicate, filter_values, index) = filter.identifier_predicate()?;
        let (after_version, after_id) = cursor(after)?;
        let query_limit = checked_query_limit(limit)?;
        if filter_values.is_empty() {
            return Ok(PriorOverlayAccess::Ready(PriorOverlayPage {
                rows: Vec::new(),
                next: None,
            }));
        }
        let sql = format!(
            "SELECT source.version_id,source.identifier_id,
                    delta.version_id,base.version_id,
                    CASE WHEN delta.version_id IS NOT NULL THEN delta.target_version_id ELSE base.target_version_id END,
                    CASE WHEN delta.version_id IS NOT NULL THEN delta.target_symbol_id ELSE base.target_symbol_id END,
                    CASE WHEN delta.version_id IS NOT NULL THEN delta.tier ELSE base.tier END,
                    CASE WHEN delta.version_id IS NOT NULL THEN delta.confidence ELSE base.confidence END,
                    CASE WHEN delta.version_id IS NOT NULL THEN delta.method ELSE base.method END,
                    CASE WHEN delta.version_id IS NOT NULL THEN delta.outcome ELSE base.outcome END,
                    CASE WHEN delta.version_id IS NOT NULL THEN delta.candidates ELSE base.candidates END
             FROM identifiers AS source {index}
             JOIN manifest_entries AS manifest
               ON manifest.view_id=? AND manifest.generation=?
              AND manifest.version_id=source.version_id
             LEFT JOIN prior_base.identifier_resolutions AS base
               ON base.version_id=source.version_id AND base.identifier_id=source.identifier_id
             LEFT JOIN resolution_identifier_deltas AS delta
               ON delta.view_id=? AND delta.delta_generation=?
              AND delta.version_id=source.version_id AND delta.identifier_id=source.identifier_id
             WHERE {predicate} AND (source.version_id,source.identifier_id)>(?,?)
             ORDER BY source.version_id,source.identifier_id LIMIT ?"
        );
        let mut values = vec![
            Value::Text(self.state.view_id.clone()),
            Value::Integer(self.state.predecessor_manifest_generation),
            Value::Text(self.state.view_id.clone()),
            Value::Integer(self.state.delta_generation),
        ];
        values.extend(filter_values);
        values.extend([
            Value::Integer(after_version),
            Value::Text(after_id.to_string()),
            Value::Integer(query_limit),
        ]);
        let mut statement = self.connection.prepare(&sql)?;
        let mut query = statement.query(params_from_iter(values.iter()))?;
        let mut rows = Vec::new();
        let mut scanned = Vec::new();
        while let Some(row) = query.next()? {
            let version_id = row.get::<_, i64>(0)?;
            let identifier_id = row.get::<_, String>(1)?;
            scanned.push(PriorOverlayKey::new(version_id, identifier_id.clone()));
            if row.get::<_, Option<i64>>(2)?.is_none() && row.get::<_, Option<i64>>(3)?.is_none() {
                return Ok(PriorOverlayAccess::FullFallback(
                    PriorOverlayFallback::OverlayRowMissing {
                        table: "identifier_resolutions",
                        version_id,
                        local_id: identifier_id,
                    },
                ));
            }
            rows.push(ResolutionIdentifierRow {
                version_id,
                identifier_id,
                target_version_id: row.get(4)?,
                target_symbol_id: row.get(5)?,
                tier: row.get(6)?,
                confidence: row.get(7)?,
                method: row.get(8)?,
                outcome: row.get(9)?,
                candidates: row.get(10)?,
            });
        }
        let next = page_next(&scanned, limit);
        rows.truncate(limit);
        Ok(PriorOverlayAccess::Ready(PriorOverlayPage { rows, next }))
    }

    fn pending_page(
        &self,
        filter: Filter<'_>,
        after: Option<&PriorOverlayKey>,
        limit: usize,
    ) -> Result<PriorOverlayAccess<PriorOverlayPage<ResolutionPendingRow>>, PriorOverlayError> {
        let (predicate, filter_values, index) = filter.pending_predicate()?;
        let (after_version, after_id) = cursor(after)?;
        let query_limit = checked_query_limit(limit)?;
        if filter_values.is_empty() {
            return Ok(PriorOverlayAccess::Ready(PriorOverlayPage {
                rows: Vec::new(),
                next: None,
            }));
        }
        let sql = format!(
            "SELECT source.version_id,source.pending_relationship_id,
                    delta.operation,
                    CASE WHEN delta.operation='replace' THEN delta.target_version_id ELSE base.target_version_id END,
                    CASE WHEN delta.operation='replace' THEN delta.target_symbol_id ELSE base.target_symbol_id END,
                    CASE WHEN delta.operation='replace' THEN delta.tier ELSE base.tier END,
                    CASE WHEN delta.operation='replace' THEN delta.confidence ELSE base.confidence END,
                    CASE WHEN delta.operation='replace' THEN delta.method ELSE base.method END
             FROM pending_relationships AS source {index}
             JOIN manifest_entries AS manifest
               ON manifest.view_id=? AND manifest.generation=?
              AND manifest.version_id=source.version_id
             LEFT JOIN prior_base.pending_resolutions AS base
               ON base.version_id=source.version_id
              AND base.pending_relationship_id=source.pending_relationship_id
             LEFT JOIN resolution_pending_deltas AS delta
               ON delta.view_id=? AND delta.delta_generation=?
              AND delta.version_id=source.version_id
              AND delta.pending_relationship_id=source.pending_relationship_id
             WHERE {predicate}
               AND (delta.operation='replace'
                    OR (delta.operation IS NULL AND base.version_id IS NOT NULL))
               AND (source.version_id,source.pending_relationship_id)>(?,?)
             ORDER BY source.version_id,source.pending_relationship_id LIMIT ?"
        );
        let mut values = vec![
            Value::Text(self.state.view_id.clone()),
            Value::Integer(self.state.predecessor_manifest_generation),
            Value::Text(self.state.view_id.clone()),
            Value::Integer(self.state.delta_generation),
        ];
        values.extend(filter_values);
        values.extend([
            Value::Integer(after_version),
            Value::Text(after_id.to_string()),
            Value::Integer(query_limit),
        ]);
        let mut statement = self.connection.prepare(&sql)?;
        let mut query = statement.query(params_from_iter(values.iter()))?;
        let mut rows = Vec::new();
        let mut scanned = Vec::new();
        while let Some(row) = query.next()? {
            let version_id = row.get::<_, i64>(0)?;
            let pending_id = row.get::<_, String>(1)?;
            scanned.push(PriorOverlayKey::new(version_id, pending_id.clone()));
            rows.push(ResolutionPendingRow {
                version_id,
                pending_relationship_id: pending_id,
                target_version_id: row.get(3)?,
                target_symbol_id: row.get(4)?,
                tier: row.get(5)?,
                confidence: row.get(6)?,
                method: row.get(7)?,
            });
        }
        let next = page_next(&scanned, limit);
        rows.truncate(limit);
        Ok(PriorOverlayAccess::Ready(PriorOverlayPage { rows, next }))
    }

    fn pending_keys(
        &self,
        keys: &[PriorOverlayKey],
    ) -> Result<PriorOverlayAccess<Vec<ResolutionPendingRow>>, PriorOverlayError> {
        if keys.is_empty() {
            return Ok(PriorOverlayAccess::Ready(Vec::new()));
        }
        for key in keys {
            validate_key(key.version_id, &key.local_id)?;
        }
        let values = key_values_clause(keys.len())?;
        let sql = format!(
            "WITH wanted(version_id,local_id) AS (VALUES {values})
             SELECT source.version_id,source.pending_relationship_id,
                    delta.operation,
                    CASE WHEN delta.operation='replace' THEN delta.target_version_id ELSE base.target_version_id END,
                    CASE WHEN delta.operation='replace' THEN delta.target_symbol_id ELSE base.target_symbol_id END,
                    CASE WHEN delta.operation='replace' THEN delta.tier ELSE base.tier END,
                    CASE WHEN delta.operation='replace' THEN delta.confidence ELSE base.confidence END,
                    CASE WHEN delta.operation='replace' THEN delta.method ELSE base.method END
             FROM wanted
             JOIN pending_relationships AS source
               ON source.version_id=wanted.version_id
              AND source.pending_relationship_id=wanted.local_id
             JOIN manifest_entries AS manifest
               ON manifest.view_id=? AND manifest.generation=?
              AND manifest.version_id=source.version_id
             LEFT JOIN prior_base.pending_resolutions AS base
               ON base.version_id=source.version_id
              AND base.pending_relationship_id=source.pending_relationship_id
             LEFT JOIN resolution_pending_deltas AS delta
               ON delta.view_id=? AND delta.delta_generation=?
              AND delta.version_id=source.version_id
              AND delta.pending_relationship_id=source.pending_relationship_id
             WHERE delta.operation='replace'
                OR (delta.operation IS NULL AND base.version_id IS NOT NULL)
             ORDER BY source.version_id,source.pending_relationship_id COLLATE BINARY"
        );
        let mut values = key_params(keys);
        values.extend([
            Value::Text(self.state.view_id.clone()),
            Value::Integer(self.state.predecessor_manifest_generation),
            Value::Text(self.state.view_id.clone()),
            Value::Integer(self.state.delta_generation),
        ]);
        let mut statement = self.connection.prepare(&sql)?;
        let mut query = statement.query(params_from_iter(values.iter()))?;
        let mut rows = Vec::with_capacity(keys.len());
        while let Some(row) = query.next()? {
            rows.push(ResolutionPendingRow {
                version_id: row.get(0)?,
                pending_relationship_id: row.get(1)?,
                target_version_id: row.get(3)?,
                target_symbol_id: row.get(4)?,
                tier: row.get(5)?,
                confidence: row.get(6)?,
                method: row.get(7)?,
            });
        }
        Ok(PriorOverlayAccess::Ready(rows))
    }

    fn identifier_belongs_to_predecessor(
        &self,
        version_id: i64,
        identifier_id: &str,
    ) -> Result<bool, PriorOverlayError> {
        Ok(self.connection.query_row(
            "SELECT EXISTS(
               SELECT 1 FROM identifiers AS source
               JOIN manifest_entries AS manifest
                 ON manifest.version_id=source.version_id
                AND manifest.view_id=?1 AND manifest.generation=?2
               WHERE source.version_id=?3 AND source.identifier_id=?4)",
            params![
                self.state.view_id,
                self.state.predecessor_manifest_generation,
                version_id,
                identifier_id
            ],
            |row| row.get(0),
        )?)
    }

    fn pending_belongs_to_predecessor(
        &self,
        version_id: i64,
        pending_relationship_id: &str,
    ) -> Result<bool, PriorOverlayError> {
        Ok(self.connection.query_row(
            "SELECT EXISTS(
               SELECT 1 FROM pending_relationships AS source
               JOIN manifest_entries AS manifest
                 ON manifest.version_id=source.version_id
                AND manifest.view_id=?1 AND manifest.generation=?2
               WHERE source.version_id=?3 AND source.pending_relationship_id=?4)",
            params![
                self.state.view_id,
                self.state.predecessor_manifest_generation,
                version_id,
                pending_relationship_id
            ],
            |row| row.get(0),
        )?)
    }
}

impl Drop for PriorOverlayReader {
    fn drop(&mut self) {
        let _ = self.connection.execute_batch("ROLLBACK;");
    }
}

enum Filter<'a> {
    Names(&'a [&'a str]),
    Versions(&'a [i64]),
}

impl Filter<'_> {
    fn identifier_predicate(
        &self,
    ) -> Result<(String, Vec<Value>, &'static str), PriorOverlayError> {
        match self {
            Self::Names(names) => Ok((
                format!("source.name IN ({})", placeholders(names.len())?),
                text_filter_values(names)?,
                "INDEXED BY idx_read_identifiers_name_kind",
            )),
            Self::Versions(versions) => Ok((
                format!("source.version_id IN ({})", placeholders(versions.len())?),
                version_filter_values(versions)?,
                "",
            )),
        }
    }

    fn pending_predicate(&self) -> Result<(String, Vec<Value>, &'static str), PriorOverlayError> {
        match self {
            Self::Names(names) => Ok((
                format!(
                    "source.target_terminal_name IN ({})",
                    placeholders(names.len())?
                ),
                text_filter_values(names)?,
                "INDEXED BY idx_read_pending_terminal",
            )),
            Self::Versions(versions) => Ok((
                format!("source.version_id IN ({})", placeholders(versions.len())?),
                version_filter_values(versions)?,
                "",
            )),
        }
    }
}

fn read_scope_state(
    connection: &Connection,
    view_id: &str,
) -> Result<Option<ResolutionScopeState>, rusqlite::Error> {
    connection
        .query_row(
            "SELECT view_id,predecessor_manifest_generation,predecessor_manifest_hash,
                    base_id,delta_generation,resolver_output_epoch,current_manifest_generation,
                    current_manifest_hash,journal_through_transition_id
             FROM resolution_scope_state WHERE view_id=?1",
            [view_id],
            |row| {
                Ok(ResolutionScopeState {
                    view_id: row.get(0)?,
                    predecessor_manifest_generation: row.get(1)?,
                    predecessor_manifest_hash: row.get(2)?,
                    base_id: row.get(3)?,
                    delta_generation: row.get(4)?,
                    resolver_output_epoch: row.get(5)?,
                    current_manifest_generation: row.get(6)?,
                    current_manifest_hash: row.get(7)?,
                    journal_through_transition_id: row.get(8)?,
                })
            },
        )
        .optional()
}

fn read_base_catalog(
    connection: &Connection,
    base_id: &str,
) -> Result<Option<BaseCatalogRow>, rusqlite::Error> {
    connection
        .query_row(
            "SELECT manifest_hash,resolver_output_epoch,state,relative_path,identifier_count,
                    pending_count,file_bytes,file_sha256
             FROM resolution_bases WHERE base_id=?1",
            [base_id],
            |row| {
                Ok(BaseCatalogRow {
                    manifest_hash: row.get(0)?,
                    resolver_output_epoch: row.get(1)?,
                    state: row.get(2)?,
                    relative_path: row.get(3)?,
                    identifier_count: row.get(4)?,
                    pending_count: row.get(5)?,
                    file_bytes: row.get(6)?,
                    file_sha256: row.get(7)?,
                })
            },
        )
        .optional()
}

fn read_delta_catalog(
    connection: &Connection,
    state: &ResolutionScopeState,
) -> Result<Option<DeltaCatalogRow>, rusqlite::Error> {
    connection
        .query_row(
            "SELECT base_id,manifest_generation,manifest_hash,resolver_output_epoch,
                    identifier_replacements,pending_replacements,pending_tombstones
             FROM resolution_deltas WHERE view_id=?1 AND delta_generation=?2",
            params![state.view_id, state.delta_generation],
            |row| {
                Ok(DeltaCatalogRow {
                    base_id: row.get(0)?,
                    manifest_generation: row.get(1)?,
                    manifest_hash: row.get(2)?,
                    resolver_output_epoch: row.get(3)?,
                    identifier_replacements: row.get(4)?,
                    pending_replacements: row.get(5)?,
                    pending_tombstones: row.get(6)?,
                })
            },
        )
        .optional()
}

fn delta_row_count(
    connection: &Connection,
    state: &ResolutionScopeState,
    table: &'static str,
    operation: Option<&str>,
) -> Result<i64, rusqlite::Error> {
    match (table, operation) {
        ("resolution_identifier_deltas", None) => connection.query_row(
            "SELECT COUNT(*) FROM resolution_identifier_deltas
             WHERE view_id=?1 AND delta_generation=?2",
            params![state.view_id, state.delta_generation],
            |row| row.get(0),
        ),
        ("resolution_pending_deltas", Some(operation)) => connection.query_row(
            "SELECT COUNT(*) FROM resolution_pending_deltas
             WHERE view_id=?1 AND delta_generation=?2 AND operation=?3",
            params![state.view_id, state.delta_generation, operation],
            |row| row.get(0),
        ),
        _ => Err(rusqlite::Error::InvalidQuery),
    }
}

fn base_root_set_mismatch(
    connection: &Connection,
    base_id: &str,
) -> Result<Option<String>, rusqlite::Error> {
    let catalog_only = connection
        .query_row(
            "SELECT version_id FROM resolution_base_versions WHERE base_id=?1
             EXCEPT SELECT version_id FROM prior_base.resolution_base_versions
             ORDER BY version_id LIMIT 1",
            [base_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    if let Some(version_id) = catalog_only {
        return Ok(Some(format!(
            "base root set contains catalog-only version {version_id}"
        )));
    }
    let file_only = connection
        .query_row(
            "SELECT version_id FROM prior_base.resolution_base_versions
             EXCEPT SELECT version_id FROM resolution_base_versions WHERE base_id=?1
             ORDER BY version_id LIMIT 1",
            [base_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    Ok(
        file_only
            .map(|version_id| format!("base root set contains file-only version {version_id}")),
    )
}

fn first_incoherent_row(
    connection: &Connection,
    state: &ResolutionScopeState,
) -> Result<Option<PriorOverlayFallback>, rusqlite::Error> {
    for (table, sql) in [
        (
            "identifiers",
            "SELECT delta.version_id,delta.identifier_id
             FROM resolution_identifier_deltas AS delta
             LEFT JOIN identifiers AS source
               ON source.version_id=delta.version_id AND source.identifier_id=delta.identifier_id
             WHERE delta.view_id=?1 AND delta.delta_generation=?2
               AND source.identifier_id IS NULL
             ORDER BY delta.version_id,delta.identifier_id LIMIT 1",
        ),
        (
            "pending_relationships",
            "SELECT delta.version_id,delta.pending_relationship_id
             FROM resolution_pending_deltas AS delta
             LEFT JOIN pending_relationships AS source
               ON source.version_id=delta.version_id
              AND source.pending_relationship_id=delta.pending_relationship_id
             WHERE delta.view_id=?1 AND delta.delta_generation=?2
               AND source.pending_relationship_id IS NULL
             ORDER BY delta.version_id,delta.pending_relationship_id LIMIT 1",
        ),
    ] {
        if let Some((version_id, local_id)) = connection
            .query_row(sql, params![state.view_id, state.delta_generation], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .optional()?
        {
            return Ok(Some(PriorOverlayFallback::SourceRowMissing {
                table,
                version_id,
                local_id,
            }));
        }
    }
    for (table, sql) in [
        (
            "identifiers",
            "SELECT base.version_id,base.identifier_id
             FROM prior_base.identifier_resolutions AS base
             LEFT JOIN identifiers AS source
               ON source.version_id=base.version_id AND source.identifier_id=base.identifier_id
             WHERE source.identifier_id IS NULL
             ORDER BY base.version_id,base.identifier_id LIMIT 1",
        ),
        (
            "pending_relationships",
            "SELECT base.version_id,base.pending_relationship_id
             FROM prior_base.pending_resolutions AS base
             LEFT JOIN pending_relationships AS source
               ON source.version_id=base.version_id
              AND source.pending_relationship_id=base.pending_relationship_id
             WHERE source.pending_relationship_id IS NULL
             ORDER BY base.version_id,base.pending_relationship_id LIMIT 1",
        ),
    ] {
        if let Some((version_id, local_id)) = connection
            .query_row(sql, [], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .optional()?
        {
            return Ok(Some(PriorOverlayFallback::SourceRowMissing {
                table,
                version_id,
                local_id,
            }));
        }
    }
    if let Some((version_id, local_id)) = connection
        .query_row(
            "SELECT source.version_id,source.identifier_id
             FROM identifiers AS source
             JOIN resolution_base_versions AS root
               ON root.base_id=?1 AND root.version_id=source.version_id
             LEFT JOIN prior_base.identifier_resolutions AS base
               ON base.version_id=source.version_id AND base.identifier_id=source.identifier_id
             LEFT JOIN resolution_identifier_deltas AS delta
               ON delta.view_id=?2 AND delta.delta_generation=?3
              AND delta.version_id=source.version_id AND delta.identifier_id=source.identifier_id
             WHERE base.identifier_id IS NULL AND delta.identifier_id IS NULL
             ORDER BY source.version_id,source.identifier_id LIMIT 1",
            params![state.base_id, state.view_id, state.delta_generation],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?
    {
        return Ok(Some(PriorOverlayFallback::OverlayRowMissing {
            table: "identifier_resolutions",
            version_id,
            local_id,
        }));
    }
    if let Some((version_id, local_id)) = connection
        .query_row(
            "SELECT delta.version_id,delta.pending_relationship_id
             FROM resolution_pending_deltas AS delta
             LEFT JOIN prior_base.pending_resolutions AS base
               ON base.version_id=delta.version_id
              AND base.pending_relationship_id=delta.pending_relationship_id
             WHERE delta.view_id=?1 AND delta.delta_generation=?2
               AND delta.operation='tombstone' AND base.pending_relationship_id IS NULL
             ORDER BY delta.version_id,delta.pending_relationship_id LIMIT 1",
            params![state.view_id, state.delta_generation],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?
    {
        return Ok(Some(PriorOverlayFallback::OverlayRowMissing {
            table: "pending_resolutions",
            version_id,
            local_id,
        }));
    }
    Ok(None)
}

fn map_identifier_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ResolutionIdentifierRow> {
    Ok(ResolutionIdentifierRow {
        version_id: row.get(0)?,
        identifier_id: row.get(1)?,
        target_version_id: row.get(2)?,
        target_symbol_id: row.get(3)?,
        tier: row.get(4)?,
        confidence: row.get(5)?,
        method: row.get(6)?,
        outcome: row.get(7)?,
        candidates: row.get(8)?,
    })
}

fn map_pending_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ResolutionPendingRow> {
    Ok(ResolutionPendingRow {
        version_id: row.get(0)?,
        pending_relationship_id: row.get(1)?,
        target_version_id: row.get(2)?,
        target_symbol_id: row.get(3)?,
        tier: row.get(4)?,
        confidence: row.get(5)?,
        method: row.get(6)?,
    })
}

fn safe_base_relative_path(value: &str) -> Option<&Path> {
    let path = Path::new(value);
    let mut components = path.components();
    if components.next() != Some(Component::Normal("bases".as_ref()))
        || components
            .clone()
            .any(|component| !matches!(component, Component::Normal(_)))
        || path.extension().and_then(|extension| extension.to_str()) != Some("db")
    {
        return None;
    }
    Some(path)
}

fn validate_key(version_id: i64, local_id: &str) -> Result<(), PriorOverlayError> {
    if version_id <= 0 || local_id.is_empty() {
        return Err(PriorOverlayError::InvalidArgument("overlay key"));
    }
    Ok(())
}

fn cursor(after: Option<&PriorOverlayKey>) -> Result<(i64, &str), PriorOverlayError> {
    match after {
        Some(after) => {
            validate_key(after.version_id, &after.local_id)?;
            Ok((after.version_id, &after.local_id))
        }
        None => Ok((0, "")),
    }
}

fn checked_query_limit(limit: usize) -> Result<i64, PriorOverlayError> {
    if limit == 0 || limit > PRIOR_OVERLAY_MAX_WINDOW {
        return Err(PriorOverlayError::InvalidArgument("overlay window"));
    }
    i64::try_from(limit + 1).map_err(|_| PriorOverlayError::InvalidArgument("overlay window"))
}

fn placeholders(count: usize) -> Result<String, PriorOverlayError> {
    if count > PRIOR_OVERLAY_MAX_FILTER_VALUES {
        return Err(PriorOverlayError::InvalidArgument("overlay filter"));
    }
    Ok(std::iter::repeat_n("?", count)
        .collect::<Vec<_>>()
        .join(","))
}

fn key_values_clause(count: usize) -> Result<String, PriorOverlayError> {
    if count > PRIOR_OVERLAY_MAX_FILTER_VALUES {
        return Err(PriorOverlayError::InvalidArgument("overlay filter"));
    }
    Ok(std::iter::repeat_n("(?,?)", count)
        .collect::<Vec<_>>()
        .join(","))
}

fn key_params(keys: &[PriorOverlayKey]) -> Vec<Value> {
    keys.iter()
        .flat_map(|key| {
            [
                Value::Integer(key.version_id),
                Value::Text(key.local_id.clone()),
            ]
        })
        .collect()
}

fn text_filter_values(values: &[&str]) -> Result<Vec<Value>, PriorOverlayError> {
    if values.iter().any(|value| value.is_empty()) {
        return Err(PriorOverlayError::InvalidArgument("overlay name filter"));
    }
    Ok(values
        .iter()
        .map(|value| Value::Text((*value).to_string()))
        .collect())
}

fn version_filter_values(values: &[i64]) -> Result<Vec<Value>, PriorOverlayError> {
    if values.iter().any(|value| *value <= 0) {
        return Err(PriorOverlayError::InvalidArgument("overlay file filter"));
    }
    Ok(values.iter().copied().map(Value::Integer).collect())
}

fn page_next(scanned: &[PriorOverlayKey], limit: usize) -> Option<PriorOverlayKey> {
    if scanned.len() <= limit {
        return None;
    }
    scanned.get(limit - 1).cloned()
}
