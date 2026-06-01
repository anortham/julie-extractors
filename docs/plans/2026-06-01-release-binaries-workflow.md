# Release Binaries Workflow Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Add a GitHub Actions workflow that builds `julie-extract` release binaries, stages versioned release packages, and uploads them as workflow artifacts.

**Architecture:** Keep release binary production separate from fast CI and specialist verification gates. The new workflow builds the CLI binary per platform, runs the existing `xtask release package` staging command, and uploads the staged package directory without publishing a GitHub Release yet.

**Tech Stack:** GitHub Actions, Rust stable toolchain, existing `cargo xtask release package` command, `actions/upload-artifact`.

**Architecture Quality:** Release packaging is strategy-tier in `RAZORBACK.md`. The workflow must reuse the existing package staging contract instead of adding a second package format or product behavior.

---

## Accepted Design

- Create `.github/workflows/release-binaries.yml`.
- Trigger the workflow with `workflow_dispatch` and tag pushes matching `v*`.
- Build a matrix for Linux, macOS, and Windows.
- Build `julie-extract` in release mode for each matrix row.
- Stage packages with the existing `cargo xtask release package` command.
- Upload each staged package directory as a GitHub Actions artifact.
- Keep release upload/publishing out of scope for this workflow.

## Files

- Create: `.github/workflows/release-binaries.yml`
- Modify: `xtask/tests/commands_contract.rs`
- Modify: `docs/release.md`

## Implementation Tasks

### Task 1: Workflow Contract Test

Add an `xtask` convention test that proves the release-binaries workflow:

- exists at `.github/workflows/release-binaries.yml`;
- has `workflow_dispatch`;
- has tag push trigger for `v*`;
- includes Linux, macOS, and Windows runners;
- runs `cargo build --release -p julie-extract-cli --bin julie-extract`;
- stages packages with `cargo xtask release package`;
- uploads artifacts with `actions/upload-artifact`.

### Task 2: Workflow

Create `.github/workflows/release-binaries.yml` with one matrix job:

- Linux: `ubuntu-latest`, target label `x86_64-unknown-linux-gnu`, binary path `target/release/julie-extract`.
- macOS: `macos-15`, target label `aarch64-apple-darwin`, binary path `target/release/julie-extract`.
- Windows: `windows-2022`, target label `x86_64-pc-windows-msvc`, binary path `target/release/julie-extract.exe`.

The package out-dir must be `target/release-package/v${{ inputs.version }}-${{ matrix.target }}` and the uploaded artifact name must include both version and target.

### Task 3: Release Docs

Update `docs/release.md` to document:

- the workflow name and triggers;
- artifact staging behavior;
- the fact that the workflow uploads Actions artifacts only and does not publish a GitHub Release.

## Verification Strategy

**Project source of truth:** `AGENTS.md`, `RAZORBACK.md`, `docs/release.md`, existing GitHub workflows, and `xtask/tests/commands_contract.rs`.

**Worker red/green scope:** `cargo test -p xtask workflow_commands_keep_release_binary_workflow_explicit`

**Worker ceiling:** `cargo test -p xtask`

**Worker gate invariant:** Workflow conventions must prove release binaries are built, staged, and uploaded outside fast CI.

**Lead affected-change scope:** `cargo fmt --check` and `cargo test -p xtask`

**Branch gate:** `cargo xtask test default` and `cargo xtask test contract` before merge or push if the workflow branch is handed off.

**Replay/metric evidence:** GitHub Actions run result is required after pushing the workflow; local tests prove only static workflow contract.

**Escalation triggers:** Broaden to full CI if workflow syntax, release packaging, or xtask routing changes outside the planned files.

**Assigned verification failure:** Stop and investigate root cause before editing around a failed workflow or test.

**Verification ledger:** Record command, scope, commit SHA, result, and timestamp in the final report.

## Model Routing

**Project source of truth:** `RAZORBACK.md`

**Strategy tier:** release packaging and workflow contract decisions.
- Harness mapping: inherit.

**Implementation tier:** bounded edits to the workflow, workflow convention test, and release docs.
- Harness mapping: inherit.

**Mechanical tier:** formatting and docs wording that does not change release behavior.
- Harness mapping: inherit.

**Gate-interpretation reviewer:** lead reads local test output and GitHub Actions result.
- Harness mapping: inherit.

**Escalation tier:** workflow failures after push, release-package contract failures, or artifact path ambiguity.
- Harness mapping: inherit.

**Worker eligibility:** allowed for disjoint file edits only; this run is small and sequential.

**Escalation triggers:** package contents change, target naming ambiguity, release publishing requests, or repeated workflow failures.

**Mechanical exclusion:** mechanical edits cannot own the workflow gate or release artifact acceptance.

**Unsupported harness behavior:** no per-agent model routing is needed for this single-threaded run.
