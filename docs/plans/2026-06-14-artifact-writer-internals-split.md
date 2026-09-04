# Artifact Writer Internals Split Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Reduce `crates/julie-extract-artifact/src/writer.rs` coupling by moving stable writer helper families into focused internal submodules without changing the public `ArtifactWriter` API or artifact schema behavior.

**Architecture:** Keep `writer.rs` as the public writer facade and transaction orchestration module. Move capability snapshot synchronization into `writer/capabilities.rs` and row-family insertion/lookup helpers into `writer/rows.rs`; both modules stay private to the `writer` module and expose only the functions/types used by the facade.

**Tech Stack:** Rust, `julie-extract-artifact`, SQLite via `rusqlite`, existing writer contract/performance tests.

**Architecture Quality:** Medium-risk internal module-boundary refactor. The caller-facing interface is still `ArtifactWriter`; the risk is accidental schema/write-count/data-loss-guard behavior drift while moving private SQL helpers. Tests stay at the public writer and CLI contract surfaces, with convention tests guarding helper ownership.

---

## File Structure

- Create: `crates/julie-extract-artifact/src/writer/capabilities.rs`
  Owns capability snapshot key loading, deletions, upserts, JSON serialization, and boolean conversion.
- Create: `crates/julie-extract-artifact/src/writer/rows.rs`
  Owns file/child row inserters, row-family insert functions, preserved-failure update helpers, parse-diagnostic replacement, and symbol/identifier/type-argument lookup helpers.
- Modify: `crates/julie-extract-artifact/src/writer.rs`
  Keep public errors, spool types, `ArtifactWriter`, transaction/revision orchestration, metadata writing, data-loss guard, delete/revision helpers, and module wiring.
- Modify: `crates/julie-extract-artifact/tests/writer_contract.rs`
  Add convention tests that prevent the moved helper families from drifting back into `writer.rs`.

## Task 1: Extract Capability Snapshot Sync

**Files:**
- Create: `crates/julie-extract-artifact/src/writer/capabilities.rs`
- Modify: `crates/julie-extract-artifact/src/writer.rs`
- Test: `crates/julie-extract-artifact/tests/writer_contract.rs`

**What to build:** Move `sync_optional_capability_snapshot_in_tx`, `sync_capability_snapshot_in_tx`, capability key loaders, parser/language/fixture/gap upserts, `bool_int`, and capability JSON serialization out of `writer.rs`.

**Approach:** Add a failing writer convention test first. Preserve `ArtifactWriter::sync_capability_snapshot` and staged snapshot behavior exactly; `writer.rs` should call the moved `capabilities::sync_optional_capability_snapshot_in_tx` and `capabilities::sync_capability_snapshot_in_tx` helpers.

**Acceptance criteria:**
- [x] Failing convention test proves `writer.rs` still owns capability sync helpers before the move.
- [x] Capability snapshot sync contract test still writes static rows once and reports unchanged rows on the second sync.
- [x] Writer contract tests pass.

## Task 2: Extract Row-Family Inserters

**Files:**
- Create: `crates/julie-extract-artifact/src/writer/rows.rs`
- Modify: `crates/julie-extract-artifact/src/writer.rs`
- Test: `crates/julie-extract-artifact/tests/writer_contract.rs`

**What to build:** Move `FileRowInserters`, `ChildRowInserters`, row-family insert functions, preserved-failure helpers, parse-diagnostic replacement, and lookup helpers out of `writer.rs`.

**Approach:** Add a failing convention test first. Keep transaction orchestration in `writer.rs`; expose only the row helper types/functions needed by scan/update/spooled flows. Move the writer prepare metrics with the row inserters so the existing performance tripwire still measures prepared-statement setup.

**Acceptance criteria:**
- [x] Failing convention test proves `writer.rs` still owns row inserters before the move.
- [x] Existing writer contract and writer batching contract tests pass.
- [x] `writer.rs` no longer defines row inserter structs or row-family insert helpers.

## Verification Strategy

**Project source of truth:** `AGENTS.md`, `RAZORBACK.md`, `docs/testing-strategy.md`, and writer contract/batching contract tests.

**Worker red/green scope:** For each slice, run the new convention test first to see the intended failure, then run the focused writer test that covers the moved behavior.

**Worker ceiling:** `cargo test -p julie-extract-artifact --test writer_contract` and `cargo test -p julie-extract-artifact --test writer_batching_contract`.

**Worker gate invariant:** Public `ArtifactWriter` methods, SQLite rows, revision counts, capability row counts, data-loss guard behavior, and prepared-statement batching remain unchanged.

**Lead affected-change scope:** `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --all-features --no-deps -- -D warnings`, `cargo test -p julie-extract-artifact --test writer_contract`, and `cargo test -p julie-extract-artifact --test writer_batching_contract`.

**Branch gate:** `cargo xtask test default`. Run `cargo xtask test contract` only if schema/report/JSONL contracts change, which this plan should avoid.

**Replay/metric evidence:** Writer batching contract tests are hard gates for prepared-statement batching behavior. No new report-only metric is required.

**Escalation triggers:** Any public API change, schema SQL change, row count change, data-loss guard behavior change, or unexpected default-suite runtime growth requires strategy-tier review.

**Assigned verification failure:** Workers stop and report when assigned verification fails, unless the failure is the expected RED convention test before implementation.

**Verification ledger:** Record invariant, command, scope label, commit SHA, result, and timestamp in the final checkpoint.

## Model Routing

**Project source of truth:** `RAZORBACK.md`.

**Strategy tier:** Public writer API, schema behavior, row count behavior, and data-loss guard interpretation.
- Harness mapping: inherit.

**Implementation tier:** Bounded internal helper extraction with public behavior already decided.
- Harness mapping: inherit.

**Mechanical tier:** Formatting, imports, and rote move cleanup.
- Harness mapping: inherit.

**Gate-interpretation reviewer:** Strategy tier for any ambiguity in writer contract/performance evidence.
- Harness mapping: inherit.

**Escalation tier:** Schema/API/report/release/capability/parser dependency changes or repeated verification failures.
- Harness mapping: inherit.

**Worker eligibility:** Workers are not used for this run because both tasks touch the same large file and are sequential.

**Escalation triggers:** Same as `RAZORBACK.md`: public schema/API changes, weak evidence, old Julie coupling, or unexpected default-suite runtime growth.

**Mechanical exclusion:** Mechanical workers cannot own failing tests, writer performance evidence, or acceptance gates.

**Unsupported harness behavior:** This session does not require per-agent model routing; inherit the active harness defaults.
