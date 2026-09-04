# Audit Wave 1: Hot-Path Waste Removal Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Remove per-symbol and per-file work whose output nobody reads, so extraction and store import do each unit of work once.

**Architecture:** Every task deletes or hoists existing work. No new abstractions, no new tables, no schema or contract bump. The one new shared type is the containing-symbol index that already exists in the Rust extractor, promoted into `base/` so every language uses it. Measurement brackets the plan: one baseline before Task 1 and one after the last task, with the same workload.

**Tech Stack:** Rust, tree-sitter, rusqlite. No new dependencies.

**Architecture Quality:** Affected modules: `julie-extractors/src/base` (symbol creation, containing-symbol lookup), `julie-extract-cli/src/store/executor.rs`, `julie-extract-cli/src/extraction.rs`, `julie-extract-artifact/src/store/writer.rs`. Caller-facing interface: `BaseExtractor` loses `symbol_map`, `extract_code_context`, and `line_ranges`; `find_containing_symbol*` keeps its signatures but runs on an index. `write_level_in_transaction` keeps its signature. Test surface: the existing per-language tests, golden fixtures, and store contract tests prove behavior through the same interfaces callers use. Rejected shortcuts: a global cache for the capability snapshot (hides the per-file call instead of removing it), and keeping `symbol_map` behind a feature flag. Architecture risk: low. If code reality contradicts this shape, report a plan mismatch instead of redesigning locally.

Source: `docs/findings/2026-09-04-architecture-and-performance-audit.md`, items E1, E2, E3, C1, C2, A2, C4.

## Global Constraints

- No change to any golden fixture `expected.json`, JSONL output, or SQLite row content. Every task must leave `cargo xtask test golden` green without regenerating fixtures. A task that needs a fixture change has found a plan mismatch.
- `EXTRACTION_IDENTITY_EPOCH`, `EXTRACTION_CONTRACT_VERSION`, artifact schema version, store schema version, and JSONL contract version do not change.
- `unsafe_code = "forbid"` stays.
- Do not touch `attach_containing_symbols` in `base/containing_symbol.rs` or `base/source_regions.rs`. That is wave 5 (E4).
- Zero inline narration comments. Tests get zero comments.
- Windows rules from `CLAUDE.md` apply to every path change.

## Verification Strategy

**Project source of truth:** `docs/testing-strategy.md`, `CLAUDE.md` Test Discipline.

**Worker red/green scope:** the narrowest command that covers the change:
- Extractor base changes: `cargo test -p julie-extractors --lib base::` plus `cargo xtask test language <name>` for each language the task touches.
- CLI store changes: `cargo test -p julie-extract-cli --test operations_contract` and `cargo test -p julie-extract-cli --test store_cli_contract`.
- Artifact store writer: `cargo test -p julie-extract-artifact --test store_writer_performance` and `cargo test -p julie-extract-artifact store::`.

**Worker ceiling:** `cargo xtask test default`.

**Worker gate invariant:** the touched behavior still produces identical rows, and no test needed a fixture regeneration.

**Lead affected-change scope:** `cargo xtask test changed <touched paths>` after each batch.

**Branch gate:** `cargo xtask test default` then `cargo xtask test contract`, then `cargo fmt --check` and `cargo clippy --workspace --all-targets`.

**Security scope:** none declared.

**Replay/metric evidence:** Task 0 and Task 7 run the same two performance commands. Hard gate: no metric regresses beyond noise (median worse by more than 5 percent). Report-only: the improvement percentages.

**Escalation triggers:** any golden fixture diff, any change to `capabilities.json`, any store contract failure. On a golden diff, stop and report; do not regenerate.

**Assigned verification failure:** Workers stop and report when assigned verification fails, unless this plan explicitly says to update that gate.

**Verification ledger:** Record invariant, command, scope label, commit SHA, result, and timestamp. Reuse a passing ledger entry for the same HEAD instead of rerunning the same gate.

