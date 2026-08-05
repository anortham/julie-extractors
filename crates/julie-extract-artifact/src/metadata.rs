use std::collections::BTreeMap;

use rusqlite::{Connection, params};

use crate::schema::{EXTRACT_CONTRACT_VERSION, SQLITE_SCHEMA_VERSION};

pub const REQUIRED_METADATA_KEYS: &[&str] = &[
    "artifact_id",
    "root_path",
    "schema_version",
    "extract_contract_version",
    "sqlite_schema_version",
    "binary_version",
    "hash_algorithm",
    "parser_inventory_fingerprint",
    "capability_snapshot_fingerprint",
    "created_at",
    "updated_at",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactMetadata {
    pub artifact_id: String,
    pub root_path: String,
    pub binary_version: String,
    pub hash_algorithm: String,
    pub parser_inventory_fingerprint: String,
    pub capability_snapshot_fingerprint: String,
    pub created_at: String,
    pub updated_at: String,
}

impl ArtifactMetadata {
    pub fn rows(&self) -> [(&'static str, String); 11] {
        [
            ("artifact_id", self.artifact_id.clone()),
            ("root_path", self.root_path.clone()),
            ("schema_version", SQLITE_SCHEMA_VERSION.to_string()),
            (
                "extract_contract_version",
                EXTRACT_CONTRACT_VERSION.to_string(),
            ),
            ("sqlite_schema_version", SQLITE_SCHEMA_VERSION.to_string()),
            ("binary_version", self.binary_version.clone()),
            ("hash_algorithm", self.hash_algorithm.clone()),
            (
                "parser_inventory_fingerprint",
                self.parser_inventory_fingerprint.clone(),
            ),
            (
                "capability_snapshot_fingerprint",
                self.capability_snapshot_fingerprint.clone(),
            ),
            ("created_at", self.created_at.clone()),
            ("updated_at", self.updated_at.clone()),
        ]
    }
}

pub fn initialize_metadata(conn: &Connection, metadata: &ArtifactMetadata) -> rusqlite::Result<()> {
    let mut statement = conn.prepare(
        "INSERT INTO artifact_metadata (key, value) VALUES (?1, ?2) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )?;

    for (key, value) in metadata.rows() {
        statement.execute(params![key, value])?;
    }

    Ok(())
}

/// `artifact_metadata` key recording the artifact's extraction level.
///
/// ABSENT means `full`: every pre-levels artifact and every full-level
/// artifact read the same way, so old binaries and old artifacts stay
/// compatible in both directions. A `symbols`-level artifact carries
/// `index_level = "symbols"` so consumers can distinguish "empty because the
/// reference layer was never extracted" from "empty because nothing was
/// found". The level is fixed for the artifact's lifetime — an upgrade is a
/// rebuild into a fresh artifact, never an in-place widen. Deliberately not
/// part of `REQUIRED_METADATA_KEYS` / `ArtifactMetadata::rows()`, same as the
/// resolution keys.
pub const KEY_INDEX_LEVEL: &str = "index_level";

pub fn write_index_level(conn: &Connection, level: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO artifact_metadata (key, value) VALUES (?1, ?2) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![KEY_INDEX_LEVEL, level],
    )?;
    Ok(())
}

pub fn read_index_level(conn: &Connection) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT value FROM artifact_metadata WHERE key = ?1",
        params![KEY_INDEX_LEVEL],
        |row| row.get::<_, String>(0),
    )
    .map(Some)
    .or_else(|error| match error {
        rusqlite::Error::QueryReturnedNoRows => Ok(None),
        other => Err(other),
    })
}

/// Additive `artifact_metadata` keys recording that this artifact was retargeted
/// at a new root, and what it was retargeted from.
///
/// Deliberately absent from [`REQUIRED_METADATA_KEYS`] and
/// [`ArtifactMetadata::rows`], same as the level and resolution keys: an artifact
/// that was never rebound carries none of them, and a reader that does not know
/// them reads the artifact exactly as before.
pub const KEY_REBOUND_FROM_ROOT: &str = "rebound_from_root";
pub const KEY_REBOUND_FROM_ARTIFACT_ID: &str = "rebound_from_artifact_id";
pub const KEY_REBOUND_AT: &str = "rebound_at";

/// The complete set of `artifact_metadata` values one rebind writes.
pub struct RebindMetadata<'a> {
    pub previous_root: &'a str,
    pub previous_artifact_id: &'a str,
    pub new_root: &'a str,
    pub new_artifact_id: &'a str,
    /// RFC3339 stamp shared by `updated_at` and `rebound_at`, so the two never
    /// disagree about when the retarget happened.
    pub rebound_at: &'a str,
}

/// Retarget an artifact's recorded root and identity, writing nothing else.
///
/// `created_at`, `binary_version`, both capability fingerprints, the resolution
/// keys, `index_level`, and every data table are left untouched: a rebind states
/// where the artifact now lives, not what it contains. Every statement runs on
/// the caller's connection so the caller owns the transaction — an interrupted
/// rebind rolls back to a metadata-identical artifact.
pub fn apply_rebind(conn: &Connection, rebind: &RebindMetadata<'_>) -> rusqlite::Result<()> {
    let mut statement = conn.prepare(
        "INSERT INTO artifact_metadata (key, value) VALUES (?1, ?2) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )?;

    for (key, value) in [
        ("root_path", rebind.new_root),
        ("artifact_id", rebind.new_artifact_id),
        ("updated_at", rebind.rebound_at),
        (KEY_REBOUND_FROM_ROOT, rebind.previous_root),
        (KEY_REBOUND_FROM_ARTIFACT_ID, rebind.previous_artifact_id),
        (KEY_REBOUND_AT, rebind.rebound_at),
    ] {
        statement.execute(params![key, value])?;
    }

    Ok(())
}

pub fn read_metadata(conn: &Connection) -> rusqlite::Result<BTreeMap<String, String>> {
    let mut statement = conn.prepare("SELECT key, value FROM artifact_metadata ORDER BY key")?;
    statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect()
}
