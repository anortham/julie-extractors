# CT Test-Detection Readiness Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Close every extractor-side gap found by the 2026-08-25 CT-readiness audit so Miller continuous testing gets complete, honest test-role facts for ten languages plus the shared contract.

**Architecture:** Shared contract work lands first: a `test_role` string in symbol metadata, lifecycle direction detection, wider path guards, and a generic container-scoping pass generalized from `normalize_qml_test_roles`. Per-language work then lands as vertical slices — detection rules, golden fixtures, ledger honesty, and language docs together. One extraction-identity epoch bump covers the whole branch.

**Tech Stack:** Rust, tree-sitter, SQLite/JSONL extraction artifacts, golden JSON fixtures, Node.js quality reporting.

**Architecture Quality:** The caller-facing interface stays the versioned artifact. Three new seams in `crates/julie-extractors/src/test_detection.rs`: (1) a lifecycle-direction result (setup / teardown / ambiguous) behind `is_test_lifecycle`, (2) a generic scoped-role normalize helper that clears roles with no test-container ancestor, (3) `test_role` string emission at the single metadata write point. Main risk: a shared-guard change regresses another language's roles. Controls: per-language goldens with negative controls, the bidirectional capability gate, and the strict quality report. Architecture risk: medium.

## Global Constraints

- SQLite is the primary durable output; JSONL is secondary. No MCP, daemon, search, watcher, dashboard, or editing behavior.
- `test_role` values are exactly the strings Miller's `ContinuousTestClassifier` reads: `test_case`, `parameterized_test`, `fixture_setup`, `fixture_teardown`, `test_container`. Do not invent new values in this plan.
- The typed columns (`is_test`, `test_container`, `test_lifecycle`) and the metadata keys must always agree. Miller reads both.
- One `EXTRACTION_IDENTITY_EPOCH` bump for the whole branch, in Task 1 only. Later tasks MUST NOT bump it again. One compatibility ledger entry describes the full branch scope.
- Every `supported` ledger claim needs a golden fixture that exercises the idiom in published output, with a negative control in the same fixture. Every `not_applicable` needs a named contract and a source citation.
- After capability or fixture changes: `node scripts/language-data-quality-report.mjs --strict` must keep `silent_cells` and `quality_bar_debts` at `0`.
- Path guards are string work on paths. They must handle both `/` and `\` separators; Windows is a first-class target. Never use `Path::join` for contract output paths.
- Default tests stay fast. New tests go in the per-language targets; no real-world corpora in the default suite.
- TDD for every behavior change. Test names state behavior; test bodies carry zero comments.
- Widening a shared guard (path guard, vocabulary, annotation keys) requires a false-positive control test in the same task.
- Do not back-port any of this into `/Users/murphy/source/julie`.

---

## Audit Source

All findings come from the 2026-08-25 workflow audit (12 agents; per-language evidence with file:line references). Goldfish checkpoint `checkpoint_ffbce0e9` summarizes it. Workers get their language's gap list verbatim inside their task below; treat those lists as the finding inventory, and verify each claimed location with Miller before editing.

## Verification Strategy

**Project source of truth:** `AGENTS.md` / `CLAUDE.md`, `fixtures/extraction/capabilities.json`, `xtask/src/test_tiers.rs`, `scripts/language-data-quality-report.mjs`.

**Worker red/green scope:** The focused unit-test module for the owned change (e.g. `cargo test -p julie-extractors <module>::`), then `cargo xtask test language <lang>` for the owned language.

**Worker ceiling:** Focused unit tests plus the assigned per-language command(s). Workers do not run the full default, all-golden, real-world, or Windows suites.

**Worker gate invariant:** The per-language gate proves role classification (all three flags plus `test_role`), negative controls, deterministic golden output, and ledger agreement for the owned language.

**Lead affected-change scope:** After each batch: `cargo xtask test language <lang>` for every language in the batch, then `cargo xtask test golden`, `cargo xtask test capability`, `cargo xtask test contract`, and `node scripts/language-data-quality-report.mjs --strict`.

**Branch gate:** `cargo xtask test default`, `cargo xtask test golden`, `cargo xtask test capability`, `cargo xtask test contract`, `node scripts/language-data-quality-report.mjs --strict`, and `scripts/check-agent-doc-sync.sh` once at the final HEAD.

**Security scope:** `security-secrets` and `security-deps` through `razorback:security-review` at the branch gate. No new dependencies or parsers are expected in this plan; flag any task that adds one.

**Replay/metric evidence:** The strict quality report's `silent_cells=0` and `quality_bar_debts=0` are hard gates. Real-world corpus row counts recorded in `docs/languages/<lang>.md` are report-only evidence.

**Escalation triggers:** Any change to `is_test_path` or other path-string logic requires the Windows suite via the `win-test` skill before the branch gate. Any change to `crates/julie-extract-artifact` schema or writer requires `cargo xtask test contract`.

**Assigned verification failure:** Workers stop and report when assigned verification fails, unless this plan explicitly says to update that gate.

**Verification ledger:** Record invariant, command, scope label, commit SHA, result, and timestamp. Reuse passing evidence for the same HEAD instead of rerunning expensive gates.

## Parallel Execution Contract

Shared files (`crates/julie-extractors/src/test_detection.rs`, `fixtures/extraction/capabilities.json`, `docs/decisions/2026-08-20-test-role-contract-closure.md`) are owned at symbol / per-language-section granularity. Batch tasks use `parallel-lead-commit`: the lead stages task diffs in the listed order and resolves adjacent-hunk overlaps, as in the QML parallel batches. Serial tasks use `serial-worker-commit`.

| Task | Parallel batch | File ownership | Serialization required | Dependency reason |
|---|---|---|---|---|
| Task 1: test_role contract + epoch bump | None - serial | `test_detection.rs` (metadata write path, `is_test_lifecycle` signature), `crates/julie-extractors/src/lib.rs` (epoch), artifact metadata writer + contract tests, compatibility ledger entry | Yes | Every later task emits through this contract. |
| Task 2: shared guards + generic scoping helper | None - serial | `test_detection.rs` (`is_test_path`, new `normalize_scoped_test_roles`), its unit tests | Yes | Every language task depends on the widened guards and the helper; edits the same functions many tasks read. |
| Task 3: JS/TS classifier fixes | Batch A | `crates/julie-extractors/src/javascript/`, `crates/julie-extractors/src/typescript/` (call classifier, vocab), new `src/tests/javascript/test_detection.rs`, existing typescript test module | No | None - safe parallel batch. |
| Task 4: Python detection + evidence | Batch A | `test_detection.rs` python arms only, `fixtures/extraction/python/`, `src/tests/python/`, python rows in capabilities.json + decision doc, `docs/languages/python.md` | No | None - safe parallel batch. |
| Task 5: Rust detection + evidence | Batch A | `test_detection.rs::detect_rust` only, `crates/julie-extractors/src/rust/helpers.rs`, rust module-annotation path, `fixtures/extraction/rust/`, rust ledger rows, `docs/languages/rust.md` | No | None - safe parallel batch. |
| Task 6: C# detection + evidence | Batch A | `test_detection.rs::detect_csharp` + dotnet key lists + `mark_dotnet_test_containers` only, `fixtures/extraction/csharp/`, `src/tests/csharp/`, csharp ledger rows, `docs/languages/csharp.md` | No | None - safe parallel batch. |
| Task 7: change-journal coverage for unsupported files | Batch A | `crates/julie-extract-artifact/` writer + journal + contract tests only | No | None - safe parallel batch (different crate). |
| Task 8: JS/TS fixtures, ledger, docs | Batch B | `fixtures/extraction/{javascript,typescript,jsx,tsx}/`, their ledger rows + decision-doc row, `docs/languages/javascript.md`, `docs/languages/typescript.md` | Yes (after Task 3) | Golden output depends on Task 3's classifier fixes. |
| Task 9: Go detection + evidence | Batch B | `crates/julie-extractors/src/go/`, `test_detection.rs::detect_go` + go lifecycle arm + go container pass, `fixtures/extraction/go/`, `src/tests/go/`, go ledger rows, `docs/languages/go.md` | No | None - safe parallel batch. |
| Task 10: Java detection + evidence | Batch B | `test_detection.rs` java/TestNG key lists + `mark_java_test_containers` only, `fixtures/extraction/java/`, `src/tests/java/`, java ledger rows, `docs/languages/java.md` | No | None - safe parallel batch. |
| Task 11: Ruby detection + evidence | Batch B | `crates/julie-extractors/src/ruby/`, `test_detection.rs` ruby arms only, `fixtures/extraction/ruby/`, new `src/tests/ruby/test_detection.rs`, ruby ledger rows, `docs/languages/ruby.md` | No | None - safe parallel batch. |
| Task 12: PHP detection + evidence | Batch C | `crates/julie-extractors/src/php/`, `test_detection.rs::detect_php` + php lifecycle arm + new php container pass, `fixtures/extraction/php/`, php ledger rows, `docs/languages/php.md` | No | None - safe parallel batch. |
| Task 13: Kotlin detection + evidence | Batch C | `crates/julie-extractors/src/kotlin/`, `test_detection.rs` kotlin key lists only, `fixtures/extraction/kotlin/`, kotlin ledger rows, `docs/languages/kotlin.md` | No | None - safe parallel batch. |
| Task 14: Swift detection + evidence | Batch C | `crates/julie-extractors/src/swift/`, `test_detection.rs::detect_swift` + swift lifecycle arm only, `fixtures/extraction/swift/`, swift ledger rows, `docs/languages/swift.md` | No | None - safe parallel batch. |
| Task 15: test_linkage/test_coverage contract decision + pilot | None - serial | New decision doc, `test_detection.rs` metadata write path, one pilot language's emission + fixture (csharp), contract tests | Yes | Extends the Task 1 metadata contract; touches the shared write path after all batches settle. |
| Task 16: dialect language identity decision (jsx/tsx) | None - serial | New decision doc only (plus contract note in `docs/architecture/` if accepted) | Yes | Cross-repo contract decision; must not race the JS/TS tasks. |
| Task 17: cross-language closure sweep | None - serial | Shared tables in decision doc, capabilities.json final reconciliation, branch-gate evidence | Yes | Depends on every earlier task's ledger rows. |

---

### Task 1: `test_role` string contract + single epoch bump

**Files:**
- Modify: `crates/julie-extractors/src/test_detection.rs` (`is_test_lifecycle` at :200, `apply_callable_test_metadata` at :218, and the metadata write point)
- Modify: `crates/julie-extractors/src/lib.rs:133` (`EXTRACTION_IDENTITY_EPOCH: u32 = 5` → `6`)
- Modify: `crates/julie-extract-artifact/src/writer/rows.rs` and `crates/julie-extract-artifact/tests/schema_contract.rs` only if the metadata passthrough needs contract coverage for the new key
- Modify: the compatibility ledger that accompanied the epoch-4→5 bump (find it: `search` for `EXTRACTION_IDENTITY_EPOCH` references and follow the prior bump commit's pattern)
- Test: extend the existing `test_detection` unit tests; add a contract test asserting typed columns and metadata keys agree

**Interfaces:**
- Consumes: existing boolean role flags and the `is_test`/`test_lifecycle`/`test_container` metadata keys.
- Produces: (a) `test_role` string key in `symbols.metadata_json` with values `test_case` / `parameterized_test` / `fixture_setup` / `fixture_teardown` / `test_container`; (b) `is_test_lifecycle` returns a direction (`Setup` / `Teardown` / `Ambiguous` / `None`) instead of a bare bool — every per-language arm maps its hook names to a direction; (c) a single helper (extend `apply_callable_test_metadata`) that writes booleans + `test_role` together so they can never disagree.
- All later language tasks call these; their signatures are frozen after this task.

**Contract inputs:** Miller reads `test_role` first with confidence 1.0 (`ContinuousTestClassifier`), and reads the three metadata booleans as extractor evidence. `parameterized_test` is emitted only where a language task explicitly detects a parameterized idiom; the default for a plain detected test is `test_case`.

**File ownership:** `test_detection.rs` (metadata write path, `is_test_lifecycle` signature), `crates/julie-extractors/src/lib.rs` (epoch), artifact metadata writer + contract tests, compatibility ledger entry

**Serialization required:** Yes

**Dependency reason:** Every later task emits through this contract.

**What to build:** Make the shared metadata writer emit the `test_role` string alongside the booleans, derive setup-vs-teardown from a direction-aware lifecycle result, bump the extraction identity epoch once with a compatibility ledger entry covering the whole branch ("test-role contract expansion: test_role string, lifecycle direction, per-language role corrections").

**Approach:** Convert `is_test_lifecycle`'s per-language arms to return the direction enum; existing arms already know the hook names, so mapping each name to Setup or Teardown is local. Where a hook genuinely wraps both (e.g. `around`), use `Ambiguous` and emit `fixture_setup` (document the choice in the decision doc row). Keep the booleans' emitted values identical for currently-supported languages — this task changes metadata additively; role corrections come in language tasks. Update every existing golden `expected.json` that now carries the new key (regeneration, not hand-editing).

**Acceptance criteria:**
- [ ] Every symbol with any test flag also carries a correct `test_role` string; a contract test proves typed columns and metadata always agree.
- [ ] `is_test_lifecycle` returns direction; all existing language arms compile and keep their current classifications.
- [ ] Epoch is 6 with one compatibility ledger entry; no golden regression outside the additive key.
- [ ] `cargo xtask test golden`, `cargo xtask test capability`, `cargo xtask test contract` pass.
- [ ] Worker-scope verification passes and the change is committed per `serial-worker-commit`.

### Task 2: Shared path guards + generic container-scoping helper

**Files:**
- Modify: `crates/julie-extractors/src/test_detection.rs` (`is_test_path` at :21; generalize `normalize_qml_test_roles` at :262 into a reusable `normalize_scoped_test_roles`)
- Test: `test_detection` unit tests — one positive and one false-positive control per new convention

**Interfaces:**
- Consumes: Task 1's contract.
- Produces: (a) `is_test_path` accepts, in addition to current rules: `_test.rb`, `_spec.rb`, `*_test.py`, `conftest.py`, `*Test.php`, `*Cest.php`, `*Spec.php`, Xcode `*Tests/` directory and `*Tests.swift` suffixes, JS `e2e/`, `cypress/`, `integration/` segments and `.cy.` infix, Gradle source sets `integrationTest/`, `testFixtures/`, `androidTest/`, `functionalTest/`; (b) `normalize_scoped_test_roles(symbols, container_ids)` — language-neutral pass that clears role flags (and `test_role`) from any symbol lacking a test-container ancestor, exactly what `normalize_qml_test_roles` does for QML. QML switches to the generic helper.

**Contract inputs:** Existing guard semantics must not regress: keep every currently-accepted pattern. Both `/` and `\` separators. Case behavior must match the existing guard's conventions.

**File ownership:** `test_detection.rs` (`is_test_path`, new `normalize_scoped_test_roles`), its unit tests

**Serialization required:** Yes

**Dependency reason:** Every language task depends on the widened guards and the helper; edits the same functions many tasks read.

**What to build:** Widen the shared path guard with every convention the audit found missing, and extract the QML scoping pass into a helper any language can call.

**Approach:** Guard widening is name/suffix matching on path segments — follow the existing style in `is_test_path`. The scoping helper takes the container-id set the caller computed; it does not decide what a container is. Verify the QML golden is byte-identical after the switch. Windows: these are string paths already normalized by the caller — add a `\`-separated test case per new rule.

**Acceptance criteria:**
- [ ] Every new convention has a passing accept test and a false-positive control test.
- [ ] QML golden output unchanged after switching to the generic helper.
- [ ] `cargo xtask test language qml` passes; `win-test` scheduled at branch gate (path logic changed).
- [ ] Worker-scope verification passes and the change is committed per `serial-worker-commit`.

### Task 3: JS/TS call-classifier fixes

**Files:**
- Modify: `crates/julie-extractors/src/javascript/` and `crates/julie-extractors/src/typescript/` (call classification, vocabulary)
- Create: `crates/julie-extractors/src/tests/javascript/test_detection.rs`
- Modify: the existing typescript test-detection test module

**Interfaces:**
- Consumes: Task 1 contract, Task 2 guards.
- Produces: corrected call-style classification consumed by Task 8's fixtures. Vocabulary and classification rules below are the contract Task 8 encodes into goldens.

**Contract inputs:** Audit findings to close, all in the call-style path: (1) Playwright — `test.describe(...)` must be `test_container` (currently `is_test=1`, container 0) and `test.beforeEach`/`test.afterAll` must emit lifecycle symbols (currently nothing) — fix dotted-property callee classification; (2) parameterized — `test.each([...])("name", fn)`, `it.each` (incl. tagged template), `describe.each` must emit symbols with `test_role=parameterized_test` (resolve the chained-call callee); (3) production false positives — bare `describe`/`it`/`test`/`before`/`after`/`suite`/`context` in non-test files must not emit roles: add an import-or-test-path guard (file imports a known test framework, or `is_test_path` passes); (4) node:test subtests `t.test(...)`; (5) Mocha TDD interface `setup`/`teardown`/`suiteSetup`/`suiteTeardown` and `specify` alias; (6) focused/disabled aliases `xit`, `fit`, `xtest`, `xdescribe`, `fdescribe`, `xcontext`; (7) Vitest `bench(...)`; (8) QUnit `QUnit.module`/`QUnit.test`; (9) decorator frameworks (testdeck) — make JS/TS `is_test_symbol` honor `annotation_keys` like java/kotlin do.

**File ownership:** `crates/julie-extractors/src/javascript/`, `crates/julie-extractors/src/typescript/` (call classifier, vocab), new `src/tests/javascript/test_detection.rs`, existing typescript test module

**Serialization required:** No

**Dependency reason:** None - safe parallel batch.

**What to build:** Fix the nine classifier defects above with unit tests per defect, both dialect families.

**Approach:** The dotted-property and chained-call fixes are callee-resolution changes in one classification function per dialect — inspect the current resolution first with Miller; jsx/tsx share the js/ts code paths, so verify all four dialects in tests. The import-or-path guard is the risky change: it must not drop currently-detected real tests — golden diff on all four existing fixtures is the control.

**Acceptance criteria:**
- [ ] Each of the nine findings has a red-then-green unit test.
- [ ] Existing four dialect goldens still pass (or their expected change is documented for Task 8).
- [ ] `cargo xtask test language javascript` and `cargo xtask test language typescript` pass.
- [ ] Verified diff handed to the lead per `parallel-lead-commit`.

### Task 4: Python detection + evidence

**Files:**
- Modify: `crates/julie-extractors/src/test_detection.rs` python arms only
- Modify: `fixtures/extraction/python/` (rebuild `test_roles` fixture), python rows in `fixtures/extraction/capabilities.json`
- Modify/Create: `crates/julie-extractors/src/tests/python/` unit tests; `docs/languages/python.md`; python row in `docs/decisions/2026-08-20-test-role-contract-closure.md`

**Interfaces:**
- Consumes: Task 1 direction enum, Task 2 guards (`*_test.py`, `conftest.py` already added there).
- Produces: honest python ledger rows and goldens.

**Contract inputs:** Findings: (1) relax the case-name rule from `test_` to `test` prefix (unittest/pytest both collect `test*` — `def testAddition` is real); (2) add pytest xunit hooks to the lifecycle arm: `setup_method`/`teardown_method`, `setup_class`/`teardown_class`, `setup_function`/`teardown_function`, `setup_module`/`teardown_module`, plus unittest `setUpModule`/`tearDownModule`, `asyncSetUp`/`asyncTearDown`; (3) `@pytest.fixture` → `fixture_setup` role (reverse the deliberate exclusion; record the decision change in the decision doc); (4) fix dead lowercase match arms for `unittest.skipif`/`skipunless`/`expectedfailure` (annotation keys are lowercased before matching); (5) `@pytest.mark.parametrize` → `parameterized_test`; (6) rewrite the two vacuous unit tests to drive a real `PythonExtractor`; (7) rebuild the golden with real pytest code: parametrize, fixture, `setup_method`, plain pytest class, async test, camelCase unittest method, one negative control per shape; (8) add `type_usage` to the python identifiers ledger row (already emitted, under-declared); (9) record or implement the cross-file inheritance gap: a `TestCase` base in another file currently produces no pending edge — implement structured pending extends edges if feasible within the session, else record an `open_gaps` entry with reason, required closure, planned task.

**File ownership:** `test_detection.rs` python arms only, `fixtures/extraction/python/`, `src/tests/python/`, python rows in capabilities.json + decision doc, `docs/languages/python.md`

**Serialization required:** No

**Dependency reason:** None - safe parallel batch.

**What to build:** Close the nine findings, rebuild the golden, write the ledger row, decision-doc row, and `docs/languages/python.md` with a pinned real-world corpus scan (follow `docs/languages/qml.md` as the template).

**Approach:** All rule changes are in the python arms of `test_detection.rs`; keep them there. Fixture-role decision (finding 3) changes published output for real projects — state the rationale in the decision-doc row.

**Acceptance criteria:**
- [ ] All findings closed or (finding 9 only) recorded as a complete `open_gaps` entry.
- [ ] Golden exercises every claimed idiom with negative controls; `cargo xtask test language python` passes.
- [ ] Ledger, decision-doc row, and `docs/languages/python.md` complete.
- [ ] Verified diff handed to the lead per `parallel-lead-commit`.

### Task 5: Rust detection + evidence

**Files:**
- Modify: `crates/julie-extractors/src/test_detection.rs::detect_rust` (:109) only
- Modify: `crates/julie-extractors/src/rust/helpers.rs` (compound-cfg container check), rust module extraction (pass module annotations through)
- Modify: `fixtures/extraction/rust/` (rewrite `test_roles`, add trait-impl fixture), rust ledger rows
- Create: `docs/languages/rust.md`; rust row updates in the decision doc

**Interfaces:**
- Consumes: Task 1 contract.
- Produces: honest rust ledger rows and goldens.

**Contract inputs:** Findings: (1) widen `detect_rust` beyond `test`/`tokio::test`/`rstest`: add a named set (`test_case`, `actix_web::test`, `actix_rt::test`, `sqlx::test`, `async_std::test`, `wasm_bindgen_test`, `quickcheck`, `proptest`, `googletest::test`, `gtest`, `test_log::test`, `traced_test`, `rstest::rstest`) plus a "last path segment equals `test`" suffix rule for qualified attribute macros; (2) fix `has_exact_cfg_test_attr` in `rust/helpers.rs` to accept compound cfgs — `#[cfg(all(test, ...))]` and `#[cfg(any(test, ...))]` mark the module as `test_container`; (3) pass module annotations through `extract_module` so `#[cfg(test)]` reaches `symbol_annotations`; (4) rstest `#[fixture]` → `fixture_setup`, and reconcile the `test_lifecycle: not_applicable` ledger claim with this (the claim moves to supported, or the decision doc names the narrower contract); (5) `#[test_case(...)]` and `#[rstest]` with case attributes → `parameterized_test`; (6) rewrite the golden to exercise `#[tokio::test]`, `#[rstest]`, a nested `#[test]` inside `#[cfg(test)] mod`, a compound cfg module, and negative controls; add a trait-impl fixture and add `implements` to the relationships ledger row (emitted today, under-declared); (7) benchmarks: record `#[bench]`/criterion/divan and rustdoc doc-tests as `open_gaps` entries with reasons (provider lists doc-tests; extractor emits nothing) — do not implement in this task.

