# Language Data Quality Completion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Raise `julie-extractors` into the best available tree-sitter
extraction implementation for its supported languages: deep, AST-backed,
fixture-proven semantic data across the board, with explicit limitations only
where the language truly lacks the construct.

**Architecture:** Keep the current product contract shape from
`docs/decisions/0003-domain-coverage-via-kind-coverage.md`: no schema bump, no
new artifact table, and no server/search behavior. Work through the existing
extraction result domains, `fixtures/extraction/capabilities.json`, golden
fixtures, and shared helpers such as `base/complexity_metrics.rs`,
`base/source_regions.rs`, `base/string_literals.rs`, and
`base/annotations.rs`.

**Tech Stack:** Rust workspace, tree-sitter, golden JSON fixtures,
`capability_matrix.rs`, cargo-nextest, xtask test tiers.

**Architecture Quality:** High strategic importance, medium implementation
risk. Capability claims are public contract data, and shared helper changes
affect many languages. The caller-facing interface is the extraction artifact
data and `julie-extract languages --json`, not private helper functions. Tests
must prove behavior through golden fixtures, targeted language tests, and
downstream dogfood scans. Rejected shortcuts: lowering the bar to current
coverage, adding broad capability claims from source inspection alone, or
using `not_applicable` to hide ordinary extractor gaps.

---

## Current Baseline

This plan starts after `docs/plans/2026-06-09-extraction-data-quality.md`.
That plan is complete on branch `feature/extraction-data-quality`.

Current fixture-proven domain counts from
`node scripts/language-data-quality-report.mjs --strict` after the literal,
structural-fact, complexity, annotation, doc-comment, and type-argument slices:

- `symbols`: 36/36
- `relationships`: 35/36
- `identifiers`: 33/36
- `body_spans`: 35/36
- `structural_facts`: 36/36
- `complexity_metrics`: 28/36
- `annotations`: 23/36
- `doc_comments`: 35/36
- `literals`: 36/36
- `source_regions`: 35/36
- `pending_relationships`: 30/36
- `types`: 28/36
- `type_argument_usages`: 20/36

Current explicit quality-bar debt:

- `silent_cells`: 0
- `quality_bar_debts`: 0

The key finding is now stronger than the original baseline: there are no silent
capability cells and no explicit quality-bar debts in the current report. That
is still not the destination. The remaining problem is that the raw `N/36`
counts mix three different states:

- real fixture-proven native coverage;
- legitimate non-applicability where the language does not have the construct;
- convention-only or low-value cases where the language has related runtime
  idioms, but not a stable tree-sitter-backed construct worth claiming as the
  same domain.

The next phase must make those states visible in the scorecard so the team can
delegate bigger batches without confusing a raw missing row with extractor
debt. For example, `type_argument_usages` is 20/36 fixture-proven, with the
remaining 16 classified as 13 true `not_applicable`, 3 convention-only
(`php`, `ruby`, `lua`), and 0 native implementation debt after the Elixir
slice. The current report cannot express that distinction yet.

## Product Quality Bar

The target is not "honest about what is missing." The target is high-quality
extraction for every language this repo advertises.

For general-purpose code languages, the expected bar is:

- rich symbol coverage with signatures, visibility, parent linkage, body
  spans, body hashes, and doc comments;
- relationships and pending relationships where references can cross files or
  resolve later;
- identifiers for calls, member access, variable references, and type usage
  where the grammar exposes them;
- type facts and type-argument usages for statically typed or generic-capable
  languages;
- literals with carrier context for downstream routing, query, URL, and
  configuration analysis;
- source regions for comments, doc comments, strings, and embedded language
  regions;
- complexity metrics for real code constructs;
- annotations/decorators/attributes where the language has them;
- structural facts for high-value semantic constructs, frameworks, and
  language features.

For data, markup, query, or domain-specific languages, the bar is not to mimic
general-purpose code. It is to extract the language's own semantics deeply:
schema structure, links, selectors, bindings, routes, anchors, imports,
queries, DDL/DML/procedure structure, embedded languages, and other constructs
that downstream tools can use directly.

`not_applicable` is allowed only when the construct genuinely does not exist in
the language. `open_gaps` is temporary debt, not an acceptable end state for a
language where the grammar can support the data.

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
fixture output for the same domain. A missing code-language domain is presumed
to be a bug or debt until AST inspection proves otherwise. `not_applicable`
requires a language-semantics reason, not a missing implementation.

