# Post-Bootstrap Stabilization And v0.1.0 Release Readiness Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Turn the migrated bootstrap into a release-ready standalone extraction product with dogfood evidence, CI gates, release packaging, and v0.1.0 docs.

**Architecture:** Keep `julie-extract` and the SQLite/JSONL/report contracts as the product APIs. Put repo-only orchestration in `xtask/`, CI in `.github/workflows/`, release evidence in `docs/release-evidence/`, and non-Rust consumption examples in `examples/`. Do not add Julie MCP, server, daemon, search, embedding, watcher, dashboard, or editing behavior.

**Tech Stack:** Rust workspace, `cargo xtask`, SQLite, JSONL, GitHub Actions, Python stdlib `sqlite3` for a non-Rust consumer example.

**Architecture Quality:** Medium risk. Release packaging and test-tier policy are strategy-tier areas in `RAZORBACK.md`; dogfood and CI must exercise the public CLI/artifact interfaces without changing the product boundary.

---

## Source Documents

- `AGENTS.md`: product boundary, concise communication, verification, test discipline, and Julie read-only rule.
- `RAZORBACK.md`: strategy-tier areas, worker eligibility, escalation triggers, and verification ownership.
- `README.md`: current product overview and status.
- `docs/product/vision.md`: release output and product quality bar.
- `docs/release.md`: release commands and allowed package contents.
- `docs/testing-strategy.md`: tier definitions, branch gate, and slow-gate routing.
- `docs/contracts/cli.md`: process contract for `julie-extract`.
- `docs/contracts/sqlite-schema-v1.md`: SQLite schema, required indexes, and performance contract.
- `docs/contracts/jsonl-v1.md`: JSONL export contract.
- `docs/contracts/reports.md`: JSON report and error contract.
- `docs/plans/2026-05-31-julie-code-migration-implementation-plan.md`: completed migration baseline and task history.

## Current Baseline

- `main` contains the migrated Rust workspace and contracts.
- `cargo xtask test default` and `cargo xtask test contract` passed after the migration merge.
- `xtask/src/test_tiers.rs` currently owns the top-level route for both `test` and `release package-list`; this should be split before adding dogfood and package commands.
- No `.github/workflows/` directory exists.
- `xtask/src/release.rs` defines the release package manifest, but not a staging command or checksum writer.
- No dogfood command, release-evidence document, or non-Rust SQLite consumer example exists.
- `README.md` still reports "Planning and bootstrap", which is stale after the migration merge.

## Scope

In scope:

- Dogfood `julie-extract` on this repo through the CLI and generated SQLite/JSONL artifacts.
- Capture hard release evidence for v0.1.0 readiness and report-only performance metrics.
- Add CI for default and contract gates.
- Add specialist workflow hooks for certification, real-world gates, dogfood, and package staging.
- Add a tested release package staging command with SHA-256 checksums.
- Add a small Python SQLite consumer example that reads the artifact without Rust.
- Update release, testing, README, and release-note docs to match the implemented commands.

Out of scope:

- Public schema, JSONL, report, or CLI contract changes unless a task finds a contract mismatch and escalates it.
- Julie MCP/server/daemon/search/embedding/watcher/dashboard/editing behavior.
- Back-porting features into `/Users/murphy/source/julie`.
- Publishing release assets to GitHub or a package registry.

## Architecture Quality

**Affected modules:** `xtask/src/main.rs`, `xtask/src/lib.rs`, `xtask/src/test_tiers.rs`, `xtask/src/release.rs`, new xtask routing/dogfood modules, `.github/workflows/`, `docs/release.md`, `docs/testing-strategy.md`, `README.md`, release evidence, release notes, and `examples/python/`.

**Caller-facing interface:** The product interface remains `julie-extract` plus SQLite v1, JSONL v1, and JSON reports. New `cargo xtask` commands are repo tooling and are not downstream product APIs.

**Depth/locality check:** Extraction behavior stays in `crates/julie-extractors`, artifact behavior stays in `crates/julie-extract-artifact`, and CLI behavior stays in `crates/julie-extract-cli`. Dogfood, package staging, and workflow command composition stay in `xtask/` and `.github/`.

