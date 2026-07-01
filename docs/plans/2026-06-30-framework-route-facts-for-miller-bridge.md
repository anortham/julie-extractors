# Framework Route Facts for Miller Bridge Implementation Plan

> **For agentic workers:** Use the repo's normal implementation flow in `/Users/murphy/source/julie-extractors`. This plan is written as a handoff from Miller dogfooding; it should be executed in a dedicated `julie-extractors` session/worktree.

**Goal:** Emit the upstream structural facts Miller needs for reliable framework bridge traces: ASP.NET minimal API route group prefixes and Vue route definitions.

**Architecture:** Keep `julie-extractors` extraction-only. Parser-backed or source-backed recognition happens here as versioned `structural_facts`; Miller consumes those facts downstream to build bridge edges, observation nodes, and diagnostics. Do not add any Miller-like trace/linking behavior to this repo.

**Tech Stack:** Rust 2024, tree-sitter-backed extraction where available, existing `StructuralFact` contract, SQLite/JSONL artifact output, golden/capability tests.

**Context From Miller Dogfood:** Miller can now consume `aspnet.minimal_api.route.v1`, `htmx.attribute.v1`, and downstream Vue route-reference facts. The remaining upstream gaps are:

- AccessIQ-style `MapGroup("/prefix")` route grouping. Miller currently has a conservative source-region fallback for `MapGroup`; that should become extractor metadata/facts.
- Tycho-style Vue navigation. Miller sees `RouterLink to="/calendar"` but lacks route-definition facts to trace `RouterLink -> Vue route definition -> view component`.

## Global Constraints

- This repo owns extraction only: do not add bridge resolution, trace, MCP, search, dashboard, or semantic behavior.
- Keep facts versioned and generic enough for downstream consumers beyond Miller.
- Add capability/golden evidence for any new or changed fact shape.
- Structural facts must remain deterministic and sorted by existing conventions.
- Avoid ad hoc downstream-specific names in fact contracts. Metadata names should describe source facts, not Miller internals.
- Do not publish/release from this plan unless separately approved.

---

## Target Files

- Modify: `crates/julie-extractors/src/base/framework_structural_facts.rs`
- Modify: `crates/julie-extractors/src/base/web_structural_facts.rs`
- Modify: `crates/julie-extractors/src/tests/razor/structural_facts.rs` or add focused C# structural tests in the existing C# test module if one is more appropriate
- Modify: `crates/julie-extractors/src/tests/vue/structural_facts.rs`
- Modify: `crates/julie-extractors/src/lib.rs`
- Modify: `crates/julie-extractors/src/tests/api_surface.rs`
- Modify: `fixtures/extraction/capabilities.json` and any golden fixtures touched by capability updates
- Modify: `docs/contracts/jsonl-v3.md`
- Modify: `docs/contracts/sqlite-schema-v3.md`
- Modify: `docs/plans/2026-06-09-structural-facts-design.md`

## Fact Contract Additions

### ASP.NET Route Group Facts

Add parser/source-backed extraction for minimal API route groups.

Preferred contract:

- `pattern_id`: `aspnet.minimal_api.route_group.v1`
- `language`: `csharp`
- `capture_name`: `route_group`
- `metadata`:
  - `framework = "aspnet"`
  - `api_style = "minimal_api"`
  - `route_prefix = "/admin/connectors"`
  - `route_source = "string_literal"`
  - `group_variable = "admin"` when the group is assigned to a local variable
  - `source_kind = "map_group"`

Also enrich `aspnet.minimal_api.route.v1` where the route is called from a recognized group:

- `route_group_prefix = "/admin/connectors"`
- `effective_route_template = "/admin/connectors/save"`
- `route_group_source = "map_group"`

If reliable route-to-group enrichment is not possible for all local shapes in this slice, still emit `aspnet.minimal_api.route_group.v1` and document which route-call shapes are enriched.

### Vue Route Reference Facts

Add extraction for static Vue Router references from templates.

Preferred contract:

- `pattern_id`: `vue.route_reference.v1`
- `language`: `vue`
- `capture_name`: `route_reference`
- `metadata`:
  - `framework = "vue"`
  - `query_family = "frontend_navigation"`
  - `target_path = "/calendar"`
  - `source_kind = "router_link"`
  - `route_source = "string_literal"`
  - `attribute_name = "to"`

Start with static `<RouterLink to="/path">` references. Do not emit dynamic `:to`, object-expression, named-route, or runtime expression references in this slice unless they can be represented deterministically with the same contract.

### Vue Route Definition Facts

Add extraction for Vue Router route definitions.

Preferred contract:

