# Reader setup write regression

The shared writer configuration assigned `PRAGMA auto_vacuum=INCREMENTAL` on every
connection, even when the file already had that setting. SQLite committed the repeated
assignment. Reader acquisition and release each opened a coordinator writer, so both
paid for an unnecessary commit before their required registration update.

`configure_writer_pragmas` now queries the current mode and assigns it only when it
differs from 2. All read-back validation remains. New database initialization, conversion
from FULL, refusal of an incompatible existing NONE database, `synchronous=FULL`,
reader lifetime, and checkpoint behavior are preserved.

The SQLite [auto_vacuum contract](https://www.sqlite.org/pragma.html#pragma_auto_vacuum)
defines the query form and conversion restrictions. The unnecessary commit was reproduced
locally and guarded through the public `StoreCoordinator::open` interface.

## Measurements

[Raw samples, binary hashes, and syscall evidence](2026-09-06-reader-pragma-cost.json).

Producer source baseline: `e4f6a5db601ec7f1b9812b373346a74a7ccd8117`, version 2.40.5.
The modified producer was built with `cargo build --release -p julie-extract-cli --bin julie-extract`
and placed in an isolated copy of the Miller runtime. The installed runtime was not replaced.
Both Miller executables were `1.28.0+66e302949943` and read the same current index,
revision 87528, with an unchanged manifest hash before and after measurement.

Workload: `miller inspect FullRebuildPromotion --depth summary --json`, cwd
`/home/murphy/source/miller`, `MILLER_SEMANTIC=off`, `MILLER_CT=off`. One serial
request, alternating before/after order, discard one warmup pair, retain 20 pairs.
These are fresh CLI processes against warm OS caches, with normal background services active.

| Metric | Before | After |
|---|---:|---:|
| Median wall time | 347 ms | 313 ms |
| Nearest-rank p95 | 1,906 ms | 1,830 ms |
| Maximum | 1,952 ms | 1,985 ms |
| Disk sync calls in separate diagnostic trace | 12 | 10 |

Median improved about 10%; median paired reduction was 30 ms. All 42 calls succeeded
and returned byte-identical output. Large storage-related spikes affected both groups,
and the maximum did not improve. This is evidence for removing redundant work, not a
claim that tail latency or the whole v1.28 regression is resolved.

The diagnostic sync count includes unfinished/resumed syscalls reconciled as one call.
Before and after traces experienced different individual sync durations and are not a
timing comparison. The regression guard measures committed changes, not elapsed time.

## Verification

- The new coordinator-open test failed on the original implementation because an observer's
  `PRAGMA data_version` changed from 2 to 3 solely from opening the coordinator.
- After the fix, three coordinator open/close cycles leave the observer's data version unchanged.
- The existing writer-configuration test now starts in FULL mode and verifies conversion to
  INCREMENTAL, along with all required durability and connection settings.
- The focused connection suite passed 29 tests before the final coverage expansion.
- The complete Linux default tier passed in 128 seconds on the final code and tests.
- Release build, formatting, and diff whitespace checks passed.

The fix is independent of extracted language data. No schema, CLI, or report contract changed.
It is a source repair; publication and Miller pin adoption remain separate release steps.
