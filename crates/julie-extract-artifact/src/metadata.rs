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

pub fn read_metadata(conn: &Connection) -> rusqlite::Result<BTreeMap<String, String>> {
    let mut statement = conn.prepare("SELECT key, value FROM artifact_metadata ORDER BY key")?;
    statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect()
}
