# Writer Current-Schema Performance Guard Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Add a non-default `cargo xtask performance writer-current-schema` command that writes a large deterministic current-schema SQLite artifact and records release-evidence metrics.

**Architecture:** Keep the new guard in `xtask::performance`, next to the existing repeatable baseline command. The command uses the public `julie_extract_artifact` writer/model API directly so it isolates SQLite writer scale without changing the public extraction CLI, SQLite schema, JSONL schema, or downstream Miller/Eros contracts.

**Tech Stack:** Rust 2024, `xtask`, `julie-extract-artifact`, `rusqlite` through the artifact writer, `serde`/`serde_json`, repository test-tier commands documented in `docs/testing-strategy.md`.

**Architecture Quality:** Approved shape is a new xtask evidence command with low-to-medium architecture risk. The only new dependency is `xtask` depending on `julie-extract-artifact`; the artifact writer remains the production interface under test and no artifact contract changes are allowed.

---

## Source Design

Approved spec:

- `docs/plans/2026-06-09-writer-current-schema-performance-design.md`

This implementation plan covers only TODO item 6. Items 7, 8, and 9 stay out of
scope until item 6 is implemented and verified.

## File Structure

- Modify `xtask/Cargo.toml`: add a path dependency on
  `julie-extract-artifact`.
- Modify `xtask/src/performance.rs:12-383`: add plan structs, parser, fixture
  generation, writer execution, summary serialization, and subcommand routing
  for `writer-current-schema`.
- Modify `xtask/tests/performance_baseline_contract.rs:1-219`: add parser,
  summary serialization, small execution, and row-total tests for the new
  writer command while keeping existing baseline tests intact.
- Modify `xtask/tests/commands_contract.rs:129-153`: add command-routing
  coverage for `cargo xtask performance writer-current-schema`.
- Modify `docs/testing-strategy.md:198-235`: document the manual writer guard,
  its non-default status, output files, and how it relates to real-repo
  performance baselines.
- Modify `TODO.md:74-107`: after implementation and verification only, mark
  item 6 done with the actual command and evidence used. Preserve existing user
  edits around items 7-9 and the untracked `.memories/2026-06-05/161313_6210.md`.

## Verification Strategy

**Project source of truth:** `AGENTS.md`, `CLAUDE.md`,
`docs/testing-strategy.md`, `.github/workflows/ci.yml`, and
`.github/workflows/specialist-gates.yml`.

**Worker red/green scope:** Use `cargo test -p xtask performance_baseline_contract`
for parser, summary, and tiny execution checks. Use
`cargo test -p xtask commands_contract` for top-level xtask routing. Use
`cargo test -p julie-extract-artifact writer_performance` after adding the
xtask dependency to prove existing fast writer tripwires still pass.

**Worker ceiling:** Workers may run the three focused commands above and a tiny
manual writer command with explicit small dimensions. Workers do not own
`cargo xtask test default`, `cargo xtask test contract`, real-repo baselines, or
release evidence acceptance.

**Worker gate invariant:** The focused xtask tests prove argument parsing,
summary serialization, command routing, and a small real SQLite artifact write.
The artifact writer test proves existing fast writer tripwires still stay in
the default artifact test surface.

**Lead affected-change scope:** After implementation batches, run
`cargo test -p xtask` and
`cargo test -p julie-extract-artifact writer_performance`.

**Branch gate:** Before closing TODO item 6, run:

```bash
cargo test -p xtask
cargo xtask test default
cargo xtask test contract
```

**Replay/metric evidence:** The new writer command is report-only for timing.
Hard gates are successful artifact write, non-zero rows in every current v3
child-row domain, valid JSON summary, and default/contract verification passing.
Elapsed time, rows per second, and artifact size are recorded metrics, not CI
thresholds.

**Escalation triggers:** Escalate if implementation requires a SQLite schema
change, JSONL schema change, public CLI status/exit-code change, parser
dependency change, language capability claim, or a default-suite runtime
increase. Escalate if `xtask` depending on `julie-extract-artifact` creates a
cycle or hidden release-packaging issue.

**Assigned verification failure:** Workers stop and report when assigned
verification fails unless the failing gate is explicitly part of their task.

**Verification ledger:** Record invariant, command, scope label, commit SHA,
result, and timestamp. For the writer command, also record output paths, row
totals, artifact size, elapsed write time, and rows per second. Reuse an
existing passing ledger entry only if it is for the same HEAD and same scope.

## Model Routing

**Project source of truth:** `RAZORBACK.md`.

**Strategy tier:** planning, architecture, decomposition, lead review, finding
triage.
- Harness mapping: inherit current Codex session model. The repo policy defines
  tiers but does not pin provider-specific model names.

**Implementation tier:** bounded worker tasks from this clear plan.
- Harness mapping: inherit current Codex session model.

**Mechanical tier:** docs, manifests, formatting, and rote edits with no metric
or acceptance-gate ownership.
- Harness mapping: inherit current Codex session model.