- `pattern_id`: `vue.route_definition.v1`
- `language`: `vue` for `.vue` files and `typescript` / `javascript` when router config lives in TS/JS, if those collectors can be reached cleanly in this slice
- `capture_name`: `route_definition`
- `metadata`:
  - `framework = "vue"`
  - `query_family = "frontend_navigation"`
  - `target_path = "/calendar"`
  - `route_name = "calendar"` when present
  - `component_name = "CalendarView"` when statically recoverable
  - `component_path = "../views/CalendarView.vue"` when statically recoverable
  - `source_kind = "vue_router_route"`
  - `route_source = "string_literal"`

Emit `vue.route_reference.v1` alongside `vue.route_definition.v1`; neither replaces `vue.sfc_section.v1` or `vue.template_directive.v1`.

## Task 1: ASP.NET `MapGroup` Facts and Route Enrichment

**Files:**
- Modify: `crates/julie-extractors/src/base/framework_structural_facts.rs`
- Test: `crates/julie-extractors/src/tests/razor/structural_facts.rs` or existing C# structural facts tests

**Interfaces:**
- Consumes: existing `collect_aspnet_minimal_api_routes` scanning path and `ASPNET_ROUTE_METHODS`.
- Produces: `aspnet.minimal_api.route_group.v1` facts and route metadata enrichment on `aspnet.minimal_api.route.v1`.

**What to Build:** Recognize `MapGroup("/prefix")` calls and common local-group route call shapes:

```csharp
var admin = app.MapGroup("/admin/connectors");
admin.MapPost("/save", SaveAsync);
admin.MapGet("/preview-email", PreviewEmailAsync);
```

Route group fact spans should cover the `MapGroup(...)` invocation. Route call facts should keep their existing route span but include prefix metadata when the receiver is a known group variable.

**Approach:**

- Add `ASPNET_MINIMAL_API_ROUTE_GROUP_PATTERN_ID`.
- Add `MapGroup` scanning similar to the existing `MapGet` / `MapPost` scan.
- Track simple assignment shapes within one file:
  - `var group = app.MapGroup("/prefix");`
  - `RouteGroupBuilder group = app.MapGroup("/prefix");`
  - `var group = endpoints.MapGroup("/prefix");`
- When a route call receiver matches a known group variable, add `route_group_prefix` and `effective_route_template`.
- Keep fallback behavior conservative. If the receiver cannot be tied to a group variable, emit the route without prefix metadata rather than guessing.

**Acceptance Criteria:**

- [x] `MapGroup("/admin/connectors")` emits `aspnet.minimal_api.route_group.v1`.
- [x] `group.MapPost("/save", SaveAsync)` emits `aspnet.minimal_api.route.v1` with `route_group_prefix=/admin/connectors`.
- [x] The same route fact emits `effective_route_template=/admin/connectors/save`.
- [x] Ungrouped `app.MapGet("/health", ...)` remains unchanged.
- [x] Route-like text in comments/strings is not emitted as a route group fact.
- [x] Pattern capability lists include the new route group pattern for C#.

## Task 2: Vue Router Reference and Definition Facts

**Files:**
- Modify: `crates/julie-extractors/src/base/web_structural_facts.rs`
- Test: `crates/julie-extractors/src/tests/vue/structural_facts.rs`

**Interfaces:**
- Consumes: existing `collect_vue_structural_facts`, `scan_vue_sections`, and template attribute scanning.
- Produces: `vue.route_reference.v1` facts for static `<RouterLink to="...">` references and `vue.route_definition.v1` facts for static route objects.

**What to Build:** Recognize static Vue Router references in Vue SFC templates and static Vue Router route definitions in Vue SFC script blocks. Also cover TS/JS router files if this can be done without broad collector restructuring.

Minimum fixture shape:

```vue
<template>
  <RouterLink to="/calendar">Calendar</RouterLink>
</template>

<script setup lang="ts">
import CalendarView from '../views/CalendarView.vue'

const routes = [
  {
    path: '/calendar',
    name: 'calendar',
    component: CalendarView,
  },
]
</script>
```

Also cover common router file syntax if practical:

```ts
import CalendarView from '../views/CalendarView.vue'

const routes = [
  { path: '/calendar', name: 'calendar', component: CalendarView },
]
```

**Approach:**

- Add `VUE_ROUTE_REFERENCE_PATTERN_ID`.
- Add `VUE_ROUTE_DEFINITION_PATTERN_ID`.
- Reuse source-backed static scans for Vue template `RouterLink` tags with literal `to` attributes.
- Reuse source-backed static scans if current Vue structural facts are source-backed; keep it bounded to object literals containing a string `path`.
- Recover optional `name` and `component` fields only when statically simple.
- For component paths, map imported component identifiers to import specifiers within the same script block or router file.
- Do not attempt dynamic references, dynamic routes, spreads, function-built routes, lazy imports, or runtime expressions in this patch unless they fall out cheaply and safely.

**Acceptance Criteria:**

