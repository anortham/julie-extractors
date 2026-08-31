# Extraction identity epoch 8 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Bump `EXTRACTION_IDENTITY_EPOCH` from 7 to 8 so family stores re-extract C# after BRE-16.

**Architecture:** File-version identity is `(path, content_hash, extraction_epoch)`. BRE-16 already emits `visibility=internal` for explicit C# `internal`, but v2.38.0 left the epoch at 7, so store import reuses completed epoch-7 rows. Epoch 8 allocates a new `file_versions` row for unchanged content. Capability snapshot rows are keyed by extraction epoch; the bump writes a fresh snapshot and avoids the epoch-7 F# collision (`INSERT OR IGNORE` + exact match).

**Tech Stack:** Rust workspace, SQLite family store, extraction-output ledger, crate version 2.38.2 candidate metadata.

**Architecture Quality:** No Architecture Impact. This is the same epoch-bump contract used for epoch 7 (`a3073cbd`).

## Global Constraints

- `EXTRACTION_IDENTITY_EPOCH` becomes `8`.
- Do not add `record_struct_declaration` to C# `extract_symbol`.
- Do not delete live `file_versions` rows.
- Do not change miller. Miller 1.26.0 stays on julie-extract 2.38.1 / epoch 7 until a later pin.
- Do not push, tag, or publish.
- Keep artifact schema 7, JSONL v5, store schema 2, store format epoch 1.
- Classify the change as compatible.
- Crate version becomes 2.38.2 so published v2.38.1 (epoch 7) and this source do not share a version number.
- Ledger heading is `## 2.38.2`.
- Capability snapshot writes stay keyed to `version.extraction_epoch()` (now 8). Do not reuse epoch-7 snapshot rows.
- Epoch 7 rows stay immutable.

## Verification Strategy

**Project source of truth:** `AGENTS.md`, `docs/testing-strategy.md`, `docs/contracts/{store-v1,extraction-output-changes}.md`, `docs/release.md`.

**Worker red/green scope:** `cargo test -p julie-extractors --lib tests::api_surface::test_public_contract_version_marks_current_fact_families -- --exact`

**Worker ceiling:** Focused epoch, store-import identity, store-equivalence, ledger-parse, and release-preflight tests. Workers do not push, tag, or publish.

**Worker gate invariant:** Current epoch asserts 8; store import of unchanged content at a new epoch allocates a new `file_versions` row; current crate version is 2.38.2; ledger `## 2.38.2` parses as compatible.

**Lead affected-change scope:** The worker tests plus `cargo xtask test default`.

**Branch gate:** `cargo xtask test default`; `cargo fmt --all -- --check`; `scripts/check-agent-doc-sync.sh`; `git diff --check`; `cargo xtask release preflight --version 2.38.2`.

**Replay/metric evidence:** Epoch assertion, store identity test, and version/ledger parse are hard gates. Timing is report-only.

**Escalation triggers:** Same-epoch output change, capability snapshot conflict, version mismatch, or a test that still names epoch 7 as current.

**Assigned verification failure:** Fix the candidate. Do not weaken gates.

**Verification ledger:** Record invariant, command, scope, commit SHA, result, and timestamp in the Goldfish checkpoint.

## Parallel Execution Contract

| Task | Parallel batch | File ownership | Serialization required | Dependency reason |
|---|---|---|---|---|
| Task 1: Fail the current-epoch assertion at 8 | None - serial | Modify `crates/julie-extractors/src/tests/api_surface.rs` | Yes | TDD red step before production change. |
| Task 2: Bump the epoch constant and current-epoch pins | None - serial | Modify `crates/julie-extractors/src/lib.rs`, `docs/contracts/store-v1.md`, `docs/architecture/versioned-index-store.md`, `docs/contracts/extraction-output-changes.md` | Yes | Production change after the failing assertion. |
| Task 3: Prepare 2.38.2 candidate metadata | None - serial | Modify three `Cargo.toml` files, `Cargo.lock`, workflow defaults, `docs/release-notes/`, `TODO.md` | Yes | Version and ledger must match after the epoch is 8. |

## Task Structure

### Task 1: Fail the current-epoch assertion at 8

**Files:**
- Modify: `crates/julie-extractors/src/tests/api_surface.rs:20`

**Interfaces:**
- Consumes: `crate::EXTRACTION_IDENTITY_EPOCH`
- Produces: a failing assertion that the current epoch is 8

**Contract inputs:** Current source still has `EXTRACTION_IDENTITY_EPOCH = 7`.