## Parallel Execution Contract

| Task | Parallel batch | File ownership | Serialization required | Dependency reason |
|---|---|---|---|---|
| Task 0: Baseline measurement | None - serial | Creates `docs/evidence/2026-09-audit-wave-1-baseline.md` | Yes | Must run on the pre-change tree. |
| Task 1: Delete symbol `code_context` | Batch A | Modify `crates/julie-extractors/src/base/extractor.rs`, `base/creation_methods.rs`, `base/types.rs`, `markdown/semantic_symbols.rs`, `go/functions.rs`, `vue/test_calls.rs`, `src/tests/base.rs` | No | None - safe parallel batch. |
| Task 2: Delete `BaseExtractor::symbol_map` | Batch A | Modify `crates/julie-extractors/src/base/extractor.rs` (struct field only, coordinate with Task 1 by editing disjoint lines), `cpp/functions.rs`, `ruby/mod.rs`, `ruby/assignments.rs`, `erlang/mod.rs`, `erlang/definition_forms.rs`, `elixir/mod.rs`, `go/functions.rs` (the insert line only) | Yes | Shares `extractor.rs` and `go/functions.rs` with Task 1. Dispatch after Task 1 lands. |
| Task 3: Shared containing-symbol index | None - serial | Create `crates/julie-extractors/src/base/containing_symbol_index.rs`; modify `base/creation_methods.rs`, `base/mod.rs`, `rust/identifiers/containing_symbols.rs`, `rust/identifiers/mod.rs`, `typescript/relationships.rs`, `javascript/relationships.rs`, `sql/mod.rs` | Yes | Depends on Task 2 (removes the map the old lookup path used). |
| Task 4: Remove the import spool detour | Batch B | Modify `crates/julie-extract-cli/src/store/executor.rs` lines 36 and 587-648 only | No | None - safe parallel batch. |
| Task 5: Capability snapshot once per quantum | Batch B | Modify `crates/julie-extract-cli/src/store/executor.rs` (call sites at 1126 and 1723, `execute_quantum`), `crates/julie-extract-artifact/src/store/writer.rs` (`write_level_in_transaction`), `crates/julie-extract-artifact/tests/store_writer_performance.rs` | Yes | Shares `executor.rs` with Task 4. Dispatch after Task 4 lands. |
| Task 6: Detect language once per file | Batch C | Modify `crates/julie-extract-cli/src/extraction.rs`, `crates/julie-extract-cli/src/commands.rs` (line 1882 region), `crates/julie-extract-cli/src/store/executor.rs` (line 605 region) | Yes | Shares `executor.rs` with Task 5. Dispatch after Task 5 lands. |
| Task 7: After measurement and closure | None - serial | Modify `docs/evidence/2026-09-audit-wave-1-baseline.md`, `docs/findings/2026-09-04-architecture-and-performance-audit.md` | Yes | Needs every other task merged. |

Commit mode: `serial-worker-commit` for serial tasks; `parallel-lead-commit` inside Batch A, B, C.

---

## Task 0: Baseline measurement

**Files:** Create `docs/evidence/2026-09-audit-wave-1-baseline.md`.

**What to build:** A recorded before-measurement. Run on the unchanged tree at the plan's starting commit.

**Approach:**
1. `cargo build --release -p julie-extract-cli --bin julie-extract`.
2. `cargo xtask performance baseline --root . --out-dir target/performance/audit-wave-1-before --binary target/release/julie-extract --runs 3`.
3. `cargo xtask performance writer-current-schema --out-dir target/performance/audit-wave-1-writer-before`.
4. Store import path: create a temp family store and time `julie-extract store import` of this repository three times. Use the exact command from `docs/contracts/cli.md`. Record min, median, max wall clock.
5. Record every number, the commit SHA, the machine, and the commands in the evidence file.

**Acceptance criteria:**
- [x] Evidence file exists with all three measurements, commit SHA, and commands.
- [x] No source file changed.