**Test surface:** Prove behavior through public command routes and artifact outputs: `cargo xtask` tests for routing/package planning, `julie-extract` dogfood reports, SQLite readback, JSONL validation, package contents, checksums, and workflow command lists.

**Seams/adapters:** Add a small xtask command-routing module so `test_tiers` stays about test plans and `release` stays about package logic. Dogfood code should have a pure plan/validation layer plus a command-runner boundary so unit tests do not need to scan the repo.

**Rejected shortcuts:** Do not stuff dogfood/release routing into `test_tiers.rs`. Do not shell out to old Julie. Do not make dogfood, certification, real-world, or release packaging part of the default tier. Do not hand-copy release assets outside a tested xtask command. Do not make performance evidence depend on incidental human log output.

**Architecture risk:** Medium because release evidence and package shape become project commitments, but the risk stays contained if all new behavior is routed through `xtask` and verified through the public artifact contracts.

## Verification Strategy

**Project source of truth:** `AGENTS.md`, `RAZORBACK.md`, `docs/testing-strategy.md`, `docs/release.md`, and the contracts under `docs/contracts/`.

**Worker red/green scope:** Use the narrowest command that proves the changed behavior: `cargo test -p xtask --test commands_contract`, `cargo test -p xtask --test dogfood_contract`, `cargo test -p xtask --test release_contract`, one CLI/artifact test target, `python3 -m py_compile examples/python/sqlite_consumer.py`, or a docs scan for the touched docs.

**Worker ceiling:** Workers may run `cargo test -p xtask`, one crate's tests, one integration test target, `cargo xtask test default`, and local docs/example checks. Workers do not own certification, real-world release, release asset publication, or interpreting performance acceptance.

**Worker gate invariant:** Each worker gate must prove a named behavior: top-level xtask route stability, dogfood command plan shape, dogfood artifact validation, exact package manifest and checksum creation, CI command coverage, Python consumer readback, or release-doc command accuracy.

**Lead affected-change scope:** After each coherent batch, run changed crate tests plus `cargo test -p xtask`, docs scans, and the command route affected by the batch. For workflow changes, parse the YAML with an available local parser before relying on CI.

**Branch gate:** Before handoff, run `cargo xtask test default` and `cargo xtask test contract`.

**Replay/metric evidence:** Hard gates are dogfood command success, JSON reports with `status: ok`, valid SQLite metadata, nonzero file and symbol counts for this repo, valid JSONL, exact package contents, and matching SHA-256 checksum files. Dogfood duration, artifact bytes, JSONL bytes, rows per second, and database page statistics are report-only for this plan.

**Escalation triggers:** Public schema/report/CLI changes, parser dependency changes, capability claim changes, default-suite runtime growth, hidden coupling to old Julie internals, weak evidence around dogfood validity, or package contents that expand beyond `docs/release.md`.

**Assigned verification failure:** Workers stop and report when assigned verification fails unless the task explicitly says to update that gate.

**Verification ledger:** Record invariant, command, scope label, commit SHA, result, and timestamp in the task report or release evidence document. For dogfood and package staging, record hard-gate metrics and report-only metrics. If the same HEAD already has a passing ledger entry for the required scope, reuse that evidence instead of rerunning an expensive gate.

## Model Routing

**Project source of truth:** `RAZORBACK.md`.

**Strategy tier:** Planning, architecture, decomposition, public contract interpretation, release packaging decisions, lead review, and finding triage.
- Harness mapping: inherit in this Codex session unless the lead explicitly selects an available override.

**Implementation tier:** Bounded worker tasks from this plan when public interfaces are decided, file ownership is narrow, verification ceiling is explicit, and the task does not reinterpret schema/report/release evidence.
- Harness mapping: inherit.

**Mechanical tier:** Docs links, manifest cleanup, fixture copying, formatting, and rote command-list updates with no test, metric, replay, or acceptance-gate ownership.
- Harness mapping: inherit.

