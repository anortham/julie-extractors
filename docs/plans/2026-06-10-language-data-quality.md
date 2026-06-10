# Language Data Quality Completion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Move language extraction quality from uneven best-effort coverage to
an explicit, fixture-proven matrix where every advertised language/domain pair
is either supported, not applicable, or tracked as an open gap with a closure
task.

**Architecture:** Keep the current product contract shape from
`docs/decisions/0003-domain-coverage-via-kind-coverage.md`: no schema bump, no
new artifact table, and no server/search behavior. Work through the existing
extraction result domains, `fixtures/extraction/capabilities.json`, golden
fixtures, and shared helpers such as `base/complexity_metrics.rs`,
`base/source_regions.rs`, `base/string_literals.rs`, and
`base/annotations.rs`.

**Tech Stack:** Rust workspace, tree-sitter, golden JSON fixtures,
`capability_matrix.rs`, cargo-nextest, xtask test tiers.

**Architecture Quality:** Medium risk. Capability claims are public contract
data, and shared helper changes affect many languages. The caller-facing
interface is the extraction artifact data and `julie-extract languages --json`,
not private helper functions. Tests must prove behavior through golden fixtures
and capability rows. Rejected shortcut: adding broad capability claims from
source inspection alone. Positive claims require fixture evidence; otherwise
record `not_applicable` or an `open_gaps` row.

---

## Current Baseline

This plan starts after `docs/plans/2026-06-09-extraction-data-quality.md`.
That plan is complete on branch `feature/extraction-data-quality`.

Current fixture-proven domain counts:

- `symbols`: 36/36
- `relationships`: 36/36
- `pending_relationships`: 30/36
- `identifiers`: 33/36
- `types`: 28/36
- `body_spans`: 35/36
- `source_regions`: 35/36
- `doc_comments`: 23/36
- `structural_facts`: 12/36
- `complexity_metrics`: 12/36
- `annotations`: 9/36
- `literals`: 9/36
- `type_argument_usages`: 1/36

The key finding is that there are no positive `kind_coverage` claims without
fixture evidence. The remaining problem is that empty cells are often silent,
so consumers cannot tell deliberate non-applicability from unaudited or missing
support.

## Verification Strategy

**Project source of truth:** `AGENTS.md`/`CLAUDE.md` for product boundaries and
test discipline, `RAZORBACK.md` for strategy-tier routing, and
`xtask/src/test_tiers.rs` for test tier definitions.

**Worker red/green scope:** For extractor behavior, run
`cargo xtask test language <name>` plus a focused golden command when fixtures
change:

```bash
UPDATE_GOLDEN=1 cargo nextest run -p julie-extractors --features test-golden golden
cargo nextest run -p julie-extractors --features test-golden golden
```

For capability-only policy changes, run:

```bash
cargo nextest run -p julie-extractors capability_matrix
```

**Worker ceiling:** `cargo xtask test default`. Workers do not own contract,
real-world, certification, release, or broad performance gates.

**Worker gate invariant:** A positive capability row must be proven by golden
fixture output for the same domain. A negative or empty row must be documented
as `not_applicable` or `open_gaps` with a planned closure task.

**Lead affected-change scope:** Run `cargo xtask test changed` after each
coherent phase, unless endpoint protection blocks an equivalent xtask binary;
then use the tier-equivalent cargo commands from `xtask/src/test_tiers.rs` and
record the substitution.

**Branch gate:** `cargo fmt --check`, `cargo clippy --workspace --all-targets`,
`cargo xtask test default`, `cargo xtask test contract`, and
`scripts/check-agent-doc-sync.sh` before push or PR handoff.

**Escalation triggers:** Any capability-claim change, public CLI/report output
change, artifact schema change, parser dependency change, default-suite runtime
growth, or weak evidence behind a passing test.

**Verification ledger:** Record invariant, command, scope label, commit SHA,
result, and timestamp. For generated goldens, include the changed language set
and row counts per affected domain.

## Model Routing

**Project source of truth:** `RAZORBACK.md`.

**Strategy tier:** Domain policy, capability claim interpretation, schema/report
questions, and final lead review.
- Harness mapping: inherit.

