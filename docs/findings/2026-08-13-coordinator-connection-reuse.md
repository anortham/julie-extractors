# The coordinator opened `coord.db` per call, and SQLite eventually refused

- **Date:** 2026-08-13
- **Release state:** v2.33.2 is prepared and pending publication; v2.33.1 remains the current
  published release. The exact-tree release-prep gates are green; live publication and asset
  verification remain outstanding.
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

## Verification of the coordinator fix

- Feature-enabled store resolution contract suite, serial: **10 runs × 24 tests, 0 occurrences of
  `SQLITE_PROTOCOL`**, counting both `FileLockingProtocolFailed` and `locking protocol`. A separate
  lease-fence failure can still be reported by the test run; it is not counted as a protocol hit.
- The 2-thread measurement was **8 runs, 0 occurrences**, where the protocol previously appeared on
  most runs.
- The same suite went from about **90 seconds to about 45**. That gap was the open storm.
- `cargo test -p julie-extract-artifact` passes, including the 62-test coordinator contract suite.
- Every guard borrow was checked scope-aware for re-entrancy: `std::sync::Mutex` is not reentrant,
  and a sibling call made while holding the guard would deadlock. There are 21 guard borrows and
  none is held across one. `next_pending_request` looks like a violation and is not — it scopes the
  guard to the block that collects the request ids, and releases it before the loop.

## The first retry pass missed the second database open

Holding one connection per instance did not remove every open. `ResolveHeartbeat` starts a thread
that opens its OWN coordinator, because it needs a connection while the caller already holds one and
`Connection` is not `Sync`. Every such construction still lands on a moving WAL index, and it failed
the resolve:

```
resolve claim lost — the coordinator could not be opened: locking protocol
```

This was an intermediate candidate state, measured at roughly 1 run in 7, against most runs before
the reuse change.

A measurement warning worth recording: this was briefly called "a different cause" because the
counting grep looked for `FileLockingProtocolFailed`, and this path renders the same condition as
`locking protocol`. The same error prints two ways depending on which layer formats it. Count
`SQLITE_PROTOCOL` by both spellings.

The treatment is a bounded retry: `SQLITE_PROTOCOL` means "the WAL index kept moving under me, try
again", so the only correct handling is another attempt. It took two rounds to place it, and the
first round is the lesson:

1. The retry went into `open_coordinator`, which covers `coord.db`. The suite kept failing at
   roughly the same rate, with the same message.
2. The message was the same because `StoreCoordinator::open_with_liveness` does TWO opens.
   After the retried `coord.db` open it calls `coordinator_store_family`, which opens **`store.db`**
   read-only to read the family id. That one had no retry, and both failures render as
   `locking protocol`, so the message could not tell them apart. `store.db` has its own WAL and its
   own zero-crossings — in the hard-kill tests, killed writers churn it constantly.

Both construction opens are retried now, and the retry lives in one place
(`store/wal_retry.rs`) so a future site cannot be treated as fatal while its sibling retries. The same
read-only helper also covers the store read in `reconcile`, `ensure_writer_eligible`'s complete
store-metadata validation before lease acquisition, the resolution base and scratch readers, and
`connection.rs::open_reader`. It wraps the whole read, not just the `Connection::open` call:
SQLite opens lazily, so the locking-protocol failure surfaces at the first statement — pragma
configuration for the coordinator, the `store_meta` query for the family id, or a reader's first query.

Deliberately NOT retried, and the reason: `connection.rs::open_writer` and the lease validation it
performs sit on paths that take a writer lease. Retrying those side-effecting paths on a signal that has
never been measured there would risk more than it fixes. The coordinator's writer/lease mutations are
also outside the helper; only the read-only eligibility check before acquisition is covered. If
`SQLITE_PROTOCOL` ever appears from an excluded path, it will be visible and this note is the map.

A retry was rejected earlier in this investigation, and the reason it is right NOW is worth being
explicit about, because the earlier objection was sound. While every call opened its own connection,
a retry sat on the hot path inside a five-second claim window, and SQLite's internal retry ladder
makes each attempt cost seconds — a retry there could burn the claim it was protecting. With one
connection per instance, an open happens at construction and on the three lease paths, so the same
retry is affordable. The reuse change is what made the retry safe; neither alone is the fix.

## Lease acquisition sampled a stale clock

The original acquisition path sampled wall time before `BEGIN IMMEDIATE`. If SQLite waited on a busy
coordinator, the five-second lease could already be expired by the time the transaction inserted or
took over the row. The next heartbeat then returned `Ok(false)` even though acquisition had just
returned `Acquired`. Production acquisition now samples wall time after `BEGIN IMMEDIATE` succeeds;
logical-time contract callers retain their injected timestamp.

## Lease renewal is token-checked and resilient

The writer heartbeat now retries transient coordinator errors within the heartbeat tick and can reclaim
a lapsed row only when the fencing token is unchanged. A successor's token returns an immediate loss;
a busy or locked coordinator no longer silently ends the heartbeat. The operation stops the heartbeat
before releasing the lease so renewal cannot race release.

## A single changed path remains scoped

The delta planner's name arm can select identifiers outside the changed file. A regression promoted a
single-changed-path request to a full resolution when that arm was broad, defeating the single-file
exemption. The planner now keeps one changed path scoped even when the name arm selects all identifier
rows. The old live behavior measured **182,867 ms** and **549,149 rows** for that wrong full path; those
values are historical measurements, not a v2.33.2 full-gate result.

## Windows diagnostics and test guards

Report diagnostics normalize both root and relative components before joining them with the report
contract's `/` separator, so Windows output no longer mixes separators. The Windows store-resolution
gate now enables the feature and runs its 24-test target with `--test-threads=1`; the repeat harness
also asserts that 24 tests ran. That zero-test guard prevents Cargo's `test result: ok` from being
mistaken for a real suite run. The contract tests print a failing child command's output and detect a
child that exits before its fault-injection point.

## Release-prep verification

The exact tree passed the release-prep gates on 2026-08-13: `cargo fmt --all -- --check`,
`cargo test -p xtask`, `cargo xtask test default` (exit 0 in 96.6 seconds), and `cargo xtask test contract`
(exit 0 in 713.2 seconds, including the serial feature-enabled 24-test resolution target, which passed
24/24 tests in 101.54 seconds). Strict workspace
`cargo clippy --workspace --all-targets --all-features --no-deps -- -D warnings` passed.
`cargo xtask release preflight --version 2.33.2` validated 4 targets and 32 inputs, and
`cargo xtask release package-list` passed. `git diff --check` passed. These checks make the source
candidate release-ready; v2.33.2 is still unpublished until the source push and live asset
verification are complete.
