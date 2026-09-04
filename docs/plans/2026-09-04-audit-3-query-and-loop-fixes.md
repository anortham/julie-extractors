# Audit Wave 3: Query and Loop Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Replace quadratic paging, per-open migration work, per-file syscalls, and redundant reads with once-per-run or keyset forms, without changing any output row.

**Architecture:** Each task is local to one function or one call site. No new modules. The only interface change is that `BaseExtractor::new` accepts a pre-normalized relative path from the CLI.

**Tech Stack:** Rust, rusqlite, tree-sitter.

**Architecture Quality:** Affected modules: `julie-extract-artifact/src/store/{generation,connection,schema,coordinator,model}.rs`, `julie-extract-artifact/src/writer{,/rows}.rs`, `julie-extract-cli/src/{commands,discovery,artifact_access}.rs`, `julie-extract-cli/src/store/{import,executor}.rs`, `julie-extractors/src/{base/span.rs,utils/paths.rs,language_spec/mod.rs,pipeline.rs}`. Caller-facing interface: unchanged except the path normalization entry. Test surface: store contract tests, operations contract, golden tier. Rejected shortcuts: bumping the store schema to drop the migration (refuses every existing family store; see `docs/decisions/2026-08-18-resolution-write-path-retirement.md`). Instead Task 5 records a one-time migration marker in `store_meta`. Architecture risk: low.

Source: findings A3, A4, A5, A6, A7, A11, C3, C5, C9, E6, E7.

## Global Constraints

- No golden fixture changes. No artifact, store, or JSONL contract version change.
- Store stays on WAL plus `synchronous=FULL`. Task 6 touches only `julie-extract-artifact/src/writer.rs`, never `store/writer.rs`.
- Determinism of scan order and row order is preserved.
- Windows rules from `CLAUDE.md`: keep the verbatim-prefix stripping and handle-identity comparisons where they exist today.

## Verification Strategy

**Project source of truth:** `docs/testing-strategy.md`.

**Worker red/green scope:**
- Artifact store: `cargo test -p julie-extract-artifact store::` and the relevant `tests/*.rs` target named in the task.
- Artifact writer: `cargo test -p julie-extract-artifact --test writer_batching_contract` and `cargo xtask performance writer-current-schema --out-dir target/performance/wave3-writer`.
- CLI: `cargo test -p julie-extract-cli --test operations_contract`, `--test store_cli_contract`.
- Extractors: `cargo xtask test language c`, `cpp`, plus `cargo test -p julie-extractors --lib base::span`.

**Worker ceiling:** `cargo xtask test default`.

**Worker gate invariant:** identical rows before and after, proven by the existing contract tests, plus the task's new focused test.

**Lead affected-change scope:** `cargo xtask test changed <touched paths>`.

**Branch gate:** `cargo xtask test default`, `cargo xtask test contract`, `cargo test -p julie-extract-artifact --features test-store-crash --test store_crash_contract`, `cargo fmt --check`, `cargo clippy --workspace --all-targets`.

**Security scope:** none declared.

**Replay/metric evidence:** Task 1 records the promote time on a store with at least 200k rows before and after. Hard gate: after is not slower. Report-only: the speedup.

**Escalation triggers:** any store crash-contract failure; any change to store `file_versions` content; any Windows-only path behavior change (run the `win-test` skill suite for Tasks 8 and 9).

**Assigned verification failure:** Workers stop and report when assigned verification fails, unless this plan explicitly says to update that gate.

**Verification ledger:** Record invariant, command, scope label, commit SHA, result, and timestamp.

## Parallel Execution Contract

