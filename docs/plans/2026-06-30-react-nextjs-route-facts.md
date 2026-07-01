# React and Next.js Route Facts Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Emit versioned structural facts for common React Router and Next.js route definitions and static navigation references.

**Architecture:** Keep `julie-extractors` extraction-only. React Router and Next.js recognition should produce deterministic `structural_facts` from local syntax and file paths; downstream tools decide how to join definitions, references, symbols, and files. Do not add route matching, app graph construction, semantic search, watcher, dashboard, or Miller-specific bridge behavior here.

**Tech Stack:** Rust 2024, existing tree-sitter-backed JavaScript/JSX/TypeScript/TSX extraction, source-backed path parsing for Next.js file routes, existing `StructuralFact` contract, SQLite/JSONL artifact output, golden/capability tests.

**Architecture Quality:** Affected modules are the web structural fact collector and public contract docs. Caller-facing interface is limited to new structural fact pattern IDs and metadata keys. Test surface is canonical extraction through existing pipeline helpers and fixture goldens. No new parser dependencies or cross-file resolver seams are approved. Architecture risk: medium, because this changes public capability claims and emits new contract rows.

## Global Constraints

- This repo owns extraction only: do not add bridge resolution, trace, MCP, search, dashboard, daemon, watcher-service, or semantic behavior.
- Do not add runtime React, React Router, or Next.js dependencies.
- New facts must be deterministic from a single file's content and path.
- Only emit facts for static string literals and statically recoverable object `pathname` values in this patch.
- Do not infer dynamic route targets from variables, template expressions, spread objects, conditional expressions, or runtime functions.
- Every new pattern ID must be backed by focused tests, golden fixture evidence, and `fixtures/extraction/capabilities.json`.
- Metadata names must describe source facts, not Miller internals.
- Keep facts sorted by existing conventions.
- Do not publish or release from this plan unless separately approved.

---

## Current Docs Grounding

Verified against official docs on 2026-06-30:

- React Router `Link` currently uses `to` for client-side navigation: https://reactrouter.com/api/components/Link
- React Router `NavLink` wraps `Link` and also uses `to`: https://reactrouter.com/api/components/NavLink
- React Router `Route` uses `path`, `index`, `element`, and `Component`: https://reactrouter.com/api/components/Route
- React Router `createBrowserRouter` takes `RouteObject[]`: https://reactrouter.com/api/data-routers/createBrowserRouter
- Next.js App Router uses file-system routing from `app/**/page` files, folders as URL segments, and dynamic folders like `[slug]`: https://nextjs.org/docs/app/getting-started/layouts-and-pages
- Next.js route groups are folders wrapped in parentheses and are not included in the URL path: https://nextjs.org/docs/app/api-reference/file-conventions/route-groups
- Next.js `Link` uses required `href` and supports string or object values with `pathname`: https://nextjs.org/docs/pages/api-reference/components/link

## Target Files

