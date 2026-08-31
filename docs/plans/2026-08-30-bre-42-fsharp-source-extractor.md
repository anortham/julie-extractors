# BRE-42 F# source extractor implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Add evidence-backed F# extraction for `.fs`, `.fsx`, and `.fsi` while publishing one stable artifact language, `fsharp`.

**Architecture:** Pin `tree-sitter-fsharp` 0.3.0, select its implementation or signature parser by file path inside the existing parse pipeline, and keep the public language name unchanged. Build a dedicated F# extractor with declaration, semantic-fact, and test-role modules; publish capability claims only after canonical and real-extract evidence exists.

**Tech Stack:** Rust, Tree-sitter 0.26.11, `tree-sitter-fsharp` 0.3.0, SQLite artifacts, Julie golden fixtures and capability matrix.

**Architecture Quality:** Parser choice is hidden behind the existing language factory and normal extraction APIs. The new language is isolated in `src/fsharp/`; the only cross-cutting change is path-aware parser selection. Architecture risk is medium.

## Global Constraints

- Follow `docs/plans/2026-08-30-extractor-gap-closure-design.md` and Linear BRE-42.
- Every emitted artifact row for `.fs`, `.fsx`, and `.fsi` uses `language = "fsharp"`.
- Use `tree_sitter_fsharp::LANGUAGE_FSHARP` for `.fs` and `.fsx`; use `tree_sitter_fsharp::LANGUAGE_SIGNATURE` for `.fsi`.
- Keep public `get_tree_sitter_language("fsharp")` compatible by returning the implementation parser; path-aware selection is an internal pipeline contract.
- Pin the dependency exactly as `tree-sitter-fsharp = "=0.3.0"` and record the lockfile source.
- The verified external API is documented at <https://github.com/ionide/tree-sitter-fsharp/blob/main/bindings/rust/README.md>; do not infer different constant names.
- Do not publish a separate `fsharp_signature` language.
- Do not claim a capability without useful emitted rows and golden evidence.
- Any unsupported grammar-backed domain remains an `open_gaps` entry with reason, required closure, and planned task.
- Append `.fsharp-v1` to `EXTRACTION_CONTRACT_VERSION` when the evidence task publishes the new language.
- `node scripts/language-data-quality-report.mjs --strict` must report `silent_cells = 0` and `quality_bar_debts = 0`.
- Miller pin updates and Miller CT provider work remain separate downstream changes.

---

## Verification Strategy

**Project source of truth:** `AGENTS.md`, `docs/testing-strategy.md`, `docs/languages/new-language-checklist.md`, `docs/architecture/grammar-dependency-policy.md`, the approved design, and the current capability schema.

**Worker red/green scope:** Before fixture registration, run new tests by full name under `tests::fsharp`. After registration, run `cargo xtask test language fsharp`. Parser-plumbing tests run under `tests::pipeline` and `tests::api_surface`.

**Worker ceiling:** Exact F# and parser tests, `cargo xtask test language fsharp`, `cargo xtask test golden`, `cargo xtask test capability`, and dependency-policy checks assigned to Task 4.

**Worker gate invariant:** The selected parser matches the extension; emitted language remains `fsharp`; every claimed domain has deterministic positive and negative evidence.

**Lead affected-change scope:** After each task, run the exact F# module tests and `cargo check -p julie-extractors`. After Task 4, run `cargo xtask test language fsharp`, golden, capability, contract, strict data-quality, grammar-freshness tests, and dependency policy.

**Branch gate:** `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets --all-features --no-deps -- -D warnings`; `cargo test -p xtask`; `cargo xtask test default`; `cargo xtask test contract`; `cargo xtask test certification`; `cargo deny --all-features check`; `cargo audit`; `node --test scripts/grammar-freshness-report.test.mjs`; `node scripts/grammar-freshness-report.mjs --format json`; `node scripts/language-data-quality-report.mjs --strict`; `git diff --check`.

