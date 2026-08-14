//! Absorbs SQLite's transient WAL locking-protocol signal on selected read-only store operations.
//!
//! SQLite raises `SQLITE_PROTOCOL` ("locking protocol", `FileLockingProtocolFailed`, code 15) from
//! exactly one place: the WAL read-transaction retry ladder in `walTryBeginRead`, after its own
//! retry limit. Those retries come from WAL-index recovery, and recovery is driven by the
//! open-connection count on the database reaching **zero** — the last connection out checkpoints
//! and unlinks the `-wal` and `-shm` files, and the next open rebuilds them underneath every other
//! connection.
//!
//! The signal is therefore transient: it means "the WAL index kept moving under me, try again". It
//! is NOT corruption, and reporting it as a hard failure was always wrong. The helper wraps the
//! complete read-only operation at each opted-in site, including lazy metadata queries after an
//! open. `ensure_writer_eligible` uses it for its store metadata validation before a lease
//! acquisition. Writer opens and lease validation deliberately remain outside the helper because
//! those paths perform mutations and have not produced this signal.
//!
//! Retrying is affordable only because opens are now rare. While `StoreCoordinator` opened
//! `coord.db` for every one of its calls, a retry sat on the hot path inside a five-second claim
//! window, and SQLite's own ladder makes each attempt cost seconds — a retry there could have
//! burned the claim it was protecting. Holding one connection per coordinator instance is what made
//! this safe; neither change alone is the fix.

use std::time::Duration;

/// How many times a read-only operation is retried before the locking-protocol failure is reported.
const RETRIES: u32 = 5;

/// Base backoff between attempts. Attempt N waits `N * BACKOFF`, so five retries span 375ms.
const BACKOFF: Duration = Duration::from_millis(25);

/// Whether an error is SQLite's transient WAL locking-protocol signal.
pub(super) fn is_locking_protocol(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(failure, _)
            if failure.code == rusqlite::ErrorCode::FileLockingProtocolFailed
    )
}

/// Runs `attempt` until it succeeds, fails for another reason, or exhausts the retry budget.
///
/// `is_protocol` reports whether the caller's own error type is carrying the locking-protocol
/// signal, so each caller keeps its error type instead of converting through a shared one.
pub(super) fn with_locking_protocol_retry<T, E>(
    mut attempt: impl FnMut() -> Result<T, E>,
    is_protocol: impl Fn(&E) -> bool,
) -> Result<T, E> {
    let mut retries = 0;
    loop {
        match attempt() {
            Err(error) if retries < RETRIES && is_protocol(&error) => {
                retries += 1;
                std::thread::sleep(BACKOFF * retries);
            }
            outcome => return outcome,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn protocol_error() -> rusqlite::Error {
        rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_PROTOCOL),
            Some("locking protocol".to_string()),
        )
    }

    #[test]
    fn recognizes_the_locking_protocol_signal() {
        assert!(is_locking_protocol(&protocol_error()));
        assert!(!is_locking_protocol(&rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
            None,
        )));
    }

    #[test]
    fn retries_a_locking_protocol_failure_until_it_clears() {
        let attempts = Cell::new(0);
        let outcome = with_locking_protocol_retry(
            || {
                attempts.set(attempts.get() + 1);
                if attempts.get() < 3 {
                    Err(protocol_error())
                } else {
                    Ok(attempts.get())
                }
            },
            is_locking_protocol,
        );
        assert_eq!(outcome.unwrap(), 3);
    }

    #[test]
    fn reports_a_locking_protocol_failure_that_never_clears() {
        let attempts = Cell::new(0);
        let outcome = with_locking_protocol_retry::<(), _>(
            || {
                attempts.set(attempts.get() + 1);
                Err(protocol_error())
            },
            is_locking_protocol,
        );
        assert!(outcome.is_err());
        assert_eq!(
            attempts.get(),
            RETRIES + 1,
            "the first call is an attempt, not a retry"
        );
    }

    #[test]
    fn returns_another_error_without_retrying() {
        let attempts = Cell::new(0);
        let outcome = with_locking_protocol_retry::<(), _>(
            || {
                attempts.set(attempts.get() + 1);
                Err(rusqlite::Error::SqliteFailure(
                    rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
                    None,
                ))
            },
            is_locking_protocol,
        );
        assert!(outcome.is_err());
        assert_eq!(attempts.get(), 1);
    }
}
