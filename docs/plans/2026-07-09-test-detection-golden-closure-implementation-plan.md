# Test Detection Golden Closure Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Add registered golden fixtures for test-role extraction across every executable supported language, resolve applicability for non-executable languages, and promote `test_detection` into the strict language quality bar.

**Architecture:** Each language receives one registered `test_roles` fixture containing the smallest language-native positive cases and nearby negative controls. The fixture proves `test_case`, `test_container`, and `test_lifecycle` only where the extractor emits them; unsupported but meaningful roles remain explicit gaps, and genuinely absent roles are source-backed `not_applicable`. After every language is classified and at least test-case evidence exists for each executable language, the strict report treats `test_detection` as a general-purpose-language expectation.

**Tech Stack:** Rust/tree-sitter extractors, golden JSON fixtures, capability matrix, `cargo xtask` language/golden/capability tiers, Node.js quality report.

**Architecture Quality:** This plan changes extraction evidence, not the runtime interface. Each language-local extractor remains the implementation boundary; shared `test_detection.rs` and `test_calls.rs` remain the classification seams. Golden fixtures are the caller-facing test surface. Rejected shortcuts: declaring support from unit tests alone, one synthetic syntax across languages, generic fallback claims without a negative control, or weakening the strict report. Architecture risk: medium because fixture changes span many grammars but remain language-local.

## Global Constraints

- The prerequisite plan `docs/plans/2026-07-09-test-evidence-capability-contract-implementation-plan.md` is implemented first.
- Use fixture name `test_roles` and paths `fixtures/extraction/<language>/test_roles/source.<ext>` plus `expected.json`.
- Every fixture contains at least one positive native test case and one similar non-test negative control.
- Add container/lifecycle positives only when the language/framework has a stable construct and the extractor supports it.
- `supported` requires the registered golden to emit the exact role.
- `not_applicable` requires source-backed language/framework reasoning recorded in the capability row or a finding note.
- A real unsupported role remains `open_gaps` with a concrete closure; do not mislabel it not applicable to make the gate pass.
- Do not add runner commands, result formats, framework execution, watcher behavior, or continuous-testing scheduling.
- Do not broaden detector heuristics merely to make a fixture pass. If a fixture reveals a false negative/positive, fix it with a separate TDD step and language-local tests before updating the golden.
- Preserve existing golden outputs except for intentional role evidence.
- After each language group run its narrow language commands; after all groups run default, golden, capability, contract, and strict-report gates.
- Execution uses @razorback:test-driven-development for detector fixes and @razorback:verification-before-completion before every group commit.
- No release, tag, push, Miller pin, or Eros change is part of this plan.

---

## Fixture Standard

The source uses the native framework shape already covered by the language's detector tests. For example:

```rust
#[test]
fn extracts_test_case() {}

fn test_named_but_unannotated_is_not_a_test() {}
```

```csharp
public sealed class CalculatorTests
{
    [Fact] public void Adds() { }
    public void FactNamedButUnannotated() { }
}
```

```javascript
describe("calculator", () => {
  beforeEach(() => {});
  it("adds", () => {});
});
function itNamedButNotCalled() {}
```

The expected JSON must show the emitted role in symbol metadata and retain the negative control as an ordinary symbol or no test-role symbol, as appropriate. Do not hand-copy byte positions from another fixture; generate/inspect the language's real extraction and review every changed row.

## Verification Strategy

**Project source of truth:** `AGENTS.md`, `fixtures/extraction/capabilities.json`, `crates/julie-extractors/src/tests/test_detection.rs`, language-local test-detection modules, and `cargo xtask test` tiers.

**Worker red/green scope:** For each language, run `cargo xtask test language <language>` after registering its fixture. If detector code changes, run the exact language-local test module first and observe red/green.

**Worker ceiling:** Narrow language tiers and `cargo xtask test golden` for a completed group. Workers do not run certification or real-world corpora.

**Worker gate invariant:** Each registered source exactly matches expected extraction, positive roles are present, negative controls are not test roles, and the capability row matches observed roles.

**Lead affected-change scope:** `cargo xtask test golden`, `cargo xtask test capability`, and the strict data-quality report after every coherent group.

**Branch gate:** `cargo xtask test default`, `cargo xtask test golden`, `cargo xtask test capability`, `cargo xtask test contract`, and `node scripts/language-data-quality-report.mjs --strict`.

**Replay/metric evidence:** Hard gates are registered golden equality, no false-positive negative control, exact capability classification, and zero silent/debt counts. Role counts per language and remaining open role gaps are report-only.

