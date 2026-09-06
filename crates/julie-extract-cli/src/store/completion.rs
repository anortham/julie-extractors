use std::io::Write;
use std::path::Path;
use std::time::Duration;

use julie_extract_artifact::store::StoreLayout;
use rusqlite::{Connection, OpenFlags};

use super::args::{
    StoreCommand, StoreMaintenanceCommand, StoreMaintenanceCursorCommand, StoreReaderCommand,
};

pub(super) fn write_command_root(command: &StoreCommand) -> Option<&Path> {
    match command {
        StoreCommand::Import(args) => Some(&args.store),
        StoreCommand::Update(args) => Some(&args.store),
        StoreCommand::Delete(args) => Some(&args.store),
        StoreCommand::Export(_) => None,
        StoreCommand::Reader(args) => match &args.command {
            StoreReaderCommand::Acquire(args) => Some(&args.store),
            StoreReaderCommand::Renew(args) => Some(&args.store),
            StoreReaderCommand::Release(args) => Some(&args.store),
        },
        StoreCommand::Maintain(args) => match &args.command {
            StoreMaintenanceCommand::Inspect(_) => None,
            StoreMaintenanceCommand::Gc(args)
            | StoreMaintenanceCommand::Repair(args)
            | StoreMaintenanceCommand::Promote(args) => args.apply.then_some(args.store.as_path()),
            StoreMaintenanceCommand::RetireView(args) => args.apply.then_some(args.store.as_path()),
            StoreMaintenanceCommand::Cursor(args) => match &args.command {
                StoreMaintenanceCursorCommand::Advance(args) => {
                    args.apply.then_some(args.store.as_path())
                }
                StoreMaintenanceCursorCommand::Release(args) => {
                    args.apply.then_some(args.store.as_path())
                }
            },
        },
    }
}

/// One opportunistic attempt per write command, including replay and no-change.
/// Busy readers leave the WAL for a later command; they never invalidate a commit.
pub(super) fn checkpoint(root: &Path, command_succeeded: bool) {
    let layout = match StoreLayout::open(root) {
        Ok(layout) => layout,
        Err(error) => {
            // A failed command already reports an unusable store. Preserve its
            // output contract when there is no selectable generation to clean.
            if command_succeeded {
                let _ = writeln!(
                    std::io::stderr(),
                    "wal_checkpoint status=unavailable remaining_wal_bytes=unknown: {error}"
                );
            }
            return;
        }
    };
    for path in [layout.store_db(), layout.coordinator_db()] {
        let mut wal = path.as_os_str().to_os_string();
        wal.push("-wal");
        // Even an empty TRUNCATE invalidates observers' data_version. A WAL
        // header is 32 bytes; without frames there is no checkpoint work.
        match std::fs::metadata(Path::new(&wal)) {
            Ok(metadata) if metadata.is_file() && metadata.len() <= 32 => continue,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            _ => {} // Unknown WAL state retains the bounded checkpoint attempt.
        }
        let result = (|| -> rusqlite::Result<i64> {
            // Never create a database after a failed command or generation race.
            let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
            connection.busy_timeout(Duration::from_millis(250))?;
            connection.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| row.get(0))
        })();
        let remaining = match std::fs::metadata(Path::new(&wal)) {
            Ok(metadata) => Some(metadata.len()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some(0),
            Err(_) => None,
        };
        let status = match &result {
            Ok(0) if remaining == Some(0) => continue,
            Ok(0) => "remaining",
            Ok(_) => "busy",
            Err(_) => "unavailable",
        };
        let remaining = remaining.map_or_else(|| "unknown".to_string(), |bytes| bytes.to_string());
        // A concurrent writer may append after successful truncation. Report what
        // remains instead of claiming a size bound or deleting a live WAL.
        let _ = writeln!(
            std::io::stderr(),
            "wal_checkpoint database={} status={status} remaining_wal_bytes={remaining}",
            path.display()
        );
    }
}
