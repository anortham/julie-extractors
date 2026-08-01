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
- [x] SQL snippets from `docs/contracts/sqlite-schema-v1.md` execute through `sqlite3 :memory:`.
- [x] JSON examples from `docs/contracts/jsonl-v1.md` and `docs/contracts/reports.md` parse with `jq`.
- [x] Placeholder scan over touched docs returns no matches.
- [x] Commit contains docs/checkpoints only, no moved Rust code.
- [x] Worker-scope verification passes and commit is recorded.

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
- [x] Workspace manifest names all crates that exist at the end of the task.
- [x] No Julie server, MCP, daemon, search, embedding, watcher, dashboard, or editing crates are referenced.
- [x] `cargo metadata` succeeds once the first crate is present.
- [x] Worker-scope verification passes and commit is recorded.

### Task 2: Move Extractor Crate And Fixtures

**Files:**
- Copy from: `/Users/murphy/source/julie/crates/julie-extractors/`
- Create: `crates/julie-extractors/`
- Copy from: `/Users/murphy/source/julie/fixtures/extraction/`
- Create: `fixtures/extraction/`
- Copy from: `/Users/murphy/source/julie/fixtures/elixir/basic.ex`
- Create: `fixtures/elixir/basic.ex`
- Copy from: `/Users/murphy/source/julie/fixtures/qml/real-world/cool-retro-term-main.qml`
- Copy from: `/Users/murphy/source/julie/fixtures/qml/real-world/kde-plasma-desktop-main.qml`
- Create: `fixtures/qml/real-world/`
- Copy from: `/Users/murphy/source/julie/fixtures/r/real-world/ggplot2-geom-point.R`
- Create: `fixtures/r/real-world/`
- Copy from: `/Users/murphy/source/julie/fixtures/real-world/json/memories.jsonl`
- Create: `fixtures/real-world/json/memories.jsonl`
- Copy from: `/Users/murphy/source/julie/fixtures/scala/basic.scala`
- Create: `fixtures/scala/basic.scala`

**What to build:** Move the reusable extraction engine and its fixture evidence mostly intact.

**Approach:** Preserve extractor source structure first, then fix only path and workspace breakage needed for compilation. Do not redesign language extractors during the move. Keep fixture paths stable unless include paths require a coordinated update.

**Acceptance criteria:**
- [x] `crates/julie-extractors/src/` exists with base modules, language modules, registry, pipeline, manager, factory, and capability snapshot.
- [x] `fixtures/extraction/capabilities.json` and golden fixtures exist.
- [x] `fixtures/elixir/basic.ex` exists for the copied Elixir full-fixture unit test.
- [x] Referenced QML, R, and JSON real-world support fixtures exist.
- [x] `fixtures/scala/basic.scala` exists for the copied Scala full-fixture unit test.
- [x] Capability snapshot path references resolve from the new crate location.
- [x] No dependency points back to `/Users/murphy/source/julie`.
- [x] Narrow extractor compile/test command passes or failing output is recorded with a concrete next task.
- [x] Worker-scope verification passes and commit is recorded.

### Task 3: Restore Extractor Test Tiers

**Files:**
- Modify: `crates/julie-extractors/Cargo.toml`
- Modify/Create: `crates/julie-extractors/src/tests/`
- Create/Modify: `xtask/` test-tier files if needed
- Modify: `docs/testing-strategy.md` only if command names need clarification

**What to build:** Make extractor tests runnable without letting slow parser certification or real-world corpora into the default suite.

**Approach:** Bring up default, language, golden fixture, capability matrix, certification, and real-world tiers according to `docs/testing-strategy.md`. The first passing suite should be narrow; slow gates must be tagged or routed out immediately.

**Acceptance criteria:**
- [x] Default test command is documented in the task report.
- [x] Golden/capability tests are runnable as a named non-default or contract/language tier.
- [x] Certification and real-world gates are not part of default tests.
- [x] A convention check or documented test-tier rule prevents slow tests from entering default.
- [x] Worker-scope verification passes and commit is recorded.

### Task 4: Move Extraction-Owned Language Configuration

**Files:**
- Copy or split from: `/Users/murphy/source/julie/languages/*.toml`
- Create/Modify: `languages/*.toml`
- Create/Modify: `languages/README.md`
- Create/Modify: `crates/julie-extractors/src/language_policy.rs`
- Modify: `crates/julie-extractors/src/lib.rs`
- Create/Modify: `crates/julie-extractors/src/tests/language_policy.rs`

