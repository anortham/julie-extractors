# QML First-Class Extraction Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Make QML, `qmldir`, `.qmltypes`, and Qt Quick Test semantics complete, versioned extraction-artifact citizens.

**Architecture:** Grammar-specific logic stays in focused QML and qmldir modules and publishes only existing artifact families plus registered structural-fact kinds. Import symbols are the generic resolver input; object instantiations use either a resolved relationship or one structured pending relationship, never duplicate channels.

**Tech Stack:** Rust, tree-sitter, `tree-sitter-qmljs`, `tree-sitter-qmldir`, SQLite/JSONL extraction artifacts, golden JSON fixtures, cargo-nextest, Node.js quality reporting.

**Architecture Quality:** The caller-facing interface remains the versioned artifact. The main risk is ambiguous or duplicated downstream evidence, controlled by normalized import metadata, one instantiation-resolution channel, registry contracts, and end-to-end goldens. Architecture risk: medium.

## Global Constraints

- `julie-extract` is the primary integration surface; SQLite is primary and JSONL is secondary.
- Do not add workspace-global resolution, MCP, daemon, search, watcher, dashboard, or editing behavior.
- Pin `tree-sitter-qmldir` to `c57e00865a1a6f1cca83340d6dad91f13df55479` from `https://github.com/tree-sitter-grammars/tree-sitter-qmldir`.
- Keep `tree-sitter-qmljs` at its current semantic pin unless certification proves a newer pin changes supported syntax safely.
- Import symbols use the existing metadata keys `source`, `alias`, `local_name`, `imported_name`, and `is_namespace`.
- Structural facts do not become a second generic import-binding input.
- Each object use emits either one local concrete `instantiates` relationship or one structured pending `instantiates` relationship.
- `.qmltypes` uses the existing extractor file-size limit; no generated-file bypass.
- `test_*_data` and `init_data` are non-test functions; `benchmark_*` and `benchmark_once_*` are runnable tests.
- Default tests remain corpus-free and fast; real-world and Windows gates stay explicit.
- Capability changes must keep `silent_cells=0` and `quality_bar_debts=0` under the strict report.
- Use TDD for every behavior change and preserve unrelated working-tree changes.

---

## Architecture Quality

**Affected modules:** `language_spec`, registry dispatch, QML/qmldir extractors, structural-fact registry, test detection, golden harness, capability matrix, and xtask tiers.

**Caller-facing interface:** versioned symbols, type facts, relationships, structured pending relationships, test roles, and structural facts emitted by `julie-extract`.

**Depth/locality check:** QML traversal delegates imports, typeinfo, and relationships to focused modules. `qmldir` parsing is isolated from QML source parsing. Shared code only registers language/fact contracts and routes tests.

**Test surface:** artifact goldens and public extractor entry points, not private traversal helpers alone.

**Seams/adapters:** existing import metadata and pending-relationship contracts absorb new semantics. New fact kinds are added only for domain data that has no existing typed row.

**Rejected shortcuts:** regex-only `qmldir`, directory-name module inference, language-specific downstream conventions hidden in display names, duplicate instantiation edges, and capability claims without goldens.

**Architecture risk:** medium.

## Verification Strategy

**Project source of truth:** `AGENTS.md`, `README.md`, `fixtures/extraction/capabilities.json`, `xtask/src/test_tiers.rs`, and the repository release/test scripts.

**Worker red/green scope:** Run the exact QML/qmldir unit test module or one golden fixture first, then `cargo xtask test language qml` or `cargo xtask test language qmldir` for the owned slice.

**Worker ceiling:** Focused language tests, focused registry/contract tests, and the assigned per-language command. Workers do not run the full default, all-golden, real-world, or Windows suites.

**Worker gate invariant:** Focused tests prove normalized facts, exact relationship cardinality, role classification, parser registration, and deterministic golden output for the owned behavior.

**Lead affected-change scope:** After each coherent batch, run `cargo xtask test language qml`, `cargo xtask test language qmldir`, `cargo xtask test golden`, `cargo xtask test capability`, `cargo xtask test contract`, and `node scripts/language-data-quality-report.mjs --strict`.

**Branch gate:** Run `cargo xtask test default`, `cargo xtask test golden`, `cargo xtask test capability`, `cargo xtask test contract`, and `node scripts/language-data-quality-report.mjs --strict` once at the final HEAD.

**Security scope:** `security-secrets` and `security-deps` through `razorback:security-review`; dependency review includes the new git-pinned parser, license, source, and lockfile.

**Replay/metric evidence:** Hard gates are exact goldens, zero duplicated instantiation edges, correct Quick Test role positives/negatives, parser error-free fixtures, and strict report values `silent_cells=0` and `quality_bar_debts=0`. Real-world file counts and parser error rates are report-only until existing certification policy defines a threshold.

