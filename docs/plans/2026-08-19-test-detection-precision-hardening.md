# Test Detection Precision Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Stop Python fixtures/mocks and ordinary Scala/Elixir helpers from emitting false-positive test facts without reducing proven framework test-role coverage.

**Architecture:** Refine the existing language branches inside `is_test_symbol`; do not add a new classifier, metadata key, schema column, or framework registry. Preserve call-style test extraction and prove the correction through shared dispatch tests plus full language extraction.

**Tech Stack:** Rust, tree-sitter language extractors, Cargo test tiers, registered golden fixtures.

**Architecture Quality:** Existing `test_detection` remains the single policy module and `is_test_symbol` remains the caller-facing interface. Risk is low: emitted values change for false positives, but no module boundary or contract shape changes.

## Global Constraints

- Follow `AGENTS.md` and keep the product boundary at versioned extraction evidence.
- Use `razorback:test-driven-development`: every production change follows a test that was observed failing for the expected false-positive reason.
- Use Miller for all codebase exploration; inspect symbols before modifying them and trace references before changing public APIs.
- Do not add inline narration comments; tests contain no comments.
- Preserve `is_test`, `test_container`, and `test_lifecycle` field names and artifact schemas.
- Preserve existing Scala/Elixir call-style role extraction and Python lifecycle/name-path detection.
- Python decorator positives are `pytest.mark.*`, `unittest.skip`, `unittest.skipIf`, `unittest.skipUnless`, and `unittest.expectedFailure` only.
- Python decorator negatives include `pytest.fixture` and every `unittest.mock.*` key.
- No capability row is promoted, demoted, or reclassified by this slice.
- Official framework evidence is recorded in `docs/plans/2026-08-19-test-detection-precision-hardening-design.md`.

---

## Verification Strategy

**Project source of truth:** `AGENTS.md`, `docs/testing-strategy.md`, `fixtures/extraction/capabilities.json`, and the existing test-role contract/golden fixtures.

**Worker red/green scope:** Run the exact new test filter first and observe the expected false-positive assertion failure, then rerun it green. Covering commands are `cargo test -p julie-extractors tests::test_detection -- --nocapture`, `cargo test -p julie-extractors tests::python::test_detection -- --nocapture`, `cargo test -p julie-extractors tests::scala::test_detection -- --nocapture`, and `cargo test -p julie-extractors tests::elixir::test_detection -- --nocapture`.

**Worker ceiling:** The focused commands above plus `cargo xtask test language python`, `cargo xtask test language scala`, `cargo xtask test language elixir`, and `cargo xtask test golden`. Workers do not own default-tier, strict-report, formatting, or branch acceptance.

**Worker gate invariant:** Shared dispatch tests prove exact detector policy. Language-local tests prove real extracted symbol metadata. Language tiers and golden fixtures prove existing role emission stays intact.

**Lead affected-change scope:** `cargo xtask test language python`; `cargo xtask test language scala`; `cargo xtask test language elixir`; `cargo xtask test golden`; `cargo xtask test capability`; `node scripts/language-data-quality-report.mjs --strict`.

**Branch gate:** `cargo fmt --check`; `git diff --check`; `cargo xtask test default`.

**Security scope:** none declared.

**Replay/metric evidence:** `silent_cells = 0` and `quality_bar_debts = 0` are hard gates. `open_gap_backlog` is report-only and must not increase from this slice.

**Escalation triggers:** Any capability snapshot change, artifact schema/JSONL change, or lost existing positive role requires stopping as a plan mismatch. Unexpected failures outside the three languages require lead diagnosis before broadening scope.

**Assigned verification failure:** Workers stop and report when assigned verification fails, unless this plan explicitly says to update that gate.

**Verification ledger:** Record invariant, command, scope label, commit SHA, result, and timestamp. Reuse a passing entry only for the same HEAD and scope.

## Parallel Execution Contract

| Task | Parallel batch | File ownership | Serialization required | Dependency reason |
|---|---|---|---|---|
| Task 1: Harden callable and decorator evidence | None - serial | Modify `crates/julie-extractors/src/test_detection.rs`, `crates/julie-extractors/src/tests/test_detection.rs`, `crates/julie-extractors/src/tests/python/test_detection.rs`, `crates/julie-extractors/src/tests/scala/test_detection.rs`, and `crates/julie-extractors/src/tests/elixir/test_detection.rs`; update this plan's checkboxes and write only the assigned Razorback report/ledger files. | Not applicable - single task. | Not applicable - single task. |

### Task 1: Harden callable and decorator evidence

**Files:**
- Modify: `crates/julie-extractors/src/test_detection.rs:119-153,527-530`
- Test: `crates/julie-extractors/src/tests/test_detection.rs`
- Test: `crates/julie-extractors/src/tests/python/test_detection.rs`
- Test: `crates/julie-extractors/src/tests/scala/test_detection.rs`
- Test: `crates/julie-extractors/src/tests/elixir/test_detection.rs`

**Interfaces:**
- Consumes: `is_test_symbol(language, name, file_path, kind, annotation_keys, doc_comment) -> bool`, normalized Python decorator keys, and existing call-style test symbol extraction.
- Produces: the same boolean interface with stricter positive evidence; no new exported symbol or artifact field.

**Contract inputs:** The exact detector boundaries and official framework URLs in the design document; existing `test_case`, `test_container`, and `test_lifecycle` golden output is invariant.

**File ownership:** Modify `crates/julie-extractors/src/test_detection.rs`, `crates/julie-extractors/src/tests/test_detection.rs`, `crates/julie-extractors/src/tests/python/test_detection.rs`, `crates/julie-extractors/src/tests/scala/test_detection.rs`, and `crates/julie-extractors/src/tests/elixir/test_detection.rs`; update this plan's checkboxes and write only the assigned Razorback report/ledger files.

**Serialization required:** Not applicable - single task.

**Dependency reason:** Not applicable - single task.

**What to build:** Add failing shared and full-extraction tests for path-only Scala/Elixir helpers and Python fixture/mock decorators. Then minimally narrow `detect_scala`, `detect_elixir`, and Python annotation detection while leaving all call-style adapters, lifecycle names, artifact mapping, and capabilities unchanged.

**Approach:** Introduce one private Python annotation predicate if it makes the allowlist clearer. Remove path-only positive branches from Scala and Elixir callable detection. Keep the implementation language-local inside the existing detector rather than adding framework configuration or changing extractor call sites.

**Acceptance criteria:**
- [x] New regression tests were observed failing before production edits and the report records the expected failures.
- [x] Ordinary Scala methods in `src/test/scala/...` and ordinary Elixir functions in `test/...` remain symbols without `is_test = true`.
- [x] Existing Scala/Elixir DSL cases, containers, and lifecycle hooks retain their role metadata.
- [x] `pytest.mark.parametrize` remains positive outside a conventional test path.
- [x] `pytest.fixture` and `unittest.mock.patch` helpers remain non-test symbols in a conventional test file.
- [x] `unittest.skip`, `unittest.skipIf`, `unittest.skipUnless`, and `unittest.expectedFailure` remain positive annotation evidence.
- [x] No artifact schema, JSONL, capability, fixture registration, or public API shape changes.
- [x] Worker-scope verification passes and the worker commits the owned files with `serial-worker-commit`.