**What to build:** Bring over language policy that directly affects artifact-producing extraction.

**Approach:** Include only `[literal_carriers]` config used to classify and
gate persisted `literals` artifact rows. Do not copy test-role TOML into this
task because SQLite/JSONL v1 does not define a `test_role` artifact field.
Language extension mapping stays in `crates/julie-extractors/src/language_spec/`
and fixture/capability evidence stays in `fixtures/extraction/capabilities.json`.
Leave Julie workspace/search/tokenizer/scoring/embedding/watcher policy behind.

**Acceptance criteria:**
- [x] Every moved config key has an artifact-producing purpose.
- [x] Non-extraction Julie policy is not copied.
- [x] Extractor tests that depend on language policy pass.
- [x] Worker-scope verification passes and commit is recorded.

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
- [x] Schema creates all v1 public tables.
- [x] Required indexes from the SQLite contract exist.
- [x] Metadata keys required by the contract are inserted and readable.
- [x] Query-plan or schema tests fail if required indexes are missing.
- [x] SQL contract snippets and schema tests pass.
- [x] Worker-scope verification passes and commit is recorded.

### Task 6: Batched SQLite Writer

**Files:**
- Modify: `crates/julie-extract-artifact/src/writer.rs`
- Create/Modify: `crates/julie-extract-artifact/src/model.rs`
- Create: `crates/julie-extract-artifact/tests/writer_contract.rs`
- Create: `crates/julie-extract-artifact/tests/writer_performance.rs`

**What to build:** Persist normalized extraction results into SQLite v1 with transaction and performance behavior required by the contract.

**Approach:** Write through explicit transactions, prepared statements, batched inserts, indexed file replacement, and data-loss guard checks. Start with tiny fixtures and deterministic row counts before broad language coverage.

**Acceptance criteria:**
- [x] `scan`-style batch writes multiple files in one transaction.
- [x] `update`-style write replaces exactly one file's rows.
- [x] `delete`-style write removes exactly one file's rows.
- [x] Unchanged file hashes avoid row churn for incremental/single-file writes.
- [x] Force scans rewrite rows even when content hashes are unchanged.
- [x] Data-loss guard preserves known-good rows on parser/read failure evidence.
- [x] Tiny-fixture performance tripwire detects per-row commits.
- [x] Worker-scope verification passes and commit is recorded.

### Task 7: JSON Report Model

**Files:**
- Modify: `crates/julie-extract-artifact/src/reports.rs`
- Create: `crates/julie-extract-artifact/tests/report_contract.rs`

**What to build:** Implement the stable JSON report and error-code model from `docs/contracts/reports.md`.

**Approach:** Keep report structs product-neutral. Include `input`, `artifact`, `tool`, `revision`, exhaustive `counts.rows_written`, exhaustive `counts.totals`, typed `errors`, and typed `warnings`.

**Acceptance criteria:**
- [x] Reports serialize with `report_schema_version: 1`.
- [x] Every status value from the report contract is covered by tests.
- [x] Every v1 error code has a stable serialized spelling.
- [x] Single-file success reports include `input.file_path` and `input.root_relative_path`.
- [x] Report row-count keys are exhaustive for SQLite v1.
- [x] Worker-scope verification passes and commit is recorded.

### Task 8: JSONL Exporter

**Files:**
- Create: `crates/julie-extract-artifact/src/jsonl.rs`
- Create: `crates/julie-extract-artifact/tests/jsonl_contract.rs`

**What to build:** Export SQLite v1 artifacts as JSONL v1 records.

**Approach:** Read canonical SQLite rows and emit deterministic `snapshot` records with the envelope and exact payload schemas from `docs/contracts/jsonl-v1.md`.

**Acceptance criteria:**
- [x] Export starts with `artifact` records and follows the documented record order.
- [x] Every record kind in JSONL v1 has a contract test.
- [x] SQLite JSON text columns are decoded into JSON objects or arrays.
- [x] Output is deterministic for the same artifact.
- [x] Failed export does not claim a complete output file.
- [x] Worker-scope verification passes and commit is recorded.

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
- [x] Binary name is `julie-extract`.
- [x] `scan`, `update`, `delete`, `info`, `export`, and `languages` parse.
- [x] `--json` reports match the report contract.
- [x] Exit codes `0`, `1`, `2`, and `3` are covered by tests.
- [x] No `analyze` command is implemented.
- [x] No server, daemon, MCP, search, embedding, watcher, dashboard, or editing behavior is linked.
- [x] Worker-scope verification passes and commit is recorded.