**Lead affected-change scope:** Run `cargo xtask test changed` after each
coherent phase, unless endpoint protection blocks an equivalent xtask binary;
then use the tier-equivalent cargo commands from `xtask/src/test_tiers.rs` and
record the substitution.

**Branch gate:** `cargo fmt --check`, `cargo clippy --workspace --all-targets`,
`cargo xtask test default`, `cargo xtask test contract`, and
`scripts/check-agent-doc-sync.sh` before push or PR handoff.

**Escalation triggers:** Any capability-claim change, public CLI/report output
change, artifact schema change, parser dependency change, default-suite runtime
growth, weak evidence behind a passing test, or a proposal to mark a code
language/domain pair not applicable.

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
- Harness mapping: inherit unless the harness has a dedicated mechanical tier.

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

## Acceleration Model

The first half of this branch proved that small, tightly reviewed Cursor slices
work. Continuing one language at a time is now the bottleneck. The next
execution model is:

1. Lead session owns domain policy, applicability calls, scorecard semantics,
   and review.
2. Cursor workers own larger implementation batches only after the expected
   rows, files, and verification commands are explicit.
3. Every worker output is reviewed through the same gates: diff inspection,
   language-focused tests, golden fixture checks, `cargo fmt --check`,
   `git diff --check`, and `node scripts/language-data-quality-report.mjs
   --strict`.
4. Parallel workers must use separate worktrees or non-overlapping file sets.
   Do not let two workers edit `fixtures/extraction/capabilities.json`,
   `docs/findings/2026-06-09-language-coverage-review.md`, or shared base
   helpers in the same batch unless the lead session owns the merge.

### Applicability classes

Use these classes when planning or reviewing each domain/language pair:

- `fixture_proven_native`: the language has a native construct and golden rows
  prove the extractor output.
- `not_applicable`: the construct genuinely does not exist in the language.
- `convention_only`: the language has dynamic/runtime conventions that resemble
  the domain, but there is no stable native syntax to claim as that domain.
- `native_debt`: the language has the construct, tree-sitter exposes it, and
  the extractor does not yet emit product-grade rows.
- `quality_debt`: rows exist, but they are shallow, ambiguous, missing carrier
  context, or not useful enough for downstream tools.

`not_applicable` and `convention_only` require written evidence. `native_debt`
and `quality_debt` require closure tasks.

### Larger work packages

Use these as the next Cursor-sized slices. Each slice should be large enough to
matter, but small enough that review can remain exact.

1. **Applicability-aware scorecard v2:** Extend
   `scripts/language-data-quality-report.mjs` so it preserves the raw fixture
   counts, then adds an applicability-adjusted view for every observed domain.
   Start with `type_argument_usages` because the audit is already complete.
   The script-local metadata may be temporary; do not change artifact schemas
   or public CLI contracts in this task.
2. **Raw-gap applicability audit:** Classify every remaining raw gap in
   `relationships`, `identifiers`, `body_spans`, `source_regions`,
   `pending_relationships`, and `types` as `fixture_proven_native`,
   `not_applicable`, `convention_only`, `native_debt`, or `quality_debt`.
   This is audit work first; implementation follows only for true debt.
3. **Depth audit for complete-looking domains:** Review `structural_facts`,
   `literals`, `doc_comments`, and `complexity_metrics` for useful carrier
   context and downstream value. A 36/36 count is not enough if rows are thin.
4. **Annotation/decorator closure:** Batch the remaining annotation-capable
   languages by grammar family. Skip only with evidence that the language has
   no stable annotation/decorator/metadata construct.
5. **SQL and data-language semantic depth:** Treat SQL, YAML, TOML, JSON,
   Markdown, HTML, CSS, Regex, and Vue/Razor embedded regions as domain
   languages, not failed general-purpose code languages. Improve schema,
   selector, route/link, embedded-region, and query facts where useful.
6. **Downstream dogfood pack:** After the scorecard can report applicability,
   run the extractor against the dependent projects and representative
   real-world corpora. Record before/after counts and parse diagnostics in
   release evidence.

### Cursor prompt contract

Every delegated prompt must include:

- the exact domain and language set;
- the expected applicability class for each language/domain pair, or an
  instruction to audit and report before implementing;
- allowed files and forbidden files;
- required golden fixture behavior;
- required commands;
- a warning that workers may not add fake `not_applicable`, fake support, or
  schema changes to make the report greener.

The lead session should prefer fewer, larger prompts that map to the work
packages above, then review each returned diff before committing.

