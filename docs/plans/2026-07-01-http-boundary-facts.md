# HTTP Boundary Facts Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Emit both sides of the HTTP boundary — client request facts (`fetch`/`axios`) and API route-handler definition facts (Next.js route handlers, Nuxt server routes, ASP.NET controller attribute routes) — so Miller can bridge `fetch("/api/x")` to the handler that serves it, and close the htmx-in-JSX/Vue coverage gap.

**Architecture:** Four new fact families plus one language-coverage extension, all through the existing `structural_facts` row family. Client requests and JS-framework handlers live in the split `base/web_structural_facts/` module (this plan assumes `docs/plans/2026-07-01-web-structural-facts-module-split.md` has landed); ASP.NET attribute routes live in `base/framework_structural_facts.rs` beside the minimal-API collector. Join currency stays route-path strings: references carry `target_path`; frontend definitions/file routes carry `route_path` + `normalized_route_template` (the documented M6 naming rule); ASP.NET server-side definitions keep the established template naming from `aspnet.minimal_api.route.v1` — `route_template` + `effective_route_template` (`framework_structural_facts.rs:408`, `docs/contracts/jsonl-v3.md:511`). Miller joins client `target_path` against frontend `route_path`/`normalized_route_template` and against ASP.NET `effective_route_template`.

**Tech Stack:** Rust, tree-sitter, structural-facts pipeline, golden fixtures + capability matrix.

**Architecture Quality:** New public contract surface (four pattern ids, one coverage extension) — strategy-tier decisions are made in this plan; workers implement decided shapes. Risk is medium: detection is heuristic source scanning consistent with the existing collectors, and the M2 doctrine (silent unless evidenced, no guessed routes) applies to every family. If code reality contradicts a decided shape, workers report a plan mismatch.

## Global Constraints

- Prerequisite: the web collector module split (`2026-07-01-web-structural-facts-module-split.md`) is merged. File targets below reference the split layout; if it is absent, stop and report.
- Contracts are API: every new pattern id and metadata key lands in `docs/contracts/jsonl-v3.md` and `docs/contracts/sqlite-schema-v3.md` in the same task that ships it.
- Capability claims require golden fixture evidence in `fixtures/extraction/capabilities.json`; after capability or fixture changes run `node scripts/language-data-quality-report.mjs --strict` and keep `silent_cells` and `quality_bar_debts` at `0`.
- Language parity: a capability claimed for one language of the JS family must cover `javascript`, `jsx`, `typescript`, `tsx` before it is claimed.
- Dynamic/unresolvable URL and route expressions stay silent (no guessed routes) — existing doctrine. Static string literals only; template literals and interpolations are not emitted.
- Naming rule (documented M6 contract): references carry `target_path`; frontend definitions/file routes carry `route_path`. ASP.NET families (minimal API and the new attribute routes) deliberately keep `route_template`/`effective_route_template` — do NOT rename them to `route_path`; instead document in the contract that `effective_route_template` is the server-side join key (2026-07-01 codex review reconciliation).
- One `EXTRACTION_CONTRACT_VERSION` bump for this plan: append `.http-boundary-facts-v1` (`crates/julie-extractors/src/lib.rs:127`) and add the marker to `crates/julie-extractors/src/tests/api_surface.rs` (`test_public_contract_version_marks_current_fact_families`) with the first shape-changing task; verify in the release task.
- Grounding (razorback:grounding-in-current-docs): before Tasks 3 and 4, verify against current framework docs — Next.js route-handler verb export set, and Nuxt/Nitro `server/api` vs `server/routes` conventions and method-suffix list. Record the checked source in the task commit message.
- Tests prove behavior through emitted `structural_facts` rows; helper unit tests may supplement, never substitute.
- Default suite stays under the 90s tripwire.

---

## Decided Fact Contracts

### `http.client_request.v1` (references — client side)

Languages: `javascript`, `jsx`, `typescript`, `tsx`, `vue` (script sections). Metadata:

- `client`: `"fetch"` | `"axios"`
- `target_path`: URL string exactly as written
- `url_kind`: `"path"` (leading `/`) | `"relative"` | `"absolute"` (has scheme)
- `verb`: upper-case HTTP method
- `verb_source`: `"attested"` (explicit `method:`/axios verb method) | `"default"` (bare `fetch()` is GET per spec)
- `import_source`: present for axios (`"axios"`); absent for global `fetch`

