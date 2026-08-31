# BRE-53 Rust doc-test facts implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Emit `rust.doc_test.v1` structural facts for executable rustdoc fences while preserving comments as comments rather than symbols.

**Architecture:** Add a Rust-only structural-fact collector that scans tree-sitter doc-comment nodes, parses fenced blocks with byte-accurate spans, attaches facts to existing documented symbols, and joins the standard structural-fact pipeline before the final deterministic sort.

**Tech Stack:** Rust, tree-sitter-rust, Julie `StructuralFact`, structural-fact registry, canonical fixtures.

**Architecture Quality:** The existing artifact interface is reused with one new versioned pattern id. Rustdoc parsing remains in one language-local collector; architecture risk is medium because fence spans cross comment-node text and containing-symbol attachment.

## Global Constraints

- Follow `docs/plans/2026-08-30-extractor-gap-closure-design.md` and Linear BRE-53.
- Emit structural facts only; do not create symbols or `test_role` metadata for comments.
- Pattern id is exactly `rust.doc_test.v1`.
- Supported modes are `run`, `ignore`, `no_run`, and `compile_fail`.
- Untagged fences and `rust` fences are executable; `text` and explicit non-Rust fences are silent.
- Preserve exact byte and line spans for each fence, including multiple fences in one doc block.
- Append `.rust-doc-test-facts-v1` to `EXTRACTION_CONTRACT_VERSION`.
- Close capability gap `rust.doc_test_cases` only after registry, fixture, and golden evidence agree.
- `node scripts/language-data-quality-report.mjs --strict` must report `silent_cells = 0` and `quality_bar_debts = 0`.

---

## Verification Strategy

**Project source of truth:** `AGENTS.md`, `docs/testing-strategy.md`, `docs/languages/rust.md`, the structural-fact registry tests, capability matrix, and approved design.

**Worker red/green scope:** Add exact collector tests under `tests::rust::doc_tests` and run them by full name, then run `cargo test -p julie-extractors tests::rust::doc_tests -- --nocapture`.

**Worker ceiling:** `cargo xtask test language rust`, `cargo test -p julie-extractors structural_fact_registry -- --nocapture`, `cargo xtask test golden`, and `cargo xtask test capability`.

**Worker gate invariant:** Every emitted fact has the expected fence span, mode, containing symbol, deterministic id, and registered metadata shape; excluded fences emit nothing.

**Lead affected-change scope:** `cargo xtask test language rust`; `cargo test -p julie-extractors structural_fact_registry -- --nocapture`; `cargo xtask test golden`; `cargo xtask test capability`; `node scripts/language-data-quality-report.mjs --strict`.

**Branch gate:** `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets --all-features --no-deps -- -D warnings`; `cargo test -p xtask`; `cargo xtask test default`; `cargo xtask test contract`; `git diff --check`.

**Security scope:** none declared.

**Replay/metric evidence:** Exact structural-fact golden rows are hard gates. The number of future Miller-selected doctests is report-only and outside this repository.

**Escalation triggers:** Escalate if tree-sitter comment nodes do not preserve enough source range information for exact fence spans, or if rustdoc attribute combinations require a metadata shape beyond the approved `mode` contract.

**Assigned verification failure:** Workers stop and report when assigned verification fails, unless this plan explicitly says to update that gate.

**Verification ledger:** Record invariant, command, scope label, commit SHA, result, and timestamp. Reuse a passing entry for the same HEAD and scope.

## Parallel Execution Contract

| Task | Parallel batch | File ownership | Serialization required | Dependency reason |
|---|---|---|---|---|
| Task 1: Emit and publish Rust doc-test facts | None - serial | `crates/julie-extractors/src/base/rust_doc_test_facts.rs`, `crates/julie-extractors/src/base/mod.rs`, `crates/julie-extractors/src/registry.rs`, `crates/julie-extractors/src/base/structural_fact_registry/builtins/core.rs`, `crates/julie-extractors/src/tests/rust/**`, `fixtures/extraction/rust/structural_facts/**`, `fixtures/extraction/capabilities.json`, `docs/languages/rust.md`, `crates/julie-extractors/src/lib.rs`, `crates/julie-extractors/src/tests/api_surface.rs` | Not applicable - single task. | Not applicable - single task. |

