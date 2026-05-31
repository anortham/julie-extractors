# Julie Code Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Move extraction-owned code from `/Users/murphy/source/julie` into `/Users/murphy/source/julie-extractors` without importing Julie server, workspace, search, daemon, MCP, embedding, watcher, dashboard, or editing behavior.

**Architecture:** Start by moving the reusable extractor crate, fixtures, and extraction-owned language configuration. Then build new product modules around the contracts in `docs/contracts/`: a SQLite artifact writer, JSONL exporter, and `julie-extract` CLI. Old Julie database and external extract code are evidence sources, not copy targets.

**Tech Stack:** Rust workspace, tree-sitter extractor crate, SQLite, JSONL, clap CLI, fixture-based tests, xtask-style test tiers, contract tests.

**Architecture Quality:** Medium-high risk. The extractor engine can move mostly intact, but artifact writing, CLI/report behavior, and test-tier boundaries are public product interfaces and must be designed against the new contracts rather than old Julie internals.

---

## Source Documents

- `AGENTS.md`: product boundary and test discipline.
- `RAZORBACK.md`: worker eligibility, strategy-tier areas, and verification ownership.
- `docs/decisions/0001-standalone-extraction-product.md`: accepted product boundary.
- `docs/architecture/product-boundary.md`: source tree to versioned artifact boundary.
- `docs/architecture/schema-principles.md`: schema and performance principles.
- `docs/contracts/cli.md`: `julie-extract` command contract.
- `docs/contracts/sqlite-schema-v1.md`: SQLite v1 schema and performance contract.
- `docs/contracts/jsonl-v1.md`: JSONL v1 export contract.
- `docs/contracts/reports.md`: JSON report and error contract.
- `docs/testing-strategy.md`: fast default suite and slow-tier routing.
- `docs/plans/2026-05-31-migration-inventory.md`: source-to-target migration map.
- `docs/plans/2026-05-31-repo-bootstrap-implementation-plan.md`: bootstrap task history.

## Architecture Quality

**Affected modules:** extractor crate, fixtures, language configuration, workspace manifests, test-tier tooling, SQLite artifact writer, JSONL exporter, CLI, release packaging.

**Caller-facing interface:** `julie-extract` CLI, SQLite schema v1, JSONL v1, JSON reports, language capability snapshot, and Rust crate API.

**Depth/locality check:** Parser complexity stays inside `crates/julie-extractors`. Artifact persistence stays inside `crates/julie-extract-artifact`. Process-facing behavior stays inside `crates/julie-extract-cli`. Test-tier orchestration stays in `xtask/` or equivalent repo-local tooling.

**Test surface:** Workers prove behavior through crate tests, CLI contract tests, schema/readback tests, JSONL export tests, and fixture gates. Private helper tests are acceptable only as support for public behavior tests.

**Seams/adapters:** The only intended seams are extractor engine to normalized model, normalized model to artifact writer, artifact to JSONL exporter, and CLI to product operations. Do not introduce Julie compatibility adapters unless the user explicitly asks.

**Rejected shortcuts:** Do not copy Julie's database schema wholesale. Do not port `julie-server extract analyze`. Do not back-port extractor features into `/Users/murphy/source/julie`. Do not make MCP/server/search behavior compile in this repo.

**Architecture risk:** Medium-high until the new artifact writer and CLI contract tests are in place.

## Verification Strategy

**Project source of truth:** `AGENTS.md`, `RAZORBACK.md`, `docs/testing-strategy.md`, and the contracts under `docs/contracts/`.

**Worker red/green scope:** Use the narrowest available command for the changed area. Before Cargo exists here, worker scope is docs/file-presence checks. After the workspace exists, worker scope is the specific crate/test target for the touched module, such as extractor unit tests, one fixture case, one schema writer test, or one CLI contract test.

**Worker ceiling:** Workers may run default tests, one crate's unit tests, one language/fixture gate, one CLI contract test target, and docs checks. Workers must not own certification, real-world corpus, release packaging, or broad performance acceptance gates.

**Worker gate invariant:** Each worker-owned gate must prove one concrete behavior: extractor output shape, fixture stability, required SQLite table/index existence, batched writer behavior, report shape, JSONL record shape, or CLI exit/status mapping.

**Lead affected-change scope:** After each coherent phase, run all tests for changed crates plus docs checks and contract snippet checks. Once test tiers exist, use the affected-change tier from `docs/testing-strategy.md`.

**Branch gate:** Before handoff, run the default tier plus the contract tier. Release, certification, and real-world tiers are not branch gates unless the phase changes parser versions, capability claims, packaging, or real-world evidence.