| Task | Parallel batch | File ownership | Serialization required | Dependency reason |
|---|---|---|---|---|
| Task 1: Keyset paging in `copy_table` | Batch A | Modify `crates/julie-extract-artifact/src/store/generation.rs`; add test in `crates/julie-extract-artifact/tests/store_generation_contract.rs` | No | None - safe parallel batch. |
| Task 2: Per-file structural fact dedupe | Batch A | Modify `crates/julie-extract-artifact/src/writer/rows.rs` | No | None - safe parallel batch. |
| Task 3: Store projection by value | Batch A | Modify `crates/julie-extract-artifact/src/store/model.rs`; modify callers in `crates/julie-extract-cli/src/store/executor.rs` (lines 646 and 1118 only) | No | None - safe parallel batch. |
| Task 4: Heartbeat connection reuse | Batch A | Modify `crates/julie-extract-artifact/src/store/coordinator.rs` (`heartbeat_lease_at`, `release_lease_at`, the heartbeat thread) | No | None - safe parallel batch. |
| Task 5: One-time migration marker | Batch B | Modify `crates/julie-extract-artifact/src/store/connection.rs`, `store/schema.rs`, `store/layout.rs` | Yes | Depends on Task 4 (both touch coordinator open paths). |
| Task 6: Artifact writer checkpoint policy | Batch B | Modify `crates/julie-extract-artifact/src/writer.rs` | No | None - safe parallel batch. |
| Task 7: Discovery carries the language | Batch C | Modify `crates/julie-extract-cli/src/discovery.rs`, `crates/julie-extract-cli/src/commands.rs` (`spool_discovered_files`, line 732 region) | No | None - safe parallel batch. |
| Task 8: Scan report attribution only for JSON | Batch C | Modify `crates/julie-extract-cli/src/commands.rs` (line 559 region), `crates/julie-extract-cli/src/artifact_access.rs` | Yes | Shares `commands.rs` with Task 7. Dispatch after Task 7 lands. |
| Task 9: Store import single read | Batch C | Modify `crates/julie-extract-cli/src/store/import.rs`, `store/executor.rs` (`extract`), `crates/julie-extract-cli/src/extraction.rs` (`read_source_snapshot` at line 86) | Yes | Shares `executor.rs` with Task 3. Dispatch after Batch A lands. |
| Task 10: Normalize the path once | Batch D | Modify `crates/julie-extractors/src/base/span.rs`, `utils/paths.rs`, `base/extractor.rs` (`new`), `pipeline.rs`, `crates/julie-extract-cli/src/extraction.rs` | No | None - safe parallel batch. |
| Task 11: Reuse the header detection parse | Batch D | Modify `crates/julie-extractors/src/language_spec/mod.rs` (`detect_language_for_path`, `header_parse_prefers_cpp`), `pipeline.rs` (`extract_canonical_at`, `parse_for_language`) | Yes | Shares `pipeline.rs` with Task 10. Dispatch after Task 10 lands. |

Commit mode: `parallel-lead-commit` inside each batch.

---

## Task 1: Keyset paging in `copy_table`

**Files:** Modify `crates/julie-extract-artifact/src/store/generation.rs:821` (`copy_table`) and its callers at 650 and 1026.

**What to build:** Paging by "primary key greater than the last key seen", with the SELECT prepared once per table, not once per page.

**Approach:** Each table's ordered key columns already flow in as `order` and `keys`. Build the SELECT as `WHERE (k1, k2) > (?1, ?2) ORDER BY k1, k2 LIMIT ?n` using SQLite row-value comparison. For a table with no declared key, page on `rowid`. Test: a synthetic table with 2,000 rows and a window of 512 copies every row exactly once, in order, with and without `ignore_conflicts`.

**Acceptance criteria:**
- [x] No `OFFSET` in `generation.rs`.
- [x] Copy test proves exact row parity and order.
- [x] Promote timing recorded before and after; after is not slower.

## Task 2: Per-file structural fact dedupe

**Files:** Modify `crates/julie-extract-artifact/src/writer/rows.rs`: `structural_fact_ids` field (160, 257, 328) and `insert_structural_facts` (1052).

**What to build:** A per-file `HashSet<&str>` inside `insert_structural_facts`, matching the pattern `SymbolLookup` uses. The transaction-wide set goes away.

**Approach:** Confirm with Miller that the transaction-wide set exists only to skip duplicate ids inside one artifact write. The `structural_facts` primary key already rejects cross-file duplicates. Keep the insert statement as it is unless a test proves a cross-file duplicate was previously silently skipped; if so, switch to `INSERT OR IGNORE` and record why.

**Acceptance criteria:**
- [x] `ChildRowInserters` has no `HashSet<String>` that lives past one file.
- [x] `cargo xtask performance writer-current-schema` shows no row-count change.
- [x] Writer contract tests pass.