**Implementation tier:** Per-language extractor and fixture slices where the
target behavior is already decided.
- Harness mapping: inherit.

**Mechanical tier:** Fixture-only evidence additions and docs-only updates that
do not own gate interpretation.
- Harness mapping: inherit unless the harness supports a cheaper mechanical
model.

**Gate-interpretation reviewer:** Lead session.
- Harness mapping: inherit.

**Escalation tier:** Lead session for weak evidence, repeated test failure,
parser grammar uncertainty, or broad helper changes.
- Harness mapping: inherit.

**Worker eligibility:** Workers may handle bounded language-family slices only
when file ownership is narrow, verification ceiling is explicit, and the task
does not reinterpret public contracts.

**Mechanical exclusion:** Mechanical workers cannot decide whether a passing
fixture proves a domain claim.

## Phase 0 - Matrix Policy And Audit Tooling

### Task 1: Add a repeatable language-quality scorecard

**Files:**
- Create: `scripts/language-data-quality-report.mjs`
- Modify: `docs/findings/2026-06-09-language-coverage-review.md`

**What to build:** Add a repo-local script that reads
`fixtures/extraction/capabilities.json` and every
`fixtures/extraction/<language>/**/expected.json`, then prints a compact table
of fixture-proven domains, `kind_coverage` claims, open gaps, and silent empty
cells. The script should not modify files.

**Acceptance criteria:**
- Running `node scripts/language-data-quality-report.mjs` prints the same
  domain counts listed in this plan or clearly shows any updated counts after
  implementation.
- The script identifies every language/domain pair where `supported`,
  `not_applicable`, and `open_gaps` are all empty.
- The findings doc records the latest scorecard output after each phase.

### Task 2: Fail closed on silent empty domain cells

**Files:**
- Modify: `fixtures/extraction/capabilities.json`
- Modify: `crates/julie-extractors/src/tests/capability_matrix.rs`
- Modify: `crates/julie-extractors/src/tests/capability_snapshot_test.rs`

**What to build:** Add a capability-matrix convention test requiring every
language/domain pair in the 10 `kind_coverage` domains to be explicit. Each
domain must have at least one of:

- non-empty `supported`
- non-empty `not_applicable`
- non-empty `open_gaps` with `required_closure` and `planned_closure_task`

Update `capabilities.json` with honest initial rows. Do not claim support in
this task unless existing golden evidence already proves it.

**Acceptance criteria:**
- `cargo nextest run -p julie-extractors capability_matrix` fails before the
  matrix is updated and passes after.
- Empty cells for `complexity_metrics`, `structural_facts`, `annotations`,
  `doc_comments`, `literals`, and `source_regions` are no longer silent.
- Format/data languages use `not_applicable` when the domain is not meaningful.
- Real gaps use `open_gaps`, not fake support.

## Phase 1 - Cheap Evidence Closures

### Task 3: Align literal goldens and capability claims

**Files:**
- Modify: `fixtures/extraction/<language>/basic/source.*`
- Regenerate: affected `fixtures/extraction/<language>/**/expected.json`
- Modify: `fixtures/extraction/capabilities.json`
- Review existing tests under `crates/julie-extractors/src/tests/<language>/literals.rs`

**Languages to audit first:** `rust`, `c`, `cpp`, `go`, `zig`, `python`,
`java`, `vbnet`, `php`, `swift`, `kotlin`, `scala`, `dart`, `elixir`, `qml`,
`gdscript`, and `razor`.

**What to build:** For each language with existing literal unit tests, add one
minimal golden fixture case that emits a representative literal row through the
public extraction result. Update `kind_coverage.literals.supported` only when
the golden proves it. If a language has unit tests but no fixture output, fix
the extractor or record an `open_gaps` row explaining the missing wiring.

**Acceptance criteria:**
- Literal support is no longer limited to the current 9 languages when unit
  tests already prove broader support.
- Every positive `literals` claim has golden evidence.
- `cargo nextest run -p julie-extractors --features test-golden golden` passes
  without `UPDATE_GOLDEN` after regeneration.
- `capability_matrix` passes.