## Task 1: Delete symbol `code_context`

**Files:**
- Modify `crates/julie-extractors/src/base/extractor.rs`: remove `line_ranges` field (line 34), its construction (line 67, 79), `extract_code_context` (line 372), `content_line_ranges` (line 480) and its unit test.
- Modify `crates/julie-extractors/src/base/creation_methods.rs`: remove the call at line 43 and the field at line 77. Set nothing; the field goes away.
- Modify `crates/julie-extractors/src/base/types.rs`: remove `ContextConfig` (line 225) and `Symbol.code_context` (line 306). Keep `Identifier.code_context` (line 384): the identifiers table owns that column and the CLI writes `None`.
- Modify `crates/julie-extractors/src/markdown/semantic_symbols.rs:416`, `go/functions.rs:88`, `vue/test_calls.rs:104`: remove the context builders.
- Modify every `code_context: None` on `Symbol` literals: `base/containing_symbol.rs:136`, `csharp/mod.rs:84`, `rust/identifiers/containing_symbols.rs:225`, `vue/manual_symbols.rs:68`, and any others the compiler reports.
- Modify `crates/julie-extractors/src/tests/base.rs`: delete the `ContextConfig` tests.

**Interfaces:** `Symbol` loses `code_context`. `BaseExtractor` loses `extract_code_context`. `Identifier.code_context` stays.

**What to build:** Nothing new. Delete the field and everything that exists only to fill it.

**Approach:** Remove the field first, then follow compiler errors. Check `grep -rn code_context crates/julie-extract-cli/src crates/julie-extract-artifact/src` afterwards: every remaining hit must be an identifier row, not a symbol row. Confirm the golden tier passes without regeneration; symbols never serialized this field, so nothing should change.

**Acceptance criteria:**
- [x] `Symbol` has no `code_context` field; `ContextConfig`, `extract_code_context`, `line_ranges`, `content_line_ranges` are gone.
- [x] `cargo xtask test language markdown`, `go`, `vue` pass.
- [x] `cargo xtask test golden` passes with zero fixture changes.

## Task 2: Delete `BaseExtractor::symbol_map`

**Files:**
- Modify `crates/julie-extractors/src/base/extractor.rs`: remove the `symbol_map` field and its initialization.
- Modify `crates/julie-extractors/src/base/creation_methods.rs:81`: remove the insert. Return the symbol without cloning.
- Modify `crates/julie-extractors/src/cpp/functions.rs` (reads at 375, 388, 441): build a local `HashMap<&str, &Symbol>` from the symbols vector the function already has, or pass the symbols slice.
- Modify `crates/julie-extractors/src/ruby/mod.rs` (clear at 54, iterate at 67) and `ruby/assignments.rs` (get at 81, insert at 161): give the Ruby extractor its own `HashMap<String, Symbol>` field for the assignment path. Keep behavior identical.
- Modify `crates/julie-extractors/src/erlang/mod.rs:169` and `erlang/definition_forms.rs:147` (body-hash rewrite): read from the symbols vector by id instead.
- Modify `crates/julie-extractors/src/elixir/mod.rs:55`: delete the `clear()` call.
- Modify `crates/julie-extractors/src/go/functions.rs:95`: delete the insert.

**Interfaces:** `BaseExtractor` loses the public `symbol_map` field. No other signature changes.

**What to build:** Local indexes in cpp, ruby, and erlang that replace the shared map for their own reads.

**Approach:** Inspect each reader with Miller before editing. Ruby's assignment path both reads and writes the map during extraction, so it needs a real replacement, not a one-shot index. Run `cargo xtask test language` for cpp, ruby, erlang, elixir, go, and php.

**Acceptance criteria:**
- [x] `grep -rn symbol_map crates/julie-extractors/src/base` returns nothing.
- [x] `create_symbol` and `create_symbol_from_span` return the symbol without a clone.
- [x] Language tiers for cpp, ruby, erlang, elixir, go pass. Golden tier passes with zero fixture changes.