**Replay/metric evidence:** Tiny-fixture writer budget and required-index/query-plan checks are hard gates once implemented. Real-world scan throughput is report-only until a release plan marks a concrete threshold as a hard gate.

**Escalation triggers:** Public artifact schema changes, CLI status/exit/error changes, parser dependency changes, capability claim changes, weak evidence around performance gates, repeated worker failures, or hidden coupling to old Julie internals.

**Assigned verification failure:** Workers stop and report when assigned verification fails unless the task explicitly says to update that gate.

**Verification ledger:** Record invariant, command, scope label, commit SHA, result, and timestamp. For performance or replay evidence, record hard-gate metrics and report-only metrics. Reuse a passing ledger entry only when HEAD and scope match.

## Model Routing

**Project source of truth:** `RAZORBACK.md`.

**Strategy tier:** Planning, architecture, decomposition, contract interpretation, lead review, finding triage.
- Harness mapping: inherit in this Codex session unless the lead explicitly selects an available override.

**Implementation tier:** Bounded migration tasks after interfaces are decided and file ownership is narrow.
- Harness mapping: inherit.

**Mechanical tier:** Docs links, manifest cleanup, fixture copying, formatting, and rote path updates with no gate interpretation.
- Harness mapping: inherit.

**Gate-interpretation reviewer:** Review of failing tests, schema contract failures, report mismatches, or performance-gate failures.
- Harness mapping: inherit; escalate to strategy tier when failure meaning is ambiguous.

**Escalation tier:** Public schema/API changes, parser dependency issues, subtle cross-language correctness, repeated verification failures, weak performance evidence.
- Harness mapping: inherit.

**Worker eligibility:** Workers are eligible only when the public interface is already decided, file ownership is narrow and non-overlapping, verification ceiling is explicit, the task does not reinterpret schema/report/release evidence, and parser dependency versions are not modified.

**Escalation triggers:** Any change to public artifact schema, CLI status, exit code, error code, language capability claim, parser dependency version, or default-suite runtime.

**Mechanical exclusion:** Mechanical workers cannot own failing tests, replay evidence, metrics, or acceptance gates.

**Unsupported harness behavior:** If a harness cannot choose models per agent, use `inherit` and continue.

## File Structure

Create or modify these product-owned areas:

- `Cargo.toml`: root workspace manifest.
- `crates/julie-extractors/`: moved extractor engine and Rust API.
- `crates/julie-extract-artifact/`: new SQLite v1 writer, JSONL exporter, artifact metadata, and report data model.
- `crates/julie-extract-cli/`: `julie-extract` binary and command handling.
- `fixtures/extraction/`: moved golden fixtures and capability snapshot.
- `languages/`: extraction-owned language policy copied or split from old Julie language config.
- `xtask/`: test-tier, certification, performance, and release helpers when code work reaches those gates.
- `docs/contracts/`: public contracts that implementation must follow.
- `docs/plans/`: migration progress and follow-on implementation plans.

Read-only source paths:

- `/Users/murphy/source/julie/crates/julie-extractors/`
- `/Users/murphy/source/julie/fixtures/extraction/`
- `/Users/murphy/source/julie/languages/*.toml`
- `/Users/murphy/source/julie/src/external_extract/`
- `/Users/murphy/source/julie/src/database/`
- `/Users/murphy/source/julie/src/indexing_core/`
- `/Users/murphy/source/julie/src/tools/workspace/indexing/file_policy.rs`
- `/Users/murphy/source/julie/xtask/`

## Tasks

### Task 0: Contract Baseline Commit

**Files:**
- Stage: `README.md`
- Stage: `docs/architecture/schema-principles.md`
- Stage: `docs/testing-strategy.md`
- Stage: `docs/contracts/cli.md`
- Stage: `docs/contracts/sqlite-schema-v1.md`
- Stage: `docs/contracts/jsonl-v1.md`
- Stage: `docs/contracts/reports.md`
- Stage: `docs/plans/2026-05-31-julie-code-migration-implementation-plan.md`
- Stage: `.memories/2026-05-31/165329_074d.md`
- Stage: `.memories/2026-05-31/165518_aaa7.md`
- Stage: `.memories/2026-05-31/170234_a83a.md`

**What to build:** Preserve the contract baseline before any Rust code migration begins.

**Approach:** Verify docs snippets, stage only product-bootstrap docs and checkpoint files, then commit with a docs-focused message. Do not include copied code in this commit.