### Task 4: Normalize doc-comment policy and fill missing evidence

**Files:**
- Create or modify: `crates/julie-extractors/src/base/doc_comments.rs`
- Modify: per-language doc-comment helpers where marker handling is local
- Modify: `fixtures/extraction/<language>/basic/source.*`
- Regenerate: affected `expected.json` files
- Modify: `fixtures/extraction/capabilities.json`
- Modify: `docs/contracts/extracted-data-v2.md`

**Languages to audit:** `c`, `cpp`, `zig`, `tsx`, `vbnet`, `scala`, `elixir`,
`lua`, `qml`, `r`, `gdscript`, `regex`, and `yaml`.

**What to build:** Define a single doc-comment normalization policy for symbol
`doc_comment` values. Apply it consistently enough that new fixtures can make
stable assertions. Add fixture evidence where the language has a meaningful doc
comment syntax. Mark `regex` and `yaml` not applicable unless the audit finds a
stable language-native documentation construct.

**Acceptance criteria:**
- The contract doc states whether `doc_comment` values preserve or strip
  comment markers.
- Existing doc-comment goldens are updated intentionally, not accidentally.
- New doc-comment support or non-applicability rows are explicit in
  `capabilities.json`.
- Per-language tests and golden tests pass for affected languages.

### Task 5: Turn hidden annotation support into claims or gaps

**Files:**
- Modify as needed: `crates/julie-extractors/src/cpp/*`,
  `crates/julie-extractors/src/php/*`,
  `crates/julie-extractors/src/vbnet/*`,
  `crates/julie-extractors/src/powershell/*`,
  `crates/julie-extractors/src/scala/*`,
  `crates/julie-extractors/src/swift/*`,
  `crates/julie-extractors/src/kotlin/*`
- Modify fixtures and `fixtures/extraction/capabilities.json`
- Add or update focused annotation tests under
  `crates/julie-extractors/src/tests/<language>/`

**What to build:** Audit each language that either has an attribute/decorator
syntax or already calls `normalize_annotations`. For supported cases, add a
fixture and `kind_coverage.annotations` claim. For unsupported but meaningful
cases, add an `open_gaps` row with a planned closure task. For truly
inapplicable languages, add `not_applicable`.

**Acceptance criteria:**
- Kotlin annotation wiring is no longer unverified.
- Any existing helper that already emits annotations is backed by a golden
  fixture or explicitly scoped out.
- `capability_matrix` rejects future annotation helper/golden drift.

## Phase 2 - Complexity Metrics Breadth

### Task 6: Add complexity configs for straightforward code languages

**Files:**
- Modify: `crates/julie-extractors/src/base/complexity_metrics.rs`
- Add tests under: `crates/julie-extractors/src/tests/<language>/complexity.rs`
- Modify: `fixtures/extraction/capabilities.json`
- Regenerate: affected goldens

**First language batch:** `zig`, `php`, `ruby`, `scala`, `elixir`, `lua`.

**Second language batch:** `vbnet`, `r`, `bash`, `powershell`, `gdscript`,
`qml`.

**What to build:** Add `ComplexityLanguageConfig` entries only where the grammar
has stable decision, loop, parameter, and callable body nodes. Each language
test must include a hand-tallied snippet for decision count, loop count, max
nesting depth, and parameter count.

**Acceptance criteria:**
- Each supported language emits both `file` and `symbol` complexity scopes, or
  the plan records why only one scope is meaningful.
- Config/data/markup languages are explicit `not_applicable`.
- `supported_complexity_languages_emit_file_and_symbol_metrics` remains the
  cross-language guard.
- Golden fixtures prove the new metric rows.

### Task 7: Decide embedded/web complexity semantics

**Files:**
- Modify as needed: `crates/julie-extractors/src/base/complexity_metrics.rs`
- Modify as needed: `crates/julie-extractors/src/vue/*`,
  `crates/julie-extractors/src/razor/*`,
  JavaScript/TypeScript JSX or TSX paths
- Modify: `fixtures/extraction/capabilities.json`
- Add focused tests under `tests/vue`, `tests/razor`,
  `tests/javascript`, and `tests/typescript`