### Task 1: Emit and publish Rust doc-test facts

**Files:**
- Create: `crates/julie-extractors/src/base/rust_doc_test_facts.rs`
- Modify: `crates/julie-extractors/src/base/mod.rs`
- Modify: `crates/julie-extractors/src/registry.rs:769-865`
- Modify: `crates/julie-extractors/src/base/structural_fact_registry/builtins/core.rs`
- Create: `crates/julie-extractors/src/tests/rust/doc_tests.rs`
- Modify: `crates/julie-extractors/src/tests/rust/mod.rs`
- Modify: `fixtures/extraction/rust/structural_facts/source.rs`
- Modify: `fixtures/extraction/rust/structural_facts/expected.json`
- Modify: `fixtures/extraction/capabilities.json`
- Modify: `docs/languages/rust.md:132-156`
- Modify: `crates/julie-extractors/src/lib.rs:130`
- Modify: `crates/julie-extractors/src/tests/api_surface.rs:14-51`

**Interfaces:**
- Consumes: `collect_rust_doc_test_facts(language, tree, file_path, content, symbols)`, existing `StructuralFact`, `stable_location_id`, containing-symbol attachment rules, and final `sort_structural_facts`.
- Produces: `rust.doc_test.v1` facts with `capture_name = "doc_test"`, synthetic `node_kind = "rustdoc_fence"`, exact fence span, optional `containing_symbol_id`, confidence `1.0`, and metadata `{ "mode": <run|ignore|no_run|compile_fail> }`.

**Contract inputs:** Rustdoc fence attribute semantics, exact source bytes, existing structural-fact registry metadata typing, and current source-region doc-comment classification.

**File ownership:** `crates/julie-extractors/src/base/rust_doc_test_facts.rs`, `crates/julie-extractors/src/base/mod.rs`, `crates/julie-extractors/src/registry.rs`, `crates/julie-extractors/src/base/structural_fact_registry/builtins/core.rs`, `crates/julie-extractors/src/tests/rust/**`, `fixtures/extraction/rust/structural_facts/**`, `fixtures/extraction/capabilities.json`, `docs/languages/rust.md`, `crates/julie-extractors/src/lib.rs`, `crates/julie-extractors/src/tests/api_surface.rs`

**Serialization required:** Not applicable - single task.

**Dependency reason:** Not applicable - single task.

**What to build:** Walk Rust doc-comment nodes, normalize only the comment prefix needed for fence recognition while retaining a source-offset map, pair opening and closing fences, classify rustdoc attributes, and build one structural fact per executable fence. Add the collector to `extract_for_language_at` only for Rust and register the exact pattern metadata.

**Approach:** Begin with failing tests for an outer-doc run fence, inner-doc run fence, multiple fences, and each mode. Add negative controls for `text`, another language, an unterminated fence, and ordinary comments. Derive line, column, and byte positions from mapped source offsets rather than reconstructed text lengths. Reuse normal containing-symbol attachment where the fact span is inside the documented declaration; handle file/module inner docs explicitly without fabricating a symbol. Regenerate the Rust structural-fact golden and inspect every new row.

**Acceptance criteria:**
- [x] Executable untagged and Rust fences emit deterministic `rust.doc_test.v1` facts.
- [x] `ignore`, `no_run`, and `compile_fail` emit the exact corresponding `mode`; ordinary executable fences emit `run`.
- [x] `text`, explicit non-Rust, ordinary-comment, and unterminated fences remain silent.
- [x] Multiple fences retain distinct ids and exact source spans.
- [x] Outer docs attach to the documented symbol; inner docs use the nearest valid module/file context without a fabricated callable.
- [x] Registry metadata, emitted facts, capability claims, and golden evidence agree.
- [x] `EXTRACTION_CONTRACT_VERSION` contains `rust-doc-test-facts-v1` and its API-surface test passes.
- [x] Golden, capability, strict data-quality, affected-change, and branch gates pass.
- [x] Worker-scope verification passes and the change is committed per `serial-worker-commit`.
