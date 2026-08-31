# BRE-43 Go t.Run subtests implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Emit deterministic child test-case symbols for literal Go `t.Run` subtests without weakening Ginkgo or testify detection.

**Architecture:** Keep Ginkgo recognition import-gated and add a separate standard-library subtest collector in `go/test_calls.rs`. Route each `call_expression` through a private dispatcher that returns either a Ginkgo symbol or a validated `t.Run` child symbol under the enclosing test.

**Tech Stack:** Rust, tree-sitter-go, Julie test-role metadata, canonical fixtures.

**Architecture Quality:** Existing symbol and test-role interfaces remain unchanged. The new behavior stays local to Go call extraction; architecture risk is low.

## Global Constraints

- Follow `docs/plans/2026-08-30-extractor-gap-closure-design.md` and Linear BRE-43.
- Match only selector calls with exact member name `Run`, a receiver bound to an active `*testing.T` parameter, a literal first argument, a function-literal callback, and an enclosing test symbol.
- Do not guess dynamic names or turn unrelated `Run` calls into tests.
- Preserve Ginkgo import gating, scoped-role normalization, and testify behavior.
- Miller owns runtime `go test -run Parent/child` selector construction.
- Append `.go-subtests-v1` to `EXTRACTION_CONTRACT_VERSION` because canonical symbols change.
- Close capability gap `go.subtest_names` only after positive and negative fixture evidence exists.
- `node scripts/language-data-quality-report.mjs --strict` must report `silent_cells = 0` and `quality_bar_debts = 0`.

---

## Verification Strategy

**Project source of truth:** `AGENTS.md`, `docs/testing-strategy.md`, `docs/languages/go.md`, `fixtures/extraction/capabilities.json`, and the approved design.

**Worker red/green scope:** Add exact tests in `tests::go::test_detection`, run each new test by full name, then run `cargo test -p julie-extractors tests::go::test_detection -- --nocapture`.

**Worker ceiling:** `cargo xtask test language go`, `cargo xtask test golden`, and `cargo xtask test capability`.

**Worker gate invariant:** Only statically named standard-library subtests under a real enclosing test emit child `test_case` symbols; existing Go test frameworks remain byte-stable outside intended fixture additions.

**Lead affected-change scope:** `cargo xtask test language go`; `cargo xtask test golden`; `cargo xtask test capability`; `node scripts/language-data-quality-report.mjs --strict`.

**Branch gate:** `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets --all-features --no-deps -- -D warnings`; `cargo test -p xtask`; `cargo xtask test default`; `cargo xtask test contract`; `git diff --check`.

**Security scope:** none declared.

**Replay/metric evidence:** Positive, nested, and negative golden rows are hard gates. The 363-site corpus count is report-only.

**Escalation triggers:** If tree-sitter-go does not expose the receiver, argument list, or callback in stable named fields, report the grammar evidence before changing the accepted recognition contract.

**Assigned verification failure:** Workers stop and report when assigned verification fails, unless this plan explicitly says to update that gate.

**Verification ledger:** Record invariant, command, scope label, commit SHA, result, and timestamp. Reuse a passing entry for the same HEAD and scope.

## Parallel Execution Contract

| Task | Parallel batch | File ownership | Serialization required | Dependency reason |
|---|---|---|---|---|
| Task 1: Extract and prove Go subtest symbols | None - serial | `crates/julie-extractors/src/go/test_calls.rs`, `crates/julie-extractors/src/go/mod.rs`, `crates/julie-extractors/src/tests/go/test_detection.rs`, `fixtures/extraction/go/test_roles/**`, `fixtures/extraction/capabilities.json`, `docs/languages/go.md`, `crates/julie-extractors/src/lib.rs`, `crates/julie-extractors/src/tests/api_surface.rs` | Not applicable - single task. | Not applicable - single task. |

### Task 1: Extract and prove Go subtest symbols

**Files:**
- Modify: `crates/julie-extractors/src/go/test_calls.rs:47-173`
- Modify: `crates/julie-extractors/src/go/mod.rs:77-137`
- Modify: `crates/julie-extractors/src/go/mod.rs:226-297`
- Modify: `crates/julie-extractors/src/tests/go/test_detection.rs:30-291`
- Modify: `fixtures/extraction/go/test_roles/source_test.go`
- Modify: `fixtures/extraction/go/test_roles/expected.json`
- Modify: `fixtures/extraction/capabilities.json`
- Modify: `docs/languages/go.md:108-116`
- Modify: `crates/julie-extractors/src/lib.rs:130`
- Modify: `crates/julie-extractors/src/tests/api_surface.rs:14-51`

**Interfaces:**
- Consumes: Go `call_expression` nodes, the current enclosing symbol id from `GoExtractor::walk_tree`, `build_test_call_symbol`, and existing test-role metadata.
- Produces: one child symbol with `test_role = "test_case"`, a deterministic id/span, and `parent_id` equal to the enclosing test or subtest.

**Contract inputs:** Exact AST evidence for selector expression, argument list, interpreted string literal, function literal, a lexically active receiver parameter typed `*testing.T`, and enclosing symbol test metadata.

**File ownership:** `crates/julie-extractors/src/go/test_calls.rs`, `crates/julie-extractors/src/go/mod.rs`, `crates/julie-extractors/src/tests/go/test_detection.rs`, `fixtures/extraction/go/test_roles/**`, `fixtures/extraction/capabilities.json`, `docs/languages/go.md`, `crates/julie-extractors/src/lib.rs`, `crates/julie-extractors/src/tests/api_surface.rs`

**Serialization required:** Not applicable - single task.

**Dependency reason:** Not applicable - single task.

**What to build:** Add an `extract_standard_subtest_call` helper beside `extract_ginkgo_test_call` and a Go extractor dispatcher that tries the standard-library shape independently of Ginkgo enablement. Let the normal tree walk make a recognized subtest the parent of nested child calls.

**Approach:** Start with failing tests for one literal child, one nested child, and a valid `*testing.T` parameter named something other than `t`. Add controls for a dynamic name, a receiver not bound to `*testing.T`, an ordinary `Run` method, an incorrect callback, and a file-scope call. Validate the active receiver binding and enclosing symbol's test role before materialization, decode only a literal first argument, and reuse the established symbol builder rather than adding a new symbol vocabulary. Regenerate the Go test-role golden and inspect all changed rows.

**Acceptance criteria:**
- [x] A literal `t.Run` inside `TestXxx` emits one deterministic child `test_case` symbol.
- [x] Nested literal subtests preserve parent-child identity.
- [x] Receiver variable spelling is irrelevant when it is bound to `*testing.T`; dynamic names, unrelated receiver types, incorrect callback shapes, and calls outside an enclosing test remain silent.
- [x] Existing Ginkgo and testify focused tests and golden rows remain unchanged.
- [x] Capability gap `go.subtest_names` is removed and fixture evidence names the test-role golden.
- [x] `docs/languages/go.md` describes supported literal subtests and remaining dynamic-name limits.
- [x] `EXTRACTION_CONTRACT_VERSION` contains `go-subtests-v1` and its API-surface test passes.
- [ ] Golden, capability, strict data-quality, affected-change, and branch gates pass.
- [x] Worker-scope verification passes and the change is committed per `serial-worker-commit`.