**Escalation triggers:** Parser upgrades, cross-language resolution, schema changes, or slow real-world fixtures require separate scope and gates.

**Assigned verification failure:** Workers stop and report when assigned verification fails, unless this plan explicitly says to update that gate.

**Verification ledger:** Record language, invariant, command, scope label, commit SHA, result, and timestamp. Include supported/not-applicable/open role units after each group.

## Parallel Execution Contract

The language groups serialize because every task registers fixtures and updates the shared `fixtures/extraction/capabilities.json`. Parallel workers may research or draft language-local source files, but only the active task owns the shared matrix and accepted branch state.

| Task | Parallel batch | File ownership | Serialization required | Dependency reason |
|---|---|---|---|---|
| Task 1: Systems languages | None - serial | Rust, C, C++, Go, Zig test-role fixtures and capability rows | Yes | Shared capability matrix; establishes fixture pattern. |
| Task 2: JavaScript and component languages | None - serial | JavaScript, JSX, TypeScript, TSX, Vue fixtures and rows | Yes | Shared capability matrix; follows Task 1. |
| Task 3: Managed and JVM languages | None - serial | C#, VB.NET, Razor, Java, Kotlin, Scala fixtures and rows | Yes | Shared capability matrix; follows Task 2. |
| Task 4: Script languages | None - serial | Python, PHP, Ruby, Lua, R, Bash, PowerShell fixtures and rows | Yes | Shared capability matrix; follows Task 3. |
| Task 5: Remaining executable languages | None - serial | Swift, Dart, Elixir, GDScript, QML fixtures and rows | Yes | Shared capability matrix; follows Task 4. |
| Task 6: Non-executable applicability audit | None - serial | HTML, CSS, SQL, regex, Markdown, JSON, TOML, YAML rows plus finding note | Yes | Requires completed executable-language baseline. |
| Task 7: Promote the strict quality bar | None - serial | report expectations, matrix guards, contract docs, final findings | Yes | Requires Tasks 1-6 evidence. |

Every task uses `serial-worker-commit` after its language/group gate passes so the branch stays bisectable and golden-valid.

### Task 1: Systems languages

**Files:**
- Create: `fixtures/extraction/rust/test_roles/source.rs`, `expected.json`
- Create: `fixtures/extraction/c/test_roles/source.c`, `expected.json`
- Create: `fixtures/extraction/cpp/test_roles/source.cpp`, `expected.json`
- Create: `fixtures/extraction/go/test_roles/source_test.go`, `expected.json`
- Create: `fixtures/extraction/zig/test_roles/source.zig`, `expected.json`
- Modify: `fixtures/extraction/capabilities.json`
- Test when required: language-local files under `crates/julie-extractors/src/tests/{rust,c,cpp,go,zig}/`

**Interfaces:**
- Consumes: annotation/macro/name detectors already implemented for these languages.
- Produces: registered native test-case goldens and exact role capability rows.

**Contract inputs:** Use Rust `#[test]`, C/C++ supported test DSL forms, Go `TestXxx`, and Zig `test "name"` from current detector tests.

**File ownership:** Rust, C, C++, Go, Zig test-role fixtures and capability rows

**Serialization required:** Yes.

**Dependency reason:** Shared capability matrix; establishes fixture pattern.

**Step 1: Write each source with positive and negative controls.**

**Step 2: Register one fixture at a time and run `cargo xtask test language <language>` to observe the missing/incorrect expected output.**

**Step 3: Capture/review the real normalized output, fix any detector defect through its language-local test, then write the expected JSON and capability classification.**

**Step 4: Run all five language tiers, `cargo xtask test golden`, and `cargo xtask test capability`.**

**Step 5: Use `serial-worker-commit` and record the SHA.**

**Acceptance criteria:**
- [x] Five registered fixtures pass.
- [x] Every positive emits `test_case`; negative controls do not.
- [x] Container/lifecycle units are supported, not applicable, or concretely open—never silent.
- [x] Capability and golden tiers pass and the task is committed.

### Task 2: JavaScript and component languages

**Files:**
- Create: `fixtures/extraction/javascript/test_roles/source.js`, `expected.json`
- Create: `fixtures/extraction/jsx/test_roles/source.jsx`, `expected.json`
- Create: `fixtures/extraction/typescript/test_roles/source.ts`, `expected.json`
- Create: `fixtures/extraction/tsx/test_roles/source.tsx`, `expected.json`
- Create: `fixtures/extraction/vue/test_roles/source.vue`, `expected.json`
- Modify: `fixtures/extraction/capabilities.json`
- Test when required: language-local JS/TS/Vue test-detection modules

