# Task 4 Report: Store Scratch Resolution and Streaming Diff

## Status and state

- Status: complete for the Task 4 worker scope.
- Authoritative base: `b7bc598505f9a7c1251b55484354ba00debe2097`.
- Verified pre-commit HEAD: `b7bc598505f9a7c1251b55484354ba00debe2097`.
- Branch: `codex/index-store-ph2c`.
- Worktree: `/Users/murphy/source/julie-extractors/.claude/worktrees/index-store-ph2c`.
- Pre-commit status: ten owned source/test paths modified or added plus this report; no unrelated path changed.
- Recorded base correction: Task 3's authoritative tip superseded the original brief value `fc74b4cc2a6389c27e03e0db20a6aac1fbb5b733` with `b7bc598505f9a7c1251b55484354ba00debe2097`.
- Ownership expansions authorized by the lead: `crates/julie-extract-cli/src/resolution.rs`, `crates/julie-extract-cli/tests/resolution_session_contract.rs`, `crates/julie-extract-artifact/src/store/resolution.rs`, and the minimal export surface in `crates/julie-extract-artifact/src/store/mod.rs`.

## Delivered behavior

- `StoreScratchResolutionSession` validates Store family/schema/view/generation/manifest/L2 through `StoreConnectionFactory` before creating output, reads indexed and failed-preserved extraction versions, and exposes failed entries as path facts without extraction rows.
- The generic resolver owns one fallible policy over `CandidateLookup`; Legacy adapts its memory index and Store implements the same lookup ports with owned version-qualified hits and bounded ordered SQL visitor pages.
- Resolver phases freeze immutable membership in the local scratch database, page Store reads through fresh readers, batch-hydrate one phase page, and flush each bounded batch in a scratch transaction.
- Store windows are explicitly capped at 300 keys, keeping the three-parameter row-value predicate below SQLite's 999-variable default. Every SQL limit conversion is checked.
- Scratch creation uses the artifact-owned validated path helper, preserves non-UTF8 paths with `OsString`, refuses symlinked ancestors before file creation, and applies/read-verifies page size 4096, incremental auto-vacuum, WAL, synchronous FULL, foreign keys and secure-delete ON, and autocheckpoint 8000.
- Streaming base and scratch writers retain ordered-row validation, target validation, fixed-memory integrity scans, and the two-close completion durability boundary.
- `stream_resolution_diff` and `apply_base_delta` page-merge identifier and pending tables by natural key and payload. Pending removals become tombstones; identifier removal is legal only when the exact version-root set excludes the source version. Gap facts are emitted in-band in deterministic order and sink failure leaves no completed delta.
- Eager builders/readers remain compatibility wrappers. The production Store session, diff, and apply paths use bounded windows and do not materialize the corpus.

## TDD ledger

1. Environment RED: the default toolchain was Rust 1.94 and rejected the workspace's 1.95 requirement. All subsequent commands used `cargo +1.95.0`.
2. Generic API RED: the bounded-port source contract found `WorkspaceCandidateIndex` in the Store-facing contract. The bulk index/locator/overlay seams were replaced by `open_resolution_pass`, explicit phase chunks, fallible locator/qualification, and direct `CandidateLookup` visitor ports.
3. Window RED: Legacy returned a whole phase `Vec`. A window-size input and `VecDeque` chunking produced identical output at window sizes 1 and 7 with maximum chunks at the configured bound.
4. Manifest RED: the Store mechanism test initially failed to compile because `StoreScratchResolutionSession` did not exist. The smallest green established factory-based manifest identity validation, ordered windows, indexed plus failed-preserved visibility, retained exclusion, failed path facts, and pre-output L2 refusal.
5. Candidate-policy RED: the first temporary candidate-index attempt produced empty pending parity and violated the approved architecture. It was removed. The shared tier functions now call fallible lookup ports directly; the pinned Store/Legacy semantic dump and a 10,000-collision ambiguity fixture pass, with Store's observed SQL page at 7 and ambiguity evidence capped at 2.
6. Phase RED: unbounded phase output and mutable same-phase membership were replaced with scratch-frozen keys, bounded pages, visibility barriers, fresh Store readers, batch hydration, and scratch transactions. Exact output is identical at windows 1 and 7; later keys are neither skipped nor duplicated.
7. Streaming writer RED: artifact tests initially had no streaming base/delta APIs; the first SQLite insert-select implementation also failed with `near "DO": syntax error`. The corrected ordered writers pass out-of-order, missing-visible-target, incomplete-artifact, catalog, and durability tests.
8. Diff RED: the matrix froze add, replace, delete, multi-delete, path/version reuse, failed, failed-preserved, duplicate-local-ID, exact gap order, totality violation, and sink rollback behavior. The ordered merge and replay implementation makes applied base+delta equal the exact artifact in both semantic tables.
9. Ceiling RED: full CLI testing found `ambiguous_candidates_sorted_by_symbol_id` expected `['alpha','mid','zeta']` while the bounded exactly-one policy correctly returned `['alpha','mid']`. The contract now asserts deterministic sorted and bounded ambiguity evidence; the full feature suite then passed 641/641.
10. Hardening RED: review found unchecked/capless window casts and silent phase hydration loss. Typed invalid-window and phase-corruption errors now cover both; tests assert the 300-key cap and exact frozen-key hydration.
11. Containment RED: review showed `<exact>.work` was opened before artifact path validation and was derived through lossy display formatting. The artifact-owned creator and symlink-parent test prove refusal with no redirected outside file.
12. RSS GREEN: the real Store session, scratch database, candidate/phase readers, and streaming exact writer ran in child processes at 1,000 and 8,000 rows with a 32-row window. Peak RSS growth stayed within the fixed 24 MiB allowance; the full test took 72.79 seconds.