## Task 3: Shared containing-symbol index for identifier lookup

**Files:**
- Create `crates/julie-extractors/src/base/containing_symbol_index.rs`: move `ContainingSymbolIndex`, `symbol_contains_position`, `is_better_containing_symbol`, `symbol_priority`, and their tests from `rust/identifiers/containing_symbols.rs`. Make the type `pub(crate)`.
- Modify `crates/julie-extractors/src/base/mod.rs`: declare the module and re-export the index.
- Modify `crates/julie-extractors/src/base/creation_methods.rs`: `find_containing_symbol` (236), `find_containing_symbol_from_map` (244), `find_containing_symbol_from_map_filtered` (252), and `find_containing_symbol_from_iter` (267) run on the index. Keep the public signatures so the 30 per-language wrappers named `find_containing_symbol_id` do not change in this task. Preserve the current priority and tie-break rules exactly; the Rust index test at `containing_symbols.rs:96` documents them.
- Modify `crates/julie-extractors/src/rust/identifiers/containing_symbols.rs` and `rust/identifiers/mod.rs`: use the shared index.
- Modify `crates/julie-extractors/src/typescript/relationships.rs:65` and `javascript/relationships.rs:63`: return `&Symbol` instead of `.cloned()`.
- Modify `crates/julie-extractors/src/sql/mod.rs:102`: build the `HashMap<String, &Symbol>` once per walk, not per string-literal node.

**Interfaces:** `find_containing_symbol*` keep their signatures. New crate-private `ContainingSymbolIndex::new(&[Symbol], file_path)` and `find(node) -> Option<&Symbol>`.

**What to build:** One index implementation used by every lookup. The index is built once per identifier pass. Where a language calls the lookup inside a loop over identifiers, the wrapper must construct the index once outside the loop; check each of the 30 wrappers with Miller `trace` and fix any that would rebuild it per call.

**Approach:** Write the base test first: an inline source with nested symbols, assert the same containing symbol the old filter-and-sort returned for a function, a nested function, a class method, and a top-level identifier with no container. Then swap the implementation. The old comparator sorted by priority, then span size, then start position; the index must produce identical answers, so keep the old function as a test oracle inside the test module until the comparison test passes, then delete it.

**Acceptance criteria:**
- [x] `find_containing_symbol_from_iter` no longer collects a `Vec` or sorts per call.
- [x] Oracle comparison test passes for at least four languages (rust, typescript, python, csharp) over their `basic` golden sources.
- [x] `cargo xtask test golden` passes with zero fixture changes.
- [x] `cargo xtask test default` passes.

## Task 4: Remove the store import spool detour

**Files:** Modify `crates/julie-extract-cli/src/store/executor.rs`: delete `IMPORT_SPOOL_IO` (line 36) and lines 620-647 in `extract`. Call `StoreFileVersion::try_from_artifact_file(EXTRACTION_IDENTITY_EPOCH, &artifact)` directly after the `Spooled` progress advance.

**Interfaces:** None change. `extract` keeps its signature. The `spool_dir` parameter becomes unused in this function; remove it from the signature only if no other caller needs it, otherwise leave it and report.

**What to build:** Nothing. Delete the round trip.

**Approach:** Confirm with Miller `trace` that `create_scan_spool` still has callers in the scan path (it does; do not delete it). Run the store contract tests. Confirm the `Spooled` progress counter still advances once per file so the progress report contract is unchanged.

**Acceptance criteria:**
- [ ] No `Mutex` in `executor.rs`.
- [ ] `store import` of `fixtures/extraction` produces a store whose `file_versions` rows are identical to before (compare row counts and content hashes with a SQL query in the test).
- [ ] `cargo test -p julie-extract-cli --test operations_contract` and `--test store_cli_contract` pass.

## Task 5: Build the capability snapshot once per quantum

