# Structural Fact Pattern Registry Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Replace out-of-band knowledge of structural-fact metadata payloads with a machine-readable, contract-tested registry: every `pattern_id` declares its metadata keys, value types, and requiredness, emission is tested against the declaration, and consumers can read the registry from `languages --json`.

**Architecture:** A single Rust source of truth (`base/structural_fact_registry.rs`) declaring specs for all ~63 pattern ids currently defined across the base collectors (`code_structural_facts.rs`, `data_structural_facts.rs`, `sql_structural_facts.rs`, `framework_structural_facts.rs`, and the split `web_structural_facts/` module). Three consumers of the registry: (1) a golden-fixture-driven conformance test that fails when emission and declaration diverge, (2) a checked-in JSON export (`docs/contracts/structural-fact-patterns.json`) kept in sync by test, (3) a new `structural_fact_patterns` section in the `languages --json` report so Miller can validate at index time.

**Tech Stack:** Rust, serde, golden fixtures, CLI report contract.

**Architecture Quality:** New public contract surface (the registry export and the report section) — strategy-tier per RAZORBACK.md; shapes are decided in this plan. Risk is medium: the initial registry authoring will surface real emission/doc drift (that is the point); drift resolutions that change emission are escalations, not silent fixes.

## Global Constraints

- This plan may run before or after `2026-07-01-http-boundary-facts.md`; whichever lands second adds its pattern ids to the registry in the same task that ships them (add that requirement to the later plan's contract-sweep task at execution time).
- The registry describes the existing v3 contract; it must not silently change emission. Any discovered emission bug or doc drift is reported to the lead and fixed as an explicit, separately-committed correction (metadata-only fixes allowed; span/gate changes escalate).
- Report changes are additive: `report_schema_version` stays `3`; the new `languages --json` section is a new key, documented in `docs/contracts/reports.md` and `docs/contracts/cli.md` in the same task.
- No `EXTRACTION_CONTRACT_VERSION` bump unless an emission correction changes the extraction shape; if one does, bump once with marker `.structural-fact-metadata-v1` and update `crates/julie-extractors/src/tests/api_surface.rs`.
- Default suite stays under the 90s tripwire — the conformance test runs under the existing `test-golden` feature gate, not in default.
- Contract docs (`docs/contracts/sqlite-schema-v3.md` structural-fact pattern tables and metadata key lists) are reconciled against the registry, and the registry becomes their stated source of truth.

---

## Decided Registry Shape

```rust
pub struct StructuralFactPatternSpec {
    pub pattern_id: &'static str,          // e.g. "nextjs.file_route.v1"
    pub languages: &'static [&'static str],
    pub query_family: &'static str,
    pub description: &'static str,         // one sentence, consumer-facing
    pub metadata_keys: &'static [MetadataKeySpec],
}

pub struct MetadataKeySpec {
    pub key: &'static str,                 // e.g. "route_path"
    pub value_type: MetadataValueType,     // String | Bool | Number | StringArray
    pub presence: KeyPresence,             // Always | Optional
    pub description: &'static str,
}
```

JSON export shape (one object per spec, keys as above, lower_snake enums) is the contract; field additions are additive, removals/renames need a contract decision.

Conformance rule: for every emitted structural fact across the golden fixture corpus, (a) its `pattern_id` exists in the registry, (b) every metadata key it carries is declared with a matching value type, (c) every `Always` key is present. The registry may declare keys the corpus doesn't exercise (Optional), but a declared `Always` key never absent and an undeclared key never present.

## File Structure

- Create: `crates/julie-extractors/src/base/structural_fact_registry.rs`
- Create: `docs/contracts/structural-fact-patterns.json` (generated, checked in)
- Modify: `crates/julie-extractors/src/base/mod.rs` (register + re-export)
- Modify: `crates/julie-extract-cli/src/capability_snapshot.rs` + `crates/julie-extract-cli/src/commands.rs` (`languages` command section)
- Modify: `docs/contracts/reports.md`, `docs/contracts/cli.md`, `docs/contracts/sqlite-schema-v3.md`, `docs/contracts/jsonl-v3.md`
- Test: new `crates/julie-extractors/src/tests/structural_fact_registry.rs` (feature-gated conformance), `crates/julie-extract-cli/tests/cli_contract.rs` (report section)

## Task 1: Registry Module with Full Pattern Coverage

**Files:**
- Create: `crates/julie-extractors/src/base/structural_fact_registry.rs`
- Modify: `crates/julie-extractors/src/base/mod.rs`
- Test: unit tests in the module for registry invariants (unique pattern ids, non-empty languages, ids match the collectors' `*_pattern_ids_for_language` arrays)

**Interfaces:**
- Consumes: the pattern-id constants and capability arrays from the five collector modules (make the consts `pub(crate)` where needed).
- Produces: `pub fn structural_fact_pattern_specs() -> &'static [StructuralFactPatternSpec]` — Tasks 2–4 depend on it.

**What to build:** The typed registry for all pattern ids currently defined in the base collectors (63 at planning time: code, data, sql, css/html/vue/react/nextjs/nuxt web families, aspnet/htmx/alpine/razor framework families). Author each spec from the collector source (`insert_metadata`/`insert_string` call sites) — the code is the authority, docs are the cross-check.

**Approach:** An invariant test asserts the registry's per-language pattern-id sets equal the union of `web_structural_fact_pattern_ids_for_language`, `framework_structural_fact_pattern_ids_for_language`, and the code/data/sql equivalents — so a future collector can't add a pattern the registry doesn't know. Record any observed emission-vs-doc drift found while authoring in a findings list for Task 2; do not fix emission in this task.

**Acceptance criteria:**
- [ ] Every pattern id defined in the collectors has a spec; invariant test proves set equality per language.
- [ ] Registry unit tests pass; findings list (possibly empty) recorded in the task report.
- [ ] Worker-scope verification passes, committed.

## Task 2: Emission Conformance Test + Drift Resolution

**Files:**
- Create: `crates/julie-extractors/src/tests/structural_fact_registry.rs` — the MODULE is registered ungated; only the golden-corpus conformance test fns inside it carry `#[cfg(feature = "test-golden")]`, so Task 3's default-suite sync test can live in the same module without a gating conflict (2026-07-01 codex review finding)
- Modify: `crates/julie-extractors/src/tests/mod.rs` (add `pub mod structural_fact_registry;` — the test tree is an explicit module list at `tests/mod.rs:5`; feature gates there apply at module level today, e.g. `#[cfg(feature = "test-golden")] pub mod golden;`, which is why the gate must move to the test-fn level for this mixed module)
- Modify: collectors and/or registry and/or `docs/contracts/sqlite-schema-v3.md` per adjudicated drift findings

**Interfaces:**
- Consumes: `structural_fact_pattern_specs()`, the golden fixture corpus under `fixtures/extraction/`.
- Produces: a permanently enforced emission↔declaration agreement.

**What to build:** Run canonical extraction over every golden fixture, collect all structural facts, and assert the conformance rule from the Decided Registry Shape section. Failure messages name the pattern id, the offending key, and the fixture file.

**Approach:** First run will likely fail — that's the RED that surfaces real drift. Adjudication ladder per finding: (1) registry wrong → fix the spec; (2) doc wrong, emission right → fix the doc and spec together; (3) emission wrong (key emitted inconsistently across languages of one family, or a doc-promised key missing) → lead decides; metadata-only alignment may be fixed in this plan with its own commit and golden refresh, span/gate changes escalate out of this plan. Every adjudication is listed in the commit message.

**Acceptance criteria:**
- [ ] Conformance test passes over the full golden corpus.
- [ ] Each drift finding adjudicated and individually committed; any emission change carries the contract-marker bump per Global Constraints.
- [ ] Strict data-quality report clean.
- [ ] Worker-scope verification passes, committed.

## Task 3: Checked-In JSON Export

**Files:**
- Create: `docs/contracts/structural-fact-patterns.json`
- Test: sync test in `crates/julie-extractors/src/tests/structural_fact_registry.rs` comparing the serialized registry to the checked-in file — UNGATED (default suite; it's a fast serialization compare, mirroring how `schema_contract.rs` embeds contract docs via `include_str!`). This works because Task 2 registers the module ungated and gates only the conformance fns.

**Interfaces:**
- Consumes: `structural_fact_pattern_specs()` + serde serialization.
- Produces: the JSON contract artifact Miller can vendor/pin, plus a regeneration path (`UPDATE_GOLDEN`-style env flag or a documented test invocation that rewrites the file).

**What to build:** Deterministic serialization (sorted by pattern_id, stable key order) and the sync test. Reference the file from `docs/contracts/sqlite-schema-v3.md` and `docs/contracts/jsonl-v3.md` as the metadata-payload source of truth, replacing the prose key lists with a pointer plus the naming-policy rules (target_path vs route_path, verb policy) that stay prose.

**Acceptance criteria:**
- [ ] Sync test fails when the registry and file diverge; regeneration path documented.
- [ ] Contract docs point to the JSON as payload source of truth.
- [ ] Worker-scope verification passes, committed.

## Task 4: `languages --json` Publication

**Files:**
- Modify: `crates/julie-extract-cli/src/capability_snapshot.rs`, `crates/julie-extract-cli/src/commands.rs` (`fn languages` handler at `commands.rs:795`; command dispatch at `commands.rs:78`)
- Modify: `docs/contracts/reports.md`, `docs/contracts/cli.md`
- Test: `crates/julie-extract-cli/tests/cli_contract.rs` (new section present, shape matches the JSON export)

**Interfaces:**
- Consumes: `structural_fact_pattern_specs()` re-exported from the extractor crate.
- Produces: `structural_fact_patterns` top-level key in the `languages --json` report, same object shape as the checked-in JSON.

**What to build:** Additive report section so Miller can validate payloads at runtime without vendoring the repo file. Document it as additive in `reports.md` (report_schema_version stays 3) and add the CLI contract note.

**Approach:** Keep the report section byte-equivalent to the checked-in JSON content (single serializer). This is a public report contract change — lead reviews the shape before commit per RAZORBACK.md.

**Acceptance criteria:**
- [ ] `julie-extract languages --json` emits the section; CLI contract test locks it.
- [ ] `reports.md`/`cli.md` updated; report_schema_version unchanged.
- [ ] Worker-scope verification passes, committed.

## Verification Strategy

**Project source of truth:** `AGENTS.md` / `CLAUDE.md`, `xtask` test tiers.

**Worker red/green scope:** focused tests by exact name, e.g. `cargo test -p julie-extractors --features test-golden structural_fact_registry -- --nocapture`, `cargo test -p julie-extract-cli --test cli_contract <new_test> -- --nocapture`. Workers must confirm the filter matched at least one test ("0 tests run" is a FAIL — guards against unregistered test modules and wrong feature gates).

**Worker ceiling:** the named registry/conformance/CLI-contract tests plus `cargo test -p julie-extractors structural_facts`.

**Worker gate invariant:** declaration↔emission agreement over the golden corpus; registry↔export sync; report section shape.

**Lead affected-change scope:**

```bash
cargo test -p julie-extractors --features test-golden structural_fact_registry -- --nocapture
cargo test -p julie-extractors --features test-golden golden_fixtures_match_canonical_extraction -- --nocapture
cargo test -p julie-extract-cli --test cli_contract
node scripts/language-data-quality-report.mjs --strict
```

**Branch gate:**

```bash
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --no-deps -- -D warnings
cargo xtask test default
cargo xtask test contract
node scripts/language-data-quality-report.mjs --strict
```

**Replay/metric evidence:** conformance-test pass over the full golden corpus is the hard gate; the count of adjudicated drift findings is report-only.

**Escalation triggers:** any drift adjudication that changes emission spans or gates; report shape disputes; default-suite runtime growth from the sync test.

**Assigned verification failure:** Workers stop and report when assigned verification fails, unless this plan explicitly says to update that gate (Task 2's first RED run is expected and in-plan).

**Verification ledger:** Record invariant, command, scope label, commit SHA, result, and timestamp per task. Reuse passing evidence for the same HEAD.

## Model Routing

**Project source of truth:** repo `RAZORBACK.md`.

**Strategy tier:** registry shape (decided here), all Task 2 drift adjudications, Task 4 report shape review. Harness mapping: inherit.

**Implementation tier:** Task 1 spec authoring and Task 3/4 mechanics after shapes are locked. Harness mapping: inherit.

**Mechanical tier:** none — every task owns a gate.

**Gate-interpretation reviewer:** lead reads the failing conformance output and adjudicates per the Task 2 ladder. Harness mapping: inherit.

**Escalation tier:** emission changes, report/CLI contract changes (per RAZORBACK.md these are strategy-tier areas). Harness mapping: inherit.

**Worker eligibility:** Task 1 authoring is worker-eligible (reads code, writes declarations, owns invariant tests); Task 2 execution is worker-run but adjudication is lead-owned.

**Escalation triggers:** any finding in adjudication category (3); disagreement between capability matrix and registry language sets.

**Mechanical exclusion:** not applicable.

**Unsupported harness behavior:** If the harness cannot choose models per agent, use `inherit` and continue.