**Interfaces:**
- Consumes: shared call-style `test_calls` materialization and embedded script extraction.
- Produces: test-case/container/lifecycle golden evidence where emitted.

**Contract inputs:** Use framework calls already recognized by current detector vocabularies; include an ordinary same-named function/call negative.

**File ownership:** JavaScript, JSX, TypeScript, TSX, Vue fixtures and rows

**Serialization required:** Yes.

**Dependency reason:** Shared capability matrix; follows Task 1.

**Step 1: Write native sources.** Use the existing JS/TS detector vocabularies to add `describe`/test/lifecycle positives plus ordinary-call/function negatives in each source file. Keep JSX/TSX syntax valid and put Vue cases in the embedded script region the extractor currently supports.

**Step 2: Register the five fixtures.** Add exact source/expected paths to their capability rows and classify each role as a temporary open gap until the registered golden proves it.

**Step 3: Establish red, then write reviewed goldens.** Run `cargo xtask test language javascript`, `jsx`, `typescript`, `tsx`, and `vue`; confirm each new registration fails on missing/mismatched expected output. Capture the real normalized output, review positions/metadata/negative controls, write each `expected.json`, and fix any detector defect through its language-local unit test before accepting the golden.

**Step 4: Run the group gate.** Re-run all five language tiers, then `cargo xtask test golden` and `cargo xtask test capability`; update `supported` only for roles now emitted by registered fixtures.

**Step 5: Apply commit mode.** Use `serial-worker-commit` and record the SHA plus per-language role ledger.

**Acceptance criteria:**
- [x] Five registered fixtures pass.
- [x] DSL test/container/lifecycle roles match actual emitted metadata.
- [x] Embedded/component extraction does not fabricate a role the Vue path cannot support.
- [x] Negative controls remain non-test.
- [x] Task gates pass and the task is committed.

### Task 3: Managed and JVM languages

**Files:**
- Create: `fixtures/extraction/csharp/test_roles/source.cs`, `expected.json`
- Create: `fixtures/extraction/vbnet/test_roles/source.vb`, `expected.json`
- Create: `fixtures/extraction/razor/test_roles/source.razor`, `expected.json`
- Create: `fixtures/extraction/java/test_roles/source.java`, `expected.json`
- Create: `fixtures/extraction/kotlin/test_roles/source.kt`, `expected.json`
- Create: `fixtures/extraction/scala/test_roles/source.scala`, `expected.json`
- Modify: `fixtures/extraction/capabilities.json`
- Test when required: corresponding language-local test-detection modules

**Interfaces:**
- Consumes: normalized annotations, JUnit/TestNG/xUnit/NUnit/MSTest rules, and Scala DSL extraction currently implemented.
- Produces: six registered role fixtures and classifications.

**Contract inputs:** Preserve VB.NET's existing registered test evidence; the new fixture may consolidate it only if no prior golden claim is lost.

**File ownership:** C#, VB.NET, Razor, Java, Kotlin, Scala fixtures and rows

**Serialization required:** Yes.

**Dependency reason:** Shared capability matrix; follows Task 2.

**Step 1: Write native sources.** Add current xUnit/NUnit/MSTest/JUnit/TestNG/Scala-framework positives as appropriate, plus unannotated or same-named negatives. Razor uses only the embedded C# form already supported.

**Step 2: Register the six fixtures.** Add exact paths in `capabilities.json` and keep each role open until its registered expected output proves support.

**Step 3: Establish red and produce reviewed expected JSON.** Run `cargo xtask test language csharp`, `vbnet`, `razor`, `java`, `kotlin`, and `scala`; observe the new-fixture failures, then capture/review normalized extraction. If a language-native positive is missed, add a failing language-local detector test before changing extraction behavior.

**Step 4: Run the group gate.** Re-run all language tiers, `cargo xtask test golden`, and `cargo xtask test capability`; preserve any prior VB.NET evidence and update exact role classifications.

**Step 5: Apply commit mode.** Use `serial-worker-commit` and record the SHA/ledger.

**Acceptance criteria:**
- [x] Six registered fixtures pass without weakening annotation normalization.
- [x] Annotated positives and unannotated negatives are distinguished.
- [x] Razor claims only roles actually emitted through its embedded C# path.
- [x] Task gates pass and the task is committed.