**Gate-interpretation reviewer:** Review of failing tests, dogfood evidence, package contents, report mismatches, workflow gates, or performance evidence when the failure meaning is ambiguous.
- Harness mapping: inherit; escalate to strategy tier for public API or release evidence decisions.

**Escalation tier:** Public artifact schema changes, CLI status/exit/error changes, parser dependency issues, subtle cross-language correctness, release-package shape changes, weak tests, repeated verification failures, and gate interpretation.
- Harness mapping: inherit.

**Worker eligibility:** Workers are eligible only when the public interface is already decided, file ownership is narrow and non-overlapping, verification ceiling is explicit, the task does not reinterpret schema/report/release evidence, and parser dependency versions are not modified.

**Escalation triggers:** Any change to public artifact schema, CLI status, exit code, error code, language capability claim, parser dependency version, release package contents, or default-suite runtime.

**Mechanical exclusion:** Mechanical workers cannot own failing tests, replay evidence, metrics, or acceptance gates. Split docs-only edits from evidence interpretation.

**Unsupported harness behavior:** If a harness cannot choose models per agent, use `inherit` and continue.

## File Structure

- Create: `docs/plans/2026-06-01-post-bootstrap-stabilization-release-readiness.md` - this plan and progress ledger.
- Create: `xtask/src/commands.rs` - top-level `cargo xtask` command parsing, help text, and dispatch.
- Modify: `xtask/src/main.rs` - delegate to `xtask::commands::run_from_env_args`.
- Modify: `xtask/src/lib.rs` - export new xtask modules.
- Modify: `xtask/src/test_tiers.rs` - keep only test-tier planning and execution.
- Modify: `xtask/src/release.rs` - add package staging, checksum writing, package validation, and release command helpers.
- Create: `xtask/src/dogfood.rs` - dogfood command plan, execution, metrics, and artifact validation.
- Modify/Create: `xtask/tests/commands_contract.rs`, `xtask/tests/test_tiers.rs`, `xtask/tests/release_contract.rs`, `xtask/tests/dogfood_contract.rs`.
- Create: `.github/workflows/ci.yml` - default and contract CI gate.
- Create: `.github/workflows/specialist-gates.yml` - manual slow gates for certification, real-world, dogfood, and release package staging.
- Create: `docs/release-evidence/README.md` - how release evidence is recorded.
- Create: `docs/release-evidence/v0.1.0-dogfood.md` - dogfood evidence for this repo.
- Create: `docs/release-notes/v0.1.0.md` - v0.1.0 release notes.
- Modify: `docs/release.md` - release commands, package staging, checksum, and evidence requirements.
- Modify: `docs/testing-strategy.md` - document dogfood and release package staging as non-default gates.
- Modify: `README.md` - update status and quickstart to match implemented bootstrap.
- Create: `examples/python/README.md` and `examples/python/sqlite_consumer.py` - non-Rust SQLite consumer spike.

## Open Decisions

- **Release target set:** The package command must support an explicit `--target`. The first implementation stages the host-built binary and records the target string; the release owner chooses the final target matrix before asset publication.
- **Performance thresholds:** Tiny writer tripwires remain hard gates. Dogfood repository scan metrics are report-only until two same-machine baseline runs establish a stable threshold.
- **CI trigger policy:** Regular CI runs default and contract gates. Certification, real-world, dogfood, and package staging run through `workflow_dispatch` so slow gates stay intentional.

## Progress

- [x] Task 0: Plan baseline
- [x] Task 1: Split xtask command routing
- [x] Task 2: Add dogfood command and evidence model
- [x] Task 3: Add release package staging command
- [x] Task 4: Add CI and specialist workflows
- [x] Task 5: Dogfood this repo and capture v0.1.0 evidence
- [ ] Task 6: Add Python SQLite consumer spike
- [ ] Task 7: Update release docs, README, and release notes
- [ ] Task 8: Completion audit

## Tasks

### Task 0: Plan Baseline

