use rusqlite::Connection;

#[derive(Debug)]
pub(super) enum PragmaError {
    Sqlite(rusqlite::Error),
    IntegerMismatch {
        pragma: &'static str,
        expected: i64,
        found: i64,
    },
    TextMismatch {
        pragma: &'static str,
        expected: &'static str,
        found: String,
    },
}

impl From<rusqlite::Error> for PragmaError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WriterPragmaProfile {
    Routine,
    Bulk,
}

impl WriterPragmaProfile {
    fn wal_autocheckpoint(self) -> i64 {
        match self {
            Self::Routine => 1_000,
            Self::Bulk => 8_000,
        }
    }
}

pub(super) fn configure_writer_pragmas(
    connection: &Connection,
    profile: WriterPragmaProfile,
) -> Result<(), PragmaError> {
    connection.execute_batch("PRAGMA page_size = 4096;")?;
    let auto_vacuum: i64 = connection.query_row("PRAGMA auto_vacuum", [], |row| row.get(0))?;
    if auto_vacuum != 2 {
        connection.execute_batch("PRAGMA auto_vacuum = INCREMENTAL;")?;
    }
    verify_integer_pragma(connection, "page_size", 4096)?;
    verify_integer_pragma(connection, "auto_vacuum", 2)?;
    connection.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = FULL;
         PRAGMA foreign_keys = ON;
         PRAGMA secure_delete = ON;
         PRAGMA journal_size_limit = 268435456;",
    )?;
    configure_wal_autocheckpoint(connection, profile)?;
    verify_text_pragma(connection, "journal_mode", "wal")?;
    verify_integer_pragma(connection, "synchronous", 2)?;
    verify_integer_pragma(connection, "foreign_keys", 1)?;
    verify_integer_pragma(connection, "secure_delete", 1)?;
    verify_integer_pragma(connection, "journal_size_limit", 268_435_456)?;
    Ok(())
}

pub(super) fn validate_store_file_pragmas(connection: &Connection) -> Result<(), PragmaError> {
    verify_integer_pragma(connection, "page_size", 4096)?;
    verify_integer_pragma(connection, "auto_vacuum", 2)?;
    verify_text_pragma(connection, "journal_mode", "wal")
}

pub(super) fn configure_wal_autocheckpoint(
    connection: &Connection,
    profile: WriterPragmaProfile,
) -> Result<(), PragmaError> {
    let expected = profile.wal_autocheckpoint();
    connection.pragma_update(None, "wal_autocheckpoint", expected)?;
    verify_integer_pragma(connection, "wal_autocheckpoint", expected)
}

fn verify_integer_pragma(
    connection: &Connection,
    pragma: &'static str,
    expected: i64,
) -> Result<(), PragmaError> {
    let found = connection.query_row(&format!("PRAGMA {pragma}"), [], |row| row.get(0))?;
    if found == expected {
        Ok(())
    } else {
        Err(PragmaError::IntegerMismatch {
            pragma,
            expected,
            found,
        })
    }
}

fn verify_text_pragma(
    connection: &Connection,
    pragma: &'static str,
    expected: &'static str,
) -> Result<(), PragmaError> {
    let found = connection.query_row(&format!("PRAGMA {pragma}"), [], |row| {
        row.get::<_, String>(0)
    })?;
    if found.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err(PragmaError::TextMismatch {
            pragma,
            expected,
            found,
        })
    }
}
