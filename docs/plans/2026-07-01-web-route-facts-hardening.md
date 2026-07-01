# Web Route Facts Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Fix the extraction bugs and contract drift found in the 2026-07-01 cross-repo review of the htmx/vue/react/next/nuxt structural route facts, so Miller's bridge tracing receives complete and honest route evidence.

**Architecture:** All changes stay inside the existing structural-fact collectors (`web_structural_facts.rs`, `framework_structural_facts.rs`) and their contracts. No new fact families are introduced; existing `pattern_id`s gain correctness fixes, one gains new source languages (vue-router configs in plain JS/TS), and metadata contracts get reconciled with what is actually emitted. The consumer-side companion plan is `~/source/miller/docs/plans/2026-07-01-web-bridge-route-fact-fixes.md`; its Task 8 pin-bumps to the release cut from this plan.

**Tech Stack:** Rust, tree-sitter, julie-extractors structural facts pipeline, golden fixtures + capability matrix.

**Architecture Quality:** Affected modules are the web/framework structural-fact collectors and their fixtures/contract docs. External contract surface is `structural_facts` rows (SQLite + JSONL). Risk is medium: H1/H4 change span-scanning logic shared across Vue/React collection, and M2 changes when `nextjs.file_route.v1` emits at all.

## Global Constraints

- Contracts are API: any metadata key change must land in `docs/contracts/jsonl-v3.md` and `docs/contracts/sqlite-schema-v3.md` in the same task.
- Capability claims require golden fixture evidence in `fixtures/extraction/capabilities.json`; after capability or fixture changes run `node scripts/language-data-quality-report.mjs --strict` and keep `silent_cells` and `quality_bar_debts` at `0`.
- Language parity: a capability added for one language of a family (e.g. vue-router defs in `.ts`) must cover every language the family supports (`javascript`, `jsx`, `typescript`, `tsx`) before it is claimed.
- Default test suite stays fast; no real-world corpora in default tests.
- Dynamic/unresolvable route expressions stay silent (no guessed routes) — the existing doctrine.
- Any consumer-visible change to the extraction output shape (new metadata keys, changed normalization forms, changed emission gates, new language coverage for an existing family) must bump the `EXTRACTION_CONTRACT_VERSION` suffix in `crates/julie-extractors/src/lib.rs:127` (currently ending `web-route-facts-v2` → bump to `web-route-facts-v3`) **and** update the marker list in `crates/julie-extractors/src/tests/api_surface.rs:14` (`test_public_contract_version_marks_current_fact_families`) in the same commit. One bump for this plan is sufficient; land it with the first shape-changing task and verify it in Task 10.
- Tests prove behavior through emitted `structural_facts` rows (the caller-facing surface), not private helper unit tests alone. Helper-level tests may supplement but never substitute.
- `AGENTS.md`/`CLAUDE.md` sync: run `scripts/check-agent-doc-sync.sh` before committing any guideline change.

## Review Findings Being Addressed

From the 2026-07-01 cross-repo review (verified against the 2.5.9 binary; Miller-side consumption issues are in the companion Miller plan):

- **H1:** Vue SFC template scanning stops at the first `</template>`, so nested `<template>` elements (scoped slots) truncate the scan and route references after the nested close tag are lost.
- **H2:** Next.js parallel route segments (`@slot`) and intercepting segments (`(.)`, `(..)`, `(...)`, `(..)(..)`) leak into `route_path` instead of being stripped/recorded as metadata.
- **H3:** vue-router route definitions declared in plain `.js`/`.ts` router files are never extracted — `vue.route_definition.v1` only fires inside `.vue` SFCs, missing the dominant real-world pattern (`createRouter` in `router/index.ts`).
- **H4:** `find_enclosing_object_range` picks the wrong object when a nested object (e.g. `meta: { ... }`) precedes the `path` key, corrupting route-object attribution for Vue and React definitions.
- **M1:** `data-hx-*` prefixed htmx attributes are not recognized (scan matches only `hx-` prefix).
- **M2:** `nextjs.file_route.v1` false positives: plain React SPAs with a `src/pages/` directory are treated as Next.js pages-router projects.
- **M3:** Vue child routes with relative paths are dropped instead of being emitted with parent context.
- **M4:** Contract drift: documented `vue.route_reference.v1` metadata (e.g. `route_source`) does not match emission.
- **M5:** Verb inconsistency: some navigation reference patterns emit `verb=GET` metadata and others emit none.
- **M6:** Route-key naming inconsistency across fact families (`target_path` for references vs `route_path` for definitions/file routes) is undocumented as a deliberate contract rule.
- **M7:** Nuxt optional params (`[[id]]`) are normalized as if required.
- **Plan drift:** the React/Next plan promised `parent_route_path`/`effective_route_template` for nested React Router definitions; not emitted.