## Phase 0 - Raise The Bar And Make It Measurable

### Task 1: Add a repeatable language-quality scorecard

**Status:** Complete for raw reporting and strict capability debt checks.
Follow-up belongs to the applicability-aware scorecard v2 work package.

**Files:**
- Create: `scripts/language-data-quality-report.mjs`
- Modify: `docs/findings/2026-06-09-language-coverage-review.md`

**What to build:** Add a repo-local script that reads
`fixtures/extraction/capabilities.json` and every
`fixtures/extraction/<language>/**/expected.json`, then prints a compact table
of fixture-proven domains, `kind_coverage` claims, open gaps, silent empty
cells, and quality-bar failures. The script should not modify files.

**Acceptance criteria:**
- Running `node scripts/language-data-quality-report.mjs` prints the same
  domain counts listed in this plan or clearly shows any updated counts after
  implementation.
- The script identifies every language/domain pair where `supported`,
  `not_applicable`, and `open_gaps` are all empty.
- The script marks code-language gaps separately from legitimate
  domain-language limitations.
- The findings doc records the latest scorecard output after each phase.

### Task 2: Fail closed on silent empty domain cells without lowering the bar

**Status:** Complete for the current `kind_coverage` matrix. Follow-up is to
make remaining non-raw applicability states visible, not to relax the gate.

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
this task unless existing golden evidence already proves it. Do not mark a
code-language domain `not_applicable` merely because it is currently missing;
use `open_gaps` with a concrete closure task unless the grammar and language
semantics prove the construct cannot exist.

**Acceptance criteria:**
- `cargo nextest run -p julie-extractors capability_matrix` fails before the
  matrix is updated and passes after.
- Empty cells for `complexity_metrics`, `structural_facts`, `annotations`,
  `doc_comments`, `literals`, and `source_regions` are no longer silent.
- Format/data languages use `not_applicable` only when the domain is not
  meaningful after language-semantics review.
- Real gaps use `open_gaps`, not fake support and not false non-applicability.

## Phase 1 - Promote Existing Depth To Product-Grade Coverage

### Task 3: Make literal extraction broad and fixture-proven

**Status:** Complete. Literal support is now fixture-proven for all 36
languages, and `node scripts/language-data-quality-report.mjs --strict`
reports `literals`: 36/36.

**Files:**
- Modify: `fixtures/extraction/<language>/basic/source.*`
- Regenerate: affected `fixtures/extraction/<language>/**/expected.json`
- Modify: `fixtures/extraction/capabilities.json`
- Review existing tests under `crates/julie-extractors/src/tests/<language>/literals.rs`

**Languages to audit first:** `rust`, `c`, `cpp`, `go`, `zig`, `python`,
`java`, `vbnet`, `php`, `swift`, `kotlin`, `scala`, `dart`, `elixir`, `qml`,
`gdscript`, and `razor`.

**What to build:** For each language with meaningful string or command
literals, emit literal rows through the public extraction result with useful
carrier context. Existing literal unit tests are evidence to inspect, not a
ceiling. If a language has literal syntax and no fixture output, fix the
extractor unless AST inspection proves the language-specific model should be
different.

**Acceptance criteria:**
- Literal support is broad across code and scripting languages, not limited to
  the current 9 languages.
- Every positive `literals` claim has golden evidence.
- Remaining `literals.open_gaps` rows identify concrete extractor work, not
  generic "unsupported" status.
- `cargo nextest run -p julie-extractors --features test-golden golden` passes
  without `UPDATE_GOLDEN` after regeneration.
- `capability_matrix` passes.

### Task 4: Make doc comments consistent and language-wide

**Status:** Complete for fixture-proven coverage after the doc-comment slices:
35/36 raw fixture-proven, with regex explicitly non-applicable.

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
stable assertions. Add fixture evidence wherever the language has a meaningful
documentation comment or docstring convention. For languages without a
documentation construct, document why; for languages with one, implement it.

**Acceptance criteria:**
- The contract doc states whether `doc_comment` values preserve or strip
  comment markers.
- Existing doc-comment goldens are updated intentionally, not accidentally.
- New doc-comment support is implemented for languages with doc-comment
  syntax. Non-applicability rows require a language-semantics explanation.
- Per-language tests and golden tests pass for affected languages.

### Task 5: Make attributes, decorators, and annotations first-class