**Files:**
- Create: `docs/plans/2026-06-01-post-bootstrap-stabilization-release-readiness.md`

**What to build:** Capture the release-readiness plan so follow-on implementation does not drift from the product boundary, test tiers, or release evidence rules.

**Approach:** Ground the plan in the existing docs and completed migration baseline. Include architecture quality, model routing, verification strategy, file structure, task order, and explicit open decisions.

**Acceptance criteria:**
- [x] Plan uses the required Razorback implementation-plan header.
- [x] Plan references the existing product, release, test, and contract docs.
- [x] Plan keeps old Julie read-only and out of implementation scope.
- [x] Plan gives every task files, behavior, approach, and concrete acceptance criteria.

### Task 1: Split xtask Command Routing

**Files:**
- Create: `xtask/src/commands.rs`
- Modify: `xtask/src/main.rs`
- Modify: `xtask/src/lib.rs`
- Modify: `xtask/src/test_tiers.rs`
- Modify/Create: `xtask/tests/commands_contract.rs`
- Modify: `xtask/tests/test_tiers.rs`
- Modify: `xtask/tests/release_contract.rs`

**What to build:** Move top-level `cargo xtask` dispatch out of `test_tiers.rs` so test-tier planning, release commands, dogfood commands, and help text have clear owners.

**Approach:** Add a `commands` module that accepts environment args, routes `test <tier>`, `test list`, `release package-list`, dogfood commands, and package commands. Keep `test_tiers` focused on tier names, test-plan creation, command display, and plan execution. Preserve existing public behavior for `cargo xtask test list` and `cargo xtask release package-list`.

**Acceptance criteria:**
- [x] `xtask/src/test_tiers.rs` no longer owns release-package routing.
- [x] `xtask::commands::run_from_env_args` is the only top-level route used by `xtask/src/main.rs`.
- [x] `cargo test -p xtask --test commands_contract` passes.
- [x] `cargo test -p xtask --test test_tiers` passes.
- [x] `cargo test -p xtask --test release_contract` passes.
- [x] `cargo xtask test list` prints the documented tiers.
- [x] `cargo xtask release package-list` prints the existing manifest.

### Task 2: Add Dogfood Command And Evidence Model

**Files:**
- Create: `xtask/src/dogfood.rs`
- Modify: `xtask/src/commands.rs`
- Modify: `xtask/src/lib.rs`
- Create: `xtask/tests/dogfood_contract.rs`
- Modify: `docs/testing-strategy.md`

**What to build:** Add a non-default dogfood command that exercises `julie-extract` against a source tree, validates the generated artifact, and records hard-gate and report-only metrics.

**Approach:** Implement `cargo xtask dogfood repo --root <path> --out-dir <path> [--binary <path>]`. The command should run `scan --json`, `info --json`, and `export --format jsonl --json`; write `artifact.sqlite`, `artifact.jsonl`, `scan-report.json`, `info-report.json`, `export-report.json`, and `metrics.json` under the output directory; validate SQLite metadata, file counts, symbol counts, JSONL parseability, and report statuses. Unit tests should cover pure command planning and validation using small temporary artifacts, not a full repo scan.

**Acceptance criteria:**
- [x] `cargo xtask dogfood repo --root . --out-dir target/dogfood/julie-extractors` is a documented command but not part of `cargo xtask test default`.
- [x] `metrics.json` records scan duration, info duration, export duration, SQLite bytes, JSONL bytes, row totals, and rows per second when calculable.
- [x] Hard validation fails on non-`ok` JSON reports, missing SQLite metadata, schema version mismatch, zero files, zero symbols, invalid JSONL, or missing output files.
- [x] Performance fields are report-only in this task; no repository-scan runtime threshold is enforced.
- [x] `cargo test -p xtask --test dogfood_contract` passes.
- [x] `cargo test -p xtask` passes.

### Task 3: Add Release Package Staging Command

**Files:**
- Modify: `xtask/Cargo.toml`
- Modify: `xtask/src/release.rs`
- Modify: `xtask/src/commands.rs`
- Modify: `xtask/tests/release_contract.rs`
- Modify: `docs/release.md`