### Task 10: Source Discovery And Path Policy

**Files:**
- Create/Modify: `crates/julie-extract-cli/src/discovery.rs`
- Create/Modify: `crates/julie-extract-cli/src/paths.rs`
- Create: `crates/julie-extract-cli/tests/path_policy.rs`

**What to build:** Implement root/file canonicalization, ignore policy, file selection, and root-bound artifact behavior.

**Approach:** Use old Julie indexing path code as evidence, but rewrite dependencies around the standalone product. Store root-relative Unix paths. Treat unsupported/ignored update targets as stale-row cleanup.

**Acceptance criteria:**
- [x] `--root`, `--db`, `--file`, and `--ignore-file` canonicalize at the CLI boundary.
- [x] File outside root returns a typed error.
- [x] `delete --file` does not require the source file to exist.
- [x] `update --file` requires the source file to exist.
- [x] Root mismatch returns exit code `3` unless `scan --force` rebuilds.
- [x] Worker-scope verification passes and commit is recorded.

### Task 11: CLI Operations End To End

**Files:**
- Modify: `crates/julie-extract-cli/src/commands.rs`
- Modify: `crates/julie-extract-artifact/src/writer.rs`
- Create: `crates/julie-extract-cli/tests/operations_contract.rs`

**What to build:** Wire parser extraction, source discovery, artifact writer, reports, and JSONL export into end-to-end CLI behavior.

**Approach:** Start with tiny fixture roots. Prove full scan, force scan, single-file update, single-file delete, info, export, and languages. Keep watcher integration as documented command semantics, not a long-running watcher service.

**Acceptance criteria:**
- [x] `scan` creates a SQLite artifact with expected rows.
- [x] `scan` with no changes returns `no_change`.
- [x] `scan --force` rebuilds with `operation: scan`, `mode: force`.
- [x] `update` changes one file and preserves other files.
- [x] `delete` removes one file and missing rows return `not_found`.
- [x] `info` is read-only.
- [x] `export --format jsonl` emits valid JSONL v1.
- [x] `languages --json` emits capability snapshot data.
- [x] Worker-scope verification passes and commit is recorded.

### Task 12: Certification And Release Scaffolding

**Files:**
- Create/Modify: `xtask/`
- Create/Modify: release workflow files when packaging begins
- Create/Modify: docs for release and certification commands

**What to build:** Recreate extractor-owned certification, real-world evidence, and release packaging without Julie daemon packaging.

**Approach:** Use old Julie `xtask` and release workflow as evidence. The new release ships `julie-extract` binaries, checksums, schema docs, contract docs, and release notes.

**Acceptance criteria:**
- [x] Certification tier runs independently from default tests.
- [x] Real-world smoke and release profiles are separate.
- [x] Release package list contains only `julie-extract` binaries, checksums, docs, and release notes.
- [x] Parser dependency changes trigger certification gates.
- [x] Worker-scope verification passes and commit is recorded.

### Task 13: Erlang Capability Closure

**Files:**
- Modify: `crates/julie-extractors/src/erlang/`
- Modify: `fixtures/extraction/erlang/`, erlang row of `fixtures/extraction/capabilities.json`

**What to build:** Raise Erlang from the symbol-only tier to its FULL
`target_capabilities` by closing the `relationships`, `pending_relationships`,
`identifiers`, and `types` rows recorded as `status: open` in
`fixtures/extraction/capabilities.json`, then close the remaining Erlang
`kind_coverage` gaps that FULL capability status does not cover:
`literals` (string-literal call arguments with a verbatim carrier),
`structural_facts` (`erlang.behaviour_declaration` and the other `-behaviour`,
OTP callback, and `-include` shapes), and `complexity_metrics` (file and symbol
scope, which need an erlang entry in `base/complexity_metrics.rs`).