**Acceptance criteria:**
- [ ] SQL snippets from `docs/contracts/sqlite-schema-v1.md` execute through `sqlite3 :memory:`.
- [ ] JSON examples from `docs/contracts/jsonl-v1.md` and `docs/contracts/reports.md` parse with `jq`.
- [ ] Placeholder scan over touched docs returns no matches.
- [ ] Commit contains docs/checkpoints only, no moved Rust code.
- [ ] Worker-scope verification passes and commit is recorded.

### Task 1: Rust Workspace Shell

**Files:**
- Create: `Cargo.toml`
- Create: `.gitignore`
- Create: `crates/`
- Create: `fixtures/`
- Create: `languages/`
- Create: `xtask/` when the first test-tier command lands

**What to build:** Establish the standalone Rust workspace layout before moving the extractor crate.

**Approach:** Use a workspace with separate crates for extractor engine, artifact persistence, and CLI. Keep package names aligned with the product names: `julie-extractors`, `julie-extract-artifact`, and `julie-extract-cli`.

**Acceptance criteria:**
- [ ] Workspace manifest names all crates that exist at the end of the task.
- [ ] No Julie server, MCP, daemon, search, embedding, watcher, dashboard, or editing crates are referenced.
- [ ] `cargo metadata` succeeds once the first crate is present.
- [ ] Worker-scope verification passes and commit is recorded.

### Task 2: Move Extractor Crate And Fixtures

**Files:**
- Copy from: `/Users/murphy/source/julie/crates/julie-extractors/`
- Create: `crates/julie-extractors/`
- Copy from: `/Users/murphy/source/julie/fixtures/extraction/`
- Create: `fixtures/extraction/`

**What to build:** Move the reusable extraction engine and its fixture evidence mostly intact.

**Approach:** Preserve extractor source structure first, then fix only path and workspace breakage needed for compilation. Do not redesign language extractors during the move. Keep fixture paths stable unless include paths require a coordinated update.

**Acceptance criteria:**
- [ ] `crates/julie-extractors/src/` exists with base modules, language modules, registry, pipeline, manager, factory, and capability snapshot.
- [ ] `fixtures/extraction/capabilities.json` and golden fixtures exist.
- [ ] Capability snapshot path references resolve from the new crate location.
- [ ] No dependency points back to `/Users/murphy/source/julie`.
- [ ] Narrow extractor compile/test command passes or failing output is recorded with a concrete next task.
- [ ] Worker-scope verification passes and commit is recorded.

### Task 3: Restore Extractor Test Tiers

**Files:**
- Modify: `crates/julie-extractors/Cargo.toml`
- Modify/Create: `crates/julie-extractors/src/tests/`
- Create/Modify: `xtask/` test-tier files if needed
- Modify: `docs/testing-strategy.md` only if command names need clarification

**What to build:** Make extractor tests runnable without letting slow parser certification or real-world corpora into the default suite.

**Approach:** Bring up default, language, golden fixture, capability matrix, certification, and real-world tiers according to `docs/testing-strategy.md`. The first passing suite should be narrow; slow gates must be tagged or routed out immediately.

**Acceptance criteria:**
- [ ] Default test command is documented in the task report.
- [ ] Golden/capability tests are runnable as a named non-default or contract/language tier.
- [ ] Certification and real-world gates are not part of default tests.
- [ ] A convention check or documented test-tier rule prevents slow tests from entering default.
- [ ] Worker-scope verification passes and commit is recorded.

### Task 4: Move Extraction-Owned Language Configuration

**Files:**
- Copy or split from: `/Users/murphy/source/julie/languages/*.toml`
- Create/Modify: `languages/*.toml`
- Modify: `crates/julie-extractors/` language config loading code if needed

**What to build:** Bring over language policy that directly affects artifact-producing extraction.

**Approach:** Include config used for literal carrier classification, test-role classification only if artifact-owned, language extension mapping, and fixture/capability evidence. Leave Julie workspace/search policy behind.

**Acceptance criteria:**
- [ ] Every moved config key has an artifact-producing purpose.
- [ ] Non-extraction Julie policy is not copied.
- [ ] Extractor tests that depend on language policy pass.
- [ ] Worker-scope verification passes and commit is recorded.

### Task 5: Artifact Crate Skeleton And Schema v1

**Files:**
- Create: `crates/julie-extract-artifact/Cargo.toml`
- Create: `crates/julie-extract-artifact/src/lib.rs`
- Create: `crates/julie-extract-artifact/src/schema.rs`
- Create: `crates/julie-extract-artifact/src/metadata.rs`
- Create: `crates/julie-extract-artifact/src/writer.rs`
- Create: `crates/julie-extract-artifact/src/reports.rs`
- Create: `crates/julie-extract-artifact/tests/schema_contract.rs`