**What to build:** Add a tested staging command that creates the v0.1.0 release package layout from the existing manifest and writes SHA-256 checksum files.

**Approach:** Implement `cargo xtask release package --version <version> --target <target> --out-dir <path> [--binary <path>]`. The command should require `docs/release-notes/v{version}.md`, stage `dist/{target}/julie-extract{exe_suffix}`, copy only the docs listed by `release_package_items()`, write `dist/{target}/julie-extract{exe_suffix}.sha256`, and fail if any required input is missing. Add `sha2` for portable checksum generation and `tempfile` as an xtask test dev-dependency if tests need temporary directories.

**Acceptance criteria:**
- [x] `cargo xtask release package-list` output remains exact and ordered.
- [x] Package staging copies only the binary, checksum, contract docs, architecture docs, testing/release docs, and the requested release note.
- [x] The checksum file contains the SHA-256 digest and staged binary path in a deterministic format.
- [x] Missing release notes, missing binary, unsupported extra package item kinds, and forbidden package paths produce typed xtask errors.
- [x] `cargo test -p xtask --test release_contract` passes.
- [x] A local staging command using a built `julie-extract` binary succeeds before release evidence is recorded.

### Task 4: Add CI And Specialist Workflows

**Files:**
- Create: `.github/workflows/ci.yml`
- Create: `.github/workflows/specialist-gates.yml`
- Modify: `docs/testing-strategy.md`
- Modify: `docs/release.md`
- Modify/Create: `xtask/tests/commands_contract.rs`

**What to build:** Add CI workflows that keep fast gates automatic and slow gates explicit.

**Approach:** `ci.yml` should run on pull requests and pushes to `main`, with formatting, `cargo metadata`, `cargo test -p xtask`, `cargo xtask test default`, and `cargo xtask test contract`. `specialist-gates.yml` should be `workflow_dispatch` and run certification, real-world smoke, real-world release, dogfood, and release package staging. Add a local convention test or docs check that the workflow command list matches `docs/testing-strategy.md` and `docs/release.md`.

**Acceptance criteria:**
- [x] Regular CI does not run certification, real-world, dogfood, or release package staging.
- [x] Specialist workflow includes certification, real-world smoke, real-world release, dogfood repo, and release package staging commands.
- [x] Workflow YAML parses with an available local parser.
- [x] Workflow command names match the documented commands.
- [x] `cargo test -p xtask` passes after workflow docs/tests are updated.

### Task 5: Dogfood This Repo And Capture v0.1.0 Evidence

**Files:**
- Create: `docs/release-evidence/README.md`
- Create: `docs/release-evidence/v0.1.0-dogfood.md`
- Modify: `docs/release.md`
- Modify: `docs/testing-strategy.md`

**What to build:** Generate real evidence that `julie-extract` can extract this repo into the versioned artifact and export JSONL through the public CLI.

**Approach:** Run the dogfood command on `/Users/murphy/source/julie-extractors` and keep generated artifacts under `target/dogfood/julie-extractors/`. Commit only the evidence document, not the generated SQLite or JSONL files. Record exact command, commit SHA, timestamp, hard-gate results, row totals, artifact sizes, and report-only performance metrics.

**Acceptance criteria:**
- [x] `cargo xtask dogfood repo --root . --out-dir target/dogfood/julie-extractors` exits `0`.
- [x] `scan-report.json`, `info-report.json`, and `export-report.json` have `status: ok`.
- [x] SQLite metadata reports schema version `1`, extract contract version `1`, and root path for this repo.
- [x] File count, symbol count, and JSONL record count are nonzero.
- [x] `docs/release-evidence/v0.1.0-dogfood.md` records the command, commit SHA, timestamp, hard-gate results, row totals, artifact sizes, and report-only timings.
- [x] Generated artifacts remain ignored or under `target/`, not committed.

### Task 6: Add Python SQLite Consumer Spike