**Status:** Partially complete. Current raw annotation fixture evidence is
23/36. The next work should classify the remaining 13 rows by language
semantics before implementation, because several data or markup languages may
be non-applicable while other code languages may still have real debt.

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

**What to build:** Audit each language that has attribute, decorator,
annotation, metadata, or compiler-directive syntax. Add extractor support and
fixtures for every stable syntax the grammar exposes. Existing
`normalize_annotations` calls are starting points; languages without current
helpers still need AST review.

**Acceptance criteria:**
- Kotlin annotation wiring is no longer unverified.
- Attribute/decorator languages are backed by golden fixtures and capability
  claims, or have concrete `open_gaps` rows explaining the missing extractor
  work.
- `capability_matrix` rejects future annotation helper/golden drift.

## Phase 2 - Complexity Metrics Across Code Languages

### Task 6: Add complexity configs for straightforward code languages

**Status:** Complete for the broad code-language batches covered so far.
Current raw complexity fixture evidence is 28/36. Remaining rows need
applicability classification and quality review rather than another blind
language-by-language implementation pass.

**Files:**
- Modify: `crates/julie-extractors/src/base/complexity_metrics.rs`
- Add tests under: `crates/julie-extractors/src/tests/<language>/complexity.rs`
- Modify: `fixtures/extraction/capabilities.json`
- Regenerate: affected goldens

**First language batch:** `zig`, `php`, `ruby`, `scala`, `elixir`, `lua`.

**Second language batch:** `vbnet`, `r`, `bash`, `powershell`, `gdscript`,
`qml`.

**What to build:** Add `ComplexityLanguageConfig` entries for every
general-purpose or scripting language whose grammar exposes decision, loop,
parameter, and callable body nodes. If the generic config model is too weak for
a language, extend the shared engine instead of dropping the language. Each
language test must include a hand-tallied snippet for decision count, loop
count, max nesting depth, and parameter count.

**Acceptance criteria:**
- Each supported language emits both `file` and `symbol` complexity scopes, or
  the plan records why only one scope is meaningful.
- Config/data/markup languages are reviewed for their own structural metrics;
  only true non-code formats become explicit `not_applicable`.
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
SQL, define a procedural SQL metric only for control-flow-bearing routines; do
not flatten ordinary DDL into code complexity.

**Acceptance criteria:**
- The chosen semantics are documented in the test names and capability row.
- No language claims complexity just because it contains nested syntax.
- Golden evidence proves every positive claim.

## Phase 3 - Identifier And Type-Argument Depth

### Task 8: Bring weak identifier languages up to semantic parity

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
- YAML: extract anchors, aliases, references, or tag identifiers if the grammar
  exposes them; otherwise document the precise language limitation.

**Acceptance criteria:**
- No weak language remains shallow because nobody revisited it.
- Fixture rows demonstrate each newly claimed identifier kind.

### Task 9: Expand type-argument usage evidence

**Status:** Implementation complete for native type-argument syntax. Current
raw fixture evidence is 20/36. The remaining 16 are classified in the findings
doc as 13 true `not_applicable`, 3 convention-only (`php`, `ruby`, `lua`), and
0 native implementation debt. Follow-up belongs to scorecard v2 so the report
can show that this is not an open implementation gap.

**Files:**
- Review existing tests under `crates/julie-extractors/src/tests/*/type_arguments.rs`
- Modify language identifier/type modules as needed
- Modify fixtures and `fixtures/extraction/capabilities.json`

**Languages to audit:** `cpp`, `csharp`, `go`, `java`, `kotlin`, `dart`,
`swift`, `vbnet`, `php`, `scala`, `razor`, and `gdscript`.

**What to build:** The goldens currently prove `type_argument_usages` only for
TypeScript. Implement or promote type-argument usage extraction for every
generic-capable language where tree-sitter exposes the syntax. Existing
per-language type-argument tests are starting evidence, not the finish line.

**Acceptance criteria:**
- Current type-argument tests are reflected in golden fixtures where the
  product should advertise the domain.
- Unsupported language rows are explicit.
- Type-argument fixture output is stable and deterministic.

## Phase 4 - High-Value Structural Facts Across The Matrix

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

**What to build:** Add facts with clear downstream value and stable tree-sitter
evidence. Do not create filler rows, but do not accept "no structural facts"
for a language until its framework, module, concurrency, resource, schema, and
embedding constructs have been reviewed.

**Acceptance criteria:**
- Structural facts remain semantic and useful, not filler rows.
- Each new fact has a versioned kind string and golden evidence.
- Existing structural-fact tests continue to pass.