**Files:**
- Modify `crates/julie-extract-cli/src/store/executor.rs`: in `execute_quantum` (line 1492) and the from-artifact chunk loop (line 1108), build `let snapshot = artifact_capability_snapshot();` once before the file loop. Pass `Some(&snapshot)` only for the first L1 write in the quantum, and `None` for later files. Fix the `then_some` at line 1126 so L2 and L3 never build a snapshot.
- Modify `crates/julie-extract-artifact/src/store/writer.rs::write_level_in_transaction` (line 385): when `initialized` is true and a snapshot is given, run only the conflict check (`capability_snapshot_matches`), not the full `sync_capability_snapshot`. When `initialized` is true and no snapshot is given, proceed as today.
- Modify `crates/julie-extract-artifact/tests/store_writer_performance.rs:43-47`: update the expected preparation counts to the new values and add one test that a second L1 write with `Some(snapshot)` on an initialized epoch does not call `sync_capability_snapshot`.

**Interfaces:** `write_level_in_transaction` keeps its signature. Its contract changes: a snapshot on an initialized epoch is verified, not re-synced. Record this in the function's doc comment.

**What to build:** A once-per-quantum snapshot and a writer that syncs once per epoch.

**Approach:** Write the writer test first (an initialized epoch plus a matching snapshot must yield zero capability upserts). Then change the executor. Keep the conflict path: a mismatching snapshot on an initialized epoch must still return `CapabilitySnapshotConflict`; add that test too.

**Acceptance criteria:**
- [ ] `artifact_capability_snapshot()` is called at most once per `execute_quantum` and once per from-artifact chunk.
- [ ] Writer test proves no re-sync on an initialized epoch and a conflict on a mismatch.
- [ ] Store contract tests and `store_writer_performance` pass with updated counts.

## Task 6: Detect language once per file

**Files:**
- Modify `crates/julie-extract-cli/src/extraction.rs:177`: `extract_artifact_file_from_snapshot_at` uses the `language` argument it receives. Remove the internal `detect_language_for_source` call.
- Modify `crates/julie-extract-cli/src/commands.rs:1882`: keep the single content-sniffing detection here; this is the authoritative call for scan.
- Modify `crates/julie-extract-cli/src/store/executor.rs:605`: keep the single detection here for store import.
- Check `extraction.rs` for any other caller that passed a placeholder language and relied on re-detection; Miller `trace` on `extract_artifact_file_from_snapshot_at`. Fix each to pass the real language.

**Interfaces:** `extract_artifact_file_from_snapshot_at` now trusts its `language` argument. Document that in its doc comment.

**What to build:** Nothing new; remove the second and third detections.

**Approach:** Add a test in `crates/julie-extract-cli/tests/operations_contract.rs` that scans a `.h` fixture whose content is C++ and asserts the language recorded is the same as before. Header files are the case where extension-only and content-sniffing detection disagree, so this test proves the authoritative call survived.

**Acceptance criteria:**
- [ ] `detect_language_for_source` has exactly one call in the scan path and one in the store extract path.
- [ ] Header-file language test passes.
- [ ] Golden tier and operations contract pass.

## Task 7: After measurement and findings closure

**Files:** Modify `docs/evidence/2026-09-audit-wave-1-baseline.md`, `docs/findings/2026-09-04-architecture-and-performance-audit.md`.

**What to build:** The after-measurement with the same three commands as Task 0 on the merged branch, and a closure note on E1, E2, E3 (identifier lookup only), C1, C2, A2, C4 in the findings document with the commit SHA of each fix.

**Approach:** Run the commands, paste the numbers next to the before numbers, and compute percentages. If any median is worse by more than 5 percent, stop and report; do not close the finding.

**Acceptance criteria:**
- [ ] Evidence file has before and after tables.
- [ ] Findings document marks each closed item with the fixing commit.
- [ ] No median regressed beyond 5 percent.