Emission gates: URL argument is a plain static string literal. `fetch` needs no import (global). `axios.*` requires an `axios` import in the same file (mirrors React Router import-gating). Dynamic URLs stay silent.

### `nextjs.route_handler.v1` (definitions — server side)

Languages: `javascript`, `typescript` (route handlers are `route.js`/`route.ts`; `jsx`/`tsx` handler files are nonstandard — verify during grounding, and if valid, parity applies). Metadata: `router="app"`, `route_path`, `normalized_route_template`, `dynamic_segments`, `route_group_segments`, `parallel_route_segments`, `verb`, `verb_source="attested"`. One fact per exported verb handler (`export async function GET(...)`, `export const GET = ...`), span on the export so `containing_symbol_id` binds to the handler symbol.

### `nuxt.server_route.v1` (definitions — server side)

Languages: `javascript`, `typescript`. Files under `server/api/**` (route prefix `/api`) and `server/routes/**` (no prefix). `route_path` from the file path; method suffix (`users.get.ts`) sets `verb` + `verb_source="attested"`; no suffix omits `verb` (any method). Dynamic segments reuse the framework-aware Nuxt helpers (`[id]`, `[[id]]` optional, `[...slug]` catch-all). Emission gate: file contains a `defineEventHandler`-family identifier (use the existing executable-identifier signal helper).

### `aspnet.attribute_route.v1` (definitions — server side)

Language: `csharp`. Method-level facts for `[HttpGet]`/`[HttpPost]`/`[HttpPut]`/`[HttpPatch]`/`[HttpDelete]`/`[HttpHead]`/`[HttpOptions]` and `[Route("...")]` on action methods, plus class-level `[Route]`/`[controller]` prefix facts. Metadata:

- `attribute_kind`: `"http_method"` | `"route"` | `"controller_route"`
- `verb`: for `http_method` facts
- `route_template`: literal template as written (absent for bare `[HttpGet]`)
- `controller_route_template`: nearest class-level template, on method-level facts
- `effective_route_template`: joined template with `[controller]` replaced by the class name minus the `Controller` suffix and `[action]` by the method name
- `route_tokens`: tokens that were substituted (e.g. `["controller"]`), so consumers know the substitution happened

Static literal templates only; computed templates stay silent. Key naming intentionally matches `aspnet.minimal_api.route.v1` (`route_template`/`effective_route_template`, not the frontend `route_path` rule) so all ASP.NET families stay contract-consistent; the contract doc must state that `effective_route_template` is the join key Miller matches client `target_path` values against.

### `htmx.attribute.v1` coverage extension

Add `javascript`, `jsx`, `typescript`, `tsx` (JSX attributes) and `vue` (template section) to the existing family. Same fact shape, same `hx-*`/`data-hx-*` normalization. Attributes with brace-expression values (`hx-get={url}`) are dynamic and stay silent.

## File Structure

- Create: `crates/julie-extractors/src/base/web_structural_facts/http_client.rs` — `http.client_request.v1`
- Modify: `crates/julie-extractors/src/base/web_structural_facts/nextjs_nuxt.rs` — route handlers + server routes
- Modify: `crates/julie-extractors/src/base/web_structural_facts/mod.rs` — pattern ids, dispatch, capability arms
- Modify: `crates/julie-extractors/src/base/framework_structural_facts.rs` — ASP.NET attribute routes, htmx language arms
- Modify: `crates/julie-extractors/src/lib.rs`, `crates/julie-extractors/src/tests/api_surface.rs` — contract marker
- Modify: `docs/contracts/jsonl-v3.md`, `docs/contracts/sqlite-schema-v3.md` — pattern tables + metadata keys
- Modify: `fixtures/extraction/**`, `fixtures/extraction/capabilities.json` — golden + capability evidence per task
- Test: `crates/julie-extractors/src/tests/{react,vue,nuxt,razor}/structural_facts.rs`, `crates/julie-extractors/src/tests/structural_facts.rs`, new `crates/julie-extractors/src/tests/http_client/`

## Task 1: `http.client_request.v1` — fetch (JS family)