`literals` and `complexity_metrics` were closed by Task 11 of
`docs/plans/2026-07-31-erlang-xml-language-support-plan.md`: the erlang
identifier walk now records string call arguments under the verbatim callee
(`io:format`, or the bare atom for a local call), and `ERLANG_CONFIG` in
`base/complexity_metrics.rs` counts case/if/try/receive/catch containers and
their arms plus each guard alternative, with comprehensions as the loop
construct. Evidence is the `fixtures/extraction/erlang/control_flow` golden.

`structural_facts` was closed by Task 12 of that same plan: five erlang pattern
specs — `erlang.module_attribute.v1`, `erlang.behaviour_declaration.v1`,
`erlang.callback_declaration.v1`, `erlang.export_attribute.v1`, and
`erlang.include_directive.v1` — are registered in
`base/structural_fact_registry/builtins/erlang.rs` and emitted from the erlang
arm of `base/code_structural_facts.rs`. Every advertised pattern has golden
evidence in the existing erlang fixtures: `basic` carries the module,
behaviour, callback, and both export flavours, and `cross_file` carries
`-include` alongside `-include_lib`. No Erlang `kind_coverage` gap remains open.

**Approach:** This entry exists because `capability_matrix_open_rows_have_planned_closure_task`
resolves every open capability row's `planned_closure_task` against **this**
file, so it is the repository's registry of open capability work regardless of
which plan owns the implementation. The capability rows were closed by
`docs/plans/2026-07-31-erlang-xml-language-support-plan.md`: its Task 4 closed
identifiers, Task 6 relationships and pending relationships, and Task 7 types.
That plan never owned the three `kind_coverage` residuals above, so they stay
open here and are closed by this entry.

**Acceptance criteria:**
- [x] Erlang `capabilities` equals `target_capabilities` in the capability matrix.
- [x] Every closed row carries golden-fixture evidence rather than an empty vector.
- [x] `cargo xtask test capability` and `cargo xtask test golden` pass.
- [x] The erlang `literals` `kind_coverage` gap is closed with golden evidence.
- [x] The erlang `complexity_metrics` `kind_coverage` gap is closed with golden
      evidence for both the file and symbol scope.
- [x] The erlang `structural_facts` `kind_coverage` gap is closed with golden
      evidence, or re-recorded with a documented `not_applicable` reason.

### Task 14: XML Reference Edge Closure

**Files:**
- Modify: `crates/julie-extractors/src/xml/`
- Modify: `fixtures/extraction/xml/`, xml row of `fixtures/extraction/capabilities.json`

**What to build:** Turn XML's QName attribute references (`type`, `ref`, `base`,
`element`) into resolved `references` relationships, and the cross-document ones
into structured pending relationships, then raise `capabilities.relationships`
and `capabilities.pending_relationships` together with `target_capabilities`.
The XML `kind_coverage.literals` gap — attribute-value literals with a
`tag.attribute` carrier, from the `config_literals::tag_attribute_carrier` helper
html and vue already use — was closed by Task 11 of
`docs/plans/2026-07-31-erlang-xml-language-support-plan.md`, so only the two
reference-edge rows remain open here.

**Approach:** This entry exists because `capability_matrix_open_rows_have_planned_closure_task`
resolves every open capability row's `planned_closure_task` against **this**
file, so it is the repository's registry of open capability work regardless of
which plan owns the implementation. XML shipped at
`DATA_ONLY_CAPABILITIES` (symbols plus identifiers) with `capabilities ==
target_capabilities`; the two rows are recorded `status: open` only because
`capability_matrix_requires_relationship_fixture_evidence` forbids
`status: exception` for `relationships` while `capabilities.relationships` is
false. Closing them requires resolving QName prefixes through in-scope `xmlns`
declarations — the reason the reference edges were left out of the v1 tier.

**Acceptance criteria:**
- [ ] XML QName references resolve to the declaration they name, with golden evidence.
- [ ] `fixtures/extraction/xml/` gains a `negative` or `cross_file` fixture proving no wrong edges.
- [x] XML attribute-value literals are emitted with golden evidence, closing the
      `kind_coverage.literals` gap.
- [ ] `cargo xtask test capability` and `cargo xtask test golden` pass.

## Progress Tracking

Use this section as the execution ledger. Update task checkboxes only after the
task's acceptance criteria and verification ledger entry are complete.