**What to build:** Implement the clean SQLite v1 schema and metadata contract, separate from old Julie database modules.

**Approach:** Start with schema creation/readback and required indexes from `docs/contracts/sqlite-schema-v1.md`. Use old Julie database code only as evidence for extraction domains and operational pitfalls. Do not copy internal Julie schema tables.

**Acceptance criteria:**
- [ ] Schema creates all v1 public tables.
- [ ] Required indexes from the SQLite contract exist.
- [ ] Metadata keys required by the contract are inserted and readable.
- [ ] Query-plan or schema tests fail if required indexes are missing.
- [ ] SQL contract snippets and schema tests pass.
- [ ] Worker-scope verification passes and commit is recorded.

### Task 6: Batched SQLite Writer

**Files:**
- Modify: `crates/julie-extract-artifact/src/writer.rs`
- Create/Modify: `crates/julie-extract-artifact/src/model.rs`
- Create: `crates/julie-extract-artifact/tests/writer_contract.rs`
- Create: `crates/julie-extract-artifact/tests/writer_performance.rs`

**What to build:** Persist normalized extraction results into SQLite v1 with transaction and performance behavior required by the contract.

**Approach:** Write through explicit transactions, prepared statements, batched inserts, indexed file replacement, and data-loss guard checks. Start with tiny fixtures and deterministic row counts before broad language coverage.

**Acceptance criteria:**
- [ ] `scan`-style batch writes multiple files in one transaction.
- [ ] `update`-style write replaces exactly one file's rows.
- [ ] `delete`-style write removes exactly one file's rows.
- [ ] Unchanged file hashes avoid row churn.
- [ ] Data-loss guard preserves known-good rows on parser/read failure evidence.
- [ ] Tiny-fixture performance tripwire detects per-row commits.
- [ ] Worker-scope verification passes and commit is recorded.

### Task 7: JSON Report Model

**Files:**
- Modify: `crates/julie-extract-artifact/src/reports.rs`
- Create: `crates/julie-extract-artifact/tests/report_contract.rs`

**What to build:** Implement the stable JSON report and error-code model from `docs/contracts/reports.md`.

**Approach:** Keep report structs product-neutral. Include `input`, `artifact`, `tool`, `revision`, exhaustive `counts.rows_written`, exhaustive `counts.totals`, typed `errors`, and typed `warnings`.

**Acceptance criteria:**
- [ ] Reports serialize with `report_schema_version: 1`.
- [ ] Every status value from the report contract is covered by tests.
- [ ] Every v1 error code has a stable serialized spelling.
- [ ] Single-file success reports include `input.file_path` and `input.root_relative_path`.
- [ ] Report row-count keys are exhaustive for SQLite v1.
- [ ] Worker-scope verification passes and commit is recorded.

### Task 8: JSONL Exporter

**Files:**
- Create: `crates/julie-extract-artifact/src/jsonl.rs`
- Create: `crates/julie-extract-artifact/tests/jsonl_contract.rs`

**What to build:** Export SQLite v1 artifacts as JSONL v1 records.

**Approach:** Read canonical SQLite rows and emit deterministic `snapshot` records with the envelope and exact payload schemas from `docs/contracts/jsonl-v1.md`.

**Acceptance criteria:**
- [ ] Export starts with `artifact` records and follows the documented record order.
- [ ] Every record kind in JSONL v1 has a contract test.
- [ ] SQLite JSON text columns are decoded into JSON objects or arrays.
- [ ] Output is deterministic for the same artifact.
- [ ] Failed export does not claim a complete output file.
- [ ] Worker-scope verification passes and commit is recorded.

### Task 9: CLI Skeleton And Commands

**Files:**
- Create: `crates/julie-extract-cli/Cargo.toml`
- Create: `crates/julie-extract-cli/src/main.rs`
- Create: `crates/julie-extract-cli/src/args.rs`
- Create: `crates/julie-extract-cli/src/commands.rs`
- Create: `crates/julie-extract-cli/tests/cli_contract.rs`

**What to build:** Implement the `julie-extract` process interface.

**Approach:** Build only commands in `docs/contracts/cli.md`: `scan`, `update`, `delete`, `info`, `export`, and `languages`. Map command outcomes to report statuses and exit codes from the contracts.