- Modify: `crates/julie-extractors/src/base/web_structural_facts.rs`
- Modify: `crates/julie-extractors/src/tests/mod.rs`
- Create: `crates/julie-extractors/src/tests/react/structural_facts.rs`
- Create: `crates/julie-extractors/src/tests/react/mod.rs`
- Modify: `crates/julie-extractors/src/lib.rs`
- Modify: `crates/julie-extractors/src/tests/api_surface.rs`
- Modify: `fixtures/extraction/capabilities.json`
- Modify: `fixtures/extraction/javascript/structural_facts/source.js`
- Modify: `fixtures/extraction/javascript/structural_facts/expected.json`
- Create: `fixtures/extraction/javascript/nextjs_file_route/app/blog/page.js`
- Create: `fixtures/extraction/javascript/nextjs_file_route/expected.json`
- Modify: `fixtures/extraction/jsx/structural_facts/source.jsx`
- Modify: `fixtures/extraction/jsx/structural_facts/expected.json`
- Create: `fixtures/extraction/jsx/nextjs_file_route/app/blog/page.jsx`
- Create: `fixtures/extraction/jsx/nextjs_file_route/expected.json`
- Modify: `fixtures/extraction/typescript/structural_facts/source.ts`
- Modify: `fixtures/extraction/typescript/structural_facts/expected.json`
- Create: `fixtures/extraction/typescript/nextjs_file_route/pages/api/status.ts`
- Create: `fixtures/extraction/typescript/nextjs_file_route/expected.json`
- Modify: `fixtures/extraction/tsx/structural_facts/source.tsx`
- Modify: `fixtures/extraction/tsx/structural_facts/expected.json`
- Create: `fixtures/extraction/tsx/nextjs_file_route/app/(marketing)/blog/[slug]/page.tsx`
- Create: `fixtures/extraction/tsx/nextjs_file_route/expected.json`
- Modify: `docs/contracts/jsonl-v3.md`
- Modify: `docs/contracts/sqlite-schema-v3.md`
- Modify: `docs/plans/2026-06-09-structural-facts-design.md`

## Fact Contract Additions

### React Router Route References

Add `react.route_reference.v1` for static navigation references in JSX/TSX.

Preferred contract:

- `pattern_id`: `react.route_reference.v1`
- `language`: `jsx` or `tsx`
- `capture_name`: `route_reference`
- `metadata`:
  - `framework = "react"`
  - `library = "react_router"`
  - `query_family = "frontend_navigation"`
  - `target_path = "/dashboard"` or `"settings"` exactly as written
  - `attribute_name = "to"`
  - `component_name = "Link"` or `"NavLink"`
  - `import_source = "react-router"`, `"react-router-dom"`, or `"@remix-run/react"` when proven by import
  - `route_source = "string_literal"`
  - `source_kind = "react_router_link"`

Only emit for imported or locally aliased React Router components. Do not emit for unrelated `Link` components.

### React Router Route Definitions

Add `react.route_definition.v1` for static React Router route declarations.

Preferred contract:

- `pattern_id`: `react.route_definition.v1`
- `language`: `javascript`, `jsx`, `typescript`, or `tsx`
- `capture_name`: `route_definition`
- `metadata`:
  - `framework = "react"`
  - `library = "react_router"`
  - `query_family = "frontend_navigation"`
  - `route_path = "/dashboard"` or `"settings"` exactly as written
  - `route_source = "string_literal"` or `"index_route"`
  - `source_kind = "jsx_route"` or `"route_object"`
  - `route_component = "Dashboard"` when `Component: Dashboard` or `element={<Dashboard />}` is statically recoverable
  - `route_id = "dashboard"` when an `id` string literal is present
  - `index_route = true` when `index` is present and true
  - `parent_route_path` and `effective_route_template` only when nested static paths are recoverable within the same literal route tree

Start with:

- `<Route path="/dashboard" element={<Dashboard />} />`
- `<Route index element={<Home />} />`
- `<Route path="settings" Component={Settings} />`
- `createBrowserRouter([{ path: "/dashboard", element: <Dashboard /> }])`
- `useRoutes([{ path: "/dashboard", element: <Dashboard /> }])`
- `createRoutesFromElements(<Route path="/dashboard" element={<Dashboard />} />)` when practical in the same scanner

### Next.js Route References

Add `nextjs.route_reference.v1` for static `next/link` references.

Preferred contract:

- `pattern_id`: `nextjs.route_reference.v1`
- `language`: `jsx` or `tsx`
- `capture_name`: `route_reference`
- `metadata`:
  - `framework = "nextjs"`
  - `query_family = "frontend_navigation"`
  - `target_path = "/dashboard"`
  - `attribute_name = "href"`
  - `component_name = "Link"`
  - `import_source = "next/link"`
  - `route_source = "string_literal"` or `"object_pathname_literal"`
  - `source_kind = "next_link"`

Handle:

```tsx
import Link from "next/link";

<Link href="/dashboard">Dashboard</Link>;
<Link href={{ pathname: "/about", query: { name: "test" } }}>About</Link>;
```

Do not emit object `href` values without a static string `pathname`.

### Next.js File Route Definitions

Add `nextjs.file_route.v1` for Next.js routes derived from file paths.

Preferred contract:

- `pattern_id`: `nextjs.file_route.v1`
- `language`: `javascript`, `jsx`, `typescript`, or `tsx`
- `capture_name`: `file_route`
- `metadata`:
  - `framework = "nextjs"`
  - `query_family = "frontend_navigation"`
  - `router = "app"` or `"pages"`
  - `file_convention = "page"`
  - `route_path = "/blog/[slug]"` for source-faithful output
  - `normalized_route_template = "/blog/:slug"` when dynamic segments exist
  - `dynamic_segments = ["slug"]` when dynamic segments exist
  - `source_kind = "nextjs_file_route"`

Start with page routes:

- `app/page.tsx` -> `/`
- `app/blog/page.tsx` -> `/blog`
- `app/blog/[slug]/page.tsx` -> `/blog/[slug]` and normalized `/blog/:slug`
- `pages/index.tsx` -> `/`
- `pages/dashboard.tsx` -> `/dashboard`
- `pages/blog/[slug].tsx` -> `/blog/[slug]` and normalized `/blog/:slug`

Explicitly exclude Next.js API routes and route handlers from this patch unless the implementation can add them with the same contract quality and focused tests. If included, use a separate `file_convention = "route"` and do not mix them with page facts.

## Task 1: React Router Facts

**Files:**
- Modify: `crates/julie-extractors/src/base/web_structural_facts.rs`
- Create: `crates/julie-extractors/src/tests/react/structural_facts.rs`
- Create/modify module wiring as needed under `crates/julie-extractors/src/tests/`

**Interfaces:**
- Consumes: `collect_web_structural_facts(language, tree, file_path, content, symbols)` and existing source/span helpers in `web_structural_facts.rs`.
- Produces: `react.route_reference.v1` and `react.route_definition.v1` for JavaScript/JSX/TypeScript/TSX where static facts are present.

**What to build:** Recognize imported React Router navigation components and static route declarations without treating arbitrary `Link` names as React Router. Tests should exercise TSX and JSX at minimum, plus route-object declarations in TS or JS.

**Approach:**

- Add React pattern ID constants and include them in web pattern IDs where fixture evidence proves emission. Plain TypeScript advertises route definitions but not JSX link references.
- Add import scanning for `react-router`, `react-router-dom`, and `@remix-run/react`, including aliases such as `Link as RouterLink`.
- Emit references only for imported local component names and static `to="..."` string attributes.
- Emit route definitions for JSX `<Route>` elements and route-object literals with static `path` or static `index`.
- Recover component names only from simple `element={<Name />}` or `Component={Name}` shapes.
- Leave dynamic `to`, dynamic `path`, spread route objects, lazy route functions, and template literals unreported.

**Acceptance criteria:**

- [x] `<Link to="/dashboard">` imported from React Router emits `react.route_reference.v1`.
- [x] `<NavLink to="/settings">` imported from React Router emits `react.route_reference.v1`.
- [x] A non-React-Router `Link` component does not emit a route reference.
- [x] `<Route path="/dashboard" element={<Dashboard />} />` emits `react.route_definition.v1`.
- [x] `<Route index element={<Home />} />` emits `react.route_definition.v1` with `index_route=true`.
- [x] `createBrowserRouter([{ path: "/dashboard", element: <Dashboard /> }])` emits `react.route_definition.v1`.
- [x] Static component names are captured when recoverable.
- [x] Dynamic references and dynamic route paths do not emit static facts.
- [x] Worker-scope verification passes.

## Task 2: Next.js Facts