**Languages:** `tsx`, `jsx`, `vue`, and `razor`. SQL is design-gated in this
task, but implementation waits unless the task records a clear procedural SQL
complexity policy.

**What to build:** Decide whether complexity belongs to the host file, embedded
language regions, or extracted symbols. For Vue and Razor, prefer embedded
script/C# regions if the extractor can map metrics to existing symbols. For
SQL, only add complexity if procedural control-flow blocks are represented
reliably; otherwise mark not applicable for now and keep SQL quality in the
body-span task.

**Acceptance criteria:**
- The chosen semantics are documented in the test names and capability row.
- No language claims complexity just because it contains nested syntax.
- Golden evidence proves every positive claim.

## Phase 3 - Identifier And Type-Argument Depth

### Task 8: Improve weak identifier languages

**Files:**
- Modify: `crates/julie-extractors/src/bash/*`,
  `crates/julie-extractors/src/vue/*`,
  `crates/julie-extractors/src/javascript/*`,
  `crates/julie-extractors/src/typescript/*`,
  `crates/julie-extractors/src/sql/*`,
  `crates/julie-extractors/src/yaml/*` if YAML remains in scope
- Modify tests under corresponding `crates/julie-extractors/src/tests/`
- Modify fixtures and `fixtures/extraction/capabilities.json`

**Targets:**
- Bash: add variable/member references if grammar support is stable.
- JSX/TSX/Vue: add component/tag/type usage identifiers beyond `call`.
- SQL: add table/column/procedure identifier kinds beyond `member_access`.
- YAML: either prove more than `variable_ref` or mark the limited model
  explicitly.

**Acceptance criteria:**
- No weak language remains unexplained by either better identifiers or an
  explicit capability limitation.
- Fixture rows demonstrate each newly claimed identifier kind.

### Task 9: Expand type-argument usage evidence

**Files:**
- Review existing tests under `crates/julie-extractors/src/tests/*/type_arguments.rs`
- Modify language identifier/type modules as needed
- Modify fixtures and `fixtures/extraction/capabilities.json`

**Languages to audit:** `cpp`, `csharp`, `go`, `java`, `kotlin`, `dart`,
`swift`, `vbnet`, `php`, `scala`, `razor`, and `gdscript`.

**What to build:** The goldens currently prove `type_argument_usages` only for
TypeScript. Promote existing per-language type-argument behavior into golden
fixtures where it is already implemented, and add capability rows or open gaps
so this domain is not hidden.

**Acceptance criteria:**
- Current type-argument tests are reflected in golden fixtures where the
  product should advertise the domain.
- Unsupported language rows are explicit.
- Type-argument fixture output is stable and deterministic.

## Phase 4 - High-Value Structural Facts

### Task 10: Define and implement structural-fact targets by language family

**Files:**
- Modify: `crates/julie-extractors/src/base/structural_facts.rs`
- Modify family-specific modules as needed:
  `javascript`, `typescript`, `vue`, `php`, `ruby`, `java`, `kotlin`,
  `scala`, `swift`, `gdscript`, `sql`, and `razor`
- Modify tests under `crates/julie-extractors/src/tests/structural_facts.rs`
  and per-language modules
- Modify fixtures and `fixtures/extraction/capabilities.json`

**Candidate facts to evaluate:**
- JSX/TSX/Vue component and embedded-region facts.
- Java/Kotlin/Scala annotation-driven framework facts where the annotation is
  already extracted.
- PHP/Ruby route or framework declaration facts only when the syntax is stable
  without framework execution.
- SQL DDL/DML/procedure facts.
- GDScript signals, exported variables, and scene/resource facts.
- Swift concurrency or property-wrapper facts if grammar support is reliable.

**What to build:** Add only facts with clear downstream value and stable
tree-sitter evidence. Do not create a token "one fact per language" rule.
Languages without high-value structural facts should get `not_applicable` or
an explicit open gap.

**Acceptance criteria:**
- Structural facts remain semantic and useful, not filler rows.
- Each new fact has a versioned kind string and golden evidence.
- Existing structural-fact tests continue to pass.

## Phase 5 - Known Language-Specific Quality Defects

