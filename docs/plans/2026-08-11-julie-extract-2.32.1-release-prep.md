# Julie Extract v2.32.1 Release Preparation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Prepare the integrated writer-heartbeat and resolution-scope fixes as a locally verified Julie Extract v2.32.1 release candidate without publishing it.

**Architecture:** This is a metadata and documentation-only preparation over integrated local performance commit `ab3aa957d7ec658972ee0f15b1cab2c2011539ad`. Runtime behavior remains owned by the integrated fix commits; this prep aligns the publishable crate versions, lockfile package versions, release notes, and documentation map so the existing release tooling can package the candidate after explicit approval.

**Tech Stack:** Rust 1.95, Cargo workspace, `cargo xtask` release tooling, GitHub Actions four-target packaging.

**Architecture Quality:** No Architecture Impact — the task changes release metadata and documentation only.

## Global Constraints

- v2.32.0 remains the current published release until v2.32.1 is actually published.
- Release packaging remains `aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-unknown-linux-gnu`, and `x86_64-pc-windows-msvc`.
- The release notes must include both integrated fixes and their measured evidence without claiming publication.
- Do not push commits, create or push tags, publish a GitHub release, mutate remote state, or modify Miller.
- Do not modify or depend on the unmerged `fb31da0` documentation-only commit in `fix/store-resolution-scope`.

## Verification Strategy

**Project source of truth:** `AGENTS.md`, `docs/release.md`, `docs/testing-strategy.md`, `.github/workflows/release-binaries.yml`, and `xtask/src/release.rs`.

**Worker red/green scope:** `cargo check --workspace`, `cargo fmt --all -- --check`, `cargo xtask release package-list`, `cargo xtask release preflight --version 2.32.1`, the exact release contract test, and repository sync/whitespace checks.

**Worker ceiling:** Metadata-sensitive version and release-preflight gates plus the documented affected store contracts already verified on integrated `main`; no full corpus or publication gates.

**Worker gate invariant:** All three publishable crate manifests and lockfile package entries report 2.32.1, the v2.32.1 release note exists, every package input is present, package targets remain exact, and docs do not claim v2.32.1 is published.

**Lead affected-change scope:** `cargo check --workspace`, `cargo fmt --all -- --check`, `cargo xtask release package-list`, `cargo xtask release preflight --version 2.32.1`, `cargo test -p xtask --test release_contract release_preflight_checks_manifest_inputs_and_crate_versions`, `scripts/check-agent-doc-sync.sh`, and `git diff --check`.

**Branch gate:** The affected-change commands above followed by exact worktree, branch, commit, and dirty-state checks. Publication gates remain approval-bound.

**Security scope:** none declared.

**Replay/metric evidence:** Release preflight and version agreement are hard gates. Incident size/runtime/RSS and the clean resolution replay timings are report-only release-note evidence; canonical digest equality and zero row/semantic differences are hard correctness evidence inherited from integrated `main`.

**Escalation triggers:** Any version mismatch, missing package input, target-list drift, release-contract failure, or documentation claim of publication blocks the local prep commit.

**Assigned verification failure:** Diagnose and repair the metadata or documentation mismatch; do not weaken release checks.

**Verification ledger:** Record command, scope, exact candidate commit, result, and relevant counts in the Goldfish checkpoint and handoff. Do not rerun expensive unchanged-tree runtime gates already recorded on `4b7b08d`.

## Parallel Execution Contract

| Task | Parallel batch | File ownership | Serialization required | Dependency reason |
|---|---|---|---|---|
| Task 1: Prepare the v2.32.1 candidate | None - serial | Create `docs/release-notes/v2.32.1.md` and this plan; modify `docs/README.md`, the three publishable crate manifests, and `Cargo.lock`; checkpoint and commit locally. | Not applicable - single task. | Not applicable - single task. |

### Task 1: Prepare the v2.32.1 candidate

**Files:**
- Create: `docs/release-notes/v2.32.1.md`
- Create: `docs/plans/2026-08-11-julie-extract-2.32.1-release-prep.md`
- Modify: `docs/README.md`
- Modify: `crates/julie-extractors/Cargo.toml`
- Modify: `crates/julie-extract-artifact/Cargo.toml`
- Modify: `crates/julie-extract-cli/Cargo.toml`
- Modify: `Cargo.lock`

**Interfaces:**
- Consumes: integrated runtime commit `4b7b08d3ce32c4a1ab1a900b61f4086745363906`, release package manifest inputs, and the v2.32.0 published baseline.
- Produces: a version-consistent local v2.32.1 source candidate accepted by the existing four-target release preflight.

**Contract inputs:** Heartbeat incident evidence from `0500ab1`, resolution crossover and clean replay evidence from `f39d726`/`4b7b08d`, unchanged schema v2/format epoch 1/standalone artifact contracts, and the existing v2.31.4 patch-release note shape.

**File ownership:** Create `docs/release-notes/v2.32.1.md` and this plan; modify `docs/README.md`, the three publishable crate manifests, and `Cargo.lock`; checkpoint and commit locally.

**Serialization required:** Not applicable - single task.

**Dependency reason:** Not applicable - single task.

**What to build:** Bump the three publishable crates and lockfile entries from 2.32.0 to 2.32.1. Add candidate release notes covering the long from-artifact writer heartbeat, idempotent retry, pre-execution resolution-scope crossover, and exact writer fix; include measured incident and clean replay evidence, and link the candidate note from the versioned-store documentation map without moving the current-published pointer.

**Approach:** Follow the v2.31.4 patch-release pattern. Preserve every public schema, format, and CLI contract; describe the fixes as lifecycle and planner corrections. Run only metadata-sensitive release gates because integrated runtime scopes already have exact-HEAD evidence.

**Acceptance criteria:**
- [x] All three manifests and all three lockfile package entries report 2.32.1.
- [x] The release note accurately covers the integrated fixes, compatibility, measured evidence, and candidate-only status.
- [x] `docs/README.md` links v2.32.1 while `docs/release.md` still names v2.32.0 as current published.
- [x] Version, formatting, package-list, release-preflight, release-contract, agent-doc sync, and whitespace gates pass.
- [x] Goldfish checkpoint and local release-prep commit exist; no push, tag, publish, network mutation, or Miller modification occurred.