---

## File Structure

- Modify: `crates/julie-extractors/src/base/web_structural_facts.rs` — H1, H2, H3, H4, M2, M3, M5, M7, plan drift.
- Modify: `crates/julie-extractors/src/base/framework_structural_facts.rs` — M1.
- Modify: `docs/contracts/jsonl-v3.md`, `docs/contracts/sqlite-schema-v3.md` — M4, M5, M6 reconciliation.
- Modify: `fixtures/extraction/**` golden fixtures + `fixtures/extraction/capabilities.json` — every task adds fixture evidence.

## Task 1: H1 — Nested `<template>` Depth Tracking in Vue SFC Scan

**Files:**
- Modify: `crates/julie-extractors/src/base/web_structural_facts.rs` (Vue template range detection)
- Test: focused structural-facts tests + a Vue golden fixture with a scoped-slot nested template

**Interfaces:**
- Consumes: raw `.vue` source text.
- Produces: a template scan range that spans the full top-level `<template>` element regardless of nested `<template>` children.

**What to build:** Replace first-`</template>` termination with open/close depth tracking so nested `<template #slot>` blocks don't truncate the scan. Route references (`<router-link>`, `<NuxtLink>`) appearing after a nested template close must be captured.

**Approach:** Track nesting depth while scanning open/close template tags; ignore self-closing and tags inside comments/strings to the extent the current scanner already does. Fixture: a component with a scoped-slot table and a `<router-link to="/after-slot">` after the slot.

**Acceptance criteria:**
- [x] Route reference after a nested `</template>` is emitted.
- [x] Existing single-template fixtures unchanged (golden diff empty except the new fixture).
- [x] Worker-scope verification passes, committed.

## Task 2: H2 — Strip Parallel and Intercepting Segments from Next Route Paths

**Files:**
- Modify: `crates/julie-extractors/src/base/web_structural_facts.rs` (`nextjs_route_path_metadata` / segment handling)
- Modify: `docs/contracts/jsonl-v3.md` (new metadata keys)
- Test: focused tests + golden fixtures for `app/@modal/photo/page.tsx` and `app/feed/(..)photo/[id]/page.tsx`

**Interfaces:**
- Consumes: Next.js app-router file path segments.
- Produces: `route_path` free of `@slot` and interception markers; new metadata `parallel_route_segments` (slot names) and `intercepted_route` or equivalent for interception markers, documented in the contract.

**What to build:** Parallel segments (`@slot`) do not contribute to the URL — strip them from `route_path` and record slot names in metadata. Intercepting markers (`(.)`, `(..)`, `(..)(..)`, `(...)`) are matching conventions — strip the marker from the segment, keep the remaining segment in the route, and record that the route intercepts.

**Approach:** Extend the existing route-group (`(group)`) stripping logic; keep marker parsing table-driven and longest-match-first (`(...)` before `(..)` before `(.)`). Decide metadata key names against the contract doc, not ad hoc.

**Acceptance criteria:**
- [x] `app/@modal/login/page.tsx` emits `route_path=/login` with `parallel_route_segments=["modal"]`.
- [x] `app/feed/(..)photo/[id]/page.tsx` emits `route_path=/feed/photo/[id]` with interception metadata.
- [x] Contract docs describe both keys.
- [x] Worker-scope verification passes, committed.

