# v0.1.0 Release Candidate Audit Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Produce the v0.1.0 release-candidate audit evidence and update release notes so the standalone extraction product is ready for a release decision.

**Architecture:** This slice started as a release documentation and evidence audit. The audit found a release-blocking contract mismatch: SQLite/JSONL/report contracts exposed parser inventory and language capability rows, but real artifacts had zero rows. The slice therefore includes a targeted product fix to persist the existing capability snapshot without changing language capability claims. It uses the existing release package staging command as the hard packaging proof, and it treats dogfood/performance timings as report-only evidence. Public CLI, SQLite, JSONL, report, and Rust crate contract shapes remain unchanged.

**Tech Stack:** Rust workspace, `julie-extract`, `cargo xtask release package`, release binary, SHA-256 checksums, markdown release notes and evidence docs.

**Architecture Quality:** Medium Architecture Impact. The work preserves the public v1 schema shape but fills previously empty contract tables from the existing extractor capability snapshot. It does not add parser dependencies, change capability claims, or change release package manifest logic.

---

## Source Documents

- `AGENTS.md`: product boundary, release discipline, and non-goals.
- `RAZORBACK.md`: release packaging is strategy-tier; lead owns release and budget gates.
- `docs/release.md`: documented release commands and package contents.
- `docs/release-notes/v0.1.0.md`: public release note that must match shipped surfaces.
- `docs/release-evidence/2026-06-01-repeatable-performance-baseline.md`: latest repeatable performance evidence.
- `docs/plans/2026-06-01-product-completion-tracker.md`: Slice 5 acceptance criteria.
- `xtask/src/release.rs`: package manifest source of truth.

## Current Baseline

- `main` is at `1440759` after PR #7 merged the repeatable performance baseline.
- CI Fast Gates passed on PR #7 before merge.
- Release package manifest from `cargo xtask release package-list` contains:
  `julie-extract`, its SHA-256 checksum, CLI/SQLite/JSONL/report contracts,
  product-boundary and schema-principles docs, testing/release docs, and
  `docs/release-notes/v0.1.0.md`.
- Host target for this audit is `aarch64-apple-darwin`.
- Latest performance evidence records 3 release-profile runs with stable
  outputs: `1018` files, `33019` symbols, `215388` JSONL records, and JSONL
  export min/median/max `1330ms` / `1330ms` / `1333ms`.

## Verification Strategy

**Project source of truth:** `AGENTS.md`, `RAZORBACK.md`,
`docs/testing-strategy.md`, `docs/release.md`, and `xtask` release contract
tests.

**Worker red/green scope:** The audit fix is covered at the CLI and artifact
writer surfaces. For the capability-row fix, use
`cargo test -p julie-extract-cli --test operations_contract` and
`cargo test -p julie-extract-artifact --test writer_contract`.

**Worker ceiling:** `cargo test -p julie-extract-cli --test operations_contract`
plus `cargo test -p julie-extract-artifact --test writer_contract`.

**Worker gate invariant:** Release notes and audit evidence match actual staged
package contents, actual binary identity, latest dogfood/performance evidence,
and stated non-goals.

**Lead affected-change scope:**
- `cargo test -p julie-extract-cli --test operations_contract`
- `cargo test -p julie-extract-artifact --test writer_contract`
- `cargo test -p julie-extract-artifact --test jsonl_contract`
- `cargo test -p julie-extract-artifact --test report_contract`
- `cargo build --release -p julie-extract-cli --bin julie-extract`
- `cargo xtask performance baseline --root . --out-dir target/performance/julie-extractors-baseline-805da3b --binary target/release/julie-extract --runs 3`
- `cargo xtask release package --version 0.1.0 --target aarch64-apple-darwin --out-dir target/release-package/v0.1.0-aarch64-apple-darwin-c407cde --binary target/release/julie-extract`
- checksum verification inside the staged package
- `cargo test -p xtask`

**Branch gate:** `cargo fmt --all -- --check`, `cargo xtask test default`, and
`cargo xtask test contract` before push or PR.

**Replay/metric evidence:** Hard gates are release package staging success,
manifest contents, checksum verification, non-empty generated package, and
branch gates. Dogfood/performance numbers remain report-only unless a future
plan sets budgets.

**Escalation triggers:** Missing release note, staged manifest drift, checksum
failure, package output containing files outside the manifest, public contract
changes, parser dependency changes, zero capability evidence rows, or weak
evidence quality.

**Assigned verification failure:** Workers stop and report when assigned
verification fails unless this plan explicitly says to update that gate.

**Verification ledger:** Record invariant, command, scope label, commit SHA,
result, and timestamp. For package staging, record target, staged file list,
binary version, binary SHA-256, package checksum, and output path.

## Model Routing

**Project source of truth:** `RAZORBACK.md`.

**Strategy tier:** Release-candidate audit, release evidence interpretation,
package-readiness judgment, and review finding triage.
- Harness mapping: inherit in this Codex session.

