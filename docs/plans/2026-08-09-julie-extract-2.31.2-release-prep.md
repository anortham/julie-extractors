# Julie Extract v2.31.2 Release Preparation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Prepare the merged family-store fixes as a locally verified Julie Extract v2.31.2 release candidate without publishing it.

**Architecture:** This is release metadata, evidence, and local test-timing guidance work. The merged runtime commit remains the source of behavior; the prep updates the three crate versions, lockfile package versions, versioned release notes, and local timing guidance so the existing release preflight and four-target packaging workflow can consume the candidate. The published-release pointer stays at v2.31.1 until publication.

**Tech Stack:** Rust 1.95, Cargo workspace, `cargo xtask` release tooling, GitHub Actions release packaging.

**Architecture Quality:** No Architecture Impact — the task changes release metadata and documentation only.

## Global Constraints

- Store mode and the v4 contract remain explicit and unchanged by this release prep.
- v2.31.1 remains the current published release until v2.31.2 is actually published.
- Release packaging must cover `aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-unknown-linux-gnu`, and `x86_64-pc-windows-msvc`.
- Do not push commits, create or push tags, publish GitHub releases, or update Miller's Julie pin during local preparation.
- Do not modify unrelated dirty files in the primary Miller checkout or any other worktree.

## Verification Strategy

**Project source of truth:** `AGENTS.md`, `docs/release.md`, `.github/workflows/release-binaries.yml`, and the Cargo manifests.

**Worker red/green scope:** Run `RUSTUP_TOOLCHAIN=1.95.0-aarch64-apple-darwin cargo fmt --all -- --check`, `cargo xtask release preflight --version 2.31.2`, and `cargo xtask release package-list` after the metadata and release note edits.

**Worker ceiling:** The Julie default and contract tiers plus release preflight/package-manifest checks; no publication or remote-state mutation.

**Worker gate invariant:** All workspace crates and lockfile package entries report 2.31.2, the release note exists, every package input is present, and the package manifest remains restricted to the documented four-target release inputs.

**Lead affected-change scope:** `RUSTUP_TOOLCHAIN=1.95.0-aarch64-apple-darwin cargo fmt --all -- --check`, `RUSTUP_TOOLCHAIN=1.95.0-aarch64-apple-darwin cargo xtask test default`, `RUSTUP_TOOLCHAIN=1.95.0-aarch64-darwin cargo xtask test contract`, `cargo xtask release preflight --version 2.31.2`, `cargo xtask release package-list`, `scripts/check-agent-doc-sync.sh`, and `git diff --check`.

**Branch gate:** The affected-change commands above, followed by a worktree and commit check before fast-forwarding Julie `main` locally.

**Security scope:** none declared.

**Replay/metric evidence:** Release preflight and package-list output are hard gates; test timing is report-only.

**Escalation triggers:** Any manifest-version mismatch, package input failure, contract failure, or release workflow mismatch requires inspection before committing. No broader Miller gate is needed until the published Julie assets exist and Miller's pin is updated.

**Assigned verification failure:** Stop and diagnose the failed gate before committing; do not weaken the release checks.

**Verification ledger:** Record each command, scope, commit SHA, result, and timestamp in the handoff. Reuse the already passing merged runtime evidence where the same HEAD remains unchanged, and rerun metadata-sensitive release gates after the version bump.

## Verification Ledger

