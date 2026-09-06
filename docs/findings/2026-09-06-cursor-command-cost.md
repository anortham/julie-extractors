# Cursor command cost after Miller M2 dogfood

## Incident and cause

Miller's newly enabled sidecar cursors timed out while advancing a single consumer row.
The installed `julie-extract` 2.40.4 process consumed a CPU core for its 10-second attempt;
three retries still left `consumer_cursors` empty. The live store held 3,464 manifests,
7,128,961 manifest entries, and 14,619 file versions.

Both cursor handlers called `inspect_context`, which called `MaintenanceInspector::inspect`.
Before reaching the bounded coordinator mutation, this loaded every historical manifest entry
into a vector and computed a whole-store garbage-collection plan. The manifest query used its
existing compound index; a missing index or a reader lock was not the cause. A separate read-only
probe fetched all 7,128,959 non-null-version entries in 5.537 seconds without even running the
Rust GC planner. No live store or coordinator data was changed during this investigation.

An earlier 156-second import was progressing, not wedged. It published manifest 3763 after
roughly 11 seconds, continued 287 deep-level chunks, and committed at sequence 86885. The
producer already held the newly merged `StoreSidecarCursorSession.cs`, with 73 symbols and
complete L1/L2/L3 facts. New-reader admission during that import was intentionally refused
by the existing live-writer exclusion; it is a separate consumer-availability concern.

## Change and safety

Cursor advance and release now use a cursor-only context. The connection factory validates
family identity, the reader floor, supported database schemas, the current serving
generation, and the maintenance fence using read-only connections. The original coordinator
mutation methods still enforce maintenance exclusion transactionally, monotonicity,
generation conflicts, and the durable high-water bound. No reader-retention rules changed.
The current-generation check remains a preflight; this patch does not claim to make publication
and cursor mutation a new cross-database atomic operation.

The version-1 report adds optional `measurement_scope: "cursor_only"`. Existing GC-shaped
groups contain explicitly unmeasured defaults, not measured zero usage. Their fingerprints
are empty, reader summaries are omitted, and integrity checks name only work actually done.
Noncursor output omits the new field; its JSON and human snapshots remain unchanged.
Reader-compatible clients remain eligible even when the fact-writer floor or recorded writer
version is newer. Cursor metadata is not a store-fact write. An initial unshipped patch
incorrectly applied writer eligibility; a failing mixed-version regression caught that and
the check was removed. Plan mode changes no database bytes.

## Paired release measurement

The retained disposable fixture has 1,000 versions, 1,000 manifests, and 1,000,000 entries.
It was seeded from a one-file full import, then expanded with synthetic historical manifests
and version paths. No live workspace store was used for mutations or benchmark setup.

Metric: command wall time. Linux release binaries run sequentially, alternating old/new
within each repetition, with one warmup discarded and three measured repetitions per action.
The benchmark resets only its own consumer row outside the measured interval; advance starts
without that row and release starts with a generation-bound zero cursor. All runs return
the expected `advanced` or `released` disposition. No host builds or test suites ran during this
paired measurement. A Windows diagnostic suite remained active in a VM, mostly waiting for
fixture I/O; both binaries were measured under that same background condition. Nearest-rank
p95 over three samples is the largest sample, not a claim
about a large-sample latency distribution.

| Command | Old warm seconds | New warm seconds | p95 old -> new |
| --- | --- | --- | --- |
| advance --apply | 2.321249, 2.342649, 2.368428 | 0.004151, 0.005224, 0.005391 | 2.368428s -> 0.005391s |
| release --apply | 2.445090, 2.380916, 2.375073 | 0.004702, 0.004161, 0.004474 | 2.445090s -> 0.004702s |

The old binary is the installed released 2.40.4, SHA-256
`acfb332fe2795b4b60283c178fab9835ceaa5a650317bc387b623cf6818a5bc3`.
The patched release binary, built with `cargo build --release -p julie-extract-cli`, is
`9375e651549cf76cf025b2e72b238fd598a194392a5944905bd4841dde9213af`.
The development build still reports 2.40.4; it is not a published patch release.

Local reproducibility assets are retained at `/tmp/julie-cursor-bench.ID7aJK/`:

- `store/`: the shared fixture; `store/gen-001/store.db` SHA-256
  `3b340dc3fdecc2af5a3013d935b1e28adbf68d6a5e5a782097a74f65e278ab80`.
- `measure.py`: the alternating runner, SHA-256
  `4269b5d4268f42e973ee741aa8561a7d08900e24f1bb6f7b46fa130f912a8688`.
- `paired-final.log`: complete source-final measurements, SHA-256
  `ab05bc65e646fc564f205bb2182f3f872a8dd4aedbc5509920083a8add1bcffd`.

Run `python3 measure.py <old-release-binary> <patched-release-binary>` in this environment.
Each invocation uses `store maintain cursor <action> --store <fixture> --consumer
benchmark-cursor --apply --json`, with `--sequence 0` for advance.

An earlier debug-build baseline overlapped unrelated machine load: advance p95 20.784 seconds,
release p95 37.839 seconds. Those observations helped reproduce the problem but are not used
as the performance comparison above.

## Regression and verification

The deterministic regression fixture places a failing SQL function in a manifest-entry read
view. Before the fix, cursor advance fails while reading that unrelated view. After the fix,
advance/release in both plan/apply modes succeed without reading it. This protects the bounded
operation directly, without a timing assertion or a production-only test hook.

Source-final focused verification:

- `cargo test -p julie-extract-cli --test store_maintenance_cli_contract`: 25 passed.
- `cargo test -p julie-extract-artifact --test store_reader_cursor_contract`: 4 passed.
- `cargo test -p julie-extract-artifact --test store_connection_contract`: 28 passed.
- `cargo clippy -p julie-extract-cli -p julie-extract-artifact -- -D warnings`: passed.
- `cargo build --release -p julie-extract-cli`: passed.
- `cargo fmt --all` and `git diff --check`: clean.

The seven CLI cursor tests include 20 incompatible-binding combinations, four reader-compatible
newer-writer combinations, unchanged dry-run
database bytes, maintenance exclusion, sequence regression/ahead/negative refusal, generation
conflicts, and the historical-manifest read trap. Reader/cursor lifecycle independence and
retention-floor tests remain green. Source-final release-prep gates also passed:

- `cargo test -p xtask`: 119 passed; reused because xtask source was unchanged.
- `cargo xtask test default`: 4,816 passed, 0 failed, 11 ignored; tier 84s, total 85.76s.
- `cargo xtask test contract`: 376 passed, 0 failed, 3 ignored; total 256.05s.

Raw logs in the same temporary directory:

- `xtask-tests.log`: SHA-256 `c9fcc528a1d6c971b2bfcde214d993f429a6eb04f46c89c4a6ddf331481f9b19`.
- `default-final.log`: SHA-256 `a8b86ff7cc63a4418de0285db8e4ac3da0ed240b032459176ab853abaed36faf`.
- `contract-final.log`: SHA-256 `1efb13a1c597bc0e9d3f20a376f4a43f6123afef99507f1e124ca727b89a90af`.

Earlier `default-gate.log`, `contract-gate.log`, and `paired-release.log` cover the initial
953c780a patch before the reader-compatibility correction; they are superseded by these final
logs. Windows verification, versioned packaging, and publication remain release-owner gates.