**File ownership:** `test_detection.rs::detect_rust` only, `crates/julie-extractors/src/rust/helpers.rs`, rust module-annotation path, `fixtures/extraction/rust/`, rust ledger rows, `docs/languages/rust.md`

**Serialization required:** No

**Dependency reason:** None - safe parallel batch.

**What to build:** Close findings 1–6, record finding 7 as open gaps, write `docs/languages/rust.md` with pinned corpus evidence and the decision-doc reconciliation.

**Approach:** The suffix rule must not over-match: require the segment match on the normalized (lowercased, `::`-split) key, with control tests for non-test attributes ending in other words. Keep `detect_rust` annotation-only; no name heuristics.

**Acceptance criteria:**
- [ ] All listed attribute macros classify; compound-cfg modules are containers; controls pass.
- [ ] Golden and ledger agree bidirectionally; `cargo xtask test language rust` passes.
- [ ] Open-gap entries for benchmarks and doc-tests are complete (reason, closure, planned task).
- [ ] Verified diff handed to the lead per `parallel-lead-commit`.

### Task 6: C# detection + evidence

**Files:**
- Modify: `crates/julie-extractors/src/test_detection.rs` — `detect_csharp` (:186), dotnet annotation key lists, `mark_dotnet_test_containers` (:357)
- Modify: `fixtures/extraction/csharp/test_roles/`, csharp ledger rows; `crates/julie-extractors/src/tests/csharp/test_containers.rs`
- Create: `docs/languages/csharp.md`; csharp row in the decision doc