- [x] Task 0: Contract Baseline Commit
- [x] Task 1: Rust Workspace Shell
- [x] Task 2: Move Extractor Crate And Fixtures
- [x] Task 3: Restore Extractor Test Tiers
- [x] Task 4: Move Extraction-Owned Language Configuration
- [x] Task 5: Artifact Crate Skeleton And Schema v1
- [x] Task 6: Batched SQLite Writer
- [x] Task 7: JSON Report Model
- [x] Task 8: JSONL Exporter
- [x] Task 9: CLI Skeleton And Commands
- [x] Task 10: Source Discovery And Path Policy
- [x] Task 11: CLI Operations End To End
- [x] Task 12: Certification And Release Scaffolding

## Verification Ledger

| Scope | Invariant | Command | Commit | Result | Time |
|-------|-----------|---------|--------|--------|------|
| worker-red-green | Contract baseline docs parse and have no placeholders. | `sqlite3 :memory:` SQL snippet check; `jq` JSON snippet check; placeholder scan | `3a3d889` | pass | `2026-05-31T17:04Z` |
| worker-red-green | Workspace shell is a valid Cargo workspace and contains no forbidden Julie behavior crates. | `cargo metadata --format-version 1 --no-deps` | `a54eda6` | pass | `2026-05-31T17:06Z` |
| worker-red-green | Moved extractor crate compiles, capability snapshot resolves, and copied support fixtures satisfy narrow fixture gates. | `cargo check -p julie-extractors`; `cargo test -p julie-extractors capability_snapshot`; targeted Elixir, JSONL, QML, and R fixture tests; source dependency scan | `f221c4a` | pass | `2026-05-31T17:10Z` |
| worker-red-green | Extractor test tiers are selected by `cargo xtask`, slow gates are feature-gated out of default tests, and old Julie evidence refs no longer require old commits/plans. | `cargo fmt --check`; `cargo test -p xtask`; `cargo xtask test default`; `cargo xtask test language rust`; `cargo xtask test contract`; `cargo xtask test certification`; `cargo xtask test real-world`; default inventory leak scan; `cargo metadata --format-version 1 --no-deps`; `git diff --check` | `7e1160e` | pass | `2026-05-31T17:27Z` |
| worker-red-green | Extraction-owned language policy contains only literal carrier config, is embedded by the crate, aliases JSX/TSX, and classifies/gates literals without copying Julie search/test-role policy. | `cargo fmt --check`; `cargo test -p julie-extractors language_policy -- --nocapture`; `cargo xtask test default`; `cargo metadata --format-version 1 --no-deps`; `git diff --check`; non-literal TOML section scan; stale old-Julie pipeline/test-role comment scan | `f477047` | pass | `2026-05-31T17:47Z` |
| worker-red-green | Artifact crate owns SQLite schema v1 tables, required indexes, metadata rows, report row domains, and default/contract test-tier wiring without old Julie internal schema tables. | `cargo fmt --check`; `cargo test -p xtask`; `cargo xtask test default`; `cargo xtask test contract`; `cargo metadata --format-version 1 --no-deps`; `git diff --check`; SQL fence extraction from `docs/contracts/sqlite-schema-v1.md` through `sqlite3 :memory:`; forbidden old-Julie table scan | `8fe5cbf` | pass | `2026-05-31T19:16Z` |
| worker-red-green | Artifact writer persists SQLite v1 row families with one transaction per operation, exact scan/update/delete replacement semantics, incremental hash skips, force-scan rewrites, rollback on failed batches, data-loss guarding, and tiny-fixture throughput protection. | `cargo test -p julie-extract-artifact --test writer_contract`; `cargo test -p julie-extract-artifact --test writer_performance`; `cargo test -p julie-extract-artifact`; `cargo xtask test default`; `cargo xtask test contract`; `cargo fmt --check`; `cargo metadata --format-version 1 --no-deps`; `git diff --check`; placeholder scan; forbidden-boundary scan | `8d2b60f` | pass | `2026-05-31T20:20Z` |
| worker-red-green | JSON report model serializes schema version 1, stable status/error code spellings, single-file input paths, and exhaustive SQLite v1 row-count domains without old Julie analysis/status fields. | `cargo test -p julie-extract-artifact --test report_contract`; `cargo test -p julie-extract-artifact`; `cargo xtask test default`; `cargo xtask test contract`; `cargo fmt --check`; `cargo metadata --format-version 1 --no-deps`; `git diff --check`; placeholder scan; old-Julie report-field scan | `9ad4d7e` | pass | `2026-05-31T20:29Z` |
| worker-red-green | JSONL exporter emits deterministic SQLite-derived snapshot records in JSONL v1 order, decodes SQLite JSON text columns, covers every record kind with contract tests, preserves all-or-error path export behavior, and keeps report/JSONL tests inside the contract tier. | `cargo test -p julie-extract-artifact --test jsonl_contract`; `cargo test -p julie-extract-artifact`; `cargo test -p xtask`; `cargo xtask test default`; `cargo xtask test contract`; `cargo fmt --check`; `cargo metadata --format-version 1 --no-deps`; `git diff --check`; placeholder scan; forbidden-boundary scan | `549c78c` | pass | `2026-05-31T20:46Z` |
| worker-red-green | `julie-extract` binary exposes only v1 commands, emits report-contract JSON for the skeleton paths, covers exit codes `0`, `1`, `2`, and `3`, rejects old Julie `analyze`, excludes server/tool behavior, and is wired into default and contract tiers. | `cargo test -p julie-extract-cli --test cli_contract`; `cargo test -p julie-extract-cli`; `cargo test -p xtask`; `cargo xtask test default`; `cargo xtask test contract`; `cargo fmt --check`; `cargo metadata --format-version 1 --no-deps`; `git diff --check`; placeholder scan; forbidden-boundary scan | `1c8a037` | pass | `2026-05-31T20:58Z` |
| worker-red-green | CLI source path policy canonicalizes root/db/file/ignore inputs, rejects outside-root and missing update targets with typed reports, allows delete of missing source files, removes stale rows for ignored update targets, and enforces root-bound artifacts with `scan --force` rebuild behavior. | `cargo test -p julie-extract-cli --test path_policy`; `cargo test -p julie-extract-cli --test cli_contract`; `cargo test -p julie-extract-cli`; `cargo test -p xtask`; `cargo xtask test default`; `cargo xtask test contract`; `cargo fmt --check`; `cargo metadata --format-version 1 --no-deps`; `git diff --check`; placeholder scan; forbidden-boundary scan | `dd4399e` | pass | `2026-05-31T21:21Z` |
| worker-red-green | CLI operations run end-to-end from discovery and parser extraction into SQLite, preserve snapshot semantics and no-change immutability, update/delete single files, export JSONL v1, expose languages capability data, and route operations into the contract tier. | `cargo test -p julie-extract-cli --test operations_contract`; `cargo test -p julie-extract-cli --test cli_contract`; `cargo test -p julie-extract-cli --test path_policy`; `cargo test -p julie-extract-artifact --test writer_contract`; `cargo test -p julie-extract-artifact --test jsonl_contract`; `cargo test -p julie-extract-cli`; `cargo test -p julie-extract-artifact`; `cargo test -p xtask`; `cargo xtask test default`; `cargo xtask test contract`; `cargo fmt --check`; `cargo metadata --format-version 1 --no-deps`; `git diff --check`; placeholder scan; forbidden-boundary scan | `87f2e1d` | pass | `2026-05-31T21:48Z` |
| worker-red-green | Certification, changed-path, real-world, and release package scaffolding are selected by `cargo xtask`, certification stays outside default, real-world smoke and release profiles are distinct, parser dependency changes add certification gates, and the release manifest contains only standalone `julie-extract` binaries, checksums, docs, and release notes. | `cargo test -p xtask`; `cargo xtask test default`; `cargo xtask test contract`; `cargo xtask test certification`; `cargo xtask test real-world-smoke`; `cargo xtask test real-world-release`; `cargo xtask test changed crates/julie-extractors/Cargo.toml`; `cargo xtask release package-list`; `cargo fmt --check`; `cargo metadata --format-version 1 --no-deps`; `git diff --check`; placeholder scan; forbidden-boundary scan | `513aa4d` | pass | `2026-05-31T22:02Z` |

## Execution Notes

- `/Users/murphy/source/julie` is read-only evidence unless the user explicitly says otherwise.
- Commit after each task or tightly coupled task pair.
- If old Julie code reality contradicts this plan, report a plan mismatch rather than redesigning locally.
- If a task discovers needed schema/report/CLI contract changes, stop and route that change through strategy-tier review before implementation.
- Keep test output scoped. Broad certification, real-world, release, and expensive performance gates are lead-owned.