## Task 3: Store projection by value

**Files:** Modify `crates/julie-extract-artifact/src/store/model.rs:212` (`try_from_artifact_file`) to take `ArtifactFile` by value; delete the clone at 236. Update the two callers in `crates/julie-extract-cli/src/store/executor.rs`.

**What to build:** Ownership transfer instead of a deep clone.

**Approach:** The callers drop `artifact` right after the call. Where `project_reference_sites` clones `path` and `language` per site, hold them once per file and clone only the id strings.

**Acceptance criteria:**
- [x] No `file.clone()` in `model.rs`.
- [x] Store contract tests pass.

## Task 4: Heartbeat connection reuse

**Files:** Modify `crates/julie-extract-artifact/src/store/coordinator.rs`: `heartbeat_lease_at` (3158), `release_lease_at` (3103), and the heartbeat thread that calls them (around 2855).

**What to build:** The heartbeat thread opens one coordinator connection when it starts and reuses it for every tick. Release uses the thread's connection when the thread owns one.

**Approach:** Keep the reclaim path (a failed tick may reopen) as a fallback only. The doc at `coordinator.rs:668-690` explains why the main connection is held; apply the same reasoning. Test with the existing lease tests plus one that counts connection opens over five ticks (inject through the existing `UnixMillisClock` trait, no new trait).

**Acceptance criteria:**
- [x] Five heartbeat ticks open one connection.
- [x] Coordinator lease tests pass; `store_crash_contract` passes.

## Task 5: One-time migration marker

**Files:** Modify `crates/julie-extract-artifact/src/store/connection.rs:263-266`, `store/schema.rs` (`retire_resolution_store_objects` 160, `reap_retired_resolution_capability_gaps` 207), `store/layout.rs:237` (`reap_retired_resolution_files`).

**What to build:** A `store_meta` key `resolution_retired = 1`. Writer open runs the four retirement steps only when the key is absent, then sets it inside the same transaction. `ensure_read_symbol_indexes` stays per open (it is cheap after the first run).

**Approach:** Do not bump `STORE_SQLITE_SCHEMA_VERSION`; the decision record forbids it. Test: open a fixture store twice, assert the gap `DELETE` and the `read_dir` walks run once (count through the existing test hooks in `store/test_hooks.rs`).

**Acceptance criteria:**
- [x] Second open runs no retirement work.
- [x] A store that still has resolution objects is migrated on first open (existing migration test).
- [x] Store contract and crash contract pass.

## Task 6: Artifact writer checkpoint policy

**Files:** Modify `crates/julie-extract-artifact/src/writer.rs`: `checkpoint_wal` calls at 358, 459, 495, 561, 619; keep `finish_journal` (1286) for scan end.

**What to build:** `write_update`, `delete_file`, and `sync_capability_snapshot` no longer force a TRUNCATE checkpoint. The writer's `Drop` or explicit close runs one TRUNCATE checkpoint. Scan keeps its checkpoint at `finish_journal`.

**Approach:** Confirm the reader contract first: does any reader test require the WAL to be empty after a single update? Check `docs/contracts` for a WAL statement. If a consumer requires a truncated WAL after every update, stop and report. Otherwise: add the close-time checkpoint, delete the per-call ones, and add a test that ten single-file updates leave a WAL file that is folded on close.

**Acceptance criteria:**
- [x] One `checkpoint_wal` on close, one at scan end, none per update or delete.
- [x] Artifact writer tests pass; the update test proves the file is consistent after close.

## Task 7: Discovery carries the language

**Files:** Modify `crates/julie-extract-cli/src/discovery.rs`: `DiscoverySummary` (112) supported targets carry the `language` the walk already computed at `select_file` (198). Modify `crates/julie-extract-cli/src/commands.rs`: `spool_discovered_files` (1545) reads the carried language and no longer calls `select_file`; check line 732 for the same pattern.

**What to build:** A `SupportedTarget { target: FileTarget, language: String }` (or the existing shape if one exists; check with Miller) in the summary.

**Approach:** The walk classifies each file once. Store the result. Test: a discovery over `fixtures/extraction` produces the same supported list and languages as before, and `select_file` is not called during spooling (count through a test hook or by removing the call and letting the type system enforce it).