**Interfaces:**
- Consumes: Task 1 contract.
- Produces: honest csharp ledger rows and goldens.

**Contract inputs:** Findings: (1) xUnit lifecycle — inside a class already marked test-container by xUnit inference, mark the constructor, `Dispose`, `DisposeAsync`, and `InitializeAsync` as lifecycle (constructor/`InitializeAsync` → `fixture_setup`; `Dispose`/`DisposeAsync` → `fixture_teardown`); scope strictly to marked containers with a non-test-class control; (2) add missing keys: `datatestmethod` (test case), `testcasesource`, `testfixturesource` (test cases), `assemblyinitialize`, `assemblycleanup` (lifecycle), `collectiondefinition`, `setupfixture` (containers); (3) widen the container filter to `SymbolKind::Struct` (record struct / struct test classes); (4) fixture: one class per new idiom plus a non-test class with a ctor and `Dispose` as the control; (5) record SpecFlow/Reqnroll bindings and MSpec delegate fields as `open_gaps` entries — not implemented here.

**File ownership:** `test_detection.rs::detect_csharp` + dotnet key lists + `mark_dotnet_test_containers` only, `fixtures/extraction/csharp/`, `src/tests/csharp/`, csharp ledger rows, `docs/languages/csharp.md`

**Serialization required:** No