**Files:**
- Create: `crates/julie-extractors/src/base/web_structural_facts/http_client.rs`
- Modify: `crates/julie-extractors/src/base/web_structural_facts/mod.rs` (pattern id const, dispatch call, capability arms for js/jsx/ts/tsx)
- Modify: `crates/julie-extractors/src/lib.rs` + `crates/julie-extractors/src/tests/api_surface.rs` (marker `.http-boundary-facts-v1`)
- Modify: `docs/contracts/jsonl-v3.md`, `docs/contracts/sqlite-schema-v3.md` (family contract)
- Modify: `crates/julie-extractors/src/tests/mod.rs` (add `pub mod http_client;` — the test tree is an explicit module list at `tests/mod.rs:5`; an unregistered module silently never runs)
- Test: `crates/julie-extractors/src/tests/http_client/mod.rs` + golden fixtures `fixtures/extraction/{javascript,jsx,typescript,tsx}/http_client_fetch/`

**Interfaces:**
- Consumes: `js_object_scan::parse_js_string_literal` and `find_matching_paren`, `fact_builders`, tree-sitter comment/string guards from the split module.
- Produces: `pub(super) fn collect_http_client_requests(language, tree, file_path, content) -> Vec<StructuralFact>` — Task 2 extends it for axios and Vue.

**What to build:** Scan for `fetch(` call sites with a static string first argument; parse an optional options-object `method:` string property for the attested verb (reuse `parse_object_string_property`); emit per the decided contract. Guard matches with the smallest-covering-node comment/string rejection, consistent with the ASP.NET collector.

**Approach:** Negative cases that must stay silent: `fetch(url)` with identifier arg, template literal with `${}`, `fetch` as a property (`obj.fetch(`), matches inside comments/strings. `verb_source="default"` for bare fetch.

**Acceptance criteria:**
- [x] `fetch("/api/messages")` emits `target_path=/api/messages`, `verb=GET`, `verb_source=default`, `client=fetch`, `url_kind=path` in all four JS-family languages, proven via emitted rows.
- [x] `fetch("/api/messages", { method: "POST" })` emits `verb=POST`, `verb_source=attested`.
- [x] Negative cases emit nothing.
- [x] Marker bump + api_surface list updated; contract docs describe the family.
- [x] Capability rows + golden fixtures for all four languages; strict data-quality report clean.
- [x] Worker-scope verification passes, committed.

## Task 2: `http.client_request.v1` — axios + Vue script sections

**Files:**
- Modify: `crates/julie-extractors/src/base/web_structural_facts/http_client.rs`
- Modify: `crates/julie-extractors/src/base/web_structural_facts/mod.rs` (vue dispatch: run the client-request scan over Vue script sections; add `http.client_request.v1` to the vue capability arm)
- Modify: `docs/contracts/jsonl-v3.md` (axios + vue coverage notes)
- Test: extend `tests/http_client/` + golden fixtures `fixtures/extraction/vue/http_client/` and axios fixtures per JS language

**Interfaces:**
- Consumes: `js_imports::collect_js_imports` (extend the import index with an `axios` default/named import entry), `vue.rs` section scanning for script ranges.
- Produces: the complete client-request collector used by the release task.

**What to build:** `axios.get/post/put/patch/delete/head/options("literal")` and `axios("literal", { method: "..." })`, gated on an `axios` import in the same file (or Vue script section). Vue: run the scan over `<script>`/`<script setup>` section ranges only.

**Approach:** The awaited-callee pitfall from TODO item 5 (`await axios.get<T>` callee text) applies to literal carriers, not this scanner — this scanner matches source text directly, so `await axios.get<T>("/x")` must still match; add it as a test case. A file using `axios.*` without an axios import stays silent.

**Acceptance criteria:**
- [x] `await axios.get<Msg[]>("/api/messages/active")` emits `client=axios`, `verb=GET`, `verb_source=attested`, `import_source=axios` (TS + TSX).
- [x] Axios calls without an axios import emit nothing.
- [x] Vue SFC `<script setup>` fetch/axios calls emit; template section content does not produce client-request facts.
- [x] Capability parity across javascript/jsx/typescript/tsx/vue; strict report clean.
- [x] Worker-scope verification passes, committed.

## Task 3: `nextjs.route_handler.v1`