**Implementation tier:** Bounded release-note and evidence documentation edits
after commands provide facts.
- Harness mapping: inherit.

**Mechanical tier:** Formatting and wording-only documentation edits that do not
interpret evidence.
- Harness mapping: inherit.

**Gate-interpretation reviewer:** Lead interprets package staging, checksum,
branch gate, and CI evidence.
- Harness mapping: inherit.

**Escalation tier:** Public contract drift, missing release package content,
checksum mismatch, weak evidence, repeated verification failure, or default
suite runtime growth.
- Harness mapping: inherit.

**Worker eligibility:** Workers are eligible for read-only exploration and
mechanical doc checks only; the lead owns release evidence and acceptance.

**Mechanical exclusion:** Mechanical workers cannot own release readiness,
package evidence interpretation, metrics, or acceptance gates.

## File Structure

- Create: `docs/plans/2026-06-01-v0-1-0-release-candidate-audit.md` - this
  audit plan and progress ledger.
- Create: `docs/release-evidence/2026-06-01-v0-1-0-release-candidate-audit.md`
  - package staging and release-readiness evidence.
- Modify: `docs/release-notes/v0.1.0.md` - align release notes to current
  staged package, latest evidence, checksums, and non-goals.
- Modify: `docs/plans/2026-06-01-product-completion-tracker.md` - mark Slice 5
  status and final active state.
- Modify: `.memories/briefs/julie-extractors-product-completion-focus.md` -
  keep the active brief current.
- Modify: `crates/julie-extract-artifact/` and `crates/julie-extract-cli/` -
  targeted fix for capability snapshot persistence found by the audit.
- Modify: `crates/julie-extractors/` - crate metadata and docs alignment for
  the secondary Rust API surface.

## Tasks

### Task 1: Stage Release Package And Capture Facts

**What to do:**
- Build the release binary.
- Stage v0.1.0 for `aarch64-apple-darwin`.
- Verify the staged checksum.
- Record the manifest file list from the staged output.

**Acceptance criteria:**
- [x] Release binary builds.
- [x] Release package staging passes into `target/release-package/`.
- [x] Staged checksum verifies against the staged binary.
- [x] Staged files match the release package manifest and no generated files are committed.

### Task 2: Update Release Notes And Evidence

**What to do:**
- Update `docs/release-notes/v0.1.0.md` with current evidence, package staging
  status, target, binary SHA-256, and known boundaries.
- Add release-candidate audit evidence under `docs/release-evidence/`.

**Acceptance criteria:**
- [x] Release notes name current shipped surfaces and non-goals.
- [x] Release notes point at latest dogfood, buffering, performance, and
  package-staging evidence.
- [x] Evidence records commands, commit, timestamp, target, binary identity,
  staged files, checksum verification, and judgment.

### Task 2.5: Resolve Capability Evidence Contract Drift

**What to do:**
- Persist the extractor capability snapshot into SQLite artifacts.
- Include capability rows in JSONL exports from real scanned artifacts.
- Keep language capability claims unchanged.
- Align the secondary Rust crate metadata to v0.1.0 and this repository.

**Acceptance criteria:**
- [x] Real scan-created artifacts have nonzero parser inventory and language
  capability rows.
- [x] Real JSONL exports include parser inventory, capability, fixture, and gap
  records.
- [x] No-change rescans remain `status=no_change` with zero rows written.
- [x] `julie-extractors` crate metadata no longer points at old Julie.

### Task 3: Close Tracker State

**What to do:**
- Update the product tracker and active brief to mark Slice 5 complete.
- Keep next state explicit: v0.1.0 release candidate ready for user release
  decision, not automatically published.

**Acceptance criteria:**
- [x] Tracker marks Slice 5 complete.
- [x] Active brief points at current `main`, branch, and audit evidence.
- [x] No stale "active PR #7" or pre-Slice-5 status remains.

### Task 4: Verify And Finish Branch

**What to do:**
- Run focused and branch gates.

**Acceptance criteria:**
- [x] `cargo test -p xtask` passes.
- [x] `cargo fmt --all -- --check` passes.
- [x] `cargo xtask test default` passes.
- [x] `cargo xtask test contract` passes.
- [x] Changed-path gate passes for touched extractor metadata and product code
  paths.

### Task 5: Push PR And Watch CI

**What to do:**
- Push the branch and open a PR.
- Watch Fast Gates.

**Acceptance criteria:**
- [ ] PR Fast Gates pass.

## Progress

- [x] Task 0: Merge PR #7, sync `main`, and create Slice 5 worktree.
- [x] Task 1: Stage release package and capture facts.
- [x] Task 2: Update release notes and evidence.
- [x] Task 2.5: Resolve capability evidence contract drift.
- [x] Task 3: Close tracker state.
- [x] Task 4: Verify branch.
- [ ] Task 5: Push branch, open PR, and watch CI.