**Dependency reason:** None - safe parallel batch.

**What to build:** Close findings 1–4, record finding 5 as open gaps, write the decision-doc row (with named exclusions and primary sources) and `docs/languages/csharp.md` including the existing Newtonsoft.Json corpus scan plus a diagnostic breakdown.

**Approach:** The xUnit lifecycle pass runs after container marking, as a second pass over symbols — same pattern as `mark_dotnet_test_containers`. `[Theory]` already maps to test case; upgrade it to `parameterized_test` under the Task 1 contract, with `[DataTestMethod]`, `[TestCase(...)]`, `[TestCaseSource]` alike.

**Acceptance criteria:**
- [ ] xUnit constructor/Dispose lifecycle emitted only inside marked containers; control class stays clean.
- [ ] All new keys classify; struct containers marked; `cargo xtask test language csharp` passes.
- [ ] Ledger, decision-doc row, docs complete; open gaps recorded.
- [ ] Verified diff handed to the lead per `parallel-lead-commit`.

### Task 7: Change-journal coverage for unsupported files

**Files:**
- Modify: `crates/julie-extract-artifact/` writer/journal path (locate with Miller: the `revision_file_changes` writer and the discovery walk that skips unsupported files)
- Test: `crates/julie-extract-artifact/tests/` contract tests