## Verification ledger

- Worker focused Store contract: `cargo +1.95.0 test -p julie-extract-cli --features test-store-resolution-contract --test store_resolution_mechanism --no-fail-fast` — 9/9 passed, including the real RSS gate before the final containment addition. Post-containment focused rerun excluding the already-proven RSS measurement: 9/9 passed, 1 filtered.
- Resolver/oracle: `cargo +1.95.0 test -p julie-extract-cli --features test-store-resolution-contract --test resolution_session_contract --test resolution_contract --no-fail-fast` — 34/34 passed.
- Artifact focused: `cargo +1.95.0 test -p julie-extract-artifact --features test-store-resolution --test store_resolution_schema_contract` — 15/15 passed after final edits.
- CLI ceiling: `cargo +1.95.0 test -p julie-extract-cli --features test-store-resolution-contract --no-fail-fast` — 641/641 passed, including Store 9/9 and RSS 72.79 seconds.
- Artifact ceiling: `cargo +1.95.0 test -p julie-extract-artifact --features test-store-resolution --no-fail-fast` — 278/278 passed.
- CLI all-target Clippy: strict `-D warnings` first reproduced 18 unowned Rust 1.95 `clippy::collapsible_match` findings under `crates/julie-extractors/src/{cpp,elixir,go,php,razor,ruby,rust,scala,sql,typescript,zig}`. Scoped command `cargo +1.95.0 clippy -p julie-extract-cli --all-targets --features test-store-resolution-contract -- -A clippy::collapsible-match -D warnings` passed.
- Artifact all-target Clippy: `cargo +1.95.0 clippy -p julie-extract-artifact --all-targets --features test-store-resolution -- -A clippy::collapsible-match -D warnings` passed.
- Formatting and patch hygiene: `cargo +1.95.0 fmt --all -- --check` and `git diff --check` passed.
- Source guards: targeted scans found no `ATTACH`, Store-owned `WorkspaceCandidateIndex`, `CurrentResolutionOverlay`, or whole Store locator/coverage set. Store `Vec` collections are bounded page results; eager artifact `Vec` methods are compatibility readers and are not used by production diff/apply.
- Task 5's Miller-scale G1-G5 three-run measurement gate was not run, as required.

## Miller evidence and API-shape evidence

- Workspace status attempt returned exactly `diagnostic_code=workspace_status_empty`.
- Workspace onboarding attempt returned exactly `diagnostic_code=workspace_onboarding_empty`.
- Per the brief, subsequent discovery used targeted `rg` and bounded reads. No Miller context/search/inspect/trace/impact result was invented.
- `StoreConnectionFactory::open_reader(&self) -> Result<Connection, StoreConnectionError>` was verified in `artifact/src/store/connection.rs`; every Store page reopens through this validated factory.
- `ResolutionSession`, `CandidateLookup`, and `resolve_with_candidate_lookup` signatures were verified in the owned CLI modules. Generic policy has no SQLite dependency.
- `ResolutionBaseWriter`, bounded `identifier_window`/`pending_window`, `stream_resolution_diff`, and `apply_base_delta` signatures were verified in the owned artifact modules.
- Public surface remains narrow: one Store adapter, the existing generic engine, streaming base/scratch writers and readers, diff, and apply. The scratch-connection helper is the minimal artifact-owned path/pragma policy seam required for containment.

## Architecture self-review

- Depth/locality: Store manifest, SQLite, path, factory, and scratch mechanics remain behind `StoreScratchResolutionSession`; the generic resolver is storage-independent.
- Bounds: Store snapshots last at most one configured page. Candidate visitors and phase enumeration reopen a reader per page. Frozen worklists and scratch writes may use local transactions without extending a Store snapshot.
- Correctness: natural-key and full-payload comparisons cover both semantic tables. Version-qualified identities preserve duplicate local IDs. Hydration checks ordered keys exactly and reports mutation/corruption rather than silently skipping work.
- Durability/rollback: streaming outputs stamp completion only after commit/checkpoint/drop/fsync/reopen validation and a second completed-stamp close. Sink failure or target validation failure leaves no completed artifact.
- Containment: no ATTACH or cross-database FK assumption exists. Output and work paths reject symlink components and unexpected file types before creation.
- Staged schema-v1 limitation: indexed and failed-preserved entries derive language from immutable `file_versions`; failed entries have `version=None` and `language=None` and participate only in path/module-existence semantics. Task 6 must require language on every manifest entry before this mechanism becomes public runtime behavior.

## Concerns

- `finish_exact` target validation is bounded and correct but currently opens one validated Store reader/query per target through `finish_with_target_lookup`. This is the first Task 5 G3c throughput diagnostic; a bounded page target-validation API should be considered only if measurement shows it misses the gate.
- The 18 unowned Rust 1.95 `collapsible_match` findings remain a repository Clippy baseline issue outside Task 4 ownership. No lint was disabled in source; only the scoped verification invocation allowed that one baseline lint.