**Escalation triggers:** Parser dependency or grammar changes require real-world certification and downstream packaging smoke. Path, file lifecycle, or CLI artifact changes require a clean-SHA Windows run: `win-test sync julie-extractors` then `win-test run julie-extractors -- cargo xtask test default`.

**Assigned verification failure:** Workers stop and report when assigned verification fails, unless this plan explicitly says to update that gate.

**Verification ledger:** Record invariant, command, scope label, commit SHA, result, and timestamp. For replay or metric evidence, also record hard-gate metrics and report-only metrics. If the same HEAD already has a passing ledger entry for the required scope, reuse that evidence instead of rerunning the same expensive gate.

## Parallel Execution Contract

| Task | Parallel batch | File ownership | Serialization required | Dependency reason |
|---|---|---|---|---|
| Task 1: Register and extract `qmldir` | Batch A | `crates/julie-extractors/Cargo.toml`; `Cargo.lock`; `crates/julie-extractors/src/language_spec/mod.rs`; `crates/julie-extractors/src/language_spec/specs.rs`; `crates/julie-extractors/src/registry.rs`; create `crates/julie-extractors/src/qmldir/**`; create `crates/julie-extractors/src/tests/qmldir/**`; create `languages/qmldir.toml`; dependency policy files touched by the pin | No | None - safe parallel batch. |
| Task 2: Normalize QML imports and type metadata | Batch A | `crates/julie-extractors/src/qml/mod.rs`; create `crates/julie-extractors/src/qml/imports.rs`; create `crates/julie-extractors/src/qml/typeinfo.rs`; `crates/julie-extractors/src/tests/qml/types.rs`; create `crates/julie-extractors/src/tests/qml/imports.rs`; create `crates/julie-extractors/src/tests/qml/typeinfo.rs`; `crates/julie-extractors/src/tests/qml/mod.rs` | No | None - safe parallel batch. |
| Task 3: Make instantiations and Qt Quick Test roles exact | Batch A | `crates/julie-extractors/src/qml/relationships.rs`; `crates/julie-extractors/src/test_detection.rs`; `crates/julie-extractors/src/tests/qml/relationships.rs`; create `crates/julie-extractors/src/tests/qml/test_detection.rs`; QML real-world feature tests covering roles | No | None - safe parallel batch. |
| Task 4: Register domain facts and build multi-file goldens | None - serial | `crates/julie-extractors/src/base/code_structural_facts.rs`; `crates/julie-extractors/src/base/structural_fact_registry/builtins/extra.rs`; structural-fact registry tests; `fixtures/extraction/qml/**`; create `fixtures/extraction/qmldir/**`; `fixtures/extraction/capabilities.json` | Yes | Integrates the extractor outputs from Tasks 1-3 into public fact schemas and authoritative goldens. |
| Task 5: Make per-language and certification gates complete | None - serial | `xtask/src/test_tiers.rs`; `xtask/tests/test_tiers.rs`; golden harness files required for language filtering; QML/qmldir certification fixtures or manifests; QML support documentation and dependency freshness records | Yes | Depends on the final fixture names, language registrations, and capability rows from Task 4. |

Commit mode: Tasks 1-3 use `parallel-lead-commit`; Tasks 4-5 use `serial-worker-commit` after lead inline review and assigned verification.

### Task 1: Register and extract `qmldir`

