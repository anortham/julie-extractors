# Web Structural Facts Module Split Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Split the 3,676-line `base/web_structural_facts.rs` god module into focused submodules and deduplicate the markup-scanning helpers it copies from `framework_structural_facts.rs`, with zero behavior change.

**Architecture:** Convert `base/web_structural_facts.rs` into a directory module `base/web_structural_facts/` whose `mod.rs` keeps only dispatch, pattern-id constants, and re-exports. Extraction logic moves into per-concern submodules (css, html, vue, react, nextjs_nuxt, js_imports, jsx_scan, js_object_scan, fact_builders). Markup attribute scanning shared with `framework_structural_facts.rs` moves to a new `base/markup_scan.rs` consumed by both collectors. This mirrors the prior `commands.rs` and `writer.rs` splits, including their `include_str!` convention-test guardrails.

**Tech Stack:** Rust, tree-sitter, existing structural-facts pipeline, golden fixtures.

**Architecture Quality:** Approved shape: `base/web_structural_facts/mod.rs` (dispatch + pattern ids + `pub(crate)` surface unchanged) with private submodules; `base/markup_scan.rs` owning `MarkupAttribute` and the tag/attribute scanners for both collectors. Risk is low-medium: pure code motion, but the two collectors' near-duplicate helpers have small signature differences that must be unified without changing emission. If code reality contradicts this shape, workers report a plan mismatch rather than redesigning locally.

## Global Constraints

- Zero behavior change: golden fixtures must be byte-identical (`cargo test -p julie-extractors --features test-golden golden_fixtures_match_canonical_extraction` passes without `UPDATE_GOLDEN=1`).
- No contract changes: `EXTRACTION_CONTRACT_VERSION` (`crates/julie-extractors/src/lib.rs:127`) is NOT bumped; no docs/contracts changes; no capability changes.
- The `pub`/`pub(crate)` surface of the module stays identical: `collect_web_structural_facts`, `web_structural_fact_pattern_ids_for_language`, and everything `registry.rs` imports today keep their paths via re-exports in `mod.rs`.
- Every task ends compilable with worker-scope verification green, committed.
- Default suite stays under the 90s tripwire (`xtask/src/test_tiers.rs:5`).
- `AGENTS.md`/`CLAUDE.md` untouched (no guideline changes in this plan).

---

## Current File Map (source of truth for the split)

`crates/julie-extractors/src/base/web_structural_facts.rs` sections by line (verified 2026-07-01, commit 1728dad):

- Dispatch + pattern-id constants: 1–108 (`collect_web_structural_facts` :73, `web_structural_fact_pattern_ids_for_language` :96, consts :11–71)
- CSS collection: 109–262, plus CSS helpers 3146–3197 (`css_selector_kind`, `count_css_declarations*`, `css_at_rule_prelude`)
- HTML collection: 263–730 (forms, links, scripts, attribute helpers)
- Vue SFC collection: 731–1105 (`collect_vue_structural_facts`, route refs/defs, `collect_nuxt_route_references`), plus Vue scanning helpers 3198–3316 (`scan_vue_sections` family) and 3511–3670 (`parse_vue_directive` family)
- JS import index: 1080–1314 (`JsImportIndex`, `collect_js_imports`, `parse_*import*`)
- React Router: 1315–1709 (`collect_react_router_*`, `ReactRouteDefinitionFact`, `react_route_definition_fact`)
- Next/Nuxt references + file routes + segment normalization: 1710–2325 (`collect_nextjs_route_references`, `nextjs_file_route_fact`, `nuxt_file_route_fact`, `route_path_metadata`, `*_dynamic_segment_metadata`, signal gates `has_nextjs_page_signal`/`has_nuxt_page_signal`)
- JSX attribute scanning: 2326–2553 (`next_markup_tag`, `jsx_*_attribute`, `find_jsx_attribute`)
- Generic JS object/string scanning: 2599–3052 (`parse_object_string_property`, `parse_js_string_literal`, `find_enclosing_object_range`, `find_matching_brace/paren/bracket`, `find_js_array_initializer_range`, `smallest_node_covering_range`, `is_comment_or_string_node`, `parent_route_path_for_object`, `join_frontend_route_paths`)
- Fact builders: 3053–3145 (`fact_for_node`, `fact_for_span`, `base_metadata`, `insert_string`, `insert_string_array`, `attach_containing_symbols`, `node_text`, `child_by_kind`)
- Markup scanning (near-duplicate of `framework_structural_facts.rs:943–1082`): 3317–3510 (`scan_markup_attributes`, `scan_tag_attributes`, `parse_markup_attribute_value`, `find_tag_end`, `is_markup_tag_start`, `is_attr_name_byte`, plus `split_argument_and_modifiers` :3671)