**Gate-interpretation reviewer:** use lead/strategy tier when reading metric
evidence, failing tests, or deciding whether a timing result is acceptable.
- Harness mapping: inherit current Codex session model.

**Escalation tier:** use strategy tier for schema/report/CLI contract changes,
weak evidence, repeated failures, hidden Julie coupling, or default-suite
runtime growth.
- Harness mapping: inherit current Codex session model.

**Worker eligibility:** Workers may implement bounded subtasks because the
public interface is decided, file ownership is narrow, and verification ceilings
are explicit.

**Escalation triggers:** Escalate on any artifact schema change, public CLI
status/exit-code change, JSONL/report contract change, language capability
claim, parser dependency update, or evidence-quality uncertainty.

**Mechanical exclusion:** Mechanical workers cannot own failing tests, replay
evidence, metrics, or acceptance gates. Split docs-only updates from metric
interpretation.

**Unsupported harness behavior:** If the harness cannot choose models per
agent, use `inherit`, note it in worker output, and continue.

## Task 1: Add Writer Command Contract Surface

**Files:**

- Modify: `xtask/src/performance.rs:12-216`
- Modify: `xtask/tests/performance_baseline_contract.rs:1-219`

**What to build:** Add the public planning surface for
`performance writer-current-schema`. Keep existing `baseline` behavior
unchanged. Introduce structs that can be tested without running a large
workload.

**Approach:**

- Add `WriterCurrentSchemaPlan` with:
  - `out_dir: PathBuf`
  - `db_path: PathBuf`
  - `summary_path: PathBuf`
  - `files: usize`
  - `symbols_per_file: usize`
  - `identifiers_per_file: usize`
  - `source_regions_per_file: usize`
- Add constants for defaults. Use the approved design values unless local smoke
  evidence during implementation shows they are too heavy:
  - `files = 10_000`
  - `symbols_per_file = 8`
  - `identifiers_per_file = 24`
  - `source_regions_per_file = 12`
- Add `plan_writer_current_schema_from_args` that requires
  `writer-current-schema`, requires `--out-dir`, accepts the optional numeric
  flags above, rejects zero values, rejects missing values, and rejects unknown
  arguments.
- Update `run_from_args` so `baseline` still routes to baseline and
  `writer-current-schema` routes to the new command. Unknown performance
  subcommands should return a usage error naming the accepted subcommands.
- Add tests for:
  - default dimensions and output paths
  - explicit dimension parsing
  - zero and non-integer values fail with useful usage text
  - unknown arguments fail
  - existing baseline parser tests still pass

**Acceptance criteria:**

- [ ] Parser tests cover success, defaults, explicit dimensions, and invalid
  inputs.
- [ ] Existing baseline tests are unchanged in behavior.
- [ ] `cargo test -p xtask performance_baseline_contract` passes.

## Task 2: Generate Current-Schema Artifact Rows

**Files:**

- Modify: `xtask/Cargo.toml`
- Modify: `xtask/src/performance.rs:216-383`
- Modify: `xtask/tests/performance_baseline_contract.rs:1-219`

**What to build:** Add deterministic fixture generation and small execution
tests. The generator must create every current v3 extraction child-row domain
without using parser source text or the public extraction CLI.

**Approach:**

- Add `julie-extract-artifact = { path = "../crates/julie-extract-artifact" }`
  to `xtask/Cargo.toml`.
- In `xtask/src/performance.rs`, import public artifact model/writer types:
  `ArtifactMetadata`, `ArtifactFile`, `ArtifactSymbol`,
  `ArtifactSymbolAnnotation`, `ArtifactIdentifier`, `ArtifactRelationship`,
  `ArtifactPendingRelationship`, `ArtifactTypeFact`,
  `ArtifactTypeArgumentUsage`, `ArtifactTypeArgument`, `ArtifactLiteral`,
  `ArtifactSourceRegion`, `ArtifactParseDiagnostic`, `FileStatus`,
  `RevisionInput`, `WriteMode`, `WriteOperation`, and `ArtifactWriter`.
- Add a deterministic fixture builder that creates:
  - `files` file rows
  - `files * symbols_per_file` symbols
  - at least one symbol annotation per file
  - `files * identifiers_per_file` identifiers, each with containing and target
    symbol ids
  - local relationships between generated symbols
  - pending relationships to external target names
  - type facts attached to generated symbols
  - type argument usages attached to generated identifiers
  - type arguments attached to generated usages
  - literals attached to containing symbols
  - `files * source_regions_per_file` source regions across `comment`,
    `doc_comment`, and `string_literal`
  - parse diagnostics
- Use stable ids such as `file-{index}`, `file-{index}-symbol-{n}`, and
  `file-{index}-identifier-{n}` so row totals are reproducible.
- Use `ArtifactWriter::open_path` and `write_scan` with a deterministic
  `RevisionInput`. Remove an existing output database before writing so reruns
  produce one scan artifact, not incremental mixed evidence.