## Phase 5 - Known Language-Specific Quality Defects

### Task 11: Fix Dart recovery semantics

**Status:** Complete. Recovery guards now use tree-sitter-dart's `source_file`
root instead of obsolete `program`. Tests assert the root kind and document
when generic-modifier recovery is active vs clean class parsing.

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

**Status:** Complete. `infer_method_return_type` now locates the exact
`MethodName(` declaration token instead of substring matching. Regression tests
cover attribute/default-string contamination and preserve existing method types.

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

**Status:** Complete for views/triggers/procedures in this slice. Callable body
spans are derived from full statement text (`AS` / `BEGIN..END`), recovery rows
keep `extractedFromError` and add `bodySpanSource` metadata
(`recovery_heuristic`, `statement_text`, or `unavailable`).

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

## Phase 6 - Downstream Dogfood And Comparative Quality

### Task 14: Validate against dependent projects and real repositories

**Files:**
- Create or modify: `docs/release-evidence/<date>-language-data-quality.md`
- Modify: `docs/findings/2026-06-09-language-coverage-review.md`

**What to run:** Scan the repos that depend on this extractor layer, including
the current downstream projects using it, and at least one representative
real-world corpus per major language family. Record domain row counts,
parse-diagnostic rates, failure rows, and before/after deltas for every changed
language.

**Acceptance criteria:**
- Improvements are visible outside synthetic fixtures.
- No dependent project loses core symbols, relationships, body spans, or type
  data.
- The evidence doc records quality metrics, not only pass/fail commands.

## Phase 7 - Docs And Branch Closeout

### Task 15: Update product docs and checklist

**Files:**
- Modify: `docs/languages/new-language-checklist.md`
- Modify: `docs/contracts/extracted-data-v2.md`
- Modify: `docs/findings/2026-06-09-language-coverage-review.md`
- Add release-note draft if behavior changes need to be called out

**What to build:** Update contributor guidance so future languages must meet
the product quality bar up front. The checklist should require fixture
evidence, domain policy, negative cases, downstream relevance, and explicit
closure plans for any temporarily missing domains.

**Acceptance criteria:**
- A new language cannot be added as skeleton coverage.
- Docs describe capability depth in consumer-facing terms.
- The findings doc has an end-state scorecard.

### Task 16: Final validation and handoff

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
- The scorecard shows materially deeper coverage across code languages, not
  merely no silent empty domain cells.
- Every positive claim is fixture-proven.
- Remaining limitations have language-semantics justification or concrete
  closure tasks.

## Sequencing

1. Complete the applicability-aware scorecard v2 before more broad
   implementation batches. It lets the team distinguish real debt from
   legitimate non-applicability.
2. Run the raw-gap applicability audit next. This produces bigger, safer Cursor
   prompts because each implementation batch starts with known debt.
3. Execute annotation/decorator closure, SQL/data-language semantic depth, and
   remaining identifier/type/body/source-region implementation in parallel
   worktrees when file ownership does not overlap.
4. Run the depth audit for complete-looking domains after each family batch.
   A domain can be 36/36 and still need better carrier context.
5. Use Phase 5 defect fixes as independent slices when they do not collide with
   the active domain batch.
6. Phase 6 proves value in dependent projects and real repositories.
7. Phase 7 closes the branch.

## Current Execution Path

Applicability-aware scorecard v2, the raw-gap audit, and Phase 18-19
applicability closures are complete. Phase 20 closed the first semantic-depth
batch for data/markup/domain languages (HTML/Razor markup facts, YAML key paths,
Markdown inline links). See
`docs/findings/2026-06-09-language-coverage-review.md` Phase 20 for evidence.

Phase 21 closed carrier and embedded-region depth for config/markup languages.
See `docs/findings/2026-06-09-language-coverage-review.md` Phase 21 for
fixture-backed evidence. The next slice should target additional Razor/HTML
form/route facts where fixtures prove value, and any remaining shallow domains
surfaced by the depth audit. Implementation work must still avoid filler rows
and keep `silent_cells: 0` with honest capability semantics.

## Out Of Scope

- New languages.
- MCP server, daemon, search, embedding, watcher, dashboard, or editing-tool
  behavior.
- Parser dependency upgrades, unless a specific language task proves the
  current grammar cannot support the required evidence.
- Artifact schema changes. Use existing `kind_coverage` unless a separate approved
  plan explicitly chooses a schema bump.