**Security scope:** `cargo audit` for published advisories and `cargo deny --all-features check` for advisories, bans, licenses, and sources. Run both after the parser dependency is locked.

**Replay/metric evidence:** Golden rows, zero parse diagnostics on valid fixtures, and the pinned Expecto corpus at commit `cec2c63c8d77c6c21bf7e35d903020f74ddc1cea` are hard gates. Raw row totals are report-only after every required domain has a nonzero representative count.

**Escalation triggers:** Escalate if version 0.3.0 fails against Tree-sitter 0.26.11, `.fsi` requires a caller-visible language split, valid fixture syntax produces parser errors, or Windows path behavior differs from the documented extension contract.

**Assigned verification failure:** Workers stop and report when assigned verification fails, unless this plan explicitly says to update that gate.

**Verification ledger:** Record invariant, command, scope label, commit SHA, result, and timestamp. For real extraction, also record the SQLite query and returned per-domain counts. Reuse a passing entry for the same HEAD and scope.

## Parallel Execution Contract

| Task | Parallel batch | File ownership | Serialization required | Dependency reason |
|---|---|---|---|---|
| Task 1: Register F# and extract foundational declarations | None - serial | `crates/julie-extractors/Cargo.toml`, `Cargo.lock`, `crates/julie-extractors/src/language_spec/**`, `crates/julie-extractors/src/pipeline.rs`, `crates/julie-extractors/src/registry.rs`, `crates/julie-extractors/src/lib.rs`, `crates/julie-extractors/src/fsharp/mod.rs`, `crates/julie-extractors/src/fsharp/declarations.rs`, `crates/julie-extractors/src/tests/fsharp/mod.rs`, parser/API tests | Yes | Establishes the parser and extractor interfaces consumed by every later task. |
| Task 2: Add F# semantic facts and metrics | None - serial | `crates/julie-extractors/src/fsharp/identifiers.rs`, `relationships.rs`, `types.rs`, `literals.rs`, `crates/julie-extractors/src/fsharp/mod.rs`, `crates/julie-extractors/src/base/complexity_metrics.rs`, `crates/julie-extractors/src/tests/fsharp/semantic_facts.rs` | Yes | Consumes the registered extractor and declaration ids from Task 1. |
| Task 3: Add F# xUnit test roles | None - serial | `crates/julie-extractors/src/fsharp/test_detection.rs`, `crates/julie-extractors/src/fsharp/mod.rs`, `crates/julie-extractors/src/tests/fsharp/test_detection.rs` | Yes | Test-role classification depends on stable declaration and annotation extraction from Tasks 1-2. |
| Task 4: Publish F# evidence and contracts | None - serial | `fixtures/extraction/fsharp/**`, `fixtures/extraction/capabilities.json`, `docs/languages/fsharp.md`, `crates/julie-extractors/README.md`, `crates/julie-extractors/src/lib.rs`, `crates/julie-extractors/src/tests/api_surface.rs`, `crates/julie-extractors/src/tests/capability_matrix.rs` | Yes | Capability publication follows complete behavior and owns all shared evidence files. |

### Task 1: Register F# and extract foundational declarations