**Files:**
- Create: `examples/python/README.md`
- Create: `examples/python/sqlite_consumer.py`
- Modify: `docs/testing-strategy.md`
- Modify: `README.md`

**What to build:** Prove that a non-Rust caller can consume the SQLite artifact with only a spawned CLI and a standard SQLite reader.

**Approach:** Add a small Python script that accepts an artifact path, reads `artifact_metadata`, counts key tables, prints a compact JSON summary, and exits nonzero for missing required metadata or zero file rows. Keep it read-only and dependency-free. Document running it against the dogfood artifact after Task 5.

**Acceptance criteria:**
- [ ] `python3 -m py_compile examples/python/sqlite_consumer.py` passes.
- [ ] The script uses only Python standard library modules.
- [ ] The script opens SQLite read-only and does not mutate the artifact.
- [ ] Running the script against the dogfood artifact prints valid JSON with schema version, extract contract version, root path, file count, symbol count, and relationship count.
- [ ] The example docs show the exact command using `target/dogfood/julie-extractors/artifact.sqlite`.

### Task 7: Update Release Docs, README, And Release Notes

**Files:**
- Modify: `README.md`
- Modify: `docs/release.md`
- Modify: `docs/testing-strategy.md`
- Create: `docs/release-notes/v0.1.0.md`
- Modify: `docs/release-notes/README.md`

**What to build:** Bring public project docs in line with the migrated product and the new release-readiness workflow.

**Approach:** Update status from planning/bootstrap to post-bootstrap release readiness. Add quickstart commands for `julie-extract scan`, `info`, `export`, dogfood, and Python consumer readback. Expand release docs with package staging, checksum expectations, evidence requirements, and the distinction between regular CI and specialist gates. Write v0.1.0 notes that list shipped surfaces and known boundaries without claiming publication.

**Acceptance criteria:**
- [ ] README no longer says the project is only "Planning and bootstrap".
- [ ] README keeps the standalone product boundary and non-goals visible.
- [ ] `docs/release.md` documents `cargo xtask release package-list`, `cargo xtask release package`, dogfood evidence, checksum verification, and branch gates.
- [ ] `docs/testing-strategy.md` documents dogfood and release package staging as non-default gates.
- [ ] `docs/release-notes/v0.1.0.md` names the CLI, SQLite, JSONL, reports, release package contents, and known non-goals.
- [ ] Docs contain no claims that release assets have been published unless that has happened.

### Task 8: Completion Audit

**Files:**
- Modify: `docs/plans/2026-06-01-post-bootstrap-stabilization-release-readiness.md`
- Modify: release evidence or release docs only if audit results require correction.

**What to build:** Prove the branch is ready to merge and record the final evidence.

**Approach:** Run the full branch gate plus the specialist checks required by touched areas. Update this plan's progress checkboxes and the verification ledger entries with final command results. Keep generated artifacts under `target/`.

**Acceptance criteria:**
- [ ] `cargo fmt --check` passes.
- [ ] `cargo metadata --format-version 1 --no-deps` passes.
- [ ] `cargo test -p xtask` passes.
- [ ] `cargo xtask test default` passes.
- [ ] `cargo xtask test contract` passes.
- [ ] `cargo xtask test certification` passes.
- [ ] `cargo xtask test real-world-smoke` passes.
- [ ] `cargo xtask test real-world-release` passes.
- [ ] `cargo xtask dogfood repo --root . --out-dir target/dogfood/julie-extractors` passes.
- [ ] `cargo xtask release package --version 0.1.0 --target <host-target> --out-dir target/release-package --binary <built-julie-extract>` passes.
- [ ] `python3 examples/python/sqlite_consumer.py target/dogfood/julie-extractors/artifact.sqlite` passes.
- [ ] `rg -n "/Users/murphy/source/julie" Cargo.toml Cargo.lock crates xtask .github examples` returns no matches.
- [ ] `rg -n "julie-server|julie-daemon|julie-adapter|mcp|embedding|watcher|dashboard|editing" xtask .github examples` returns no matches.
- [ ] `git diff --check` passes.