**Files:**
- Modify: `crates/julie-extractors/src/base/web_structural_facts.rs`
- Create/modify focused React/Next structural fact tests under `crates/julie-extractors/src/tests/`

**Interfaces:**
- Consumes: file path passed to `collect_web_structural_facts` and local source text.
- Produces: `nextjs.route_reference.v1` from `next/link` usage and `nextjs.file_route.v1` from App Router and Pages Router page files.

**What to build:** Recognize static Next.js `Link` targets only when `Link` is imported from `next/link`, and derive page routes from `app/**/page.*` and `pages/**` file paths.

**Approach:**

- Add Next.js pattern ID constants and include them in web pattern IDs where fixture evidence proves emission. Plain TypeScript advertises file routes but not JSX link references.
- Require a default or named import from `next/link` before emitting `nextjs.route_reference.v1`.
- Handle `href="/path"` and `href={{ pathname: "/path" }}`.
- Derive `nextjs.file_route.v1` from `file_path`, not repository scans.
- Normalize App Router route groups conservatively:
  - Include public URL segments for ordinary folders.
  - Exclude route groups like `(marketing)` from `route_path` while retaining optional metadata `route_group_segments=["marketing"]`.
- Derive dynamic segment metadata from `[slug]`, `[...slug]`, and `[[...slug]]` when present.

**Acceptance criteria:**

- [x] `<Link href="/dashboard">` imported from `next/link` emits `nextjs.route_reference.v1`.
- [x] `<Link href={{ pathname: "/about" }}>` emits `nextjs.route_reference.v1` with `route_source=object_pathname_literal`.
- [x] A `Link` component not imported from `next/link` does not emit a Next.js route reference.
- [x] `app/page.tsx` emits `nextjs.file_route.v1` with `route_path=/`.
- [x] `app/blog/[slug]/page.tsx` emits `route_path=/blog/[slug]`, `normalized_route_template=/blog/:slug`, and `dynamic_segments=["slug"]`.
- [x] `pages/dashboard.tsx` emits `nextjs.file_route.v1` with `route_path=/dashboard`.
- [x] Dynamic `href` and missing static `pathname` object values do not emit static facts.
- [x] Worker-scope verification passes.

## Task 3: Golden, Capability, and Public Contract Evidence

**Files:**
- Modify: `fixtures/extraction/capabilities.json`
- Modify: `fixtures/extraction/javascript/structural_facts/source.js`
- Modify: `fixtures/extraction/javascript/structural_facts/expected.json`
- Modify: `fixtures/extraction/jsx/structural_facts/source.jsx`
- Modify: `fixtures/extraction/jsx/structural_facts/expected.json`
- Modify: `fixtures/extraction/typescript/structural_facts/source.ts`
- Modify: `fixtures/extraction/typescript/structural_facts/expected.json`
- Modify: `fixtures/extraction/tsx/structural_facts/source.tsx`
- Modify: `fixtures/extraction/tsx/structural_facts/expected.json`
- Modify: `crates/julie-extractors/src/lib.rs`
- Modify: `crates/julie-extractors/src/tests/api_surface.rs`

**Interfaces:**
- Consumes: new pattern IDs from Tasks 1 and 2.
- Produces: fixture-backed capability claims and a public contract marker.

**What to build:** Pin the new facts in fixture output and advertise only the languages where the facts actually emit.

**Approach:**

- Add new examples to existing `structural_facts` fixtures rather than creating a separate fixture family unless existing files become noisy.
- Update `fixtures/extraction/capabilities.json` for each language that emits a pattern.
- Add an `EXTRACTION_CONTRACT_VERSION` marker such as `react-nextjs-route-facts-v1`.
- Update the API-surface guard to require the marker.
- Regenerate goldens with `UPDATE_GOLDEN=1` only after focused tests fail and implementation passes.

**Acceptance criteria:**

