# Task 7 report

## Result

Implemented the public import-only `store import` command with durable coordinator queueing,
transaction-bound store writes, L1 manifest publication, hash-guarded Full deepening, resumable
request-global chunks, failure manifests, supervision controls, and Task 6 report routing.

## TDD checkpoints

- 2026-08-08T05:21:47Z–05:21:50Z RED: production binary rejected `store`; expected public parse.
- 2026-08-08T05:28:22Z–05:28:28Z GREEN: queued empty L1 import committed coordinator, manifest, and terminal effects.
- 2026-08-08T05:28:55Z–05:28:58Z RED: supported Rust source had no persisted version.
- 2026-08-08T05:33:12Z–05:33:19Z GREEN: cold Symbols extraction persisted L1 and published the manifest.
- 2026-08-08T05:34:16Z–05:34:19Z RED: Full import had no L2/L3 stamps.
- 2026-08-08T05:35:43Z–05:35:48Z GREEN: Full ordered L1, manifest, L2, L3, terminal.
- RED/GREEN: unchanged Full retry initially had no invocation proof; progress is now advanced exactly at the extractor invocation seam and retry reports zero extraction, versions, or level effects.
- RED/GREEN: same-key persisted L1 payload corruption initially deepened; every stored L1 row value is now compared to a staged Full L1 projection before deeper writes.
- RED/GREEN: hard-killed holder left the request ineligible; dead/expired lease takeover now transfers only that holder's claimed requests atomically. The 101-version retry has one manifest and one terminal with contiguous global chunk indexes.
- RED/GREEN: default per-version scheduling became two L1 quanta at 101 versions; `MILLER_STORE_CHUNK_VERSIONS=0` produces one-version quanta and the 128 MiB estimated WAL limit splits earlier.
- RED/GREEN: invalid source preflight bypassed manifest failure state; raw identity hashing now allows `failed` and `failed_preserved` manifest entries without failed version rows.
- RED/GREEN: source mutation after L1 publication now returns `changed_between_waves`, preserves the old L1 head, and requires a later new-hash request.
- RED/GREEN: top-level Task 6 rejection/help assertions were updated to Task 7 exposure while all legacy JSON/parser assertions remained unchanged.
- 2026-08-08 review RED/GREEN: a single drain wrote the initiating root's `a.rs` into a queued second root; payload v1 now durably carries canonical scope, requested level, sorted path/hash/bytes/WAL plan, and request controls, while the executor is generic per request.
- Review RED/GREEN: retry with a new ID reported the retry rather than the canonical request; replay now reports the original ID/payload/level and changed-level reuse is `idempotency_conflict`.
- Review RED/GREEN: inserting a lexically earlier file after a hard kill conflicted with the retry plan; stable-scope replay now drains the original durable plan and ignores post-enqueue tree reordering/insertion.
- Review RED/GREEN: failed L1 facts vanished between chunks/restarts; progress effects now persist and reload `failed`/`failed_preserved` facts plus manifest disposition.
- Review RED/GREEN: jobs was inert and per-file concurrent spools collided; import now shares the legacy extraction-pool selector, extracts bounded chunks concurrently, and serializes deterministic spool I/O only.
- Review RED/GREEN: reports returned zero counts and lost partial L1 state; reports now query the current manifest's applicable L1/L2/L3 row families and preserve partial generation/hash/disposition/completion.
- Review RED/GREEN: a non-holder returned `busy` immediately; it now polls the canonical durable request to requester timeout and returns exact `request_timeout` without canceling queued/claimed work.
- Review RED/GREEN: Full extracted separately for L2 and L3; each bounded Full chunk extracts once and writes L2 then L3 in one coordinator-supplied transaction. Empty Full completes all levels.

## Contract evidence

- Focused import: 19/19 passed, including two-root backlog isolation, canonical replay/conflict, no-zombie preflight, truthful/partial reports, non-holder timeout, empty Full, fixed-plan crash resume with durable failures, controls, projected/actual WAL bounds, row-value mismatch, and Rust+Python L1/L2/L3-family persistence.
- Coordinator: 44/44 passed; feature takeover: 2/2 passed.
- Store writer contract: 22/22 passed after factoring the legacy wrapper through the transaction-bound core.
- CLI contract: 14/14 passed.
- Full artifact and CLI regressions passed through `cargo xtask test default`.
- `cargo +1.96.0 clippy -p julie-extract-artifact -p julie-extract-cli --all-targets -- -D warnings` passed.
- `cargo +1.96.0 fmt --all -- --check` and `git diff --check` passed.
- Miller refreshed the exact worktree to revision 35; diff impact identified coordinator, manifest, writer, CLI dispatch, and their store tests, all covered above.
- Workspace-wide Clippy remains blocked by the pre-existing `xtask/src/compat.rs` `items_after_test_module` warning outside Task 7 ownership; the owned package all-target gate is clean.

## Ownership expansions

- Authorized artifact API expansion: transaction-bound ManifestStore ensure/publish and StoreWriter lookup/write/L1 projection comparison, preserving legacy wrappers.
- Authorized coordinator expansion: bulk writer pragmas before the supplied transaction and atomic claimed-request transfer on dead/expired lease takeover, plus coordinator regression tests.
- Review-cycle coordinator expansion: read-only idempotency-key lookup returns the canonical durable request so Task 7 can compare stable request scope while retaining the original immutable plan.
- Review-cycle extraction expansion: moved the existing generic extraction-pool selector into the extraction seam so legacy scan and store import share requested jobs, auto detection, one-worker fallback, and stack sizing.
- Authorized library expansion: private discovery/extraction/path/progress/spool/watchdog modules backing one library-owned store implementation.
- Authorized CLI compatibility expansion: replaced only Task 6 assertions whose purpose was proving top-level `store` absence; preserved parser/report behavior.

No update, delete, promotion, dependency, push, or later-task surface was added.