## Task 3: H3 — vue-router Definitions in Plain JS/TS Router Files

**Files:**
- Modify: `crates/julie-extractors/src/base/web_structural_facts.rs` (route-definition collection entry points)
- Modify: `fixtures/extraction/capabilities.json` + golden fixtures per language
- Modify: `docs/contracts/jsonl-v3.md` (language coverage note for `vue.route_definition.v1`)
- Test: focused tests for `createRouter`/`routes:` arrays in `.js`, `.ts` (and `.jsx`/`.tsx` if the array shape is identical)

**Interfaces:**
- Consumes: JS/TS sources that import from `vue-router` and declare route arrays (`createRouter({ routes: [...] })` or exported `RouteRecordRaw[]`).
- Produces: `vue.route_definition.v1` facts from plain JS/TS files, gated on vue-router import evidence.

**What to build:** Extract vue-router route definitions from the standard `router/index.ts` pattern. Gate emission on an import from `vue-router` in the same file (mirrors how React Router detection is signal-gated) so generic `path:` objects in unrelated code stay silent.

**Approach:** Reuse the existing route-object walker used for in-SFC definitions; the new part is collector routing. Today JS/TS files enter web structural fact collection only through `collect_react_nextjs_structural_facts` (`web_structural_facts.rs:1073`), while Vue route definitions are collected only via the Vue SFC path — so this task must explicitly wire a vue-router definition pass into the JS/TS collection entry point (either inside `collect_react_nextjs_structural_facts` or as a sibling call in the language dispatch), gated on the `vue-router` import. The capability-matrix registration `web_structural_fact_pattern_ids_for_language` (`web_structural_facts.rs:94`) must also add `vue.route_definition.v1` to the `javascript`/`jsx`/`tsx` and `typescript` arms, or the capability matrix will not audit the new coverage. Language parity: cover `javascript`, `typescript`, `jsx`, `tsx` fixtures before claiming the capability.

**Acceptance criteria:**
- [x] JS/TS collector routing runs a vue-router definition pass (new sibling of or addition to `collect_react_nextjs_structural_facts`), gated on a `vue-router` import.
- [x] `web_structural_fact_pattern_ids_for_language` lists `vue.route_definition.v1` for `javascript`, `jsx`, `tsx`, and `typescript`.
- [x] `createRouter` route arrays in `.ts` and `.js` emit definitions with correct paths, proven via emitted `structural_facts` rows.
- [x] A file with `path:` objects but no `vue-router` import emits nothing.
- [x] Capability rows + golden fixtures added for all four languages; data-quality report strict-clean.
- [x] Worker-scope verification passes, committed.

## Task 4: H4 — Correct Enclosing-Object Selection for Route Objects

**Files:**
- Modify: `crates/julie-extractors/src/base/web_structural_facts.rs` (`find_enclosing_object_range`)
- Test: focused tests with `meta:`-first and `children:`-bearing route objects for both Vue and React definitions

**Interfaces:**
- Consumes: a `path` key position within a route-object literal.
- Produces: the range of the route object that directly owns the `path` key, even when nested objects (e.g. `meta: { ... }`) precede it or siblings contain their own braces.

**What to build:** Fix the wrong-object bug: walk outward from the `path` key using brace balance to find the immediately enclosing object literal, instead of the current scan that can lock onto a preceding nested object.

**Approach:** Since this helper serves both Vue and React collection, add regression tests for both. Include: `{ meta: { requiresAuth: true }, path: '/admin', component: Admin }` and a parent route whose `children` array contains objects with their own `meta` blocks.

**Acceptance criteria:**
- [x] `meta`-first route object attributes the fact to the correct object/component.
- [x] Nested `children` objects resolve to their own route objects, not the parent's.
- [x] Existing route-definition goldens unchanged except corrected cases.
- [x] Worker-scope verification passes, committed.

## Task 5: M1 — `data-hx-*` Attribute Recognition

**Files:**
- Modify: `crates/julie-extractors/src/base/framework_structural_facts.rs` (htmx attribute scan)
- Test: focused tests + html/razor golden fixture rows