- [x] Capability claims include every emitted React and Next.js pattern ID.
- [x] Golden fixtures include at least one `react.route_reference.v1`.
- [x] Golden fixtures include at least one `react.route_definition.v1`.
- [x] Golden fixtures include at least one `nextjs.route_reference.v1`.
- [x] Golden fixtures include at least one `nextjs.file_route.v1`.
- [x] `EXTRACTION_CONTRACT_VERSION` and API-surface test include the new marker.
- [x] `node scripts/language-data-quality-report.mjs --strict` reports `silent_cells=0` and `quality_bar_debts=0`.

## Task 4: Contract Docs and Release Notes Prep

**Files:**
- Modify: `docs/contracts/jsonl-v3.md`
- Modify: `docs/contracts/sqlite-schema-v3.md`
- Modify: `docs/plans/2026-06-09-structural-facts-design.md`

**Interfaces:**
- Consumes: finalized metadata keys from Tasks 1 through 3.
- Produces: public docs for downstream consumers.

**What to build:** Document the new pattern IDs and the exact metadata keys without claiming cross-file route resolution.

**Approach:**

- Describe facts as extracted source observations.
- Document unsupported cases: dynamic `to`, dynamic `href`, dynamic `path`, lazy route modules, spread objects, runtime route generation, and Next.js route handlers if they remain out of scope.
- Keep docs generic to all downstream consumers; do not mention Miller bridge internals as extractor behavior.

**Acceptance criteria:**

- [x] Contract docs list all new pattern IDs.
- [x] Contract docs list required and optional metadata keys.
- [x] Docs state unsupported dynamic cases plainly.
- [x] Docs do not claim route graph resolution or cross-file matching.
- [x] Worker-scope verification passes.

## Verification Strategy

**Project source of truth:** `/Users/murphy/source/julie-extractors/AGENTS.md`, `RAZORBACK.md`, and existing Cargo test features.

**Worker red/green scope:**

Run focused tests by exact test name after writing each failing test:

```bash
cargo test -p julie-extractors react_router_static_route_facts -- --nocapture
cargo test -p julie-extractors nextjs_static_route_facts -- --nocapture
```

If the final test names differ, use the exact focused test names added by the worker.

**Worker ceiling:**

```bash
cargo test -p julie-extractors structural_facts -- --nocapture
cargo test -p julie-extractors test_public_contract_version_marks_current_fact_families -- --nocapture
```

**Worker gate invariant:** Focused tests prove static React Router and Next.js route/reference facts emit with correct metadata and dynamic cases remain silent. Structural-facts scope proves the new collector changes do not regress existing structural fact families.

**Lead affected-change scope:**

```bash
cargo test -p julie-extractors structural_facts -- --nocapture
UPDATE_GOLDEN=1 cargo test -p julie-extractors --features test-golden golden_fixtures_match_canonical_extraction -- --nocapture
cargo test -p julie-extractors --features test-golden golden_fixtures_match_canonical_extraction -- --nocapture
cargo test -p julie-extractors --features test-capability-matrix capability_matrix -- --nocapture
node scripts/language-data-quality-report.mjs --strict
```

**Branch gate before release consideration:**

```bash
cargo test --workspace
cargo fmt --check
git diff --check
node scripts/language-data-quality-report.mjs --strict
```

**Replay/metric evidence:** Hard gates are focused tests, golden fixture match, capability matrix, strict language data quality report with `silent_cells=0` and `quality_bar_debts=0`, workspace tests, format check, and diff whitespace check. Report-only evidence should include sample emitted facts for one React Router TSX file and one Next.js App Router file path.

**Escalation triggers:** Broader review is required if the implementation adds parser dependencies, changes SQLite schema, changes JSONL row shape beyond existing structural-fact metadata, adds API-route extraction, changes language detection, or increases default-suite runtime unexpectedly.

**Assigned verification failure:** Workers stop and report when assigned verification fails, unless this plan explicitly says to update that gate.

