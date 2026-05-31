use std::path::Path;

use rusqlite::Connection;

use crate::metadata::{ArtifactMetadata, initialize_metadata};
use crate::schema::create_schema;

pub struct ArtifactWriter {
    connection: Connection,
}

impl ArtifactWriter {
    pub fn open_in_memory(metadata: ArtifactMetadata) -> rusqlite::Result<Self> {
        let connection = Connection::open_in_memory()?;
        initialize_connection(&connection, &metadata)?;
        Ok(Self { connection })
    }

    pub fn open_path(path: impl AsRef<Path>, metadata: ArtifactMetadata) -> rusqlite::Result<Self> {
        let connection = Connection::open(path)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        initialize_connection(&connection, &metadata)?;
        Ok(Self { connection })
    }

    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    pub fn into_connection(self) -> Connection {
        self.connection
    }
}

fn initialize_connection(
    connection: &Connection,
    metadata: &ArtifactMetadata,
) -> rusqlite::Result<()> {
    create_schema(connection)?;
    initialize_metadata(connection, metadata)?;
    Ok(())
}
