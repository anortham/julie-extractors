# Extraction Data Quality & Hygiene Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Land the hygiene quick wins from the 2026-06-09 project review, protect the newest extraction domains with golden coverage, close the Tier-2 data-depth gaps (complexity metrics, annotations, doc-comment evidence), and formalize per-domain capability claims.

**Architecture:** No new modules. Phase 1 extends the existing golden-test normalization contract; Phase 2 adds per-language `ComplexityLanguageConfig` entries to the existing config-driven engine; Phase 3 wires the existing `base::normalize_annotations` helper into more language extractors; Phase 4 is an additive SQLite schema change (v3 → v4) owned by the lead.

**Tech Stack:** Rust workspace, tree-sitter, rusqlite, cargo-nextest, xtask test tiers.

**Architecture Quality:**
- Affected modules: golden test harness, `base/complexity_metrics.rs` config table, per-language extractor modules, `julie-extract-artifact` schema/writer (Phase 4 only).
- Caller-facing interface: unchanged for Phases 0–3 (rows that were always legal in schema v3 simply get populated/tested). Phase 4 changes the SQLite contract (new column + metadata cleanup) and is strategy-tier per RAZORBACK.md.
- Test surface: golden fixtures and per-language unit tests — the same interface downstream consumers read.
- Rejected shortcut: stuffing domain coverage into the existing `kind_coverage_json` blob (conflates symbol-kind coverage with domain coverage; rejected in favor of an explicit `domain_coverage_json` column in v4).
- Architecture risk: low for Phases 0–3; medium for Phase 4 (public contract).

**Source findings:** `docs/findings/2026-06-09-project-review.md`, `docs/findings/2026-06-09-data-quality-review.md`.

---

## Verification Strategy

**Project source of truth:** `CLAUDE.md` (test discipline) + `xtask/src/test_tiers.rs` (tier definitions).

**Worker red/green scope:** `cargo xtask test language <name>` for extractor work; `cargo nextest run -p <crate> <filter>` for focused non-language tests. Golden regeneration: `UPDATE_GOLDEN=1 cargo nextest run -p julie-extractors --features test-golden golden` (PowerShell: `$env:UPDATE_GOLDEN='1'` first; unset after).

**Worker ceiling:** `cargo xtask test default` (90s budget). Workers do not run contract/certification/real-world tiers.

**Worker gate invariant:** each worker's named tests prove the new rows/fields appear through the extraction results interface (golden or unit assertion), not via private helpers.

**Lead affected-change scope:** `cargo xtask test changed` after each phase batch.

**Branch gate:** `cargo xtask test default` + `cargo xtask test contract` + `cargo fmt --check` + `cargo clippy --workspace --all-targets` before merge/push.

**Escalation triggers:** any capability claim change (Phases 2–4) and the Phase 4 schema change require lead-run `cargo xtask test contract`; parser dependency changes are out of scope for this plan entirely.

**Assigned verification failure:** Workers stop and report when assigned verification fails, unless the task explicitly says to regenerate goldens (Tasks 4, 5, 8, 9 do).

**Verification ledger:** Record invariant, command, scope label, commit SHA, result, and timestamp per task. Reuse same-HEAD passing evidence rather than rerunning expensive gates.

## Model Routing

**Project source of truth:** `RAZORBACK.md`.

- **Strategy tier** (Phase 4 schema, capability claims, plan-mismatch triage): lead session — harness mapping: inherit.
- **Implementation tier** (Tasks 2–9): harness mapping: inherit.
- **Mechanical tier** (Task 1): harness mapping: `haiku`.
- **Gate-interpretation reviewer:** lead — inherit.
- **Escalation tier:** lead — inherit.
- **Worker eligibility (per RAZORBACK.md):** interface already decided, narrow non-overlapping file ownership, explicit verification ceiling, no schema/report/release-evidence reinterpretation, no parser dependency changes.
- **Escalation triggers (per RAZORBACK.md):** schema change, capability claim change, weak test evidence, default-suite runtime growth.
- **Mechanical exclusion:** Task 1 owns no failing test or acceptance gate.

---

## Phase 0 — Hygiene quick wins

### Task 1: Toolchain, CI, and workflow hygiene (mechanical)