**Verification ledger:** Record invariant, command, scope label, commit SHA, result, and timestamp. For replay or metric evidence, also record hard-gate metrics and report-only sample rows. If the same HEAD already has a passing ledger entry for the required scope, reuse that evidence instead of rerunning the same expensive gate.

| Timestamp | HEAD | Scope | Command | Result |
| --- | --- | --- | --- | --- |
| 2026-06-30T19:58:13Z | `3336baa` | focused | `cargo test -p julie-extractors tests::react::structural_facts -- --nocapture` | pass; 2 passed |
| 2026-06-30T19:58:13Z | `3336baa` | structural facts | `cargo test -p julie-extractors structural_facts -- --nocapture` | pass; 54 passed |
| 2026-06-30T19:58:13Z | `3336baa` | API surface | `cargo test -p julie-extractors test_public_contract_version_marks_current_fact_families -- --nocapture` | pass; 1 passed |
| 2026-06-30T19:58:13Z | `3336baa` | golden fixtures | `UPDATE_GOLDEN=1 cargo test -p julie-extractors --features test-golden golden_fixtures_match_canonical_extraction -- --nocapture` | pass; goldens regenerated |
| 2026-06-30T19:58:13Z | `3336baa` | golden fixtures | `cargo test -p julie-extractors --features test-golden golden_fixtures_match_canonical_extraction -- --nocapture` | pass; 1 passed |
| 2026-06-30T19:58:13Z | `3336baa` | capability matrix | `cargo test -p julie-extractors --features test-capability-matrix capability_matrix -- --nocapture` | pass; 36 passed |
| 2026-06-30T19:58:13Z | `3336baa` | data quality | `node scripts/language-data-quality-report.mjs --strict` | pass; `silent_cells=0`, `quality_bar_debts=0` |
| 2026-06-30T19:58:13Z | `3336baa` | workspace | `cargo test --workspace` | pass |
| 2026-06-30T19:58:13Z | `3336baa` | formatting | `cargo fmt --check` | pass after `cargo fmt` |
| 2026-06-30T19:58:13Z | `3336baa` | whitespace | `git diff --check` | pass |

## Model Routing

**Project source of truth:** `RAZORBACK.md`.

**Strategy tier:** Public contract, capability claims, schema/report interpretation, release readiness.
- Harness mapping: inherit.

**Implementation tier:** Bounded React/Next.js collector and focused test tasks after this plan is approved.
- Harness mapping: inherit.

**Mechanical tier:** Fixture regeneration, docs wording, formatting, and rote manifest updates that do not own failing tests or acceptance gates.
- Harness mapping: inherit.

**Gate-interpretation reviewer:** Lead agent or strategy-tier reviewer for deciding whether a failing structural-fact, golden, capability, or strict data-quality gate reflects bad implementation or bad expectations.
- Harness mapping: inherit.

**Escalation tier:** Public artifact schema changes, language capability claim ambiguity, parser dependency changes, weak evidence, repeated failures, or default-suite runtime growth.
- Harness mapping: inherit.

**Worker eligibility:** Workers are eligible only after this plan is approved and tasks have narrow file ownership and explicit verification commands.

**Escalation triggers:** Escalate if a worker finds hidden coupling to old Julie internals, needs cross-file route graph construction, wants to add dependency versions, or cannot keep facts deterministic from local source/path data.

**Mechanical exclusion:** Mechanical workers cannot own failing tests, replay evidence, metrics, or acceptance gates. Split docs-only updates from evidence interpretation.

**Unsupported harness behavior:** If the harness cannot choose models per agent, use `inherit`, note it in the verification ledger, and continue.

## Release Notes for the Implementing Session

Do not publish automatically just because this plan is complete. After implementation and verification, report:

- exact new pattern IDs
- metadata keys emitted
- unsupported static/dynamic cases
- official docs URLs used for React Router and Next.js route surfaces
- focused test commands and results
- golden/capability/language-quality commands and results
- sample structural fact rows for React Router and Next.js

Then ask for release approval.