- Add a tiny execution test using a temp directory and small dimensions such as
  `files=2`, `symbols_per_file=3`, `identifiers_per_file=4`,
  `source_regions_per_file=3`.

**Acceptance criteria:**

- [ ] Small execution test writes a real SQLite database through
  `ArtifactWriter`.
- [ ] `WriteResult.rows_written` has non-zero values for every current v3
  extraction child-row domain.
- [ ] `transactions_committed == 1` for the generated scan.
- [ ] `cargo test -p xtask performance_baseline_contract` passes.

## Task 3: Write Summary JSON And Command Execution

**Files:**

- Modify: `xtask/src/performance.rs:216-383`
- Modify: `xtask/tests/performance_baseline_contract.rs:1-219`

**What to build:** Add the actual command runner and summary JSON contract for
the writer evidence command.

**Approach:**

- Add `WriterCurrentSchemaSummary` with:
  - `out_dir`
  - `db_path`
  - `summary_path`
  - input dimensions
  - `rows_written` as a serializable domain-count object or map
  - `transactions_committed`
  - `files_changed`
  - `elapsed_write_ms`
  - `rows_per_second`
  - `sqlite_bytes`
- Compute `rows_per_second` from `rows_written.extraction_rows()` and elapsed
  seconds. If elapsed is effectively zero for tiny tests, use `None` or a safe
  finite value; do not emit `NaN` or infinity.
- Write `writer-current-schema-summary.json` with pretty JSON.
- Add tests that serialize a summary and assert stable field names:
  `input.files`, `input.symbols_per_file`, `rows_written.files`,
  `rows_written.source_regions`, `elapsed_write_ms`, `rows_per_second`,
  `sqlite_bytes`, `db_path`, and `summary_path`.
- Add a test that the tiny command runner creates both `artifact.sqlite` and
  `writer-current-schema-summary.json`.

**Acceptance criteria:**

- [ ] The runner creates the requested output directory.
- [ ] The runner writes `artifact.sqlite`.
- [ ] The runner writes `writer-current-schema-summary.json`.
- [ ] Summary JSON is finite, valid, and includes all acceptance fields.
- [ ] No timing threshold is treated as a pass/fail gate.
- [ ] `cargo test -p xtask performance_baseline_contract` passes.

## Task 4: Wire Top-Level Routing And Documentation

**Files:**

- Modify: `xtask/tests/commands_contract.rs:129-153`
- Modify: `docs/testing-strategy.md:198-235`

**What to build:** Make the new command reachable through `cargo xtask` and
document it as manual release-evidence tooling.

**Approach:**

- Add a `commands_contract` test that invokes the binary with
  `performance writer-current-schema` and tiny dimensions in a temp directory.
  Assert success and the presence of `writer-current-schema-summary.json`.
- Keep `test_tiers_module_does_not_own_release_routing` valid: `test_tiers.rs`
  must not route performance commands.
- In `docs/testing-strategy.md`, add a subsection near the existing repeatable
  performance baseline docs:
  - command example with default output under
    `target/performance/writer-current-schema`
  - note that it writes a synthetic current-schema SQLite artifact
  - note that timing/rows-per-second/artifact size are report-only
  - note that it is not regular CI and not part of default/contract tiers
  - keep the real-repo baseline matrix separate for `openclaw`,
    `hermes-agent`, `MyraNext`, and optional `eros`

**Acceptance criteria:**

- [ ] `cargo test -p xtask commands_contract` passes.
- [ ] `docs/testing-strategy.md` documents the new command and preserves the
  fast/specialist gate boundary.
- [ ] No CI workflow starts running the large writer workload.

## Task 5: Verify, Record Evidence, And Close TODO Item 6

**Files:**

- Modify: `TODO.md:74-107`

Generated Goldfish checkpoint files are not implementation source files. If the
checkpoint tool creates one during final evidence capture, include only that
newly generated file with the final commit.

**What to build:** Run the agreed verification, capture the result, and mark
TODO item 6 done only after the implementation is proven.

**Approach:**

- Run focused verification:

```bash
cargo test -p xtask performance_baseline_contract
cargo test -p xtask commands_contract
cargo test -p julie-extract-artifact writer_performance
```

- Run a manual tiny or default command:

```bash
cargo xtask performance writer-current-schema \
  --out-dir target/performance/writer-current-schema
```

- Run branch gates:

```bash
cargo test -p xtask
cargo xtask test default
cargo xtask test contract
```

- Update TODO item 6 from `open` to `done`. Include:
  - new command path
  - summary JSON path
  - row-domain coverage statement
  - verification commands that passed
  - note that timing metrics are report-only
- Preserve user-authored TODO wording for items 7-9.

**Acceptance criteria:**

- [ ] Focused verification passes.
- [ ] Branch gate verification passes.
- [ ] The writer command produces a real artifact and summary under `target/`.
- [ ] TODO item 6 is marked done with concrete evidence.
- [ ] The final commit excludes unrelated user-local files unless they are
  explicitly part of this item 6 implementation.