Known signature differences between the duplicates: web's `scan_markup_attributes(content, start, end)` vs framework's `scan_markup_attributes(content)` (`framework_structural_facts.rs:943`); web's `find_matching_paren(content, open_paren, end)` vs framework's `find_matching_paren(content, open_paren)` (`framework_structural_facts.rs:1193`); web's `base_metadata(query_family)` vs framework's `base_metadata(query_family, framework)` (`framework_structural_facts.rs:772`).

**`MarkupAttribute` is defined TWICE with different shapes** (2026-07-01 codex review finding): framework's is `{ name: String, value: Option<String>, start_byte: usize, end_byte: usize }` (`framework_structural_facts.rs:936–940`); web's is `{ tag_name: String, name: String, value: Option<String>, span: NormalizedSpan }` (`web_structural_facts.rs:3303–3309`). Web consumers rely on `attribute.span` (e.g. `vue_route_reference_fact` :825 area, :814) and `attribute.tag_name` (:3586); framework consumers rely on the byte range. The shared struct must be a superset.

## File Structure

- Create: `crates/julie-extractors/src/base/markup_scan.rs` — `MarkupAttribute` + shared tag/attribute scanners.
- Create: `crates/julie-extractors/src/base/web_structural_facts/` directory module with `mod.rs`, `css.rs`, `html.rs`, `vue.rs`, `js_imports.rs`, `react.rs`, `nextjs_nuxt.rs`, `jsx_scan.rs`, `js_object_scan.rs`, `fact_builders.rs`.
- Modify: `crates/julie-extractors/src/base/framework_structural_facts.rs` — consume `markup_scan.rs`, drop its duplicate scanners.
- Modify: `crates/julie-extractors/src/base/mod.rs` — register the new modules.
- Test: extend `crates/julie-extractors/src/tests/structural_facts.rs` with module-layout convention tests (pattern: `crates/julie-extract-artifact/tests/writer_contract.rs:18–79`).

## Task 1: Extract Shared Markup Scanning into `base/markup_scan.rs`

**Files:**
- Create: `crates/julie-extractors/src/base/markup_scan.rs`
- Modify: `crates/julie-extractors/src/base/framework_structural_facts.rs` (remove :936–1082 scanner family, import from `markup_scan`)
- Modify: `crates/julie-extractors/src/base/web_structural_facts.rs` (remove :3317–3510 + `split_argument_and_modifiers` :3671, import from `markup_scan`)
- Modify: `crates/julie-extractors/src/base/mod.rs` (add `mod markup_scan;`)

**Interfaces:**
- Consumes: nothing new — pure code motion plus a superset-struct merge.
- Produces: `pub(crate) struct MarkupAttribute { tag_name: String, name: String, value: Option<String>, start_byte: usize, end_byte: usize, span: NormalizedSpan }` — the SUPERSET of the two current shapes (see Current File Map: framework has `name/value/start_byte/end_byte`, web has `tag_name/name/value/span`). Also `pub(crate) fn scan_markup_attributes(content: &str, start: usize, end: usize) -> Vec<MarkupAttribute>` plus `scan_tag_attributes`, `parse_markup_attribute_value`, `find_tag_end`, `is_markup_tag_start`, `is_attr_name_byte`, `split_argument_and_modifiers` in `base::markup_scan`. Later tasks and both collectors import these.

**What to build:** One canonical copy of the markup scanner family returning the superset `MarkupAttribute`. The shared scanner must populate all six fields (web's copy already computes `tag_name` and `span`; framework's copy already tracks byte offsets — the merged scanner computes both). Unify on the ranged signature `(content, start, end)`; `framework_structural_facts.rs` call sites pass `(content, 0, content.len())` and keep consuming `start_byte`/`end_byte`; web call sites keep consuming `tag_name`/`span` (e.g. `vue_route_reference_fact`, `is_vue_router_link_tag`).