- [x] Static route object with `path: "/calendar"` emits `vue.route_definition.v1`.
- [x] Static `<RouterLink to="/calendar">` emits `vue.route_reference.v1`.
- [x] Dynamic `:to`, object-expression, and named-route references are not emitted as static path references.
- [x] Fact metadata includes `target_path=/calendar`.
- [x] Fact metadata includes `route_name=calendar` when `name` is a string literal.
- [x] Fact metadata includes `component_name=CalendarView` when component is an identifier.
- [x] Fact metadata includes `component_path=../views/CalendarView.vue` when the component identifier is imported statically.
- [x] Existing `vue.route_reference.v1`, `vue.sfc_section.v1`, and `vue.template_directive.v1` tests remain green.
- [x] Pattern capability lists include the new route definition pattern for Vue and any TS/JS support added in this slice.

## Task 3: Golden and Capability Evidence

**Files:**
- Modify: `fixtures/extraction/capabilities.json`
- Modify: `crates/julie-extractors/src/lib.rs`
- Modify: `crates/julie-extractors/src/tests/api_surface.rs`
- Modify golden fixtures only where this plan adds or changes emitted facts.
- Test: capability matrix tests and structural facts tests.

**What to Build:** Update fixture/golden evidence so the new fact contracts are pinned and advertised.

**Approach:**

- Update relevant structural facts expected outputs with stable metadata.
- Update `EXTRACTION_CONTRACT_VERSION` with a new marker for the route/navigation structural fact shape.
- Update the API-surface test to require that marker.
- Update `kind_coverage.structural_facts.supported` for C# and Vue.
- If TS/JS route-definition facts are added outside Vue SFCs, update those languages' capability claims too.

**Acceptance Criteria:**

- [x] `fixtures/extraction/capabilities.json` advertises every new emitted pattern.
- [x] Golden fixtures include at least one `aspnet.minimal_api.route_group.v1`.
- [x] Golden fixtures include at least one `vue.route_reference.v1`.
- [x] Golden fixtures include at least one `vue.route_definition.v1`.
- [x] `EXTRACTION_CONTRACT_VERSION` and the API-surface guard include the new marker.
- [x] `node scripts/language-data-quality-report.mjs --strict` reports `silent_cells=0` and `quality_bar_debts=0`.

## Task 4: Downstream Compatibility Notes

**Files:**
- Modify: `docs/contracts/jsonl-v3.md`
- Modify: `docs/contracts/sqlite-schema-v3.md`
- Modify: `docs/plans/2026-06-09-structural-facts-design.md`

**What to Build:** Record how downstream consumers should use the facts without embedding Miller behavior in extractor docs.

**Approach:**

- Keep wording extraction-oriented:
  - route group facts identify prefixes and group variables
  - route facts may carry `effective_route_template`
  - Vue route reference facts identify static template navigation targets
  - Vue route definition facts identify static route table entries and component targets
- Do not describe Miller bridge graph internals as extractor responsibilities.

**Acceptance Criteria:**

- [x] Docs describe emitted facts and metadata keys.
- [x] Docs do not claim extractor performs cross-file bridge resolution.
- [x] Docs mention unsupported dynamic cases if tests intentionally exclude them.

## Verification Strategy

**Project source of truth:** `/Users/murphy/source/julie-extractors/AGENTS.md`.

**Worker red/green scope:**

Run the narrow structural fact tests first:

```bash
cargo test -p julie-extractors tests::razor::structural_facts -- --nocapture
cargo test -p julie-extractors tests::vue::structural_facts -- --nocapture
```

If C# structural facts live under a different test module after inspection, use the exact C# module instead of the Razor module for Task 1.

**Affected-change scope:**

```bash
cargo test -p julie-extractors structural_facts -- --nocapture
cargo test -p julie-extractors capability_matrix -- --nocapture
node scripts/language-data-quality-report.mjs --strict
```

**Branch gate before release consideration:**

```bash
cargo test --workspace
node scripts/language-data-quality-report.mjs --strict
```

**Dogfood evidence to capture before release:**

- Run `julie-extract` on an AccessIQ-style fixture or real AccessIQ checkout and show:
  - `aspnet.minimal_api.route_group.v1`
  - route facts with `route_group_prefix`
  - route facts with `effective_route_template`
- Run `julie-extract` on a Tycho-style Vue router fixture or real Tycho checkout and show:
  - `vue.route_reference.v1`
  - `vue.route_definition.v1`
  - route definition metadata with component target where statically recoverable

## Release Notes for the Implementing Session

Do not publish automatically just because this plan is complete. After implementation and verification, report:

- exact new pattern IDs
- metadata keys emitted
- unsupported static/dynamic cases
- test commands and results
- sample rows from AccessIQ/Tycho-style dogfood

Then ask for release approval.
