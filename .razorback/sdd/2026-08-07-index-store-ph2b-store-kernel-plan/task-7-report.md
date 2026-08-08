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
- 2026-08-08 review RED/GREEN: a single drain wrote the initiating root's `a.rs` into a queued second root; payload v1 now durably carries canonical scope, requested level, sorted path/hash/byte plan, and request controls, while the executor is generic per request.
- Review RED/GREEN: retry with a new ID reported the retry rather than the canonical request; replay now reports the original ID/payload/level and changed-level reuse is `idempotency_conflict`.
- Review RED/GREEN: inserting a lexically earlier file after a hard kill conflicted with the retry plan; stable-scope replay now drains the original durable plan and ignores post-enqueue tree reordering/insertion.
- Review RED/GREEN: failed L1 facts vanished between chunks/restarts; progress effects now persist and reload `failed`/`failed_preserved` facts plus manifest disposition.
- Review RED/GREEN: jobs was inert and per-file concurrent spools collided; import now shares the legacy extraction-pool selector, extracts bounded chunks concurrently, and serializes deterministic spool I/O only.
- Review RED/GREEN: reports returned zero counts and lost partial L1 state; reports now query the request's durable manifest generation and applicable L1/L2/L3 row families while preserving partial generation/hash/disposition/completion.
- Review RED/GREEN: a non-holder returned `busy` immediately; it now polls the canonical durable request to requester timeout and returns exact `request_timeout` without canceling queued/claimed work.
- Review RED/GREEN: Full extracted separately for L2 and L3; each bounded Full chunk extracts once and writes L2 then L3 in one coordinator-supplied transaction. Empty Full completes all levels.
- 2026-08-08 second-review RED/GREEN: a crafted queued payload could redirect runtime authority and traverse outside its root; durable payloads no longer carry store paths, parent PIDs, or caller WAL estimates. The executor validates the trusted family/schema, canonical root, bounded identifiers/controls/serialized size/file count, strict sorted unique slash-relative paths, containment, and hash shape before source/progress access. The exact rejection leaves the store catalog/integrity and outside sentinel unchanged.
- Second-review RED/GREEN: replay canonicalized and rediscovered the requested root before idempotency lookup; terminal replay now observes the canonical request before root/progress/discovery/hashing, succeeds after root deletion, and leaves progress byte-identical.
- Second-review RED/GREEN: replay A after same-view request B reported B's current generation; durable report projection now uses A's terminal or persisted L1-published generation and preserves A's exact manifest, completion, and row counts.
- Second-review RED/GREEN: parent PID was durable submitter state; it is now only the current executor process watchdog, and a successor completes a queued request after the original submitter and its parent exit.
- Second-review RED/GREEN: every quantum recreated and truncated progress; the process-local request/path cache now creates once, reconstructs counters once on successor startup, and keeps a parseable monotonic multi-quantum JSONL stream.
- Second-review inline RED/GREEN: same-key replay with a different caller family returned `internal`; stored-payload integrity validation now uses the trusted store-catalog family before caller-scope comparison returns stable `idempotency_conflict`, with no manifest, log, or coordinator mutation.

## Contract evidence

- Focused import: 26/26 passed, including crafted-payload rejection, replay after root deletion, request-specific A/B reports, successor runtime supervision, append-only multi-quantum progress, two-root backlog isolation, canonical replay/family/level conflicts, no-zombie preflight, truthful/partial reports, non-holder timeout, empty Full, fixed-plan crash resume with durable failures, controls, recomputed projected/actual WAL bounds, row-value mismatch, and Rust+Python L1/L2/L3-family persistence.
- Coordinator: 44/44 passed; feature takeover: 2/2 passed.
- Store writer contract: 22/22 passed after factoring the legacy wrapper through the transaction-bound core.
- CLI contract: 14/14 passed.
- Full artifact and CLI regressions passed through `cargo xtask test default`.
- `cargo +1.96.0 clippy -p julie-extract-artifact -p julie-extract-cli --all-targets -- -D warnings` passed.
- `cargo +1.96.0 fmt --all -- --check` and `git diff --check` passed.
- Miller refreshed the exact worktree to revision 44; second-review diff impact identifies the import executor/report path and CLI dispatch, covered by the focused import, CLI, and artifact matrices above.
- Workspace-wide Clippy remains blocked by the pre-existing `xtask/src/compat.rs` `items_after_test_module` warning outside Task 7 ownership; the owned package all-target gate is clean.

## Ownership expansions

- Authorized artifact API expansion: transaction-bound ManifestStore ensure/publish and StoreWriter lookup/write/L1 projection comparison, preserving legacy wrappers.
- Authorized coordinator expansion: bulk writer pragmas before the supplied transaction and atomic claimed-request transfer on dead/expired lease takeover, plus coordinator regression tests.
- Review-cycle coordinator expansion: read-only idempotency-key lookup returns the canonical durable request so Task 7 can compare stable request scope while retaining the original immutable plan.
- Review-cycle extraction expansion: moved the existing generic extraction-pool selector into the extraction seam so legacy scan and store import share requested jobs, auto detection, one-worker fallback, and stack sizing.
- Authorized library expansion: private discovery/extraction/path/progress/spool/watchdog modules backing one library-owned store implementation.
- Authorized CLI compatibility expansion: replaced only Task 6 assertions whose purpose was proving top-level `store` absence; preserved parser/report behavior.

No update, delete, promotion, dependency, push, or later-task surface was added.

## Review verdicts

- Finding 5 is rejected under the frozen coordinator plan: the lease holder drains accepted work; only a non-holder polls until requester timeout, and a requester deadline never cancels or deletes durable queued/claimed work. The coordinator deadline contract and Task 7 non-holder timeout regression prove that behavior.
- Finding 7 is satisfied by combined Task 3/4/5 transaction-boundary tests, Task 7 crash/retry integration, the conservative projected-WAL contract plus live WAL fixture, and the applicable Rust/Python row-family proof. Task 9 retains ownership of the exhaustive real-parser language matrix.