**Interfaces:**
- Consumes: markup attributes named `data-hx-*` (htmx's W3C-valid alternate syntax) in `html` and `razor`.
- Produces: `htmx.attribute.v1` facts with `attribute_name` normalized to the canonical `hx-*` form plus a `data_prefix=true` metadata flag (or the raw name preserved in a separate key — pick one, document it).

**What to build:** Recognize `data-hx-get`, `data-hx-post`, etc., case-insensitively, everywhere `hx-*` is recognized today. Emit the canonical name so downstream consumers keep a single switch.

**Approach:** Normalize at match time (`strip data-` prefix after lowercasing); document the normalization in the contract. Update both `html` and `razor` fixtures.

**Acceptance criteria:**
- [x] `data-hx-post="/save"` emits the same fact shape as `hx-post="/save"` (modulo the raw-name metadata).
- [x] Mixed-case attributes normalize.
- [x] Contract documents the normalization rule.
- [x] Worker-scope verification passes, committed.

## Task 6: M2 — Suppress Next.js File-Route False Positives in Non-Next Repos

**Files:**
- Modify: `crates/julie-extractors/src/base/web_structural_facts.rs` (`nextjs_app_file_route` / pages-router detection)
- Test: focused tests + a React-SPA-shaped negative fixture (`src/pages/Home.tsx`, no Next signals)

**Interfaces:**
- Consumes: file paths under `pages/` / `app/` plus in-file signals.
- Produces: `nextjs.file_route.v1` only when Next.js evidence exists.

**What to build:** Stop emitting Next file routes for plain React SPAs that merely use a `pages/` directory. Require at least one Next.js signal before emitting pages-router facts: a `next`-family import in the file, a Next-only export (`getServerSideProps`, `getStaticProps`, `generateMetadata`, `metadata`), or an app-router convention filename (`page.tsx`, `layout.tsx`, `route.ts` under `app/`). App-router conventions are inherently Next-specific and keep emitting as today.

**Approach:** Extraction is per-file (no project-level `next.config.js` lookup in this pipeline), so the gate must be file-local; document the rule and its limits in the contract. If a real Next pages-router page has no in-file Next signal, it will be missed — state this trade-off explicitly in the contract doc rather than emitting false positives (silent-until-evidenced matches the existing doctrine for dynamic routes).

**Acceptance criteria:**
- [x] `src/pages/Home.tsx` in a React SPA (react-router imports, no Next signals) emits no `nextjs.file_route.v1`.
- [x] A pages-router file with `getStaticProps` or a `next/*` import still emits.
- [x] App-router files under `app/` are unaffected.
- [x] Contract documents the evidence gate and the known miss case.
- [x] Worker-scope verification passes, committed.

## Task 7: M3 + Plan Drift — Child Routes with Relative Paths and Parent Context

**Files:**
- Modify: `crates/julie-extractors/src/base/web_structural_facts.rs` (Vue and React children handling)
- Modify: `docs/contracts/jsonl-v3.md` (`parent_route_path`, `effective_route_template`)
- Test: focused tests + golden fixtures with nested `children` arrays (Vue and React)

**Interfaces:**
- Consumes: nested `children: [...]` route definitions with relative child paths (`'settings'` under `'/admin'`).
- Produces: child `*.route_definition.v1` facts emitted (not dropped) with `route_path` as written, plus `parent_route_path` and `effective_route_template` (the joined absolute template) — the metadata the 2026-06-30 React/Next plan promised but never shipped.

**What to build:** Emit relative-path children with enough parent context for a consumer to resolve the absolute route. Apply the same shape to Vue (`children` in route records) and React (`<Route>` nesting / `children` in `createBrowserRouter` objects) so the two frameworks stay contract-consistent.

**Approach:** Track the parent path stack while walking nested route objects; join with `/` semantics (relative child appends, absolute child resets — matching vue-router/react-router behavior). Do not attempt link-time resolution against references; that stays consumer-side.

**Acceptance criteria:**
- [x] Vue child `'settings'` under `'/admin'` emits with `parent_route_path=/admin` and `effective_route_template=/admin/settings`.
- [x] React nested route emits the same keys.
- [x] Absolute child paths reset the join.
- [x] Contract documents both keys for both frameworks.
- [x] Worker-scope verification passes, committed.

## Task 8: M4/M5/M6 — Contract Reconciliation and Verb Policy

**Files:**
- Modify: `docs/contracts/jsonl-v3.md`, `docs/contracts/sqlite-schema-v3.md`
- Modify: `crates/julie-extractors/src/base/web_structural_facts.rs` (verb emission alignment)
- Test: contract/emission agreement tests (extend the structural-facts test that asserts emitted metadata keys)

**Interfaces:**
- Consumes: current emitted metadata for all eight web route/navigation fact families — `vue.route_reference.v1`, `vue.route_definition.v1`, `react.route_reference.v1`, `react.route_definition.v1`, `nextjs.route_reference.v1`, `nextjs.file_route.v1`, `nuxt.route_reference.v1`, `nuxt.file_route.v1` — plus `htmx.attribute.v1` as related framework markup evidence.
- Produces: contract docs that match emission byte-for-byte, plus one documented verb policy across navigation reference patterns.

**What to build:** Three reconciliations. (a) M4: fix `vue.route_reference.v1` doc drift (`route_source` and any other key that diverges) by auditing every documented key against emission and correcting whichever side is wrong. (b) M5: standardize the verb policy — emit `verb=GET` on **all** navigation reference families (htmx keeps attested verbs; vue/react/next/nuxt references uniformly carry `verb=GET` documented as "implied navigation verb, not source-attested") or on none; pick one, document it, and note that Miller treats implied verbs as verb-unknown for confidence purposes (see the companion Miller plan Task 5). (c) M6: document the deliberate naming rule — references carry `target_path`, definitions/file routes carry `route_path` — so consumers stop guessing.

**Approach:** Source audit: enumerate emitted `insert_metadata` keys per pattern id, table them against the contract, fix drift. Keep this a docs+small-code task; no new keys beyond Tasks 2/7.

**Acceptance criteria:**
- [x] Every documented metadata key for the eight route families plus `htmx.attribute.v1` matches emission (spot-checked by test).
- [x] Verb policy is uniform across the four navigation reference families and documented.
- [x] `target_path` vs `route_path` rule documented.
- [x] Worker-scope verification passes, committed.

## Task 9: M7 — Nuxt Optional and Partial Dynamic Segments

**Files:**
- Modify: `crates/julie-extractors/src/base/web_structural_facts.rs` (`nextjs_route_path_metadata` at :1895 and `nextjs_dynamic_segment_metadata` at :1926 — shared by Next and Nuxt)
- Test: focused tests + Nuxt golden fixtures (`pages/users/[[id]].vue`, `pages/file-[name].vue`)

**Interfaces:**
- Consumes: Nuxt page paths with `[[id]]` (optional param) and mid-segment params (`file-[name]`).
- Produces: `normalized_route_template` marking optional params as optional (`:id?`) and handling mid-segment params without corrupting the segment.

**What to build:** Nuxt `[[id]]` is an optional parameter — normalize to `:id?` (and record it in `dynamic_segments` with an optional marker), not a required `:id`. Mid-segment params (`file-[name]`) either normalize faithfully (`file-:name`) or stay in bracket form with the case documented as unsupported for normalization — decide, then test the decision.

**Approach:** There is no separate Nuxt helper: both frameworks flow through the shared `nextjs_route_path_metadata` (`web_structural_facts.rs:1895`) → `nextjs_dynamic_segment_metadata` (`web_structural_facts.rs:1926`). Today `[[id]]` falls through the `[[...`/`[...` checks into the plain `[param]` arm and normalizes to a required `:id` for both frameworks. **Decision: make the helper framework-aware (pass a framework/flavor parameter) rather than duplicating it** — the bracket grammar is 90% shared, and a split would drift. In Nuxt flavor, `[[id]]` → optional `:id?`; in Next flavor, keep current behavior (`[[id]]` is not valid Next syntax — do not start rejecting it in this task, just don't change it). Check the Nuxt v4 routing docs for exact optional-param semantics before implementing (grounding note in the 2026-07-01 Nuxt plan).

**Acceptance criteria:**
- [x] `nextjs_dynamic_segment_metadata` (and its caller at :1895) is framework-aware; Nuxt and Next flavors are covered by separate tests.
- [x] `pages/users/[[id]].vue` emits an optional param in both `normalized_route_template` and `dynamic_segments` metadata.
- [x] Next.js file-route goldens are byte-identical after the change (Next flavor behavior unchanged).
- [x] Mid-segment param behavior is decided, implemented, and tested.
- [x] Contract documents the normalization forms.
- [x] Worker-scope verification passes, committed.

## Task 10: Release for Miller Pin Bump

**Files:**
- Modify: version metadata + release notes per this repo's release process

**Interfaces:**
- Consumes: Tasks 1–9 merged and green.
- Produces: a tagged patch release consumable by Miller's `scripts/julie-pins.json` bump (companion Miller plan Task 8).

**What to build:** Cut the release after the branch gate passes. Release notes must call out consumer-visible changes: new metadata keys (Task 2, 7), the M2 emission gate (facts that stop emitting), the M5 verb policy, and `data-hx-*` normalization. Do not publish without explicit user approval.

**Acceptance criteria:**
- [x] Branch gate green (see Verification Strategy).
- [x] `EXTRACTION_CONTRACT_VERSION` ends in `web-route-facts-v3` and `api_surface.rs` marker list matches (per Global Constraints).
- [x] Release notes list contract-visible changes.
- [ ] User approval obtained before publishing.
- [x] Miller pin-bump handoff noted (companion plan Task 8).

**Miller handoff:** after an approved `v2.5.10` publication, update the companion Miller plan Task 8 to pin `scripts/julie-pins.json` to `julie-extractors` `v2.5.10`.

## Verification Strategy

**Project source of truth:** `AGENTS.md` / `CLAUDE.md` and existing Cargo test features.

**Worker red/green scope:** Focused tests by exact test name, e.g.:

```bash
cargo test -p julie-extractors structural_facts::<new_test_name> -- --nocapture
```

**Worker ceiling:**

```bash
cargo test -p julie-extractors structural_facts -- --nocapture
cargo test -p julie-extractors test_public_contract_version_marks_current_fact_families -- --nocapture
```

**Worker gate invariant:** New/changed route facts emit with correct metadata, negative cases stay silent, and no other structural-fact family regresses.

**Lead affected-change scope:**

```bash
cargo test -p julie-extractors structural_facts -- --nocapture
UPDATE_GOLDEN=1 cargo test -p julie-extractors --features test-golden golden_fixtures_match_canonical_extraction -- --nocapture
cargo test -p julie-extractors --features test-golden golden_fixtures_match_canonical_extraction -- --nocapture
cargo test -p julie-extractors --features test-capability-matrix capability_matrix -- --nocapture
node scripts/language-data-quality-report.mjs --strict
```

**Branch gate (before Task 10):**

```bash
cargo test --workspace
cargo fmt --check
git diff --check
node scripts/language-data-quality-report.mjs --strict
```

**Escalation triggers:** SQLite schema changes, JSONL row-shape changes beyond documented metadata keys, new parser dependencies, language-detection changes, or unexpected default-suite runtime growth.

**Assigned verification failure:** Workers stop and report when assigned verification fails, unless this plan explicitly says to update that gate.

**Verification ledger:** Record invariant, command, scope label, commit SHA, result, and timestamp per task. Reuse passing evidence for the same HEAD instead of rerunning expensive gates.

## Model Routing

**Project source of truth:** repo `RAZORBACK.md` if present; otherwise harness default.

**All tiers:** `inherit` unless the executing harness supports per-agent model selection (Cursor is IDE-level; note the limitation and continue). Escalate to the session lead for Task 6 (emission-gate semantics) and Task 8 (verb policy) decisions if code reality contradicts this plan.