**Files:**
- Modify: `Cargo.toml` (root, `[workspace.package]`)
- Modify: `.github/workflows/ci.yml`
- Modify: `.github/workflows/release-binaries.yml:9`
- Modify: `.github/workflows/specialist-gates.yml:9`

**What to build:** Pin `rust-version` in `[workspace.package]` (and `rust-version.workspace = true` in the three crate manifests + xtask if not inherited automatically — crates must opt in). Determine the actual MSRV: `libsqlite3-sys 0.38.0` fails on 1.90; check its declared `rust-version` via `cargo metadata --format-version 1` and pin the workspace at least that high. Update both workflow `version` defaults from `2.1.0` to `2.2.1`. Add `Swatinem/rust-cache@v2` to `ci.yml` after toolchain setup.

**Acceptance criteria:**
- [ ] `cargo metadata` succeeds and every workspace crate reports the pinned `rust-version`.
- [ ] Both workflow defaults read `2.2.1`.
- [ ] `ci.yml` has a rust-cache step before the first cargo invocation.
- [ ] No test ownership — lead validates with the branch gate.

### Task 2: Code quick fixes

**Files:**
- Modify: `crates/julie-extractors/src/dart/mod.rs:147`
- Modify: `crates/julie-extractors/src/utils/paths.rs:24`
- Test: `crates/julie-extractors/src/tests/dart/` (existing module; add one regression test)

**What to build:** Replace `node.parent().unwrap()` with a `let Some(parent)` guard that skips inheritance extraction when there is no parent. Fix the `manual_strip` clippy lint in `paths.rs` (use `strip_prefix`). Add a Dart unit test covering the guarded path (extraction of a fixture snippet where the generic-modifier class node is the outermost construct still succeeds without panic).

**Acceptance criteria:**
- [ ] `cargo xtask test language dart` passes.
- [ ] `cargo clippy -p julie-extractors` reports zero warnings.

### Task 3: LazyLock regex audit

**Files:**
- Modify: the ~30 extractor files with `Regex::new` call sites (75 total; enumerate with `rg -c "Regex::new" crates/julie-extractors/src`). Known hot-path examples: `crates/julie-extractors/src/javascript/mod.rs:634-647`, `crates/julie-extractors/src/python/mod.rs`, `crates/julie-extractors/src/razor/directives.rs`, `crates/julie-extractors/src/gdscript/mod.rs`.

**What to build:** Move every `Regex::new` that executes per node/per symbol/per comment into a `std::sync::LazyLock<Regex>` static, following the existing LazyLock pattern already used elsewhere in the crate. Leave one-time/startup compilations alone. Do not change any regex pattern text.

**Acceptance criteria:**
- [ ] No `Regex::new` remains inside functions called per node/symbol/comment (verify by inspecting each remaining call site and noting why it is cold).
- [ ] `cargo xtask test default` passes (behavior unchanged).

## Phase 1 — Test protection

### Task 4: Golden contract expansion

**Files:**
- Modify: `crates/julie-extractors/src/tests/golden.rs:32-41` (`NormalizedExtraction`) plus new normalized structs alongside the existing `NormalizedSymbol` pattern.
- Regenerate: every `fixtures/extraction/*/*/expected.json`.

**What to build:** Add `structural_facts`, `complexity_metrics`, `literals`, `source_regions`, and `type_argument_usages` to `NormalizedExtraction` with `#[serde(default)]` and normalized row structs (stable ordering, no volatile fields — follow how `NormalizedSymbol` normalizes ids into keys). Regenerate all goldens with `UPDATE_GOLDEN=1`.

**Approach:** Mirror the field shapes from `ExtractionResults` (see `crates/julie-extractors/src/registry.rs` result assembly and `base/types.rs`). Sort each new vector deterministically before serialization, as the existing domains do.

**Acceptance criteria:**
- [ ] `fixtures/extraction/rust/structural_facts/expected.json` contains the unsafe-block structural fact rows (non-empty).
- [ ] Tier-1 language goldens contain non-empty `complexity_metrics`.
- [ ] `cargo nextest run -p julie-extractors --features test-golden golden` passes without `UPDATE_GOLDEN` after regeneration.
- [ ] Worker reports a per-domain summary of how many fixtures gained rows (lead sanity-checks for domains that stayed empty everywhere — that would indicate a normalization bug, not reality).