**Approach:** Diff the two copies function-by-function before deleting either — they drifted independently (web's copy handles Vue directive shorthand ranges). Where bodies differ, keep the superset behavior only if both collectors' tests stay green; otherwise keep two thin wrappers over shared internals and note the residual difference in a code comment. Emission must not change for either collector.

**Acceptance criteria:**
- [ ] `MarkupAttribute` and the scanner family exist only in `base/markup_scan.rs`; both collectors import them.
- [ ] `cargo test -p julie-extractors structural_facts` passes (covers razor/html/vue/react/nuxt fact tests).
- [ ] Golden fixtures byte-identical (lead affected-change scope).
- [ ] Worker-scope verification passes, committed.

## Task 2: Create the Directory Module; Move CSS + HTML + Fact Builders

**Files:**
- Create: `crates/julie-extractors/src/base/web_structural_facts/mod.rs` (dispatch :1–108 moves here)
- Create: `crates/julie-extractors/src/base/web_structural_facts/css.rs` (:109–262 + :3146–3197)
- Create: `crates/julie-extractors/src/base/web_structural_facts/html.rs` (:263–730)
- Create: `crates/julie-extractors/src/base/web_structural_facts/fact_builders.rs` (:3053–3145)
- Delete moved code from the old single file (which becomes `mod.rs`)

**Interfaces:**
- Consumes: `base::markup_scan` (Task 1).
- Produces: `mod.rs` re-exports keep `crate::base::collect_web_structural_facts` and `web_structural_fact_pattern_ids_for_language` resolvable exactly as `registry.rs:9` and the capability matrix use them today. Submodule items are `pub(super)`.

**What to build:** The mechanical conversion from file module to directory module, with CSS, HTML, and the shared fact-builder helpers as the first extracted submodules.

**Approach:** `git mv` semantics: keep function bodies byte-identical; only adjust `use` paths and visibility (`fn` → `pub(super) fn` where cross-submodule). Keep pattern-id constants in `mod.rs` since multiple submodules and the capability arrays reference them.

**Acceptance criteria:**
- [ ] `web_structural_facts.rs` no longer exists as a single file; directory module compiles.
- [ ] `cargo test -p julie-extractors structural_facts` passes.
- [ ] Worker-scope verification passes, committed.

## Task 3: Move Vue + JS Imports

**Files:**
- Create: `crates/julie-extractors/src/base/web_structural_facts/vue.rs` (:731–1105, :3198–3316, :3511–3670)
- Create: `crates/julie-extractors/src/base/web_structural_facts/js_imports.rs` (:1080–1314: `JsImportIndex`, `collect_js_imports`, `js_import_statement_end`, `parse_import_source`, `parse_named_imports`, `parse_default_import`)

**Interfaces:**
- Consumes: `fact_builders`, `markup_scan`.
- Produces: `pub(super) struct JsImportIndex` with fields `react_router_links/react_router_routes/react_router_route_apis/next_links` and `pub(super) fn collect_js_imports(content: &str) -> JsImportIndex` — Task 4's React/Next modules depend on these names. Vue module exposes `collect_vue_structural_facts` and `collect_vue_router_route_definitions` to `mod.rs` dispatch.

**What to build:** Vue SFC scanning (sections, directives, route references, Nuxt references, route definitions) in `vue.rs`; the hand-rolled JS import scanner in `js_imports.rs`.

**Approach:** `collect_vue_router_route_definitions` (:1135) is Vue-owned but called from the JS/TS dispatch path — keep it in `vue.rs` and have `mod.rs` dispatch call it, preserving the H3 wiring from the 2026-07-01 hardening plan.

**Acceptance criteria:**
- [ ] Vue/Nuxt-reference structural-fact tests pass (`cargo test -p julie-extractors structural_facts`).
- [ ] Worker-scope verification passes, committed.

## Task 4: Move React + Next/Nuxt + JSX/Object Scanners

**Files:**
- Create: `crates/julie-extractors/src/base/web_structural_facts/react.rs` (:1315–1709)
- Create: `crates/julie-extractors/src/base/web_structural_facts/nextjs_nuxt.rs` (:1710–2325)
- Create: `crates/julie-extractors/src/base/web_structural_facts/jsx_scan.rs` (:2326–2553)
- Create: `crates/julie-extractors/src/base/web_structural_facts/js_object_scan.rs` (:2599–3052)

**Interfaces:**
- Consumes: `js_imports::JsImportIndex`, `jsx_scan`, `js_object_scan`, `fact_builders`.
- Produces: `mod.rs` dispatch calls `react::collect_react_router_route_references/definitions`, `nextjs_nuxt::collect_nextjs_route_references`, `nextjs_nuxt::nextjs_file_route_fact`, `nextjs_nuxt::nuxt_file_route_fact` with unchanged signatures. `js_object_scan` exposes `find_enclosing_object_range`, `parent_route_path_for_object`, `join_frontend_route_paths`, the `find_matching_*` family, and `parse_js_string_literal` — the HTTP-boundary-facts plan builds on these.

**What to build:** The remaining four submodules. `collect_react_nextjs_structural_facts` (:1106) is dispatch-shaped — it moves into `mod.rs`, calling into `react.rs` and `nextjs_nuxt.rs`.

**Approach:** Pure motion again. `jsx_scan` and `js_object_scan` are shared by both `react.rs` and `nextjs_nuxt.rs` (and `vue.rs` for object walking) — keep them sibling modules, not children of either framework module.

**Acceptance criteria:**
- [ ] `cargo test -p julie-extractors structural_facts` passes.
- [ ] `cargo test -p julie-extractors test_public_contract_version_marks_current_fact_families` passes (no marker change expected).
- [ ] Worker-scope verification passes, committed.

## Task 5: Convention-Test Guardrails + Size Check

**Files:**
- Modify: `crates/julie-extractors/src/tests/structural_facts.rs` (add module-layout convention tests)

**Interfaces:**
- Consumes: the final module layout from Tasks 1–4.
- Produces: tests that fail if extraction logic drifts back into `mod.rs` or if the markup scanners are re-duplicated.

**What to build:** Following `crates/julie-extract-artifact/tests/writer_contract.rs:18–79`: (a) `include_str!` on `base/web_structural_facts/mod.rs` asserting it does not contain forbidden definitions (`fn collect_css_node`, `fn html_form_fact`, `fn scan_vue_sections`, `fn collect_react_router_route_object_definitions`, `fn nextjs_app_file_route`, `fn find_enclosing_object_range` — one representative per submodule); (b) `include_str!` on `base/framework_structural_facts.rs` asserting it no longer defines `fn scan_markup_attributes` / `struct MarkupAttribute`.

**Approach:** Match the existing convention-test naming and failure-message style so the guardrail family reads consistently across the three splits.

**Acceptance criteria:**
- [ ] Convention tests fail when a forbidden definition is reintroduced (verified red by temporarily pasting one back), then pass.
- [ ] `mod.rs` is under 400 lines; no submodule exceeds ~800 lines.
- [ ] Worker-scope verification passes, committed.

## Verification Strategy

**Project source of truth:** `AGENTS.md` / `CLAUDE.md`, `xtask` test tiers.

**Worker red/green scope:** `cargo test -p julie-extractors structural_facts -- --nocapture` (plus the specific convention test name in Task 5).

**Worker ceiling:** `cargo test -p julie-extractors structural_facts` and `cargo test -p julie-extractors test_public_contract_version_marks_current_fact_families`. Workers do not run golden/capability gates.

**Worker gate invariant:** All existing structural-fact emission tests pass unchanged — proof of zero behavior change at the emission surface.

**Lead affected-change scope (after Tasks 1, 4, and 5):**

```bash
cargo test -p julie-extractors structural_facts -- --nocapture
cargo test -p julie-extractors --features test-golden golden_fixtures_match_canonical_extraction -- --nocapture
cargo test -p julie-extractors --features test-capability-matrix capability_matrix -- --nocapture
```

Golden run must pass WITHOUT `UPDATE_GOLDEN=1` — any golden diff is a behavior change and a stop-the-line failure for this plan.

**Branch gate:**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --no-deps -- -D warnings
cargo xtask test default
cargo xtask test contract
node scripts/language-data-quality-report.mjs --strict
```

**Escalation triggers:** Any golden fixture diff; any needed change to emission logic discovered during dedupe (report as plan mismatch); default-suite runtime growth.

**Assigned verification failure:** Workers stop and report when assigned verification fails, unless this plan explicitly says to update that gate.

**Verification ledger:** Record invariant, command, scope label, commit SHA, result, and timestamp per task. Reuse passing evidence for the same HEAD.

## Model Routing

**Project source of truth:** repo `RAZORBACK.md`.

**Strategy tier:** plan interpretation, dedupe-difference adjudication (Task 1 superset-behavior decisions) — lead. Harness mapping: inherit.

**Implementation tier:** Tasks 2–4 (mechanical code motion with narrow ownership, decided interfaces, explicit ceiling). Harness mapping: inherit.

**Mechanical tier:** none (every task owns a gate).

**Gate-interpretation reviewer:** lead. Harness mapping: inherit.

**Escalation tier:** any golden diff or emission change (per RAZORBACK.md: capability claims and public artifact shape are strategy-tier). Harness mapping: inherit.

**Worker eligibility:** met for Tasks 2–4 (interfaces decided, non-overlapping files, explicit ceiling, no schema/parser changes). Task 1 starts strategy-tier because of the drift-adjudication decision, then hands the motion to a worker.

**Escalation triggers:** golden fixture diffs; behavior differences between the duplicated scanners that tests do not disambiguate.

**Mechanical exclusion:** not applicable — no mechanical-tier tasks.

**Unsupported harness behavior:** If the harness cannot choose models per agent, use `inherit` and continue.