**Interfaces:**
- Consumes: existing journal contract (`revision_file_changes`: path, revision_id, change_kind).
- Produces: journal rows for changed files the extractor does not parse (e.g. `__snapshots__/*.snap`, `.npmrc`, lockfiles), so Miller's delta reader can account for them instead of never seeing them.

**Contract inputs:** Miller fails closed on unaccounted paths (`RevisionDeltaReader`); a file absent from the journal is invisible rather than unaccounted, which silently keeps stale green verdicts. The journal is documented as covering every file the extractor processed — extend processing to record unsupported files with an explicit status, not to parse them.

**File ownership:** `crates/julie-extract-artifact/` writer + journal + contract tests only

**Serialization required:** No

**Dependency reason:** None - safe parallel batch (different crate).

**What to build:** Record changed unsupported files in the journal (and the store-mode manifest equivalent) with a distinct status/change record, without adding language rows or parsing. Respect existing ignore rules (`.git/`, target dirs, binary/size limits) — this covers files the walk already sees and then drops.

**Approach:** First map the discovery walk's decision points with Miller; the design question is which walk tier drops unsupported files today. Keep the artifact schema stable if a status value suffices; if a schema change is unavoidable, it is a contract change — add contract tests and a schema note, and flag it to the lead before finalizing. Check Windows file-identity pitfalls (path text vs identity) for the new rows.

**Acceptance criteria:**
- [ ] A changed unsupported file yields a journal row in artifact mode and a manifest diff in store mode; contract tests prove both.
- [ ] Ignore rules unchanged; no unsupported file is parsed.
- [ ] `cargo xtask test contract` passes.
- [ ] Verified diff handed to the lead per `parallel-lead-commit`.

### Task 8: JS/TS fixtures, ledger, and docs

**Files:**
- Modify: `fixtures/extraction/javascript/`, `fixtures/extraction/typescript/`, `fixtures/extraction/jsx/`, `fixtures/extraction/tsx/` (five new framework goldens registered across the four dialect rows), their capabilities.json rows
- Create: `docs/languages/javascript.md`, `docs/languages/typescript.md`; JS/TS row in the decision doc

**Interfaces:**
- Consumes: Task 3's fixed classifier (its rule list is the source of truth for expected outputs).
- Produces: dialect goldens and honest ledger rows.

**Contract inputs:** Replace the four 14-line single-idiom fixtures with real coverage: Jest/Vitest (hooks, `.each`, focused/disabled aliases, bench), Playwright (`test.describe`, hooks), Mocha TDD, node:test with subtests, QUnit — each with production-code negative controls. Every claim in the four dialect ledger rows must be backed by a golden.

**File ownership:** `fixtures/extraction/{javascript,typescript,jsx,tsx}/`, their ledger rows + decision-doc row, `docs/languages/javascript.md`, `docs/languages/typescript.md`

**Serialization required:** Yes (after Task 3)

**Dependency reason:** Golden output depends on Task 3's classifier fixes.

**What to build:** The evidence layer for Task 3: goldens, ledger honesty, the decision-doc row naming adopted frameworks and exclusions (tape, testdeck depth), and the two language docs with a pinned real-world corpus scan and diagnostic breakdown.

**Approach:** Keep per-dialect fixtures small but idiom-complete; one framework per fixture directory keeps failures readable. jsx/tsx fixtures reuse the js/ts source with dialect syntax added.

**Acceptance criteria:**
- [ ] Every ledger `supported` claim for the four dialects has golden backing; strict report stays clean.
- [ ] `cargo xtask test language javascript` and `cargo xtask test language typescript` pass.
- [ ] Docs and decision-doc row complete.
- [ ] Verified diff handed to the lead per `parallel-lead-commit`.

### Task 9: Go detection + evidence

**Files:**
- Modify: `crates/julie-extractors/src/go/functions.rs` (route through `apply_callable_test_metadata`), `crates/julie-extractors/src/go/` container pass
- Modify: `crates/julie-extractors/src/test_detection.rs::detect_go` (:474) + a new go lifecycle arm
- Modify: `fixtures/extraction/go/test_roles/`, go ledger rows; `src/tests/go/` unit tests
- Create: `docs/languages/go.md`; go row in the decision doc

**Interfaces:**
- Consumes: Task 1 contract (this fixes the root cause: go writes `is_test` by hand and can never emit lifecycle), Task 2 scoping helper.
- Produces: honest go ledger rows and goldens.

**Contract inputs:** Findings: (1) route go callables through `apply_callable_test_metadata`; (2) go lifecycle arm: `TestMain` (→ lifecycle, not test case), testify `SetupTest`/`TearDownTest`/`SetupSuite`/`TearDownSuite`/`SetupSubTest`/`TearDownSubTest`/`BeforeTest`/`AfterTest`, gocheck `SetUpTest`/`TearDownTest`/`SetUpSuite`/`TearDownSuite`; (3) mark structs embedding `suite.Suite` (and gocheck suite registration) as `test_container` — new `mark_go_test_containers`; (4) include `Benchmark` prefix as a test case (`go test -list` lists benchmarks; a benchmark-only file must not be invisible) — record the decision; (5) Ginkgo precision: add an `is_test_path`/import guard and container-ancestor scoping via `normalize_scoped_test_roles` so bare `It(...)`/`Context(...)` in production code stops matching; (6) rewrite the 11-line golden as a realistic multi-framework file (stdlib incl. `Fuzz`/`Example`/`TestMain`/benchmark, testify suite, Ginkgo, negative controls); (7) record `t.Run` subtable names and a `go.mod`/`go.sum` language row as `open_gaps`/follow-up (qmldir is the precedent for the manifest row) — not implemented here.

**File ownership:** `crates/julie-extractors/src/go/`, `test_detection.rs::detect_go` + go lifecycle arm + go container pass, `fixtures/extraction/go/`, `src/tests/go/`, go ledger rows, `docs/languages/go.md`

**Serialization required:** No

**Dependency reason:** None - safe parallel batch.