**Files:**
- Modify: `crates/julie-extractors/src/base/web_structural_facts/nextjs_nuxt.rs` (route-handler collection beside `nextjs_app_file_route`, which currently gates on `stem == "page"` and must stay unchanged for `nextjs.file_route.v1`)
- Modify: `crates/julie-extractors/src/base/web_structural_facts/mod.rs` (pattern id, dispatch, capability arms)
- Modify: `docs/contracts/jsonl-v3.md`, `docs/contracts/sqlite-schema-v3.md`
- Test: `crates/julie-extractors/src/tests/react/structural_facts.rs` + golden fixtures `fixtures/extraction/{javascript,typescript}/nextjs_route_handler/`

**Interfaces:**
- Consumes: the app-router segment parser (route groups, parallel segments, interception markers) — factor the segment walk out of `nextjs_app_file_route` so `page` and `route` stems share it rather than duplicating.
- Produces: `nextjs.route_handler.v1` facts per the decided contract.

**What to build:** For `app/**/route.{js,ts}` files, parse the route path with the shared segment logic, then scan for exported verb handlers (`export async function GET`, `export function GET`, `export const GET =`). One fact per exported verb, span on the export.

**Approach:** Grounding check first: confirm the current supported verb-export set in Next.js docs (GET/POST/PUT/PATCH/DELETE/HEAD/OPTIONS at time of planning) and whether `jsx`/`tsx` route files are valid — extend language coverage only if so. Re-exported or dynamically generated handlers stay silent.

**Acceptance criteria:**
- [x] `app/api/users/[id]/route.ts` with `export async function GET` and `export const DELETE =` emits two facts with `route_path=/api/users/[id]`, `normalized_route_template=/api/users/:id`, verbs `GET` and `DELETE`.
- [x] `nextjs.file_route.v1` goldens are byte-identical (page behavior untouched).
- [x] Contract docs + capability rows + goldens for claimed languages; strict report clean.
- [x] Worker-scope verification passes, committed.

## Task 4: `nuxt.server_route.v1`

**Files:**
- Modify: `crates/julie-extractors/src/base/web_structural_facts/nextjs_nuxt.rs`
- Modify: `crates/julie-extractors/src/base/web_structural_facts/mod.rs` (pattern id, dispatch, capability arms for javascript/typescript)
- Modify: `docs/contracts/jsonl-v3.md`, `docs/contracts/sqlite-schema-v3.md`
- Test: `crates/julie-extractors/src/tests/nuxt/structural_facts.rs` + golden fixtures `fixtures/extraction/{javascript,typescript}/nuxt_server_route/`

**Interfaces:**
- Consumes: the framework-aware Nuxt dynamic-segment helpers (`nuxt_dynamic_segment_metadata`, `parse_nuxt_dynamic_part`) and the executable-identifier signal helper.
- Produces: `nuxt.server_route.v1` facts per the decided contract.

**What to build:** File-path parsing for `server/api/**` and `server/routes/**` with method-suffix extraction, gated on a `defineEventHandler`-family identifier in the file. `server/api/users/[id].get.ts` → `route_path=/api/users/[id]`, `normalized_route_template=/api/users/:id`, `verb=GET`.

**Approach:** Grounding check first: confirm Nitro's directory conventions and the full method-suffix list in current Nuxt docs. Note the existing Nuxt file-route collector deliberately excludes `server/**` — this task claims that excluded space with its own family; do not change `nuxt.file_route.v1` emission.

**Acceptance criteria:**
- [x] API and non-API server routes emit correct paths; method suffix sets attested verb; suffix-less handler omits `verb`.
- [x] Optional (`[[id]]`) and catch-all (`[...slug]`) segments normalize per the Nuxt flavor rules.
- [x] A `server/api`-shaped file without `defineEventHandler` stays silent; `nuxt.file_route.v1` goldens byte-identical.
- [x] Contract docs + capability rows + goldens; strict report clean.
- [x] Worker-scope verification passes, committed.

## Task 5: `aspnet.attribute_route.v1`

**Files:**
- Modify: `crates/julie-extractors/src/base/framework_structural_facts.rs` (new collector beside `collect_aspnet_minimal_api_routes` :352; reuse `parse_csharp_string_literal` :806 and `join_route_templates` :596)
- Modify: `docs/contracts/jsonl-v3.md`, `docs/contracts/sqlite-schema-v3.md`
- Test: `crates/julie-extractors/src/tests/razor/structural_facts.rs` or a new csharp structural-facts test module + golden fixture `fixtures/extraction/csharp/aspnet_attribute_routes/`

