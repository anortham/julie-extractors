# Release Binaries Workflow Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Add a GitHub Actions workflow that builds `julie-extract` release binaries, stages versioned release packages, and publishes them as GitHub Release assets.

**Architecture:** Keep release binary production separate from fast CI and specialist verification gates. The workflow builds the CLI binary per platform, runs the existing `xtask release package` staging command, archives each staged package, and publishes those archives on a GitHub Release.

**Tech Stack:** GitHub Actions, Rust stable toolchain, existing `cargo xtask release package` command, `actions/upload-artifact`, `actions/download-artifact`, GitHub CLI release commands.

**Architecture Quality:** Release packaging is strategy-tier in `RAZORBACK.md`. The workflow must reuse the existing package staging contract instead of adding a second package format or product behavior.

---

## Completion Evidence

- Implemented on `main` in `a1f5069a36975e446c6a533e60bdcd3a9d3c11fa`.
- Release Binaries workflow run `26781742834` passed for v2.0.0.
- GitHub Release `v2.0.0` was published with Linux x86_64, macOS Apple
  Silicon, macOS Intel, and Windows x86_64 assets.
- Evidence: `docs/release-evidence/2026-06-01-v2-0-0-release.md`.

## Accepted Design

- Create `.github/workflows/release-binaries.yml`.
- Trigger the workflow with `workflow_dispatch` and tag pushes matching `v*`.
- Build a matrix for Linux x86_64, macOS Apple Silicon, macOS Intel, and
  Windows x86_64.
- Build `julie-extract` in release mode for each matrix row with
  `--target ${{ matrix.target }}`.
- Stage packages with the existing `cargo xtask release package` command.
- Archive each staged package directory as `.tar.gz` for Linux/macOS and `.zip`
  for Windows.
- Upload each archive as a GitHub Actions artifact.
- Create or update GitHub Release `v{version}` and upload the archives as
  release assets.

## Files

- Create: `.github/workflows/release-binaries.yml`
- Modify: `xtask/tests/commands_contract.rs`
- Modify: `docs/release.md`
- Modify: `docs/release-notes/v2.0.0.md`
- Modify: `docs/plans/2026-06-01-product-completion-tracker.md`

## Implementation Tasks

### Task 1: Workflow Contract Test

Add an `xtask` convention test that proves the release-binaries workflow:

- exists at `.github/workflows/release-binaries.yml`;
- has `workflow_dispatch`;
- has tag push trigger for `v*`;
- includes Linux x86_64, macOS Apple Silicon, macOS Intel, and Windows x86_64
  runners;
- grants `contents: write`;
- runs `cargo build --release --target ${{ matrix.target }} -p
  julie-extract-cli --bin julie-extract`;
- stages packages with `cargo xtask release package`;
- uploads build archives with `actions/upload-artifact`;
- downloads archives with `actions/download-artifact`;
- creates or updates a GitHub Release and uploads release assets.

### Task 2: Workflow

Create `.github/workflows/release-binaries.yml` with one build matrix job:

- Linux: `ubuntu-latest`, target label `x86_64-unknown-linux-gnu`, binary path `target/x86_64-unknown-linux-gnu/release/julie-extract`.
- macOS Apple Silicon: `macos-latest`, target label `aarch64-apple-darwin`, binary path `target/aarch64-apple-darwin/release/julie-extract`.
- macOS Intel: `macos-15-intel`, target label `x86_64-apple-darwin`, binary path `target/x86_64-apple-darwin/release/julie-extract`.
- Windows: `windows-latest`, target label `x86_64-pc-windows-msvc`, binary path `target/x86_64-pc-windows-msvc/release/julie-extract.exe`.

The package out-dir must be `target/release-package/v${version}-${{ matrix.target }}`. The uploaded archive name must include version and target:

- `julie-extract-v{version}-{target}.tar.gz` for Linux/macOS.
- `julie-extract-v{version}-{target}.zip` for Windows.

Add a release job that waits for all build matrix jobs, downloads the archives,
uses `docs/release-notes/v{version}.md`, creates or updates GitHub Release
`v{version}`, and uploads all archives with `gh release upload --clobber`.

### Task 3: Release Docs

Update `docs/release.md` to document:

- the workflow name and triggers;
- four-platform artifact staging behavior;
- GitHub Release creation/update behavior;
- archive naming and upload behavior.

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