**Files:**
- Modify: `crates/julie-extractors/Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `crates/julie-extractors/src/language_spec/mod.rs:7-323`
- Modify: `crates/julie-extractors/src/language_spec/specs.rs:3-331`
- Modify: `crates/julie-extractors/src/pipeline.rs:18-169`
- Modify: `crates/julie-extractors/src/registry.rs:33-865`
- Modify: `crates/julie-extractors/src/lib.rs`
- Create: `crates/julie-extractors/src/fsharp/mod.rs`
- Create: `crates/julie-extractors/src/fsharp/declarations.rs`
- Create: `crates/julie-extractors/src/tests/fsharp/mod.rs`
- Modify: `crates/julie-extractors/src/tests/mod.rs`
- Modify: `crates/julie-extractors/src/tests/pipeline.rs`
- Modify: `crates/julie-extractors/src/tests/api_surface.rs`

**Interfaces:**
- Consumes: `tree_sitter_fsharp::{LANGUAGE_FSHARP, LANGUAGE_SIGNATURE}`, `LanguageSpec`, `detect_language_for_source`, `extract_canonical_with_parse`, and the structured full-language registry macro.
- Produces: internal `get_tree_sitter_language_for_path(language: &str, file_path: &Path) -> Result<Language>`, normal `fsharp` registry entry, and foundational symbols for namespaces, modules, types, unions, records, members, functions, and values.

**Contract inputs:** Exact crate version and constant names from the official Rust binding; extensions `fs`, `fsx`, `fsi`; artifact language `fsharp`; existing symbol id, span, body hash, doc-comment, annotation, and parent-id conventions.

**File ownership:** `crates/julie-extractors/Cargo.toml`, `Cargo.lock`, `crates/julie-extractors/src/language_spec/**`, `crates/julie-extractors/src/pipeline.rs`, `crates/julie-extractors/src/registry.rs`, `crates/julie-extractors/src/lib.rs`, `crates/julie-extractors/src/fsharp/mod.rs`, `crates/julie-extractors/src/fsharp/declarations.rs`, `crates/julie-extractors/src/tests/fsharp/mod.rs`, parser/API tests

**Serialization required:** Yes.

**Dependency reason:** Establishes the parser and extractor interfaces consumed by every later task.

**What to build:** Add the exact parser dependency, path-aware parser selection, F# language registration, and a complete foundational declaration walk. Both implementation and signature grammars must flow through the same extractor and emit the same language name.

**Approach:** Write failing extension/parser tests for `.fs`, `.fsx`, `.fsi`, uppercase extensions, and unsupported suffixes. Keep the public one-argument parser lookup unchanged and use the new internal path-aware helper only in normal parse entry points. Build declaration extraction through named grammar fields and existing `BaseExtractor` builders; do not infer node shapes. Add unit fixtures in strings for each declaration kind and verify ids, spans, parents, hashes, docs, and annotations.

**Acceptance criteria:**
- [x] `tree-sitter-fsharp = "=0.3.0"` resolves with Tree-sitter 0.26.11 and the lockfile records the intended source.
- [x] `.fs` and `.fsx` use `LANGUAGE_FSHARP`; `.fsi` uses `LANGUAGE_SIGNATURE`.
- [x] Public `get_tree_sitter_language("fsharp")` returns the implementation grammar and existing callers remain source-compatible.
- [x] All three extensions emit `language = "fsharp"`.
- [x] Foundational declarations emit deterministic symbols, parentage, spans, body hashes, doc comments, and annotations.
- [x] Valid declaration tests pass with no parse diagnostics; malformed controls retain diagnostics.
- [x] Focused parser, API, and F# tests pass and the crate builds.
- [x] Task changes are committed per `serial-worker-commit`.

### Task 2: Add F# semantic facts and metrics

**Files:**
- Create: `crates/julie-extractors/src/fsharp/identifiers.rs`
- Create: `crates/julie-extractors/src/fsharp/relationships.rs`
- Create: `crates/julie-extractors/src/fsharp/types.rs`
- Create: `crates/julie-extractors/src/fsharp/literals.rs`
- Modify: `crates/julie-extractors/src/fsharp/mod.rs`
- Modify: `crates/julie-extractors/src/base/complexity_metrics.rs`
- Create: `crates/julie-extractors/src/tests/fsharp/semantic_facts.rs`

**Interfaces:**
- Consumes: stable symbol ids from Task 1, `PendingRelationship`, `StructuredPendingRelationship`, identifier occurrence contracts, `TypeFact`, `TypeArgumentUsage`, `Literal`, and generic complexity output.
- Produces: imports, calls, inheritance/interface relationships, declaration and reference identifiers, type facts/usages, literals, source-region-compatible nodes, and F# complexity contributions for branches and loops.

**Contract inputs:** Grammar-proven node kinds and fields; existing exact-span and containing-symbol rules; no workspace-global resolution.

**File ownership:** `crates/julie-extractors/src/fsharp/identifiers.rs`, `relationships.rs`, `types.rs`, `literals.rs`, `crates/julie-extractors/src/fsharp/mod.rs`, `crates/julie-extractors/src/base/complexity_metrics.rs`, `crates/julie-extractors/src/tests/fsharp/semantic_facts.rs`

**Serialization required:** Yes.

**Dependency reason:** Consumes the registered extractor and declaration ids from Task 1.

**What to build:** Fill the normal general-purpose-language fact domains using F# syntax and exact spans. Add complexity recognition for `if`, `match` clauses and guards, loops, and exception branches only where the grammar exposes stable nodes.

**Approach:** Add failing tests one fact domain at a time. Prefer structured pending relationships for qualified or imported targets and preserve unresolved facts for Miller. Keep declaration identity in symbol rows; the existing `IdentifierKind` contract is usage-only, so emit call, member, type, and variable-reference occurrences without fabricating a declaration kind. Infer types only from explicit annotations or grammar-stable literals; mark inferred facts honestly. Use shared literal and span helpers.

**Acceptance criteria:**
- [x] Imports and local/cross-file relationship candidates emit with exact caller and target evidence.
- [x] Call, member, type, and variable-reference identifiers have stable distinct occurrence ids; declarations remain canonical symbol rows.
- [x] Explicit type annotations, generic arguments, record/union fields, and literal inference emit useful type facts with honest provenance.
- [x] String, character, numeric, boolean, and unit literals use exact source spans.
- [x] Complexity counts cover grammar-stable F# branches and loops without counting pattern syntax as control flow.
- [x] Negative controls avoid guessed types, relationships, and duplicate identifiers.
- [x] Focused semantic tests pass and the crate builds.
- [x] Task changes are committed per `serial-worker-commit`.

### Task 3: Add F# xUnit test roles

**Files:**
- Create: `crates/julie-extractors/src/fsharp/test_detection.rs`
- Modify: `crates/julie-extractors/src/fsharp/mod.rs`
- Create: `crates/julie-extractors/src/tests/fsharp/test_detection.rs`

**Interfaces:**
- Consumes: extracted F# attributes and functions from Tasks 1-2 plus the shared test-role metadata writer.
- Produces: xUnit `[<Fact>]` as `test_case` and `[<Theory>]` as `parameterized_test`, with exact normalized annotation markers.

**Contract inputs:** Exact F# attribute syntax; shared role vocabulary; capability honesty for unsupported F# frameworks and lifecycle constructs.

**File ownership:** `crates/julie-extractors/src/fsharp/test_detection.rs`, `crates/julie-extractors/src/fsharp/mod.rs`, `crates/julie-extractors/src/tests/fsharp/test_detection.rs`

**Serialization required:** Yes.

**Dependency reason:** Test-role classification depends on stable declaration and annotation extraction from Tasks 1-2.

**What to build:** Detect xUnit Fact and Theory attributes on F# test functions and emit the existing role booleans and `test_role` string. Keep unrecognized attributes, similarly named functions, and attributes on non-callable declarations silent.

**Approach:** Start with failing Fact and Theory cases, then add qualified attribute spellings and negative controls. Use normalized annotation keys instead of raw text matching. Record unsupported Expecto, NUnit, FsUnit, container, and lifecycle shapes as explicit capability gaps unless the task adds matching fixture-backed behavior.

**Acceptance criteria:**
- [x] `[<Fact>]` functions emit `test_case` and `[<Theory>]` functions emit `parameterized_test`.
- [x] Qualified xUnit attribute spellings normalize to the same roles.
- [x] Attribute-like names, non-callable targets, and unannotated test-looking functions remain silent.
- [x] Annotation markers, booleans, and `test_role` strings agree.
- [x] Unsupported F# test ecosystems remain honest capability gaps.
- [x] Focused test-detection tests pass and the crate builds.
- [x] Task changes are committed per `serial-worker-commit`.

### Task 4: Publish F# evidence and contracts

**Files:**
- Create: `fixtures/extraction/fsharp/basic/source.fs`
- Create: `fixtures/extraction/fsharp/basic/expected.json`
- Create: `fixtures/extraction/fsharp/script/source.fsx`
- Create: `fixtures/extraction/fsharp/script/expected.json`
- Create: `fixtures/extraction/fsharp/signature/source.fsi`
- Create: `fixtures/extraction/fsharp/signature/expected.json`
- Create: `fixtures/extraction/fsharp/test_roles/source.fs`
- Create: `fixtures/extraction/fsharp/test_roles/expected.json`
- Modify: `fixtures/extraction/capabilities.json`
- Create: `docs/languages/fsharp.md`
- Modify: `crates/julie-extractors/README.md`
- Modify: `crates/julie-extractors/src/lib.rs:130`
- Modify: `crates/julie-extractors/src/tests/api_surface.rs:14-51`
- Modify: `crates/julie-extractors/src/tests/capability_matrix.rs`

**Interfaces:**
- Consumes: all behavior from Tasks 1-3 and the capability schema.
- Produces: canonical F# evidence, capability row, language documentation, parser inventory visibility, contract marker `fsharp-v1`, and a real SQLite extraction report.

**Contract inputs:** Exact emitted rows from normal extraction; new-language checklist; strict data-quality and grammar-dependency policies.

**File ownership:** `fixtures/extraction/fsharp/**`, `fixtures/extraction/capabilities.json`, `docs/languages/fsharp.md`, `crates/julie-extractors/README.md`, `crates/julie-extractors/src/lib.rs`, `crates/julie-extractors/src/tests/api_surface.rs`, `crates/julie-extractors/src/tests/capability_matrix.rs`

**Serialization required:** Yes.

**Dependency reason:** Capability publication follows complete behavior and owns all shared evidence files.

**What to build:** Add four canonical fixture families, document exact supported and open domains, register the capability row, and prove the new language through the CLI's SQLite output. Bump the extraction contract only in this publication task.

**Approach:** Run each fixture before creating expected JSON and confirm the missing-evidence failure. Generate goldens once, inspect every row, and hand-correct only source fixtures or extractor behavior rather than expected output. Clone <https://github.com/haf/expecto> into `target/corpora/expecto-cec2c63c`, detach at `cec2c63c8d77c6c21bf7e35d903020f74ddc1cea`, and scan it with `cargo run -p julie-extract-cli --bin julie-extract -- scan --root target/corpora/expecto-cec2c63c --db target/corpora/expecto-cec2c63c.sqlite --force --json`. Query symbols, relationships, identifiers, type facts, literals, source regions, complexity metrics, and parse diagnostics by language and kind. Keep corpus and database output under ignored `target/` and record the exact commit and queries in the verification ledger.

**Acceptance criteria:**
- [ ] Implementation, script, signature, and xUnit fixtures produce reviewed deterministic goldens.
- [ ] Capability claims name exact fixtures and unsupported domains remain explicit `open_gaps` with closure tasks.
- [ ] `docs/languages/fsharp.md` documents parsers, extensions, supported facts, test roles, gaps, and grammar freshness command.
- [ ] `EXTRACTION_CONTRACT_VERSION` contains `fsharp-v1` and its API-surface test passes.
- [ ] Real SQLite queries show nonzero F# symbols by kind plus representative relationships, identifiers, type facts, literals, source regions, complexity metrics, and test roles.
- [ ] The pinned Expecto corpus scan records its exact commit and has no unexpected F# error or missing diagnostics.
- [ ] Valid fixtures have zero error or missing parse diagnostics.
- [ ] `cargo xtask test language fsharp`, golden, capability, contract, strict data-quality, grammar-freshness, dependency, Windows path, and branch gates pass.
- [ ] The completed plan is checkpointed and the final task changes are committed per `serial-worker-commit`.