**Files:**
- Modify: `crates/julie-extractors/Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `crates/julie-extractors/src/language_spec/mod.rs`
- Modify: `crates/julie-extractors/src/language_spec/specs.rs`
- Modify: `crates/julie-extractors/src/registry.rs`
- Create: `crates/julie-extractors/src/qmldir/mod.rs`
- Create: `crates/julie-extractors/src/tests/qmldir/mod.rs`
- Create: `languages/qmldir.toml`
- Modify: dependency policy and downstream smoke files identified by Miller before implementation

**Interfaces:**
- Consumes: tree-sitter `Language` dispatch, extractor registry contract, symbol/structural-fact builders, and the verified `tree-sitter-qmldir` Rust binding.
- Produces: language id `qmldir`, basename detection for `qmldir`, module/type symbols, and typed manifest facts consumed by Task 4 and Miller.

**Contract inputs:** Qt `qmldir` directives from <https://doc.qt.io/qt-6/qtqml-modules-qmldir.html>; grammar pin `c57e00865a1a6f1cca83340d6dad91f13df55479`.

**File ownership:** `crates/julie-extractors/Cargo.toml`; `Cargo.lock`; `crates/julie-extractors/src/language_spec/mod.rs`; `crates/julie-extractors/src/language_spec/specs.rs`; `crates/julie-extractors/src/registry.rs`; create `crates/julie-extractors/src/qmldir/**`; create `crates/julie-extractors/src/tests/qmldir/**`; create `languages/qmldir.toml`; dependency policy files touched by the pin

**Serialization required:** No

**Dependency reason:** None - safe parallel batch.

**What to build:** Add the pinned parser, language detection, extractor dispatch, and semantic rows for every grammar-supported manifest directive. Named component declarations become symbols; module-wide declarations remain typed facts with exact spans.

**Approach:** Use tree-sitter nodes rather than line regexes. Prove basename detection, malformed-line recovery, versions, singleton/internal flags, JavaScript resources, plugins, typeinfo, imports, depends, prefer, and designer directives through the public extractor.

**Acceptance criteria:**
- [ ] Extensionless `qmldir` files select the new parser and extractor deterministically.
- [ ] All supported directives emit bounded typed rows with exact source spans.
- [ ] The parser dependency is pinned, licensed, locked, and covered by downstream smoke policy.
- [ ] Worker-scope verification passes and the change is handed to the lead per `parallel-lead-commit`.

### Task 2: Normalize QML imports and type metadata

**Files:**
- Modify: `crates/julie-extractors/src/qml/mod.rs`
- Create: `crates/julie-extractors/src/qml/imports.rs`
- Create: `crates/julie-extractors/src/qml/typeinfo.rs`
- Modify: `crates/julie-extractors/src/tests/qml/types.rs`
- Create: `crates/julie-extractors/src/tests/qml/imports.rs`
- Create: `crates/julie-extractors/src/tests/qml/typeinfo.rs`
- Modify: `crates/julie-extractors/src/tests/qml/mod.rs`

**Interfaces:**
- Consumes: current `QmlExtractor`, symbol metadata, type facts, and `.qml` parser dispatch.
- Produces: normalized import symbols and `.qmltypes` module/type/member evidence for artifact consumers.

**Contract inputs:** Import metadata keys `source`, `alias`, `local_name`, `imported_name`, `is_namespace`; existing extractor file-size ceiling; Qt module/type metadata conventions.

**File ownership:** `crates/julie-extractors/src/qml/mod.rs`; create `crates/julie-extractors/src/qml/imports.rs`; create `crates/julie-extractors/src/qml/typeinfo.rs`; `crates/julie-extractors/src/tests/qml/types.rs`; create `crates/julie-extractors/src/tests/qml/imports.rs`; create `crates/julie-extractors/src/tests/qml/typeinfo.rs`; `crates/julie-extractors/src/tests/qml/mod.rs`

**Serialization required:** No

**Dependency reason:** None - safe parallel batch.

**What to build:** Separate QML import parsing from general traversal and make every import machine-readable. Register `.qmltypes` as QML-family input and extract tooling types and members without embedding whole generated source blocks.

**Approach:** Derive fields from tree-sitter nodes and preserve absent fields as absent, not guessed values. Add URI, directory, versioned, aliased, JavaScript, malformed, and `.qmltypes` size-limit tests.

**Acceptance criteria:**
- [ ] Every supported import form has correct normalized metadata on the import symbol.
- [ ] `qml.import_statement.v1` values agree with the corresponding import symbol.
- [ ] `.qmltypes` emits module/type/member/revision evidence under the normal size ceiling.
- [ ] Worker-scope verification passes and the change is handed to the lead per `parallel-lead-commit`.

### Task 3: Make instantiations and Qt Quick Test roles exact

**Files:**
- Modify: `crates/julie-extractors/src/qml/relationships.rs`
- Modify: `crates/julie-extractors/src/test_detection.rs`
- Modify: `crates/julie-extractors/src/tests/qml/relationships.rs`
- Create: `crates/julie-extractors/src/tests/qml/test_detection.rs`
- Modify: QML real-world feature tests located by Miller before implementation

**Interfaces:**
- Consumes: extracted QML component symbols, structured pending relationship builders, function symbols, file paths, and containing `TestCase` evidence.
- Produces: exact `instantiates` relationship cardinality and Qt Quick Test roles compatible with existing artifact role vocabulary.

**Contract inputs:** `test_*`, `_data`, `benchmark_*`, `benchmark_once_*`, lifecycle names, `tst_*.qml`, and `TestCase` semantics from Qt 6 documentation.

**File ownership:** `crates/julie-extractors/src/qml/relationships.rs`; `crates/julie-extractors/src/test_detection.rs`; `crates/julie-extractors/src/tests/qml/relationships.rs`; create `crates/julie-extractors/src/tests/qml/test_detection.rs`; QML real-world feature tests covering roles

**Serialization required:** No

**Dependency reason:** None - safe parallel batch.

**What to build:** Replace the ineffective nested-class instantiation path with explicit QML object-use evidence. Tighten test detection so data helpers are negative controls and both benchmark families are runnable tests.

**Approach:** Resolve only targets proved local; emit one structured pending relationship otherwise. Require Quick Test context before method-name classification and keep data helpers as ordinary non-test functions.

**Acceptance criteria:**
- [ ] Local and external component uses each emit exactly one authoritative instantiation edge.
- [ ] Pending instantiations retain target and normalized import context for Miller.
- [ ] Tests, lifecycle methods, data helpers, benchmarks, and application false positives classify exactly as designed.
- [ ] Worker-scope verification passes and the change is handed to the lead per `parallel-lead-commit`.

### Task 4: Register domain facts and build multi-file goldens

**Files:**
- Modify: `crates/julie-extractors/src/base/code_structural_facts.rs`
- Modify: `crates/julie-extractors/src/base/structural_fact_registry/builtins/extra.rs`
- Modify: structural-fact registry tests identified by Miller
- Replace/expand: `fixtures/extraction/qml/cross_file/**`
- Modify: `fixtures/extraction/qml/basic/**`
- Modify: `fixtures/extraction/qml/test_roles/**`
- Create: `fixtures/extraction/qml/typeinfo/**`
- Create: `fixtures/extraction/qmldir/basic/**`
- Modify: `fixtures/extraction/capabilities.json`

**Interfaces:**
- Consumes: Task 1 manifest facts, Task 2 import/typeinfo rows, and Task 3 relationship/test-role behavior.
- Produces: registered versioned fact schemas and authoritative multi-file artifact evidence used by Julie and Miller integration tests.

**Contract inputs:** no duplicated resolver channels; all positive capability claims require a named registered golden.

**File ownership:** `crates/julie-extractors/src/base/code_structural_facts.rs`; `crates/julie-extractors/src/base/structural_fact_registry/builtins/extra.rs`; structural-fact registry tests; `fixtures/extraction/qml/**`; create `fixtures/extraction/qmldir/**`; `fixtures/extraction/capabilities.json`

**Serialization required:** Yes

**Dependency reason:** Integrates the extractor outputs from Tasks 1-3 into public fact schemas and authoritative goldens.

**What to build:** Register only durable domain-native fact kinds, then build a real module fixture with multiple QML files, `qmldir`, `.qmltypes`, local/external components, aliases, bindings, and Quick Test cases. Update capability evidence from the generated artifact.

**Approach:** Use narrow facts for imports, object instantiations, manifest declarations, and typeinfo declarations. Generate expected artifacts through the repository golden workflow, inspect diffs, then lock them.

**Acceptance criteria:**
- [ ] Every new fact kind has a fixed versioned schema and registry contract test.
- [ ] The cross-file golden contains multiple physical source files and proves local plus unresolved module behavior.
- [ ] Capabilities cite the new fixtures and record any remaining implementation gap as `open_gaps` with closure details.
- [ ] Golden, capability, contract, and strict quality gates pass; the worker commits per `serial-worker-commit`.

### Task 5: Make per-language and certification gates complete

**Files:**
- Modify: `xtask/src/test_tiers.rs`
- Modify: `xtask/tests/test_tiers.rs`
- Modify: golden harness files required for a language-family filter, identified with Miller before editing
- Modify/Create: QML/qmldir certification manifests and support documentation identified with Miller

**Interfaces:**
- Consumes: final QML/qmldir fixture registry and language ids from Task 4.
- Produces: narrow commands that run unit plus family golden tests, and explicit slow certification evidence for grammar/dependency changes.

**Contract inputs:** `cargo xtask test language qml`; `cargo xtask test language qmldir`; existing default/golden/capability/contract tier semantics.

**File ownership:** `xtask/src/test_tiers.rs`; `xtask/tests/test_tiers.rs`; golden harness files required for language filtering; QML/qmldir certification fixtures or manifests; QML support documentation and dependency freshness records

**Serialization required:** Yes

**Dependency reason:** Depends on the final fixture names, language registrations, and capability rows from Task 4.

**What to build:** Extend the language tier to include only that language family's registered goldens and document the parser recertification path. Keep corpus work out of default tests and make missing optional tooling explicit rather than silently passing.

**Approach:** Add test-plan assertions before changing xtask dispatch. Prove QML and qmldir filter independently, then run branch gates and the clean-SHA Windows gate when triggered.

**Acceptance criteria:**
- [ ] Each per-language command runs its unit tests and only its family goldens.
- [ ] Default tests do not execute real-world corpora or parser certification.
- [ ] Grammar/dependency changes have recorded provenance, freshness, packaging, and real-world evidence.
- [ ] Branch-scope verification passes and the worker commits per `serial-worker-commit`.

## Execution Handoff

- The user reviews and approves this plan before implementation begins.
- Create or reuse a dedicated task worktree after approval; preserve the current task-related Goldfish files.
- Execute with `razorback:subagent-driven-development`, TDD, Miller orientation/impact checks, inline lead review, and the commit modes above.