- 2026-08-09T19:13:20-05:00 | worker | `b2d23814b50d8f329d3f950e247bdbe2b1188ca1` | PASS | `RUSTUP_TOOLCHAIN=1.95.0-aarch64-apple-darwin cargo check --workspace`; all three packages resolved as v2.31.2.
- 2026-08-09T19:13:20-05:00 | worker | `b2d23814b50d8f329d3f950e247bdbe2b1188ca1` | PASS | `RUSTUP_TOOLCHAIN=1.95.0-aarch64-apple-darwin cargo fmt --all -- --check`.
- 2026-08-09T19:13:20-05:00 | affected-change | `b2d23814b50d8f329d3f950e247bdbe2b1188ca1` | PASS | `RUSTUP_TOOLCHAIN=1.95.0-aarch64-apple-darwin cargo xtask test default` warm-cache rerun; the first cold-cache run had all tests pass but tripped only the 90-second tier budget during compilation.
- 2026-08-09T19:13:20-05:00 | branch | `b2d23814b50d8f329d3f950e247bdbe2b1188ca1` | PASS | `RUSTUP_TOOLCHAIN=1.95.0-aarch64-apple-darwin cargo xtask test contract`; golden, capability, downstream, resolution, crash, rollback, equivalence, mixed-version, maintenance, and 38-language parity suites passed.
- 2026-08-09T19:13:20-05:00 | worker | `b2d23814b50d8f329d3f950e247bdbe2b1188ca1` | PASS | `cargo xtask release preflight --version 2.31.2`; 4 targets and 32 inputs validated.
- 2026-08-09T19:13:20-05:00 | worker | `b2d23814b50d8f329d3f950e247bdbe2b1188ca1` | PASS | `cargo xtask release package-list`, `scripts/check-agent-doc-sync.sh`, and `git diff --check`.
- 2026-08-09T19:15:00-05:00 | integration | `962aa8aec3e360879ffb8b4d06846d331d9b6dba` | PASS | Release-prep commit fast-forwarded into local Julie `main`; remote state was intentionally unchanged.
- 2026-08-09T19:21:28-05:00 | final | `3824382c56e75ec31cd3f544294400483acd3317` | PASS | Exact-HEAD `cargo xtask test default`, formatting, metadata, agent-doc sync, whitespace, release preflight, and package-list checks.
- 2026-08-09T19:21:28-05:00 | final | `3824382c56e75ec31cd3f544294400483acd3317` | PASS | Exact-HEAD `cargo xtask test contract`; captured log `/tmp/julie-v2.31.2-contract-3824382.log`, 692 lines, exit 0, no failed test result.
- 2026-08-09T19:21:28-05:00 | release boundary | `3824382c56e75ec31cd3f544294400483acd3317` | EXPECTED HOLD | `scripts/check-release-state.sh` reports the v2.31.2 tag is not on origin and local `main` is three commits ahead; no remote operation was performed.
- 2026-08-10T01:14:43Z | CI rerun `31345869462` | `2a90eba13bc973e5b840292bf9236dbdf5ffdb95` | EXPECTED FAILURE | All default-tier tests passed; the 90s GitHub Actions wall-clock guard measured about 122s including fresh package compilation after the store test expansion. CI timing is removed from the test-tier runner; reproducible timing is a local acceptance measurement instead.
- 2026-08-09T20:23:56-05:00 | local timing | `2a90eba13bc973e5b840292bf9236dbdf5ffdb95` + timing-only working tree | PASS | Three warmed-cache `cargo xtask test default` runs on this macOS host passed in 42.37s, 41.62s, and 41.26s real time; range 1.11s. This is report-only evidence, not a CI gate.

## Parallel Execution Contract

| Task | Parallel batch | File ownership | Serialization required | Dependency reason |
|---|---|---|---|---|
| Task 1: Prepare the v2.31.2 release candidate | None - serial | Create `docs/release-notes/v2.31.2.md`; modify `crates/julie-extractors/Cargo.toml`, `crates/julie-extract-artifact/Cargo.toml`, `crates/julie-extract-cli/Cargo.toml`, and `Cargo.lock`; commit the verified prep and fast-forward Julie `main` locally. | Not applicable - single task. | Not applicable - single task. |

### Task 1: Prepare the v2.31.2 release candidate

**Files:**
- Create: `docs/release-notes/v2.31.2.md`
- Modify: `crates/julie-extractors/Cargo.toml`
- Modify: `crates/julie-extract-artifact/Cargo.toml`
- Modify: `crates/julie-extract-cli/Cargo.toml`
- Modify: `Cargo.lock`

**Interfaces:**
- Consumes: merged runtime commit `b2d23814b50d8f329d3f950e247bdbe2b1188ca1` and the release inputs defined by `docs/release.md`.
- Produces: a version-consistent local v2.31.2 source tree that passes release preflight and can stage the four documented target packages after publication approval.

**Contract inputs:** v2.31.1 is the published baseline; the release note must describe physical-byte GC/retention measurement and escalation, capacity preflight, all-language contract parity, safe malformed-store rollback, and the completed store test evidence without claiming publication.

**File ownership:** Create `docs/release-notes/v2.31.2.md`; modify `crates/julie-extractors/Cargo.toml`, `crates/julie-extract-artifact/Cargo.toml`, `crates/julie-extract-cli/Cargo.toml`, and `Cargo.lock`; commit the verified prep and fast-forward Julie `main` locally.

**Serialization required:** Not applicable - single task.

**Dependency reason:** Not applicable - single task.

**What to build:** Bump all three publishable crate versions and their lockfile entries from 2.31.1 to 2.31.2. Add release notes that accurately describe the merged store fixes and verification, while leaving `docs/release.md`'s current-published pointer at v2.31.1 until a later publish session.

**Approach:** Follow the existing patch-note format. Use Cargo metadata/lockfile generation rather than hand-editing dependency records, run the documented Rust 1.95 gates, then commit the candidate in the existing isolated worktree and fast-forward Julie `main` locally. Keep remote operations and Miller pin changes outside this task because they require the published four-asset release.

**Acceptance criteria:**
- [x] All three crate manifests and the three workspace package entries in `Cargo.lock` report 2.31.2.
- [x] `docs/release-notes/v2.31.2.md` names the merged behavior changes and does not claim that v2.31.2 is published.
- [x] Formatting, default tests, contract tests, release preflight, package-list, agent-doc sync, and whitespace checks pass.
- [x] The prep commit is on Julie `main` locally, with no push, tag, or publish performed.