**Interfaces:**
- Consumes: existing C# string parsing + template joining helpers; tree-sitter spans for attribute→method/class attribution.
- Produces: `aspnet.attribute_route.v1` facts per the decided contract.

**What to build:** Scan C# attribute lists for `Route`/`Http*` attributes on classes and methods. Class-level `[Route("api/[controller]")]` emits a `controller_route` fact; each attributed action method emits an `http_method` (or `route`) fact carrying `controller_route_template` and the substituted `effective_route_template`.

**Approach:** Use tree-sitter to find attribute nodes and their owning class/method declarations rather than raw text association — attribution correctness is the risk here. `[controller]` substitution uses the class identifier minus a trailing `Controller`; `[action]` uses the method identifier; record substitutions in `route_tokens`. Attributes with non-literal arguments stay silent. `ApiController` without route attributes emits nothing (conventional routing is out of scope — document this exclusion in the contract).

**Acceptance criteria:**
- [x] `[Route("api/[controller]")]` class + `[HttpGet("{id}")]` method emits `effective_route_template=/api/users/{id}` for `UsersController.Get`, with `route_tokens=["controller"]`.
- [x] Bare `[HttpPost]` emits verb with controller-level effective template.
- [x] Minimal-API goldens byte-identical; conventional-routing exclusion documented.
- [x] Contract docs + capability rows + goldens; strict report clean.
- [x] Worker-scope verification passes, committed.

## Task 6: htmx Coverage in JSX and Vue Templates

**Files:**
- Modify: `crates/julie-extractors/src/base/framework_structural_facts.rs` (language arms for javascript/jsx/typescript/tsx/vue in `collect_framework_structural_facts` :44 and `framework_structural_fact_pattern_ids_for_language` :70)
- Modify: `crates/julie-extractors/src/base/web_structural_facts/vue.rs` (expose template-section ranges `pub(crate)` for the framework collector)
- Modify: `docs/contracts/jsonl-v3.md` (coverage note)
- Test: focused tests + golden fixtures `fixtures/extraction/{jsx,tsx}/htmx_attributes/`, `fixtures/extraction/vue/htmx_attributes/`

**Interfaces:**
- Consumes: `base::markup_scan` (shared scanner from the split plan), Vue section ranges.
- Produces: `htmx.attribute.v1` facts from JSX/TSX markup and Vue template sections, same shape as html/razor emission.

**What to build:** Run the markup attribute scan over JSX-bearing sources and Vue template sections, keeping only `hx-*`/`data-hx-*` attributes with static string values. Brace-expression values stay silent.

**Approach:** Language parity: claim all four JS-family languages plus vue together. Plain `javascript`/`typescript` files without JSX will simply emit nothing — the capability claim needs jsx/tsx fixtures and a decision recorded in capabilities.json evidence for js/ts (fixture with JSX-in-js if the parser accepts it; otherwise claim jsx/tsx/vue only — decide against parser reality, and record which).

**Acceptance criteria:**
- [x] `<button hx-post="/clicked">` in a TSX component emits the same fact shape as in HTML.
- [x] `data-hx-get` normalization holds in the new languages; dynamic values silent.
- [x] html/razor htmx goldens byte-identical; capability matrix + strict report clean.
- [x] Worker-scope verification passes, committed.

## Task 7: Contract Sweep + Release for Miller Pin Bump

**Files:**
- Modify: `docs/contracts/jsonl-v3.md`, `docs/contracts/sqlite-schema-v3.md` (final key-by-key audit of the four new families against emission)
- Modify: version metadata + release notes per `docs/release.md`

**Interfaces:**
- Consumes: Tasks 1–6 merged and green.
- Produces: a tagged release consumable by Miller's `scripts/julie-pins.json` bump, and a Miller-side companion-plan handoff for the new fetch↔handler bridge.

**What to build:** Audit every documented metadata key for the new families against actual emission (extend the emission-agreement test pattern from the 2026-07-01 hardening Task 8). Cut the release; notes must call out: four new pattern ids, htmx language expansion, the `.http-boundary-facts-v1` marker, and the naming/verb policies the new families follow. Do not publish without explicit user approval.

