# CLI Command Orchestration Split Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Reduce `crates/julie-extract-cli/src/commands.rs` size and coupling by moving stable helper families into focused internal modules without changing CLI behavior.

**Architecture:** Keep `commands.rs` as the command dispatcher and high-level operation orchestrator. Move self-contained helper families behind `pub(crate)` functions and data types so command handlers call named modules instead of owning all artifact/report/capability details.

**Tech Stack:** Rust, `julie-extract-cli`, `julie-extract-artifact`, existing CLI integration tests.

**Architecture Quality:** This is an internal module-boundary refactor. Main risk is accidentally changing public report, exit-code, schema, or capability snapshot behavior while moving private helpers; use existing contract tests plus small architecture convention tests to guard the split.

---

## File Structure

- Create: `crates/julie-extract-cli/src/capability_snapshot.rs`
  Owns artifact capability snapshot mapping, cargo-lock parser inventory lookup, and capability fingerprint helpers.
- Create: `crates/julie-extract-cli/src/artifact_access.rs`
  Owns read-only artifact opening, metadata/version checks, existing hash loading, and artifact report assembly.
- Create: `crates/julie-extract-cli/src/reports.rs`
  Owns base report construction, command error helpers, path/extraction/write/spool error report mapping, display helpers, and report writing.
- Modify: `crates/julie-extract-cli/src/commands.rs`
  Keep command handlers, scan/update/delete orchestration, extraction spooling, and write-flow decisions.
- Modify: `crates/julie-extract-cli/src/main.rs`
  Register new internal modules.
- Test: `crates/julie-extract-cli/tests/cli_contract.rs`
  Add convention tests that prevent moved helper families from drifting back into `commands.rs`.

## Task 1: Extract Capability Snapshot Mapping

**Files:**
- Create: `crates/julie-extract-cli/src/capability_snapshot.rs`
- Modify: `crates/julie-extract-cli/src/commands.rs`
- Modify: `crates/julie-extract-cli/src/main.rs`
- Test: `crates/julie-extract-cli/tests/cli_contract.rs`

**What to build:** Move `artifact_capability_snapshot`, capability/parser fingerprint helpers, cargo-lock parsing, and capability row mapping out of `commands.rs`.

**Approach:** Add a failing architecture test that asserts `commands.rs` no longer defines capability snapshot mapping helpers. Preserve exact function behavior and expose only the helpers used by command handlers: `artifact_capability_snapshot`, `current_capability_fingerprints`, and `flags`.

**Acceptance criteria:**
- [x] Failing convention test proves `commands.rs` still owns capability snapshot mapping before the move.
- [x] `commands.rs` imports capability helpers from the new module.
- [x] CLI contract tests pass.

## Task 2: Extract Report/Error Mapping

**Files:**
- Create: `crates/julie-extract-cli/src/reports.rs`
- Modify: `crates/julie-extract-cli/src/commands.rs`
- Modify: `crates/julie-extract-cli/src/main.rs`
- Test: `crates/julie-extract-cli/tests/cli_contract.rs`

**What to build:** Move report construction, command outcome helpers, diagnostics, path/extraction/write/spool error mapping, and JSON/human output helpers behind a report module.

**Approach:** Keep `CommandOutcome` public only inside the crate if needed by `commands.rs`. Do not change serialized report shapes, streams, or exit codes.

**Acceptance criteria:**
- [x] Report/error helpers are no longer defined in `commands.rs`.
- [x] CLI contract, operations contract, and path policy tests pass.
- [x] Public CLI output remains byte-compatible for existing tests.

## Task 3: Extract Artifact Access And Version Checks

**Files:**
- Create: `crates/julie-extract-cli/src/artifact_access.rs`
- Modify: `crates/julie-extract-cli/src/commands.rs`
- Modify: `crates/julie-extract-cli/src/main.rs`
- Test: `crates/julie-extract-cli/tests/cli_contract.rs`

**What to build:** Move read-only artifact open/report/version/hash helpers into an artifact access module.

**Approach:** Add a convention test for artifact-open helpers, then move `OpenArtifact`, `OpenInfoArtifact`, `ExistingArtifact`, `CommandError`, `open_artifact*`, `existing_artifact_for_root`, `load_existing_content_hashes`, `check_versions`, `artifact_report*`, metadata readers, table totals, JSONL counts, and revision lookup as a cohesive internal API.

**Acceptance criteria:**
- [x] Existing CLI behavior and error/report shapes stay unchanged.
- [x] `commands.rs` no longer defines read-only artifact access/version helpers.
- [x] `cargo test -p julie-extract-cli --test cli_contract` and `cargo test -p julie-extract-cli --test operations_contract` pass.

## Verification Strategy

**Project source of truth:** `AGENTS.md`, `docs/testing-strategy.md`, `RAZORBACK.md`, and CLI contract tests.

**Worker red/green scope:** For each slice, run the new/changed CLI convention test first to see the intended failure, then run `cargo test -p julie-extract-cli --test cli_contract` after implementation.

**Worker ceiling:** `cargo test -p julie-extract-cli --test cli_contract`, `cargo test -p julie-extract-cli --test operations_contract`, and `cargo test -p julie-extract-cli --test path_policy`.

**Worker gate invariant:** Public commands, JSON report shapes, exit codes, path policy, and operation behavior remain unchanged while helper ownership moves.

**Lead affected-change scope:** `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --all-features --no-deps -- -D warnings`, and CLI integration tests after a coherent batch.

**Branch gate:** `cargo xtask test default`; run `cargo xtask test contract` if report, schema, JSONL, or exit-code behavior changes beyond mechanical movement.

**Replay/metric evidence:** No replay or performance metric gate for this refactor. Default-suite runtime growth is report-only unless it is unexpected or large.

**Escalation triggers:** Any changed serialized report field, exit code, error code, SQLite schema metadata interpretation, capability claim, or parser dependency version requires strategy-tier review.

**Assigned verification failure:** Workers stop and report when assigned verification fails, unless the failure is the expected RED convention test before implementation.

**Verification ledger:** Record invariant, command, scope label, commit SHA, result, and timestamp in the final checkpoint for each completed slice.

## Model Routing

**Project source of truth:** `RAZORBACK.md`.

**Strategy tier:** Public CLI contract, report/error contract, and decomposition decisions.
- Harness mapping: inherit.

**Implementation tier:** Bounded helper-family extraction when public behavior is already decided.
- Harness mapping: inherit.

**Mechanical tier:** Formatting, imports, and rote move cleanup.
- Harness mapping: inherit.

**Gate-interpretation reviewer:** Strategy tier for any CLI/report/exit-code evidence ambiguity.
- Harness mapping: inherit.

**Escalation tier:** Schema/report/release/capability/parser dependency changes.
- Harness mapping: inherit.

**Worker eligibility:** Use workers only for non-overlapping helper-family moves with explicit verification ceilings.

**Escalation triggers:** Same as `RAZORBACK.md`: public artifact schema, CLI status/exit/error changes, capability claims, parser versions, weak evidence, old Julie coupling, or unexpected default-suite runtime growth.

**Mechanical exclusion:** Mechanical workers cannot own failing tests, report evidence interpretation, or acceptance gates.

**Unsupported harness behavior:** This session does not require per-agent model routing; inherit the active harness defaults.