**What to build:** Close findings 1–6, record finding 7, write the go decision-doc row and `docs/languages/go.md` with pinned corpus evidence.

**Approach:** Go method receivers carry the suite type — container marking keys on the embedded `suite.Suite` field in the struct declaration, then lifecycle/test methods attach via receiver type. `_test.go` gating stays the primary guard for stdlib names.

**Acceptance criteria:**
- [ ] `TestMain` is lifecycle; testify suites fully classified; benchmark decision recorded and implemented.
- [ ] Ginkgo controls prove no production false positives.
- [ ] Golden, ledger, docs complete; `cargo xtask test language go` passes.
- [ ] Verified diff handed to the lead per `parallel-lead-commit`.

### Task 10: Java detection + evidence

**Files:**
- Modify: `crates/julie-extractors/src/test_detection.rs` java/TestNG key lists + `mark_java_test_containers` (:386)
- Modify: `fixtures/extraction/java/test_roles/`, java ledger rows; `src/tests/java/` unit tests
- Create: `docs/languages/java.md`; java row in the decision doc

**Interfaces:**
- Consumes: Task 1 contract, Task 2 scoping helper.
- Produces: honest java ledger rows and goldens.

**Contract inputs:** Findings: (1) TestNG lifecycle keys: `beforemethod`, `aftermethod`, `beforesuite`, `aftersuite`, `beforetest`, `aftertest`, `beforegroups`, `aftergroups` (keys are normalized lowercase); (2) TestNG class-level `@Test`: mark the class `test_container` and its public methods `test_case`; (3) `@TestFactory`, `@TestTemplate` as test cases; `@ParameterizedTest` upgrades to `parameterized_test`; (4) scope the JUnit-3 `test*` name fallback with `normalize_scoped_test_roles` (helper-class control test); (5) extend the golden: TestNG hooks, `@Nested`, `extends TestCase`, `@BeforeAll`/`@AfterAll`, JUnit 4 `@Before`/`@After`, non-empty method bodies so relationships/identifiers appear in published evidence; (6) add `extends` to the java relationships ledger row (emitted today, under-declared) with a golden inheritance edge; (7) record Cucumber-JVM step bindings and `@Suite` containers as `open_gaps`.

**File ownership:** `test_detection.rs` java/TestNG key lists + `mark_java_test_containers` only, `fixtures/extraction/java/`, `src/tests/java/`, java ledger rows, `docs/languages/java.md`

**Serialization required:** No

**Dependency reason:** None - safe parallel batch.

**What to build:** Close findings 1–6, record finding 7, write the java decision-doc row and `docs/languages/java.md`.

**Approach:** Smallest task in the plan; all rule changes are key-list additions plus one container rule. Follow the existing dotnet class-level inference pattern for TestNG class-level `@Test`.

**Acceptance criteria:**
- [ ] All new keys classify; class-level `@Test` yields container + cases; fallback scoped.
- [ ] Golden proves every claim incl. an `extends` edge; `cargo xtask test language java` passes.
- [ ] Ledger, decision-doc row, docs complete; open gaps recorded.
- [ ] Verified diff handed to the lead per `parallel-lead-commit`.

### Task 11: Ruby detection + evidence

**Files:**
- Modify: `crates/julie-extractors/src/ruby/calls.rs`, `crates/julie-extractors/src/ruby/symbols.rs` (route through `apply_callable_test_metadata`)
- Modify: `crates/julie-extractors/src/test_detection.rs` ruby arms (new lifecycle arm; base-type container call)
- Modify: `fixtures/extraction/ruby/test_roles/`, ruby ledger rows
- Create: `crates/julie-extractors/src/tests/ruby/test_detection.rs`; `docs/languages/ruby.md`; ruby row in the decision doc

**Interfaces:**
- Consumes: Task 1 contract, Task 2 guards (`_test.rb`, `_spec.rb` added there) and scoping helper.
- Produces: honest ruby ledger rows and goldens.

