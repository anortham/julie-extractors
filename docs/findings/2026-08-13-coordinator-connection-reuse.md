# The coordinator opened `coord.db` per call, and SQLite eventually refused

- **Date:** 2026-08-13
- **Symptom:** resolves failed with `resolution_failed: resolve claim lost`, on Windows, with no
  competing process and nothing wrong with the claim.
- **Real error underneath:** `SQLITE_PROTOCOL` (`FileLockingProtocolFailed`, code 15, "locking
  protocol"), raised while configuring pragmas on a freshly opened connection, and reported by
  `open_coordinator` as a corrupt coordinator.

## What was wrong

`StoreCoordinator` held no connection. All 27 of its methods called `open_coordinator()`, which
opened a new `rusqlite::Connection` and re-ran the full writer pragma set on it, including
`PRAGMA journal_mode = WAL`. The resolve claim heartbeat did this four times a second for the whole
life of a resolve.

SQLite raises `SQLITE_PROTOCOL` from one place: the WAL read-transaction retry ladder in
`walTryBeginRead`, after its retry limit. Those retries come from WAL-index recovery. Recovery is
driven by the open-connection count on the database repeatedly reaching **zero**: the last connection
out checkpoints and unlinks `coord.db-wal` and `coord.db-shm`, and the next open rebuilds them and
invalidates the WAL index other connections were using.

So the defect was not a race between two writers. It was one process opening and closing the same
WAL database fast enough that the WAL index was rebuilt underneath live readers.

Two consequences worth keeping in mind:

- `PRAGMA journal_mode = WAL` never needed to run on those opens at all. WAL is a persistent property
  of the database file, and any connection enters WAL mode from the `-wal` file with no pragma.
  Re-issuing it on every open was pure cost on the hottest path.
- `SQLITE_PROTOCOL` is transient, not corruption. Mapping it to `CoordinatorError::CorruptRequest`
  named the wrong thing and hid the cause from every caller.

## Why it looked like a concurrency bug and was not

The failure got **worse** as the tests were made more serial, which is the opposite of contention:

| Configuration | Overlap between connections | Result |
|---|---|---|
| 24 libtest threads | connections almost always overlap | passed 2 of 2 |
| 2–4 libtest threads | frequent zero-crossings | failed most runs |
| 1 libtest thread (serial) | most zero-crossings | failed 3 of 3 |

Fewer concurrent connections means more moments at zero open connections, so more WAL-index
teardown and rebuild. This table is what ruled out every load-based explanation.

Three reproduction attempts failed to trigger the error and are recorded here so nobody repeats
them: an open/close storm across six threads in one process; the same across six separate processes;
and fifty hard-killed writers leaving a WAL mid-write. None produced `SQLITE_PROTOCOL`. In all three
the workers overlapped enough that the connection count rarely reached zero.

## The fix

`StoreCoordinator` now holds one connection for the life of the instance, behind a `Mutex` so the
`&self` methods keep their receivers. Three paths deliberately keep opening their own connection,
because they need one while `self` is already borrowed and `Connection` is not `Sync`:

- `release_lease_at` — the `LeaseReleaseGuard` drop path
- `reclaim_lapsed_lease_at` and `heartbeat_lease_at` — the drain's lease-heartbeat thread

A poisoned lock is recovered rather than propagated: the mutex guards a connection, not an
invariant. The guard also rolls back a transaction abandoned by a panic, so the next borrower never
inherits an open write transaction.

## Verification

- Store resolution contract suite, 2 threads: **8 runs, 0 occurrences of `SQLITE_PROTOCOL`**, where
  it previously appeared on most runs.
- The same suite went from about **90 seconds to about 45**. That gap was the open storm.
- `cargo test -p julie-extract-artifact` passes, including the 62-test coordinator contract suite.
- Every guard borrow was checked scope-aware for re-entrancy: `std::sync::Mutex` is not reentrant,
  and a sibling call made while holding the guard would deadlock. There are 21 guard borrows and
  none is held across one. `next_pending_request` looks like a violation and is not — it scopes the
  guard to the block that collects the request ids, and releases it before the loop.

## Still open

One test, `rebase_crash_boundaries_retry_with_one_ready_base_and_one_empty_delta`, still fails
occasionally (2 of 8 runs) with a different cause and no `SQLITE_PROTOCOL`. It is not this defect and
is not yet diagnosed.