**Acceptance criteria:**
- [ ] Binary name is `julie-extract`.
- [ ] `scan`, `update`, `delete`, `info`, `export`, and `languages` parse.
- [ ] `--json` reports match the report contract.
- [ ] Exit codes `0`, `1`, `2`, and `3` are covered by tests.
- [ ] No `analyze` command is implemented.
- [ ] No server, daemon, MCP, search, embedding, watcher, dashboard, or editing behavior is linked.
- [ ] Worker-scope verification passes and commit is recorded.

### Task 10: Source Discovery And Path Policy

**Files:**
- Create/Modify: `crates/julie-extract-cli/src/discovery.rs`
- Create/Modify: `crates/julie-extract-cli/src/paths.rs`
- Create: `crates/julie-extract-cli/tests/path_policy.rs`

**What to build:** Implement root/file canonicalization, ignore policy, file selection, and root-bound artifact behavior.

**Approach:** Use old Julie indexing path code as evidence, but rewrite dependencies around the standalone product. Store root-relative Unix paths. Treat unsupported/ignored update targets as stale-row cleanup.

**Acceptance criteria:**
- [ ] `--root`, `--db`, `--file`, and `--ignore-file` canonicalize at the CLI boundary.
- [ ] File outside root returns a typed error.
- [ ] `delete --file` does not require the source file to exist.
- [ ] `update --file` requires the source file to exist.
- [ ] Root mismatch returns exit code `3` unless `scan --force` rebuilds.
- [ ] Worker-scope verification passes and commit is recorded.

### Task 11: CLI Operations End To End

**Files:**
- Modify: `crates/julie-extract-cli/src/commands.rs`
- Modify: `crates/julie-extract-artifact/src/writer.rs`
- Create: `crates/julie-extract-cli/tests/operations_contract.rs`

**What to build:** Wire parser extraction, source discovery, artifact writer, reports, and JSONL export into end-to-end CLI behavior.

**Approach:** Start with tiny fixture roots. Prove full scan, force scan, single-file update, single-file delete, info, export, and languages. Keep watcher integration as documented command semantics, not a long-running watcher service.

**Acceptance criteria:**
- [ ] `scan` creates a SQLite artifact with expected rows.
- [ ] `scan` with no changes returns `no_change`.
- [ ] `scan --force` rebuilds with `operation: scan`, `mode: force`.
- [ ] `update` changes one file and preserves other files.
- [ ] `delete` removes one file and missing rows return `not_found`.
- [ ] `info` is read-only.
- [ ] `export --format jsonl` emits valid JSONL v1.
- [ ] `languages --json` emits capability snapshot data.
- [ ] Worker-scope verification passes and commit is recorded.

### Task 12: Certification And Release Scaffolding

**Files:**
- Create/Modify: `xtask/`
- Create/Modify: release workflow files when packaging begins
- Create/Modify: docs for release and certification commands

**What to build:** Recreate extractor-owned certification, real-world evidence, and release packaging without Julie daemon packaging.

**Approach:** Use old Julie `xtask` and release workflow as evidence. The new release ships `julie-extract` binaries, checksums, schema docs, contract docs, and release notes.

**Acceptance criteria:**
- [ ] Certification tier runs independently from default tests.
- [ ] Real-world smoke and release profiles are separate.
- [ ] Release package list contains only `julie-extract` binaries, checksums, docs, and release notes.
- [ ] Parser dependency changes trigger certification gates.
- [ ] Worker-scope verification passes and commit is recorded.

## Progress Tracking

Use this section as the execution ledger. Update task checkboxes only after the
task's acceptance criteria and verification ledger entry are complete.

- [ ] Task 0: Contract Baseline Commit
- [ ] Task 1: Rust Workspace Shell
- [ ] Task 2: Move Extractor Crate And Fixtures
- [ ] Task 3: Restore Extractor Test Tiers
- [ ] Task 4: Move Extraction-Owned Language Configuration
- [ ] Task 5: Artifact Crate Skeleton And Schema v1
- [ ] Task 6: Batched SQLite Writer
- [ ] Task 7: JSON Report Model
- [ ] Task 8: JSONL Exporter
- [ ] Task 9: CLI Skeleton And Commands
- [ ] Task 10: Source Discovery And Path Policy
- [ ] Task 11: CLI Operations End To End
- [ ] Task 12: Certification And Release Scaffolding

## Execution Notes

- `/Users/murphy/source/julie` is read-only evidence unless the user explicitly says otherwise.
- Commit after each task or tightly coupled task pair.
- If old Julie code reality contradicts this plan, report a plan mismatch rather than redesigning locally.
- If a task discovers needed schema/report/CLI contract changes, stop and route that change through strategy-tier review before implementation.
- Keep test output scoped. Broad certification, real-world, release, and expensive performance gates are lead-owned.