### Task 11: Fix Dart recovery semantics

**Files:**
- Modify: `crates/julie-extractors/src/dart/mod.rs`
- Modify: `crates/julie-extractors/src/tests/dart/*`
- Modify affected Dart fixtures if output changes

**What to build:** Decide whether the dead generic-modifier recovery path should
be re-enabled for `source_file` or deleted as obsolete. Use a test that proves
the chosen behavior against tree-sitter-dart's actual root node.

**Acceptance criteria:**
- There is no remaining `program` root assumption in Dart recovery code.
- The test explains whether the recovery path is active or intentionally gone.

### Task 12: Fix C# return-type inference fragility

**Files:**
- Modify: `crates/julie-extractors/src/csharp/type_inference.rs`
- Add or update tests under `crates/julie-extractors/src/tests/csharp/`
- Modify C# fixtures if output changes

**What to build:** Replace substring matching in `infer_method_return_type`
with exact identifier matching or AST-based return-type extraction. Cover the
case where an attribute argument contains the method name.

**Acceptance criteria:**
- The regression test fails before the fix and passes after.
- C# method return types do not change unexpectedly outside the targeted case.

### Task 13: Improve SQL body spans and recovery markers

**Files:**
- Modify: `crates/julie-extractors/src/sql/*`
- Modify tests under `crates/julie-extractors/src/tests/sql/`
- Modify SQL fixtures and `fixtures/extraction/capabilities.json`

**What to build:** Focus on views, triggers, and procedures that currently rely
on recovery markers or weak spans. Improve clean parse extraction where
tree-sitter node structure supports it; otherwise document limitations in
capability gaps.

**Acceptance criteria:**
- SQL body-span coverage improves or the remaining gaps are explicitly
  explained.
- Recovery-path rows are not silently treated as first-class clean extraction.

## Phase 6 - Docs And Branch Closeout

### Task 14: Update product docs and checklist

**Files:**
- Modify: `docs/languages/new-language-checklist.md`
- Modify: `docs/contracts/extracted-data-v2.md`
- Modify: `docs/findings/2026-06-09-language-coverage-review.md`
- Add release-note draft if behavior changes need to be called out

**What to build:** Update contributor guidance so future languages must define
domain policy up front. The checklist should require fixture evidence for every
positive domain claim and explicit `not_applicable` or `open_gaps` entries for
the rest.

**Acceptance criteria:**
- A new language cannot be added with silent empty domain cells.
- Docs describe capability depth in consumer-facing terms.
- The findings doc has an end-state scorecard.

### Task 15: Final validation and handoff

**Files:**
- No code ownership beyond verification-ledger updates.

**What to run:**

```bash
cargo fmt --check
cargo clippy --workspace --all-targets
cargo xtask test default
cargo xtask test contract
scripts/check-agent-doc-sync.sh
node scripts/language-data-quality-report.mjs
```

If local endpoint protection blocks xtask-spawned binaries, use the equivalent
cargo commands from `xtask/src/test_tiers.rs` and record the blocked gates for
CI validation.

**Acceptance criteria:**
- Branch gate evidence is recorded with command, result, commit SHA, and
  timestamp.
- The scorecard shows no silent empty domain cells.
- Every positive claim is fixture-proven.
- Remaining limitations are visible as `not_applicable` or `open_gaps`, not
  hidden absence.

## Sequencing

1. Phase 0 first. It prevents new hidden gaps while the rest of the plan runs.
2. Phase 1 next. Literals and doc comments have the best evidence-to-effort
   ratio.
3. Phase 2 and Phase 3 can run in parallel by language family after Phase 0.
4. Phase 4 should wait until annotation and identifier evidence is stable,
   because structural facts often build on those fields.
5. Phase 5 can run independently as focused defect fixes.
6. Phase 6 closes the branch.

## Out Of Scope

- New languages.
- MCP server, daemon, search, embedding, watcher, dashboard, or editing-tool
  behavior.
- Parser dependency upgrades, unless a specific language task proves the
  current grammar cannot support the required evidence.
- Artifact schema changes. Use existing `kind_coverage` unless a separate approved
  plan explicitly chooses a schema bump.