### Task 5: Doc-comment fixture coverage

**Files:**
- Modify: `fixtures/extraction/<lang>/basic/source.<ext>` for: rust, typescript, javascript, python, java, csharp, go, swift, kotlin, dart, php, ruby.
- Regenerate: matching `expected.json` files.

**What to build:** Add one idiomatic documented symbol per language fixture (e.g. `///` on a Rust fn, JSDoc block on a JS function, XML doc on a C# method, docstring on a Python def). Regenerate goldens. For each language, assert the golden now contains at least one non-null `doc_comment`.

**Approach:** Keep additions minimal — one documented symbol per fixture — so golden diffs stay reviewable. If a language's extractor fails to populate `doc_comment` despite the fixture, do NOT patch the extractor in this task: record it in the task report as a confirmed gap (the findings doc currently treats per-language doc coverage as unknown).

**Acceptance criteria:**
- [ ] Each listed language's `basic/expected.json` has ≥1 non-null `doc_comment`, OR the gap is explicitly reported with the language named.
- [ ] `cargo xtask test contract` golden portion passes (lead runs the full tier).

## Phase 2 — Complexity metrics for Tier-2 languages

Capability claims change here — lead reviews each task against the capability matrix tier (`RAZORBACK.md` escalation rule).

### Task 6: C# and Java complexity configs

**Files:**
- Modify: `crates/julie-extractors/src/base/complexity_metrics.rs` (`config_for_language` at :251, scope registration at :88, new `CSHARP_CONFIG`/`JAVA_CONFIG` consts following `RUST_CONFIG` at :264).
- Modify: `fixtures/extraction/capabilities.json` (complexity claims, if represented there) and any capability-matrix fixture the convention requires.
- Test: `crates/julie-extractors/src/tests/csharp/` and `tests/java/` (new complexity test modules following the existing Tier-1 complexity tests — locate with `rg complexity crates/julie-extractors/src/tests`).

**What to build:** `ComplexityLanguageConfig` entries with each grammar's decision node kinds (`if_statement`, `conditional_expression`, `switch_section`/`switch_expression_arm`, `catch_clause`, binary `&&`/`||` if the Tier-1 configs count them — match their convention), loop kinds, parameter container/parameter kinds. Unit tests assert decision/loop/nesting/parameter counts for a representative snippet per language.

**Acceptance criteria:**
- [ ] `cargo xtask test language csharp` and `cargo xtask test language java` pass with new complexity assertions.
- [ ] Golden fixtures for csharp/java now include `complexity_metrics` rows (regenerate after Task 4).
- [ ] Lead runs `cargo xtask test contract` for the capability-claim change.

### Task 7: Kotlin, Swift, and Dart complexity configs

**Files:** same shape as Task 6 — new `KOTLIN_CONFIG`, `SWIFT_CONFIG`, `DART_CONFIG` consts, registrations, per-language tests, capability fixture updates.

**What to build / acceptance:** identical pattern to Task 6 for kotlin, swift, dart; `cargo xtask test language <name>` per language passes; goldens gain complexity rows; lead runs the contract tier once for the batch.

## Phase 3 — symbol_annotations coverage

### Task 8: Annotation fixtures for already-wired languages

**Files:**
- Modify: `fixtures/extraction/csharp/basic/source.cs`, `fixtures/extraction/dart/basic/source.dart`, `fixtures/extraction/elixir/basic/source.ex`, `fixtures/extraction/gdscript/basic/source.gd` (add one attribute/annotation-carrying symbol each).
- Regenerate: matching `expected.json`.

**What to build:** Add an idiomatic annotation per fixture (`[Obsolete]` on a C# method, `@deprecated` metadata on Dart, `@doc`/module attribute on Elixir, `@export` on GDScript). Regenerate goldens and assert non-empty `annotations` for those symbols.

**Acceptance criteria:**
- [ ] Each of the four languages' goldens shows ≥1 symbol with non-empty `annotations`.
- [ ] Golden suite passes without `UPDATE_GOLDEN` afterward.

### Task 9: Annotation wiring for Java, Python, TypeScript/JavaScript, Rust

**Files:**
- Modify: `crates/julie-extractors/src/java/` (classes.rs/methods.rs/fields.rs symbol construction sites), `crates/julie-extractors/src/python/` (functions.rs/assignments.rs decorator handling), `crates/julie-extractors/src/typescript/` + `crates/julie-extractors/src/javascript/` (class/method decorators), `crates/julie-extractors/src/rust/` (attribute extraction at symbol construction).
- Pattern: `crates/julie-extractors/src/csharp/helpers.rs:34` (`extract_annotations` → `base::normalize_annotations`, attached via `SymbolOptions.annotations`).
- Test: per-language test modules + fixture additions (`@Deprecated` Java method, `@decorator` Python def, `@Component`-style TS decorator, `#[derive(Debug)]`/`#[cfg(test)]` Rust attributes).
- Fixtures: regenerate affected `expected.json`.

**What to build:** For each language, a small helper mirroring the C# one: collect the grammar's annotation/attribute/decorator nodes preceding (or attached to) the declaration, pass raw texts to `normalize_annotations(&texts, "<language>")`, attach to the symbol. First confirm the language truly lacks wiring (the findings doc lists these as "apparently missing — confirm"); if one is already wired, convert the task for that language into fixture coverage only.

**Acceptance criteria:**
- [ ] `cargo xtask test language <name>` passes for java, python, typescript, javascript, rust with new annotation assertions.
- [ ] Goldens show non-empty `annotations` for the annotated fixture symbols in all five languages.

## Phase 4 — Capability domain contract (schema v4) — lead-owned, strategy tier

### Task 10: Schema v4: per-domain capability coverage + metadata cleanup

**Files:**
- Modify: `crates/julie-extract-artifact/src/schema.rs` (SQLITE_SCHEMA_VERSION 3 → 4, `language_capabilities` DDL), `metadata.rs:38-43` (drop redundant `schema_version` key), `writer.rs` capability snapshot sync, `jsonl.rs` capability export.
- Modify: `crates/julie-extract-cli/src/commands.rs` schema checks (the `sqlite_schema_version`/`schema_version` comparison near :1872) and `info` output.
- Create: `docs/contracts/sqlite-schema-v4.md` (and JSONL contract revision if the capability record shape changes).
- Modify: `docs/contracts/sqlite-schema-v1.md`, `v2.md`, `v3.md` (deprecation/superseded notices), `docs/contracts/cli.md` (fix stale "extract contract v2" reference).
- Test: `crates/julie-extract-artifact/tests/schema_contract.rs`, `jsonl_contract.rs`, CLI contract tests.

**What to build:** Add `domain_coverage_json TEXT NOT NULL` to `language_capabilities`, holding per-domain `{target, actual}` for: complexity_metrics, structural_facts, annotations, doc_comments, literals, source_regions (booleans for target, row-presence booleans or counts for actual, mirroring the existing target_/actual_ convention). Remove the redundant `schema_version` metadata key, keeping `sqlite_schema_version` as the single source (this is the v4 contract break that justifies the bump). No migration code: v3 artifacts are rejected per existing strict-schema policy, now documented explicitly in the v4 contract and deprecation notices.

**Acceptance criteria:**
- [ ] Schema, JSONL, report, and CLI contract tests updated and passing.
- [ ] `docs/contracts/sqlite-schema-v4.md` complete; v1–v3 carry superseded notices; `cli.md` version references correct.
- [ ] `cargo xtask test contract` passes; lead records ledger evidence.
- [ ] Release-note entry drafted for the next version noting the v4 contract change.

---

## Sequencing and ownership

- Phase 0 tasks are independent and can run in parallel.
- Task 4 must land before Tasks 5–9 (they all regenerate goldens against the expanded contract).
- Tasks 6/7 and 8/9 are parallel-safe across phases (disjoint files) once Task 4 is merged.
- Task 10 runs last, lead-owned, after actual domain coverage from Phases 2–3 exists to populate it.
- Out of scope (named follow-ups, not planned here): file-level import edges domain (needs its own contract plan), identifier richness for bash/vue/jsx/yaml, SQL extraction quality, doc-comment normalization policy.