### Task 4: Script languages

**Files:**
- Create: `fixtures/extraction/python/test_roles/test_source.py`, `expected.json`
- Create: `fixtures/extraction/php/test_roles/test_source.php`, `expected.json`
- Create: `fixtures/extraction/ruby/test_roles/test_source.rb`, `expected.json`
- Create: `fixtures/extraction/lua/test_roles/test_source.lua`, `expected.json`
- Create: `fixtures/extraction/r/test_roles/test_source.r`, `expected.json`
- Create: `fixtures/extraction/bash/test_roles/test_source.sh`, `expected.json`
- Create: `fixtures/extraction/powershell/test_roles/source.ps1`, `expected.json`
- Modify: `fixtures/extraction/capabilities.json`
- Test when required: corresponding language-local test-detection modules

**Interfaces:**
- Consumes: naming/path/annotation and call-style DSL detectors.
- Produces: seven registered role fixtures and classifications.

**Contract inputs:** Every name-convention positive requires a same-file negative proving the path/name guard does not classify ordinary production functions.

**File ownership:** Python, PHP, Ruby, Lua, R, Bash, PowerShell fixtures and rows

**Serialization required:** Yes.

**Dependency reason:** Shared capability matrix; follows Task 3.

**Step 1: Write native sources.** Use current pytest/unittest, PHPUnit/call DSL, Ruby test DSL, Lua/R call DSL, shell naming/path, and Pester constructs. Every naming/path positive gets a nearby production-style negative.

**Step 2: Register the seven fixtures.** Add exact fixture entries and retain open role classifications until goldens prove them.

**Step 3: Establish red and produce reviewed expected JSON.** Run `cargo xtask test language python`, `php`, `ruby`, `lua`, `r`, `bash`, and `powershell`; observe each missing/mismatched expected failure, capture real output, and review test-role metadata plus negatives. Detector fixes require a red language-local unit test first.

**Step 4: Run the group gate.** Re-run all seven language tiers, `cargo xtask test golden`, and `cargo xtask test capability`; update capability rows from observed roles.

**Step 5: Apply commit mode.** Use `serial-worker-commit` and record the SHA/ledger.

**Acceptance criteria:**
- [x] Seven registered fixtures pass.
- [x] Naming/path guards have explicit negative controls.
- [x] Call-style containers/lifecycle roles match actual output.
- [x] Task gates pass and the task is committed.

### Task 5: Remaining executable languages

**Files:**
- Create: `fixtures/extraction/swift/test_roles/test_source.swift`, `expected.json`
- Create: `fixtures/extraction/dart/test_roles/source.dart`, `expected.json`
- Create: `fixtures/extraction/elixir/test_roles/source.ex`, `expected.json`
- Create: `fixtures/extraction/gdscript/test_roles/test_source.gd`, `expected.json`
- Create: `fixtures/extraction/qml/test_roles/test_source.qml`, `expected.json`
- Modify: `fixtures/extraction/capabilities.json`
- Test when required: corresponding language-local test-detection modules

**Interfaces:**
- Consumes: current XCTest/package:test/ExUnit/GUT/QML fallback rules.
- Produces: five registered fixtures and classifications.

**Contract inputs:** If a language's current detector lacks a native stable test form, retain an open gap rather than inventing a generic claim.

**File ownership:** Swift, Dart, Elixir, GDScript, QML fixtures and rows

**Serialization required:** Yes.

**Dependency reason:** Shared capability matrix; follows Task 4.

**Step 1: Write native sources.** Add the currently supported XCTest/package:test/ExUnit/GUT/QML test forms and explicit negative controls. Do not force a generic pattern where the language-local detector has no native form.

**Step 2: Register evidence-backed fixtures.** Add exact capability fixture entries. If source inspection shows a proposed native form is unsupported, leave its role as a concrete gap and document the required extractor closure instead of registering a false positive.

**Step 3: Establish red and produce reviewed expected JSON.** Run `cargo xtask test language swift`, `dart`, `elixir`, `gdscript`, and `qml`; observe registration failures, capture/review real output, and use TDD for any justified detector fix.

**Step 4: Run the group gate.** Re-run the five language tiers, `cargo xtask test golden`, and `cargo xtask test capability`; update supported/not-applicable/open classifications from evidence.

**Step 5: Apply commit mode.** Use `serial-worker-commit` and record the SHA/ledger.