**Acceptance criteria:**
- [x] Emission-agreement test covers the four new families.
- [x] `EXTRACTION_CONTRACT_VERSION` ends with `.http-boundary-facts-v1`; api_surface marker test green.
- [x] Branch gate green; release notes list consumer-visible changes.
- [ ] User approval obtained before publishing.
- [x] Miller handoff noted: companion plan to bridge `http.client_request.v1` ↔ `nextjs.route_handler.v1`/`nuxt.server_route.v1`/`aspnet.attribute_route.v1`/`aspnet.minimal_api.route.v1`.

## Verification Strategy

**Project source of truth:** `AGENTS.md` / `CLAUDE.md`, `xtask` test tiers.

**Worker red/green scope:** focused tests by exact name with fully qualified paths, e.g. `cargo test -p julie-extractors tests::http_client::<test_name> -- --nocapture` or `cargo test -p julie-extractors structural_facts::<new_test_name> -- --nocapture`. Workers must confirm the filter matched at least one test ("0 tests run" is a FAIL, not a pass — guards against unregistered test modules).

**Worker ceiling:** `cargo test -p julie-extractors structural_facts -- --nocapture` plus `cargo test -p julie-extractors test_public_contract_version_marks_current_fact_families -- --nocapture`.

**Worker gate invariant:** New facts emit with the decided metadata, negative cases stay silent, and no existing structural-fact family regresses (existing goldens byte-identical except files a task explicitly adds).

**Lead affected-change scope (after each task):**

```bash
cargo test -p julie-extractors structural_facts -- --nocapture
UPDATE_GOLDEN=1 cargo test -p julie-extractors --features test-golden golden_fixtures_match_canonical_extraction -- --nocapture
cargo test -p julie-extractors --features test-golden golden_fixtures_match_canonical_extraction -- --nocapture
cargo test -p julie-extractors --features test-capability-matrix capability_matrix -- --nocapture
node scripts/language-data-quality-report.mjs --strict
```

**Branch gate (before Task 7 release):**

```bash
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --no-deps -- -D warnings
git diff --check
node scripts/language-data-quality-report.mjs --strict
```

**Replay/metric evidence:** a real-repo CLI smoke scan (three-file fixture project per family, matching the pattern used for the 2026-06-09 ASP.NET/htmx slice) proving rows persist to SQLite — hard gate per family before its task closes; row counts are report-only.

**Escalation triggers:** SQLite schema changes (none expected — metadata-only), report shape changes, new parser dependencies, language-detection changes, default-suite runtime growth, or grounding checks contradicting the decided contracts (Tasks 3/4).

**Assigned verification failure:** Workers stop and report when assigned verification fails, unless this plan explicitly says to update that gate.

**Verification ledger:** Record invariant, command, scope label, commit SHA, result, and timestamp per task. Reuse passing evidence for the same HEAD.

## Model Routing

**Project source of truth:** repo `RAZORBACK.md`.

**Strategy tier:** contract shapes (decided in this plan), grounding-check adjudication for Tasks 3/4, emission-gate semantics disputes. Harness mapping: inherit.

**Implementation tier:** Tasks 1–6 implementation once contracts are locked (narrow file ownership, explicit ceilings, no parser-dependency changes). Harness mapping: inherit.

**Mechanical tier:** fixture file authoring only, and only when the fixture does not own the task's red/green gate. Harness mapping: inherit.

**Gate-interpretation reviewer:** lead reads plan + failing test + diff on any red/green dispute. Harness mapping: inherit.

**Escalation tier:** per RAZORBACK.md — capability-claim changes, contract doc changes, and anything touching `EXTRACTION_CONTRACT_VERSION` get lead review before commit. Harness mapping: inherit.

**Worker eligibility:** met once this plan's contracts stand; Tasks 3/4 workers must run the grounding check first and stop if docs contradict the decided contract.

**Escalation triggers:** grounding contradictions; golden diffs outside the task's own fixtures; capability-matrix failures.

**Mechanical exclusion:** Mechanical workers cannot own failing tests, replay evidence, or acceptance gates.

**Unsupported harness behavior:** If the harness cannot choose models per agent, use `inherit` and continue.
