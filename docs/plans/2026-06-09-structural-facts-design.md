# Structural Facts Slice Design

## Goal

Add a small, versioned structural fact contract that lets downstream tools
consume parser-backed facts without owning tree-sitter policy or language
coverage.

The first slice proved the contract with one useful Rust pattern:
`rust.unsafe_block.v1`. The completion slice expands the contract into a
representative parser-backed pattern set and publishes exact capability
metadata once there is enough coverage to make the matrix meaningful.

## Architecture Quality

**Affected modules:** extractor base types and registry, capability snapshot,
capability matrix tests, CLI extraction/capability mapping, artifact
model/schema/writer/JSONL/report surfaces, current-schema performance workload,
and contract docs.

**Caller-facing interface:** `ExtractionResults.structural_facts`,
`ArtifactFile.structural_facts`, the SQLite `structural_facts` table, the JSONL
`structural_fact` record kind, row counts, and pattern coverage metadata.

**Depth/locality check:** parser-specific matching stays inside
`crates/julie-extractors/src/base/structural_facts.rs`; artifact code only
persists and exports normalized rows. Miller and Eros get facts, not a search
engine or query DSL.

**Test surface:** tests exercise the public extraction pipeline, capability
snapshot, capability matrix fixture evidence, CLI `languages --json`, CLI scan
output, artifact schema, writer behavior, JSONL export, reports, and the
synthetic current-schema workload.

**Seams/adapters:** the CLI mapping is the adapter from extractor facts to
artifact rows. No downstream product-specific adapter is added here.

**Rejected shortcuts:** no raw AST dump, no generic query language, no source
text storage, no ranking/search table, and no broad language promise before
fixture evidence exists.

**Architecture risk:** medium. This changes public artifact contracts, but the
implementation is patterned after the existing `source_regions` row family.

## Contract Shape

Add a new extraction row domain:

```text
structural_facts
```

Each row stores:

- stable `structural_fact_id`
- `file_id`, `path`, and `language`
- versioned `pattern_id`
- `capture_name`
- matched `node_kind`
- optional `containing_symbol_id`
- normalized line, column, and byte span
- `confidence`
- optional `metadata_json`

Add indexes for common downstream access:

- by `(file_id, start_byte, end_byte)`
- by `(pattern_id, language, path)`
- by `containing_symbol_id`

JSONL exports a `structural_fact` record after `source_region` and before
`parse_diagnostic`.

Reports include `structural_facts` in row-domain counts.

## Pattern Metadata

Every emitted row records normalized metadata:

```json
{
  "pattern_version": 1,
  "query_family": "<family>"
}
```

The supported completion-slice patterns are:

| Pattern ID | Language | Capture | Node Kind(s) | Family | Meaning |
| --- | --- | --- | --- | --- | --- |
| `rust.unsafe_block.v1` | `rust` | `unsafe_block` | `unsafe_block` | `safety` | A Rust `unsafe { ... }` block. |
| `go.goroutine_launch.v1` | `go` | `go_statement` | `go_statement` | `concurrency` | A Go `go call()` launch. |
| `go.defer_statement.v1` | `go` | `defer_statement` | `defer_statement` | `lifecycle` | A Go `defer call()` statement. |
| `python.decorated_definition.v1` | `python` | `decorated_definition` | `decorated_definition` | `metadata` | A Python decorated function or class definition. |
| `javascript.await_expression.v1` | `javascript` | `await_expression` | `await_expression` | `async` | A JavaScript `await` expression. |
| `jsx.await_expression.v1` | `jsx` | `await_expression` | `await_expression` | `async` | A JSX file `await` expression. |
| `typescript.await_expression.v1` | `typescript` | `await_expression` | `await_expression` | `async` | A TypeScript `await` expression. |
| `tsx.await_expression.v1` | `tsx` | `await_expression` | `await_expression` | `async` | A TSX file `await` expression. |
| `c.preprocessor_definition.v1` | `c` | `preprocessor_definition` | `preproc_def`, `preproc_function_def` | `preprocessor` | A C preprocessor definition. |
| `cpp.preprocessor_definition.v1` | `cpp` | `preprocessor_definition` | `preproc_def`, `preproc_function_def` | `preprocessor` | A C++ preprocessor definition. |
| `aspnet.minimal_api.route.v1` | `csharp` | `route_call` | parser-covered invocation span | `framework` | A static ASP.NET minimal API `MapGet`/`MapPost`/`MapPut`/`MapPatch`/`MapDelete` route call with a literal route template. |
| `aspnet.minimal_api.route_group.v1` | `csharp` | `route_group` | parser-covered invocation span | `framework` | A static ASP.NET minimal API `MapGroup` route group with a literal route prefix. |
| `htmx.attribute.v1` | `html`, `razor` | `attribute` | parser-covered attribute span | `frontend_interaction` | An `hx-*` attribute, including request verb and static target path metadata when applicable. |
| `alpine.directive.v1` | `html`, `razor` | `directive` | parser-covered attribute span | `frontend_interaction` | An Alpine `x-*`, `@...`, or `:...` directive with normalized directive metadata. |
| `vue.route_reference.v1` | `vue` | `route_reference` | `template_attribute` | `frontend_navigation` | A static Vue Router link target such as `<RouterLink to="/calendar">`. |
| `vue.route_definition.v1` | `vue` | `route_definition` | `object` | `frontend_navigation` | A static Vue Router route-table entry with a literal `path`. |
| `nuxt.route_reference.v1` | `vue` | `route_reference` | `template_attribute` | `frontend_navigation` | A static Nuxt `NuxtLink` or `nuxt-link` target with a literal `to` path. |
| `nuxt.file_route.v1` | `javascript`, `jsx`, `typescript`, `tsx`, `vue` | `file_route` | `file` | `frontend_navigation` | A Nuxt `app/pages/**` or `pages/**` page route derived from the file path. |
| `react.route_reference.v1` | `javascript`, `jsx`, `tsx` | `route_reference` | `jsx_attribute` | `frontend_navigation` | A static React Router `Link` or `NavLink` target imported from React Router. |
| `react.route_definition.v1` | `javascript`, `jsx`, `typescript`, `tsx` | `route_definition` | `object`, `jsx_element` | `frontend_navigation` | A static React Router route object or `<Route>` element with a literal `path` or `index`. |
| `nextjs.route_reference.v1` | `javascript`, `jsx`, `tsx` | `route_reference` | `jsx_attribute` | `frontend_navigation` | A static `next/link` target from a string `href` or object `pathname`. |
| `nextjs.file_route.v1` | `javascript`, `jsx`, `typescript`, `tsx` | `file_route` | `file` | `frontend_navigation` | A Next.js App Router or Pages Router page route derived from the file path. |