**File ownership:** Modify `crates/julie-extractors/src/tests/api_surface.rs`

**Serialization required:** Yes

**Dependency reason:** TDD red step before production change.

**What to build:** Change `assert_eq!(extraction_identity_epoch, 7)` to `assert_eq!(extraction_identity_epoch, 8)` and watch it fail with left 7 / right 8.

**Approach:** Follow razorback:test-driven-development. Do not bump the constant until the failure is observed.

**Acceptance criteria:**
- [x] The focused api_surface test fails because the constant is still 7
- [x] Worker-scope verification is the failing red run, then handed to Task 2

### Task 2: Bump the epoch constant and current-epoch pins

**Files:**
- Modify: `crates/julie-extractors/src/lib.rs:134`
- Modify: `docs/contracts/store-v1.md:19`
- Modify: `docs/architecture/versioned-index-store.md:42-47`
- Modify: `docs/contracts/extraction-output-changes.md` (new `## 2.38.2` section above `## 2.38.0`)

**Interfaces:**
- Consumes: Task 1 failing assertion
- Produces: `EXTRACTION_IDENTITY_EPOCH = 8` and current-epoch contract docs

**Contract inputs:** Epoch-7 bump `a3073cbd`. Store identity test `extraction_epoch_change_creates_a_new_version_for_unchanged_content` already covers new `file_versions` allocation. Capability snapshot sync already keys rows by `version.extraction_epoch()`.

**File ownership:** Modify `crates/julie-extractors/src/lib.rs`, `docs/contracts/store-v1.md`, `docs/architecture/versioned-index-store.md`, `docs/contracts/extraction-output-changes.md`

**Serialization required:** Yes

**Dependency reason:** Production change after the failing assertion.

**What to build:** Set the constant to 8. Mirror 8 in store-v1. Fix the stale architecture sentence that still says the current build uses epoch 6. Add a compatible `## 2.38.2` ledger entry: C# `internal` already ships in 2.38.0, epoch 8 is the identity catch-up so family stores re-extract, capability snapshot is keyed to 8, consumer action is replace binary and let epoch-8 versions populate.

**Approach:** Match the epoch-7 bump. Do not regenerate golden extraction fixtures. Do not rewrite epoch-7 rows.

**Acceptance criteria:**
- [x] `EXTRACTION_IDENTITY_EPOCH == 8`
- [x] The focused api_surface test passes
- [x] `store-v1.md` names epoch 8 as current
- [x] Ledger `## 2.38.2` exists with `classification: compatible`
- [x] Worker-scope verification passes and the change is handed to Task 3

### Task 3: Prepare 2.38.2 candidate metadata

**Files:**
- Modify: `crates/julie-extractors/Cargo.toml`
- Modify: `crates/julie-extract-cli/Cargo.toml`
- Modify: `crates/julie-extract-artifact/Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `.github/workflows/release-binaries.yml`
- Modify: `.github/workflows/specialist-gates.yml`
- Modify: `docs/release-notes/README.md`
- Create: `docs/release-notes/v2.38.2.md`
- Modify: `TODO.md`

**Interfaces:**
- Consumes: epoch 8 and ledger `## 2.38.2`
- Produces: a version-consistent unpublished 2.38.2 candidate

**Contract inputs:** v2.38.1 candidate pattern (`818fee0c`). v2.38.1 remains the current published release. Do not claim publication.

**File ownership:** Modify three `Cargo.toml` files, `Cargo.lock`, workflow defaults, `docs/release-notes/`, `TODO.md`; create `docs/release-notes/v2.38.2.md`

**Serialization required:** Yes

**Dependency reason:** Version and ledger must match after the epoch is 8.

**What to build:** Advance publishable crates to 2.38.2. Add a candidate release note that the identity epoch is 8, extraction fact tables stay byte-identical to 2.38.1, and miller must pin the new extract release later. Fold the main-working-tree epoch-8 TODO brief into this branch and record that the bump is in source, with release and miller pin still open.

**Approach:** Follow `818fee0c`. Keep v2.38.1 labeled as the current published release.

**Acceptance criteria:**
- [x] Three publishable crates report 2.38.2
- [x] Workflow defaults report 2.38.2
- [x] Candidate release note exists and does not claim publication
- [x] `TODO.md` records the bump and the remaining release/miller-pin work
- [x] `cargo test -p julie-extract-cli --test store_import_contract extraction_epoch_change_creates_a_new_version_for_unchanged_content -- --exact` passes
- [x] Worker-scope verification passes