**Acceptance criteria:**
- [x] `spool_discovered_files` does not call `select_file`.
- [x] Operations contract passes; discovery unit tests pass.

## Task 8: Scan report attribution only for JSON

**Files:** Modify `crates/julie-extract-cli/src/commands.rs:559` and `crates/julie-extract-cli/src/artifact_access.rs:698` (`file_row_attribution`), `654` (`table_totals`).

**What to build:** Scan computes `file_row_attribution` only when the report is emitted as JSON. Text output does not run the fourteen `GROUP BY` queries. `info` keeps the unlimited attribution because that is its purpose.

**Approach:** Check `docs/contracts/cli.md` for the scan report contract. If the JSON report field is required, keep it for JSON and skip for text. If the text report also prints it, stop and report before changing user-visible output. Add a test that a text-mode scan does not execute the attribution query (use the rusqlite trace hook in the test).

**Acceptance criteria:**
- [x] Text scan runs no `GROUP BY file_id` query.
- [x] JSON scan output is byte-identical to before for the same input.

## Task 9: Store import single read

**Files:** Modify `crates/julie-extract-cli/src/store/import.rs:180` (`read_source_identity_or_missing`), `crates/julie-extract-cli/src/store/executor.rs` (`extract` at 587, which calls `read_source_snapshot` near 597), and `crates/julie-extract-cli/src/extraction.rs:86` (`read_source_snapshot`).

**What to build:** Planning uses `metadata().len()` and mtime to detect change candidates. The content hash is computed once at extraction and compared to the stored hash inside the chunk. A file whose size and mtime match the stored version is skipped without a read.

**Approach:** Read `docs/contracts/store-v1.md` for the change-detection contract first. If the contract requires hash-based planning, keep the hash but read the file once and pass the bytes into extraction through the planned payload (bounded by the existing chunk size). Record the choice in the task report.

**Acceptance criteria:**
- [x] Each imported file is read from disk once per import.
- [x] Store CLI contract passes; the `changed_during_l1_wave` and `changed_between_waves` errors still fire in their tests.

## Task 10: Normalize the path once

**Files:** Modify `crates/julie-extractors/src/base/span.rs:149` (`normalize_file_path`), `utils/paths.rs:13`, `base/extractor.rs:60` (`new`), `pipeline.rs`, `crates/julie-extract-cli/src/extraction.rs`.

**What to build:** `BaseExtractor::new` accepts a path that is already root-relative and normalized to `/` separators, and does no canonicalize call. The in-process convenience entry (`extract_canonical`) that receives an absolute path canonicalizes once in `pipeline.rs` and passes the relative form down.

**Approach:** The CLI passes `target.root_relative_path` today (`extraction.rs:181`). Keep the Windows verbatim-prefix stripping in the one place that still canonicalizes. Test: the existing `normalize_file_path` tests move to the pipeline entry; add a test that `BaseExtractor::new` performs no filesystem access (pass a path that does not exist and assert the symbol `file_path` is the string given).

**Acceptance criteria:**
- [x] No `canonicalize` call in `base/span.rs` or `base/extractor.rs`.
- [x] Golden tier passes with zero fixture changes.
- [x] Windows suite via the `win-test` skill passes for the path tests.

## Task 11: Reuse the header detection parse

**Files:** Modify `crates/julie-extractors/src/language_spec/mod.rs:320` (`detect_language_for_path`), `364` (`header_parse_prefers_cpp`), `pipeline.rs:18` (`extract_canonical_at`), `142` (`parse_for_language`).

**What to build:** Header detection returns the winning `Tree` together with the language. The pipeline uses that tree instead of parsing again. Non-header files keep the current one-parse path.

**Approach:** Add a crate-private `detect_language_with_tree(path, content) -> Option<(&'static str, Option<Tree>)>`. `detect_language_for_path` keeps its signature and discards the tree for public callers. Test: a `.h` file with C++ content parses exactly once in the pipeline (count parser calls through a test hook or by asserting the returned tree's root byte range matches the detection tree).

**Acceptance criteria:**
- [x] A non-empty `.h` file is parsed at most twice end to end (one probe per grammar), never three times.
- [x] `cargo xtask test language c` and `cpp` pass; golden tier passes.