Dynamic Vue `:to` bindings, named-route objects, non-literal route paths, spreads,
function-built routes, and lazy component imports are not emitted as static route
facts in this slice.
Dynamic Nuxt `to` bindings, named-route objects, external `NuxtLink` targets, and
Nuxt named-view page files are not emitted as static route facts in this slice.
Dynamic React Router `to`/`path` values, arbitrary local `Link` components, and
Next.js `href` values without a static string or object `pathname` are not
emitted as static route facts in this slice.

Framework-fact rows add framework-specific metadata:

- `aspnet.minimal_api.route.v1`: `framework = "aspnet"`,
  `api_style = "minimal_api"`, `verb`, `route_template`, `route_source`, and
  optional `handler_kind` / `handler_name`. Grouped route calls may also include
  `route_group_prefix`, `effective_route_template`, and `route_group_source`.
- `aspnet.minimal_api.route_group.v1`: `framework = "aspnet"`,
  `api_style = "minimal_api"`, `route_prefix`, `route_source`,
  `source_kind = "map_group"`, and optional `group_variable`.
- `htmx.attribute.v1`: `framework = "htmx"`, `attribute_name`, optional
  `attribute_value`, and optional `verb` / `target_path` for request
  attributes.
- `alpine.directive.v1`: `framework = "alpine"`, `directive`, optional
  `argument`, optional `modifiers`, optional `expression`, and `shorthand`.
- `vue.route_reference.v1`: `framework = "vue"`, `target_path`,
  `source_kind = "router_link"`, `route_source = "string_literal"`, and
  `attribute_name = "to"`.
- `vue.route_definition.v1`: `framework = "vue"`, `target_path`,
  `source_kind = "vue_router_route"`, `route_source = "string_literal"`, and
  optional `route_name`, `component_name`, and `component_path`.

`fixtures/extraction/capabilities.json` publishes these exact ids under
`kind_coverage.structural_facts.supported`. Languages with no current structural
patterns publish an empty `supported` list, which means "no structural pattern
claims yet"; it does not imply the parser cannot support structural facts in a
future slice.

The ASP.NET/htmx/Alpine slice remains fact-only. Cross-file route linking, such
as connecting `hx-get="/todos"` to `app.MapGet("/todos", ...)`, belongs to
downstream tools that consume these rows.

## Extraction Flow

1. Language extractors return symbols and other existing facts.
2. `registry::extract_for_language` invokes `collect_structural_facts(...)`
   beside `collect_source_regions(...)`.
3. The collector walks the syntax tree for configured node kinds.
4. Matching nodes become `StructuralFact` rows with stable IDs and containing
   symbols attached by smallest containing span.
5. The CLI maps facts to `ArtifactStructuralFact`.
6. The writer persists rows, JSONL exports them, and reports count them.

## Acceptance Criteria

- [x] SQLite creates `structural_facts` with required columns and indexes.
- [x] `ArtifactWriter` inserts, replaces, deletes, and counts structural facts.
- [x] JSONL includes `structural_fact` in the exact record-kind list.
- [x] Reports include `structural_facts` in all row-domain counts.
- [x] Rust extraction emits `rust.unsafe_block.v1` for an unsafe block.
- [x] CLI scan creates non-empty `structural_facts` rows for a Rust fixture.
- [x] Current-schema writer performance workload includes structural facts.
- [x] Contract docs describe the row shape, JSONL shape, and initial pattern.
- [x] Focused default and contract-surface tests remain fast.
- [x] Extractor tests prove every supported completion-slice pattern through
      `extract_canonical`.
- [x] `capabilities.json`, the Rust capability snapshot, SQLite artifacts, and
      `languages --json` publish `kind_coverage.structural_facts`.
- [x] Capability-matrix tests verify every advertised structural pattern has
      fixture-backed extraction evidence and every extracted supported pattern
      is advertised.
- [x] Contract docs enumerate the completed supported pattern set and explain
      that this repo only emits facts, not downstream query/search behavior.
- [x] TODO #7 is marked complete after the representative pattern set and
      capability metadata pass focused and contract test suites.

## Out Of Scope

- Query execution or ranking.
- Miller/Eros-specific workflows.
- Raw tree-sitter query source storage.
- Raw AST serialization.
- Exhaustive language coverage beyond the representative parser-backed pattern
  set above.
- A public pattern registry table in SQLite; the JSON capability snapshot is the
  public metadata surface for this slice.
