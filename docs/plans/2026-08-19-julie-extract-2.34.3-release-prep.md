# Julie Extract v2.34.3 Release Preparation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Prepare the verified test-detection precision fix as a locally verified Julie Extract v2.34.3 release candidate without publishing it.

**Architecture:** The runtime behavior remains in the integrated test-detection commits. Release preparation aligns the three crate versions, compatibility baseline/current extraction epochs, family-store identity metadata, public contract assertions, release notes, and extraction-output ledger so existing release tooling can validate the candidate against published v2.34.2.

**Tech Stack:** Rust 1.95, Cargo workspace, SQLite/JSONL extraction contracts, `cargo xtask` compatibility and release tooling.

**Architecture Quality:** No Architecture Impact — this task updates release metadata and the existing extraction identity epoch contract without changing schema or public API shape.

## Global Constraints

- v2.34.2 remains the current published release until v2.34.3 is actually published.
- Artifact schema stays 7, JSONL stays v5, family-store schema stays 2, and store format epoch stays 1.
- Extraction identity epoch advances from 2 to 3 because emitted `is_test` facts change.
- The compatibility baseline is published v2.34.2 at extraction identity epoch 2; the candidate is epoch 3.
- Do not push commits, create or push tags, publish a GitHub release, or modify Miller during release preparation.

## Verification Strategy

**Project source of truth:** `AGENTS.md`, `docs/release.md`, `docs/contracts/store-v1.md`, `docs/contracts/extraction-output-changes.md`, `.github/workflows/release-binaries.yml`, and `xtask/src/{compat,release}.rs`.

**Worker red/green scope:** Focused epoch contract tests, `cargo check --workspace`, `cargo fmt --all -- --check`, `cargo xtask release package-list`, `cargo xtask release preflight --version 2.34.3`, the exact release contract test, agent-doc sync, and whitespace checks.

**Worker ceiling:** Metadata-sensitive version, epoch, and release-preflight gates. The worker does not run the previous-binary compatibility comparison or broad branch tiers.

**Worker gate invariant:** All publishable crate and lockfile versions report 2.34.3; every current-epoch constant, store seed, assertion, and compatibility comparison reports epoch 3 against baseline epoch 2; release notes and the ledger classify the changed test facts without claiming publication.

**Lead affected-change scope:** Review the complete diff, refresh Miller, run post-edit impact/trace, and run the worker commands on the committed candidate when needed for fresh evidence.

**Branch gate:** `cargo fmt --check`, `cargo test -p xtask`, `cargo xtask test default`, `cargo xtask test contract`, `cargo xtask release package-list`, `cargo xtask release preflight --version 2.34.3`, `cargo xtask compat-check --previous-binary <published-v2.34.2-linux-binary>`, `scripts/check-agent-doc-sync.sh`, and `git diff --check`.

**Security scope:** none declared.

**Replay/metric evidence:** The compatibility report's declared epoch-3 output differences are a hard gate. Schema versions, compatibility classification, version agreement, default/contract tiers, and release preflight are hard gates; no performance metrics apply.

**Escalation triggers:** Any undeclared output difference, same-epoch output change, schema/JSONL/store version drift, version mismatch, missing package input, or publication claim blocks the candidate.

**Assigned verification failure:** Diagnose and repair the release metadata or contract mismatch; do not weaken gates or alter the runtime fix to force compatibility.

**Verification ledger:** Record invariant, command, scope, candidate commit, result, and timestamp in the worker report and Goldfish checkpoint.

## Parallel Execution Contract

| Task | Parallel batch | File ownership | Serialization required | Dependency reason |
|---|---|---|---|---|
| Task 1: Prepare the v2.34.3 candidate | None - serial | Modify the three publishable crate manifests, `Cargo.lock`, epoch source/tests/docs, compatibility constants, release-note index, this plan; create `docs/release-notes/v2.34.3.md` and checkpoint. | Not applicable - single task. | Not applicable - single task. |

### Task 1: Prepare the v2.34.3 candidate

**Files:**
- Create: `docs/release-notes/v2.34.3.md`
- Modify: `docs/release-notes/README.md`
- Modify: `crates/julie-extractors/Cargo.toml`
- Modify: `crates/julie-extract-artifact/Cargo.toml`
- Modify: `crates/julie-extract-cli/Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `crates/julie-extractors/src/lib.rs`
- Modify: `crates/julie-extractors/src/tests/api_surface.rs`
- Modify: `crates/julie-extract-artifact/src/store/layout.rs`
- Modify: `crates/julie-extract-artifact/tests/store_connection_contract.rs`
- Modify: `crates/julie-extract-cli/tests/store_equivalence.rs`
- Modify: `xtask/src/compat.rs`
- Modify: `docs/contracts/store-v1.md`
- Modify: `docs/contracts/extraction-output-changes.md`
- Modify: `docs/architecture/versioned-index-store.md`
- Modify: `docs/plans/2026-08-19-julie-extract-2.34.3-release-prep.md`
- Create: `.memories/<Goldfish checkpoint generated before commit>`

**Interfaces:**
- Consumes: the verified test-detection precision commits, published v2.34.2 as the compatibility baseline, and the existing four-target packaging contract.
- Produces: a version-consistent local v2.34.3 source candidate whose changed extraction facts are explicitly epoch 3 and compatible for existing readers.

**Contract inputs:** Python decorator precision, Scala/Elixir path-fallback removal, unchanged schema/JSONL/store formats, and file-version identity `(path, content_hash, extraction_epoch)`.

**File ownership:** Modify the three publishable crate manifests, `Cargo.lock`, epoch source/tests/docs, compatibility constants, release-note index, this plan; create `docs/release-notes/v2.34.3.md` and checkpoint.

**Serialization required:** Not applicable - single task.

**Dependency reason:** Not applicable - single task.

**What to build:** Bump 2.34.2 to 2.34.3 in the publishable crates and lockfile. Advance current extraction identity data from 2 to 3, set compatibility baseline/current epochs to 2/3, add a compatible v2.34.3 ledger entry and release note describing narrower Python/Scala/Elixir `is_test` facts, and list the candidate note without moving the published-release pointer.

**Approach:** Follow the v2.34.2 release and epoch-bump pattern. Keep arbitrary multi-epoch unit-test fixtures unchanged when their literal `2` is test data rather than the current product epoch.

**Acceptance criteria:**
- [x] All three manifests and all three lockfile package entries report 2.34.3.
- [x] Current extraction identity is epoch 3 everywhere the product contract publishes or seeds it; the compatibility baseline/current pair is 2/3.
- [x] The ledger and release note classify the narrower test facts as compatible and give actionable consumer guidance.
- [x] v2.34.2 remains the current published release; v2.34.3 is described only as a candidate.
- [x] Worker gates pass and the release-prep task is checkpointed and committed locally.
- [x] Lead branch gate and compatibility comparison against the published v2.34.2 Linux binary pass.