**Acceptance criteria:**
- [x] Every evidence-backed fixture passes.
- [x] Unsupported native forms remain explicit gaps.
- [x] Generic fallback is not presented as framework-complete support.
- [x] Task gates pass and the task is committed.

### Task 6: Non-executable applicability audit

**Files:**
- Modify: `fixtures/extraction/capabilities.json`
- Create: `docs/findings/2026-07-09-test-detection-applicability-audit.md`

**Interfaces:**
- Consumes: source verification for HTML, CSS, SQL, regex, Markdown, JSON, TOML, and YAML semantics plus existing extractor behavior.
- Produces: supported/not-applicable/open classification for all three role units in every remaining language.

**Contract inputs:** Empty current output is not evidence of not-applicability.

**File ownership:** HTML, CSS, SQL, regex, Markdown, JSON, TOML, YAML rows plus finding note

**Serialization required:** Yes.

**Dependency reason:** Requires completed executable-language baseline.

**Step 1: Gather applicability evidence.** Inspect the pinned grammar/product scope and current extractor modules for HTML, CSS, SQL, regex, Markdown, JSON, TOML, and YAML. Record whether each test role is a language construct, an external framework convention, or genuinely absent.

**Step 2: Write the audit finding before matrix changes.** For each language/role, cite the inspected repo source or pinned grammar fact and choose `supported`, `not_applicable`, or `open_gaps`. Empty extraction alone is never cited as evidence.

**Step 3: Update capability rows.** Apply the recorded classification exactly. If any role is supported by an existing registered golden, name it; otherwise keep the role open or not applicable according to Step 2.

**Step 4: Run the audit gate.** Run `cargo xtask test capability`, `cargo xtask test golden`, and `node scripts/language-data-quality-report.mjs --strict`; verify all eight rows are non-silent and the report remains zero/zero.

**Step 5: Apply commit mode.** Use `serial-worker-commit` and record the SHA plus the applicability finding path.

**Acceptance criteria:**
- [ ] All eight languages classify all role units.
- [ ] Every not-applicable claim has written source reasoning.
- [ ] Uncertain or framework-defined roles remain open gaps.
- [ ] Task gates pass and the task is committed.

### Task 7: Promote the strict quality bar

**Files:**
- Modify: `scripts/language-data-quality-report.mjs:25`
- Modify: `crates/julie-extractors/src/tests/capability_matrix.rs:1084`
- Modify: `docs/contracts/test-evidence-v1.md`
- Create: `docs/findings/2026-07-09-test-detection-golden-closure.md`

**Interfaces:**
- Consumes: completed classifications and registered goldens from Tasks 1-6.
- Produces: `test_detection` as a strict code-language expectation and final evidence report.

**Contract inputs:** Promotion is allowed only when every executable language has at least one supported role or a source-backed not-applicable classification; open role variants remain visible but cannot make a cell silent.

**File ownership:** report expectations, matrix guards, contract docs, final findings

**Serialization required:** Yes.

**Dependency reason:** Requires Tasks 1-6 evidence.

**Step 1: Add a failing test/report assertion that code languages require non-empty supported or not-applicable test detection.**

**Step 2: Run `cargo xtask test capability` and the strict report to verify the gate fails if any language remains unclassified.**

**Step 3: Add `test_detection` to `CODE_LANGUAGE_EXPECTATIONS`, update docs, and record per-language evidence/open gaps.**

**Step 4: Run the full branch gate:**

```bash
cargo xtask test default
cargo xtask test golden
cargo xtask test capability
cargo xtask test contract
node scripts/language-data-quality-report.mjs --strict
```

**Step 5: Use `serial-worker-commit` and record the final SHA/ledger.**

**Acceptance criteria:**
- [ ] `test_detection` is enforced for code languages.
- [ ] Strict report is `silent_cells: 0`, `quality_bar_debts: 0`.
- [ ] Final findings map every supported role to a golden fixture.
- [ ] Remaining role variants are explicit, owned gaps.
- [ ] Full branch gate passes and the task is committed.

## Program Exit Criteria

- [ ] Every executable supported language has registered golden test-role evidence or an evidence-backed exception.
- [ ] Every fixed role unit is supported, not applicable, or explicitly open for every language.
- [ ] Negative controls guard against false-positive role detection.
- [ ] `test_detection` is part of the strict code-language quality bar.
- [ ] No runtime CT behavior or runner inventory leaked into julie-extractors.
- [ ] Default, golden, capability, contract, and strict-report gates pass.