**Contract inputs:** Findings (two blockers first): (1) Rails macro `test "name" do ... end` must emit a `test_case` symbol — add the arm in `ruby/calls.rs`; also block-form `setup do`/`teardown do` as lifecycle; (2) kill production false positives: bare `before`/`after`/`around`/`it`/`describe`/`context` outside test scope — apply `normalize_scoped_test_roles` plus the path guard; (3) fix `extract_method_name_from_call` to read the `method` field (today it returns the receiver for `receiver.it`, so the fixture's negative control passes for the wrong reason); (4) ruby lifecycle arm: `setup`/`teardown` method names inside test containers; (5) containers: call `mark_base_type_test_containers` for `Minitest::Test`, `Test::Unit::TestCase`, `ActiveSupport::TestCase`, `ActionDispatch::IntegrationTest` (emit `base_types` on ruby classes if missing); (6) RSpec completions: `xit`, `fit`, `xdescribe`, `fdescribe`, `fcontext`, `xspecify`; `shared_examples`/`shared_context` as containers, `it_behaves_like`/`include_examples` as cases-by-reference (or record as open gap with reason if reference semantics do not fit a symbol row), `let`/`let!`/`subject` as `fixture_setup`; (7) fixture: three files (RSpec, minitest class, Rails macro) with production-path negative controls; (8) record RSpec metadata tags (annotations channel) as an open gap or implement `symbol_annotations` for tag symbols if small.

**File ownership:** `crates/julie-extractors/src/ruby/`, `test_detection.rs` ruby arms only, `fixtures/extraction/ruby/`, new `src/tests/ruby/test_detection.rs`, ruby ledger rows, `docs/languages/ruby.md`

**Serialization required:** No

**Dependency reason:** None - safe parallel batch.

**What to build:** Close findings 1–7 (finding 6's reference semantics and finding 8 may become complete open-gap entries), write the ruby decision-doc row and `docs/languages/ruby.md`.

**Approach:** The Rails `test` macro arm mirrors the existing RSpec block arm — string-literal first argument becomes the symbol name. Scoping: RSpec blocks establish containers; minitest scoping keys on the base-type containers from finding 5.

**Acceptance criteria:**
- [ ] Rails macro suites visible; false-positive controls prove production code stays clean.
- [ ] `extract_method_name_from_call` reads the method field; regression test added.
- [ ] Golden trio, ledger, decision-doc row, docs complete; `cargo xtask test language ruby` passes.
- [ ] Verified diff handed to the lead per `parallel-lead-commit`.

### Task 12: PHP detection + evidence

**Files:**
- Modify: `crates/julie-extractors/src/php/functions.rs` (route through `apply_callable_test_metadata`), php base-type normalization
- Modify: `crates/julie-extractors/src/test_detection.rs::detect_php` (:493) + php lifecycle arm + new `mark_php_test_containers`
- Modify: `fixtures/extraction/php/test_roles/`, php ledger rows
- Create: `docs/languages/php.md`; php row in the decision doc

**Interfaces:**
- Consumes: Task 1 contract, Task 2 guards (`*Test.php` etc. added there) and scoping helper.
- Produces: honest php ledger rows and goldens.

**Contract inputs:** Findings: (1) php lifecycle arm: `setUp`, `tearDown`, `setUpBeforeClass`, `tearDownAfterClass` names plus `before`/`after`/`beforeclass`/`afterclass` attribute keys and `@before`/`@after` docblocks; (2) route the declaration path through `apply_callable_test_metadata` (root cause: php can only write `is_test` today); (3) containers: normalize php class base types into `base_types` and mark `TestCase` subclasses and `#[Test]`-holding classes via `mark_php_test_containers`; (4) accept `extends TestCase` and the `*Test.php` filename as test proofs outside a `tests/` path; (5) Pest precision: containment/path guard via `normalize_scoped_test_roles` (production `test()`/`it()` control); (6) fixture: full PHPUnit class (`testFoo` name, `@test` docblock, `setUp`, `#[Before]`, data-provider negative control) plus a non-test-path Pest control; (7) record Codeception (`*Cest.php`, `_before`/`_after`), Behat step attributes, and PHPSpec as `open_gaps`; (8) note for the Miller follow-up list (not this repo): `.php`/`.phtml` missing from Miller's extension map.

**File ownership:** `crates/julie-extractors/src/php/`, `test_detection.rs::detect_php` + php lifecycle arm + new php container pass, `fixtures/extraction/php/`, php ledger rows, `docs/languages/php.md`

**Serialization required:** No

**Dependency reason:** None - safe parallel batch.

**What to build:** Close findings 1–6, record finding 7, write the php decision-doc row and `docs/languages/php.md`.

**Approach:** PHPUnit data-provider methods referenced by `#[DataProvider]` are helpers, not tests — the fixture control proves it. `parameterized_test` applies to `#[DataProvider]`-carrying test methods.

**Acceptance criteria:**
- [ ] PHPUnit lifecycle, containers, and out-of-tree detection work; Pest controls pass.
- [ ] Golden, ledger, decision-doc row, docs complete; `cargo xtask test language php` passes.
- [ ] Verified diff handed to the lead per `parallel-lead-commit`.

### Task 13: Kotlin detection + evidence

**Files:**
- Modify: `crates/julie-extractors/src/kotlin/` (identifier backtick handling, `test_calls.rs` string-invoke branch, container pass)
- Modify: `crates/julie-extractors/src/test_detection.rs` kotlin annotation key lists only
- Modify: `fixtures/extraction/kotlin/` (new `kotest_string_spec/` and kotlin.test lifecycle fixtures; extend `junit_tests/`), kotlin ledger rows
- Create: `docs/languages/kotlin.md`; kotlin row in the decision doc

**Interfaces:**
- Consumes: Task 1 contract.
- Produces: honest kotlin ledger rows and goldens.

**Contract inputs:** Findings: (1) Kotest StringSpec/FreeSpec/WordSpec emit nothing — add the string-literal-invoke branch (`"name" { }` as `call_expression` with `string_literal` callee) and the WordSpec infix `should` form; copy the Scala adapter's approach; (2) strip backticks from kotlin identifier names in `symbols.name` (JUnit/Gradle report without backticks — name matching breaks otherwise); (3) add `beforetest`/`aftertest` (kotlin.test `@BeforeTest`/`@AfterTest`) to the lifecycle keys and `testfactory`/`testtemplate` to the case keys; `@ParameterizedTest`/`@RepeatedTest` → `parameterized_test`; (4) mark Kotest/Spek spec classes as `test_container` (class whose supertype is a spec base or whose body is a spec lambda); (5) vocabulary: `feature`/`scenario`/`expect` plus `xdescribe`/`xit`/`xtest`/`xcontext`; (6) fix the self-referential `calls` relationship on lifecycle call symbols (beforeEach → beforeEach noise edge); (7) fixtures: new Kotest StringSpec golden, kotlin.test lifecycle golden, extend `junit_tests` with `@ParameterizedTest`, `@RepeatedTest`, and a backtick name; (8) record TestNG-in-Kotlin keys as covered by the java task's shared lists (verify) and Gradle extra source sets as covered by Task 2.

**File ownership:** `crates/julie-extractors/src/kotlin/`, `test_detection.rs` kotlin key lists only, `fixtures/extraction/kotlin/`, kotlin ledger rows, `docs/languages/kotlin.md`

**Serialization required:** No

**Dependency reason:** None - safe parallel batch.

**What to build:** Close findings 1–7, verify finding 8, write the kotlin decision-doc row and `docs/languages/kotlin.md`.

**Approach:** Backtick stripping is a name-normalization change — check `trace` for consumers of kotlin symbol names before changing (reference resolution may depend on the raw form; if so, store the display name stripped and keep the raw form in metadata).

**Acceptance criteria:**
- [ ] StringSpec/WordSpec/FreeSpec cases emit symbols; backtick names normalized; spec classes are containers.
- [ ] Goldens, ledger, decision-doc row, docs complete; `cargo xtask test language kotlin` passes.
- [ ] Verified diff handed to the lead per `parallel-lead-commit`.

### Task 14: Swift detection + evidence

**Files:**
- Modify: `crates/julie-extractors/src/swift/callables.rs` (`extract_function`, `extract_initializer` route through `apply_callable_test_metadata`), `crates/julie-extractors/src/swift/mod.rs` (container passes)
- Modify: `crates/julie-extractors/src/test_detection.rs::detect_swift` (:581) + swift lifecycle arm
- Modify: `fixtures/extraction/swift/test_roles/`, swift ledger rows
- Create: `docs/languages/swift.md`; swift row in the decision doc

**Interfaces:**
- Consumes: Task 1 contract, Task 2 guards (Xcode conventions added there) and scoping helper.
- Produces: honest swift ledger rows and goldens.

**Contract inputs:** Findings (two blockers first): (1) `detect_swift` ignores annotations — take `annotation_keys` and honor `@Test` (case) and `@Suite` (container); `@Test(arguments:)` → `parameterized_test`; (2) XCTestCase containers: call `mark_base_type_test_containers(&mut symbols, "XCTestCase")` in `swift/mod.rs` (base_types already recorded); (3) swift lifecycle arm: `setUp`, `tearDown`, `setUpWithError`, `tearDownWithError` → lifecycle, not test case (they are wrongly `is_test=true` today); Swift Testing suite `init`/`deinit` → lifecycle via `extract_initializer` routing; (4) scope `func testXxx` name detection with `normalize_scoped_test_roles` (helper-struct control); (5) Quick vocabulary: `sharedExamples`, `itBehavesLike`, `beforeSuite`, `afterSuite`, `aroundEach`; (6) fixture: XCTest (container + lifecycle + case + non-test method), Swift Testing (`@Suite` + `@Test` + `@Test(arguments:)` + unannotated control), keep the Quick block; (7) decision-doc row records the ledger over-claim correction (container/lifecycle were claimed with no emission path).

**File ownership:** `crates/julie-extractors/src/swift/`, `test_detection.rs::detect_swift` + swift lifecycle arm only, `fixtures/extraction/swift/`, swift ledger rows, `docs/languages/swift.md`

**Serialization required:** No

**Dependency reason:** None - safe parallel batch.

**What to build:** Close findings 1–6, write the correction row (finding 7) and `docs/languages/swift.md`.

**Approach:** Swift Testing macros arrive as attributes in the grammar — verify the exact node/key spelling with Miller against the swift grammar fixtures before coding; do not assume `@Test` normalizes to `test` without checking the annotation-key normalizer.

**Acceptance criteria:**
- [ ] Swift Testing and XCTest fully classified; lifecycle no longer mislabeled as cases.
- [ ] Golden, ledger correction, decision-doc row, docs complete; `cargo xtask test language swift` passes.
- [ ] Verified diff handed to the lead per `parallel-lead-commit`.

### Task 15: `test_linkage` / `test_coverage` contract decision + C# pilot

**Files:**
- Create: `docs/decisions/2026-08-25-test-linkage-metadata-contract.md`
- Modify: `crates/julie-extractors/src/test_detection.rs` metadata write path (pilot emission), csharp emission site, `fixtures/extraction/csharp/` pilot fixture addition, contract tests

**Interfaces:**
- Consumes: Task 1's metadata contract; Miller's `TestLinkageReader` key shapes (verify the exact expected keys in `/home/murphy/source/miller` before writing — do not guess).
- Produces: the decision doc (schema, semantics, rollout order) and a working pilot: C# test symbols emit `test_linkage` naming directly-called production symbols where the extractor already has the call edges in-file.

**Contract inputs:** Miller has the reader built (`explicit_linkage` evidence tier) and probes for the key on every graph load; no extractor writes it. The pilot proves the shape end to end; full rollout is follow-up work, one language at a time.

**File ownership:** New decision doc, `test_detection.rs` metadata write path, csharp pilot emission + fixture, contract tests

**Serialization required:** Yes

**Dependency reason:** Extends the Task 1 metadata contract; touches the shared write path after all batches settle.

**What to build:** The decision doc (with the verified Miller key shapes as primary source) and the narrowest honest pilot: in-file call-edge-derived linkage for C# test methods.

**Approach:** Derive linkage only from facts the extractor already has (same-file call relationships); no cross-file guessing — that is Miller's join. If verification against Miller shows the reader expects data the extractor cannot honestly produce in-file, write the decision doc with that finding and a `not-yet` verdict instead of a forced pilot; the doc is the deliverable, the pilot is conditional.

**Acceptance criteria:**
- [ ] Decision doc complete with verified key shapes and rollout plan.
- [ ] Pilot emits linkage for the C# fixture or the doc records the verified reason it cannot.
- [ ] `cargo xtask test contract` and `cargo xtask test language csharp` pass.
- [ ] Worker-scope verification passes and the change is committed per `serial-worker-commit`.

### Task 16: Dialect language identity decision (jsx/tsx)

**Files:**
- Create: `docs/decisions/2026-08-25-dialect-language-identity.md`

**Interfaces:**
- Consumes: the audit finding — the artifact writes `jsx`/`tsx` in `symbols.language` and `files.language`, Miller's extension map answers `javascript`/`typescript` for the same files, so CT's selector language comparison misses.
- Produces: the recorded contract decision plus the Miller follow-up item.

**Contract inputs:** Recommended direction (challenge it in the doc, do not rubber-stamp): the artifact keeps honest dialect names — dialects are real languages in the registry — and Miller maps dialect → base language at read time. The alternative (artifact publishes a `base_language` fact) is a schema change; weigh it against the clean-contract rule.

**File ownership:** New decision doc only (plus contract note in `docs/architecture/` if accepted)

**Serialization required:** Yes

**Dependency reason:** Cross-repo contract decision; must not race the JS/TS tasks.

**What to build:** A short decision doc: problem, the two options, chosen contract, and the exact Miller-side change it implies (named file: Miller's extension map / selector comparison).

**Acceptance criteria:**
- [ ] Decision doc complete; the Miller follow-up item is stated precisely enough to hand to a Miller session.
- [ ] Change committed per `serial-worker-commit`.

### Task 17: Cross-language closure sweep

**Files:**
- Modify: `docs/decisions/2026-08-20-test-role-contract-closure.md` (final table reconciliation), `fixtures/extraction/capabilities.json` (final consistency pass)
- Verify: everything

**Interfaces:**
- Consumes: every earlier task's ledger rows, decision-doc rows, and docs.
- Produces: branch-gate evidence and the reconciled shared tables.

**Contract inputs:** The shared cross-language tables and counts that hard-guard rows (marker language matrix, factory/language counts, capability snapshot tests) must reflect all changes; the strict report must show `silent_cells=0`, `quality_bar_debts=0`.

**File ownership:** Shared tables in decision doc, capabilities.json final reconciliation, branch-gate evidence

**Serialization required:** Yes

**Dependency reason:** Depends on every earlier task's ledger rows.

**What to build:** Reconcile the shared tables, run the full branch gate (including `win-test` — Task 2 changed path logic — and `scripts/check-agent-doc-sync.sh`), record the verification ledger, and checkpoint before commit.

**Acceptance criteria:**
- [ ] Branch gate green: `cargo xtask test default`, `golden`, `capability`, `contract`, strict report, doc-sync check, Windows suite.
- [ ] Every language touched in this plan has: golden-backed ledger rows, a decision-doc row, and `docs/languages/<lang>.md`.
- [ ] All open-gap entries have reason, required closure, and planned task.
- [ ] Verification ledger recorded; change committed per `serial-worker-commit`.

---

## Out of Scope — Miller Follow-Ups

Tracked here so they are not lost; they belong to Miller sessions, not this repo:

- Add `.php` / `.phtml` to Miller's `LanguageFromPath` extension map (Task 12 finding).
- Implement the dialect-mapping side of Task 16's decision in Miller's selector/extension map.
- New CT providers (Go, JVM, Ruby, PHP, Swift) in `Miller.Testing/Providers/` — separate Miller plans, unblocked by this branch's extractor facts.
- Miller already reads `test_role`, `test_linkage`, and `test_coverage`; no Miller change needed for Tasks 1 and 15.

## Estimated Effort

For AI coding agents: 17 tasks, roughly 18–22 worker sessions total (JS/TS is the largest at ~3; Java and the decision docs are the smallest at ~0.5–1 each). Human time: plan approval now, epoch-bump/ledger review at Task 1, release decision at the end.
