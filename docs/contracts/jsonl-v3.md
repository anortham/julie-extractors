# JSONL v3

## Scope

JSONL is the secondary export and streaming format. It is derived from the same
canonical rows as SQLite and must not become a separate source of truth.

JSONL does not embed complete source files. It exports file metadata,
hashes, spans, and source-derived extraction facts from the SQLite artifact.

`julie-extract export --db <path> --format jsonl --out <path|->` writes JSONL
v3 records.

## Envelope

Each line is one JSON object:

```json
{
  "jsonl_schema_version": 3,
  "extract_contract_version": 3,
  "kind": "symbol",
  "op": "snapshot",
  "artifact_id": "01hz...",
  "record_id": "sym_...",
  "record": {}
}
```

Fields:

- `jsonl_schema_version`: integer, always `3` for this contract.
- `extract_contract_version`: integer, always `3` for this contract.
- `kind`: record kind.
- `op`: operation. Full exports use `snapshot`.
- `artifact_id`: artifact identifier from SQLite metadata.
- `record_id`: stable ID for this record, or a deterministic composite ID for
  records whose SQLite primary key is composite.
- `record`: kind-specific payload.

## Record Order

Full export order is deterministic:

1. `artifact`
2. `parser_inventory`
3. `language_capability`
4. `language_capability_fixture`
5. `language_capability_gap`
6. `revision`
7. `revision_file_change`
8. `file`
9. `symbol`
10. `symbol_annotation`
11. `identifier`
12. `relationship`
13. `pending_relationship`
14. `type_fact`
15. `type_argument_usage`
16. `type_argument`
17. `literal`
18. `source_region`
19. `complexity_metric`
20. `structural_fact`
21. `parse_diagnostic`

Rows are ordered by primary key within each kind unless a kind defines a more
specific natural order.

## Record Kinds

JSON field names use lower-case snake_case. Payloads are the stable JSON shape
for SQLite v3 rows.

SQLite JSON text columns are decoded in JSONL. For example,
`metadata_json TEXT` becomes `metadata: {}` or `metadata: null` according to the
payload schema below, not an escaped JSON string.

## Shared Objects

`span` is either `null` or an object with exactly these integer fields:

- `start_line`
- `start_column`
- `end_line`
- `end_column`
- `start_byte`
- `end_byte`

`partial_span` is used only where SQLite permits partial location data. It is
either `null` or an object with these fields, each integer or `null` except
where the record kind says otherwise:

- `start_line`
- `start_column`
- `end_line`
- `end_column`
- `start_byte`
- `end_byte`

Capability flag objects have exactly these boolean fields:

- `symbols`
- `relationships`
- `pending_relationships`
- `identifiers`
- `types`

Metadata objects are decoded JSON objects. Empty metadata is `{}`. Unknown or
unset optional metadata is `null` only when the field explicitly allows `null`.

## Payload Schemas

Each record kind below lists the exact `record` keys for JSONL v3. No additional
keys are part of the v3 contract.

### `artifact`

`record_id`: `artifact_id`.

```json
{
  "artifact_id": "01hz...",
  "root_path": "/repo",
  "schema_version": 3,
  "extract_contract_version": 3,
  "sqlite_schema_version": 3,
  "binary_version": "2.0.0",
  "hash_algorithm": "blake3",
  "parser_inventory_fingerprint": "sha256:...",
  "capability_snapshot_fingerprint": "sha256:...",
  "created_at": "2026-05-31T16:00:00Z",
  "updated_at": "2026-05-31T16:05:00Z"
}
```

### `file`

`record_id`: `file_id`.

```json
{
  "file_id": "file_...",
  "path": "src/lib.rs",
  "language": "rust",
  "content_hash": "blake3:...",
  "content_bytes": 1234,
  "line_count": 42,
  "indexed_at": "2026-05-31T16:05:00Z",
  "last_revision_id": 7,
  "status": "indexed",
  "metadata": {}
}
```

### `symbol`

`record_id`: `symbol_id`.

```json
{
  "symbol_id": "sym_...",
  "file_id": "file_...",
  "path": "src/lib.rs",
  "language": "rust",
  "name": "extract",
  "kind": "function",
  "signature": "fn extract(...)",
  "doc_comment": null,
  "visibility": "public",
  "parent_symbol_id": null,
  "span": {
    "start_line": 10,
    "start_column": 0,
    "end_line": 20,
    "end_column": 1,
    "start_byte": 120,
    "end_byte": 420
  },
  "body_span": null,
  "body_hash": null,
  "semantic_group": null,
  "confidence": null,
  "content_type": null,
  "is_test": true,
  "test_container": false,
  "test_lifecycle": false,
  "metadata": {
    "is_test": true
  }
}
```

`body_hash` is present only when `body_span` is present. It is an exact normalized-body fingerprint. The algorithm id is
`julie-normalized-body-md5-v1`: take the source bytes covered by the body span,
tokenize them while preserving quoted string-like tokens, join normalized tokens
with U+001F, and emit the lowercase MD5 hex digest. The normalization ignores
whitespace and comments for the symbol language. Equal `body_hash` values are
exact normalized-body match candidates. `body_hash` does not encode duplicate severity,
near-duplicate similarity, or product-level clone ranking; consumers own those
thresholds and presentation choices.

Reserved symbol test-role fields:

- `is_test`: boolean. `true` means the extractor identified the symbol as a test
  case or test lifecycle hook.
- `test_container`: boolean. `true` means the symbol groups tests, for example
  `describe`, `context`, `suite`, or `group` constructs. Containers are not test
  cases unless `is_test` is also true.
- `test_lifecycle`: boolean. `true` means the symbol is setup, teardown, or an
  equivalent lifecycle hook. Lifecycle hooks must also carry `is_test: true`.

The same keys remain reserved inside `metadata` when present so existing
metadata-oriented consumers do not lose the old Julie signal. They are extracted
metadata, not Julie test-quality or linkage analysis.

### `parser_inventory`

`record_id`: `<language>:<parser_package>`.

Fields:

- `language`: string
- `parser_package`: string
- `parser_version`: string or `null`
- `grammar_version`: string or `null`
- `source`: string or `null`
- `metadata`: object or `null`

### `language_capability`

`record_id`: `language`.

Fields:

- `language`: string
- `parser_package`: string
- `extensions`: array of strings
- `dependency_status`: string
- `target_capabilities`: capability flag object
- `actual_capabilities`: capability flag object
- `kind_coverage`: object with `symbols`, `relationships`, `identifiers`,
  `body_spans`, `structural_facts`, and `complexity_metrics` domains

Each `kind_coverage` domain has `supported`, `not_applicable`, and `open_gaps`.

### `language_capability_fixture`

`record_id`: `<language>:<fixture_name>`.

Fields:

- `language`: string
- `fixture_name`: string
- `source_path`: string
- `expected_path`: string

### `language_capability_gap`

`record_id`: `gap_id`.

Fields:

- `gap_id`: string
- `language`: string
- `capability`: string
- `status`: string
- `reason`: string
- `required_closure`: string
- `evidence`: object

### `revision`

`record_id`: decimal string form of `revision_id`.

Fields:

- `revision_id`: integer
- `parent_revision_id`: integer or `null`
- `operation`: `scan`, `update`, or `delete`
- `mode`: `incremental`, `force`, or `single_file`
- `started_at`: RFC 3339 UTC string
- `completed_at`: RFC 3339 UTC string
- `binary_version`: string
- `extract_contract_version`: integer
- `sqlite_schema_version`: integer
- `input_root`: string or `null`
- `counts`: object

### `revision_file_change`

`record_id`: `<revision_id>:<file_id>`.

Fields:

- `revision_id`: integer
- `file_id`: string
- `path`: root-relative path string
- `change_kind`: `inserted`, `updated`, `deleted`, or `unsupported`

### `symbol_annotation`

`record_id`: `annotation_id`.

Fields:

- `annotation_id`: string
- `symbol_id`: string
- `annotation`: string
- `annotation_key`: string
- `raw_text`: string or `null`
- `carrier`: string or `null`
- `metadata`: object or `null`

### `identifier`

`record_id`: `identifier_id`.

Fields:

- `identifier_id`: string
- `file_id`: string
- `path`: root-relative path string
- `language`: string
- `name`: string
- `kind`: string
- `containing_symbol_id`: string or `null`
- `target_symbol_id`: string or `null`
- `span`: span object
- `confidence`: number
- `code_context`: string or `null`
- `metadata`: object or `null`

### `relationship`

`record_id`: `relationship_id`.

Fields:

- `relationship_id`: string
- `from_symbol_id`: string
- `to_symbol_id`: string
- `file_id`: string
- `path`: root-relative path string
- `kind`: string
- `span`: span object or `null`
- `confidence`: number
- `metadata`: object or `null`

### `pending_relationship`

`record_id`: `pending_relationship_id`.

Fields:

- `pending_relationship_id`: string
- `from_symbol_id`: string
- `caller_scope_symbol_id`: string or `null`
- `file_id`: string
- `path`: root-relative path string
- `kind`: string
- `target`: object
- `site`: partial_span object with non-null `start_line`
- `confidence`: number
- `metadata`: object or `null`

`target` has exactly these fields:

- `display_name`: string
- `terminal_name`: string
- `receiver`: string or `null`
- `namespace`: array of strings
- `import_context`: string or `null`

### `type_fact`

`record_id`: `type_fact_id`.

Fields:

- `type_fact_id`: string
- `symbol_id`: string
- `language`: string
- `resolved_type`: string
- `generic_params`: array of strings or `null`
- `constraints`: array of strings or `null`
- `is_inferred`: boolean
- `metadata`: object or `null`

### `type_argument_usage`

`record_id`: `usage_id`.

Fields:

- `usage_id`: string
- `identifier_id`: string
- `file_id`: string
- `path`: root-relative path string
- `language`: string
- `metadata`: object or `null`

### `type_argument`

`record_id`: `type_argument_id`.

Fields:

- `type_argument_id`: string
- `usage_id`: string
- `parent_type_argument_id`: string or `null`
- `ordinal`: integer
- `type_name`: string

### `literal`

`record_id`: `literal_id`.

Fields:

- `literal_id`: string
- `file_id`: string
- `path`: root-relative path string
- `language`: string
- `literal_text`: string
- `kind`: string
- `carrier`: string or `null`
- `arg_position`: integer
- `containing_symbol_id`: string or `null`
- `span`: span object
- `confidence`: number
- `metadata`: object or `null`

### `source_region`

`record_id`: `source_region_id`.

Fields:

- `source_region_id`: string
- `file_id`: string
- `path`: root-relative path string
- `language`: string
- `kind`: `comment`, `doc_comment`, `string_literal`, or `embedded`
- `containing_symbol_id`: string or `null`
- `span`: span object
- `metadata`: object or `null`

Embedded region metadata may include `embedded_language` and `host_node_kind`.

### `complexity_metric`

`record_id`: `complexity_metric_id`.

Fields:

- `complexity_metric_id`: string
- `file_id`: string
- `path`: root-relative path string
- `language`: string
- `scope`: `file` or `symbol`
- `symbol_id`: string or `null`
- `algorithm_id`: stable versioned algorithm identifier
- `covered_lines`: integer
- `covered_bytes`: integer
- `decision_count`: integer
- `loop_count`: integer
- `max_nesting_depth`: integer
- `parameter_count`: integer or `null`
- `span`: span object
- `metadata`: object or `null`

The initial algorithm id is `julie-ast-complexity-v1`. Records are primitive
metrics only; downstream tools own ranking, risk labels, and dashboards.
Supported scopes are advertised in `language_capability` records under
`kind_coverage.complexity_metrics.supported`.

### `structural_fact`

`record_id`: `structural_fact_id`.

Fields:

- `structural_fact_id`: string
- `file_id`: string
- `path`: root-relative path string
- `language`: string
- `pattern_id`: stable versioned pattern identifier
- `capture_name`: capture name within the pattern
- `node_kind`: matched tree-sitter node kind
- `containing_symbol_id`: string or `null`
- `span`: span object
- `confidence`: number
- `metadata`: object or `null`

Supported patterns are advertised in `language_capability` records under
`kind_coverage.structural_facts.supported`.

| Pattern ID | Language | Capture | Node Kind(s) | Metadata |
| --- | --- | --- | --- | --- |
| `rust.unsafe_block.v1` | `rust` | `unsafe_block` | `unsafe_block` | `{"pattern_version":1,"query_family":"safety"}` |
| `go.goroutine_launch.v1` | `go` | `go_statement` | `go_statement` | `{"pattern_version":1,"query_family":"concurrency"}` |
| `go.defer_statement.v1` | `go` | `defer_statement` | `defer_statement` | `{"pattern_version":1,"query_family":"lifecycle"}` |
| `python.decorated_definition.v1` | `python` | `decorated_definition` | `decorated_definition` | `{"pattern_version":1,"query_family":"metadata"}` |
| `javascript.await_expression.v1` | `javascript` | `await_expression` | `await_expression` | `{"pattern_version":1,"query_family":"async"}` |
| `jsx.await_expression.v1` | `jsx` | `await_expression` | `await_expression` | `{"pattern_version":1,"query_family":"async"}` |
| `typescript.await_expression.v1` | `typescript` | `await_expression` | `await_expression` | `{"pattern_version":1,"query_family":"async"}` |
| `tsx.await_expression.v1` | `tsx` | `await_expression` | `await_expression` | `{"pattern_version":1,"query_family":"async"}` |
| `c.preprocessor_definition.v1` | `c` | `preprocessor_definition` | `preproc_def`, `preproc_function_def` | `{"pattern_version":1,"query_family":"preprocessor"}` |
| `cpp.preprocessor_definition.v1` | `cpp` | `preprocessor_definition` | `preproc_def`, `preproc_function_def` | `{"pattern_version":1,"query_family":"preprocessor"}` |
| `aspnet.minimal_api.route.v1` | `csharp` | `route_call` | parser-covered invocation span | `{"pattern_version":1,"query_family":"framework","framework":"aspnet","api_style":"minimal_api","verb":"GET","route_template":"/todos","route_source":"string_literal"}` plus optional `handler_kind` and `handler_name` |
| `aspnet.minimal_api.route_group.v1` | `csharp` | `route_group` | parser-covered invocation span | `{"pattern_version":1,"query_family":"framework","framework":"aspnet","api_style":"minimal_api","route_prefix":"/admin","route_source":"string_literal","source_kind":"map_group"}` plus optional `group_variable` |
| `htmx.attribute.v1` | `html`, `razor` | `attribute` | parser-covered attribute span | `{"pattern_version":1,"query_family":"frontend_interaction","framework":"htmx","attribute_name":"hx-get","attribute_value":"/todos"}` plus optional `verb`, `target_path`, and `data_prefix` |
| `alpine.directive.v1` | `html`, `razor` | `directive` | parser-covered attribute span | `{"pattern_version":1,"query_family":"frontend_interaction","framework":"alpine","directive":"x-on","argument":"click","expression":"open = !open","shorthand":true}` plus optional `modifiers` |
| `vue.route_reference.v1` | `vue` | `route_reference` | `template_attribute` | `{"pattern_version":1,"query_family":"frontend_navigation","framework":"vue","target_path":"/calendar","source_kind":"router_link","route_source":"string_literal","attribute_name":"to","verb":"GET"}` |
| `vue.route_definition.v1` | `javascript`, `jsx`, `typescript`, `tsx`, `vue` | `route_definition` | `object` | `{"pattern_version":1,"query_family":"frontend_navigation","framework":"vue","target_path":"/calendar","source_kind":"vue_router_route","route_source":"string_literal"}` plus optional `route_name`, `component_name`, `component_path`, `parent_route_path`, and `effective_route_template` |
| `nuxt.route_reference.v1` | `vue` | `route_reference` | `template_attribute` | `{"pattern_version":1,"query_family":"frontend_navigation","framework":"nuxt","target_path":"/about","source_kind":"nuxt_link","route_source":"string_literal","attribute_name":"to","component_name":"NuxtLink","verb":"GET"}` |
| `nuxt.file_route.v1` | `javascript`, `jsx`, `typescript`, `tsx`, `vue` | `file_route` | `file` | `{"pattern_version":1,"query_family":"frontend_navigation","framework":"nuxt","router":"pages","file_convention":"page","route_path":"/blog/[slug]","normalized_route_template":"/blog/:slug","dynamic_segments":["slug"],"route_group_segments":["marketing"],"source_kind":"nuxt_file_route"}` |
| `react.route_reference.v1` | `javascript`, `jsx`, `tsx` | `route_reference` | `jsx_attribute` | `{"pattern_version":1,"query_family":"frontend_navigation","framework":"react","library":"react_router","target_path":"/dashboard","source_kind":"react_router_link","route_source":"string_literal","attribute_name":"to","component_name":"RouterLink","import_source":"react-router-dom","verb":"GET"}` |
| `react.route_definition.v1` | `javascript`, `jsx`, `typescript`, `tsx` | `route_definition` | `object`, `jsx_element` | `{"pattern_version":1,"query_family":"frontend_navigation","framework":"react","library":"react_router","route_path":"/dashboard","source_kind":"route_object","route_source":"string_literal"}` plus optional `route_component`, `route_id`, `index_route`, `parent_route_path`, and `effective_route_template` |
| `nextjs.route_reference.v1` | `javascript`, `jsx`, `tsx` | `route_reference` | `jsx_attribute` | `{"pattern_version":1,"query_family":"frontend_navigation","framework":"nextjs","target_path":"/dashboard","source_kind":"next_link","route_source":"string_literal","attribute_name":"href","component_name":"Link","import_source":"next/link","verb":"GET"}` |
| `nextjs.file_route.v1` | `javascript`, `jsx`, `typescript`, `tsx` | `file_route` | `file` | `{"pattern_version":1,"query_family":"frontend_navigation","framework":"nextjs","router":"app","file_convention":"page","route_path":"/blog/[slug]","normalized_route_template":"/blog/:slug","dynamic_segments":["slug"],"source_kind":"nextjs_file_route"}` plus optional `route_group_segments`, `parallel_route_segments`, `intercepting_route_markers`, and `intercepted_route_segments` |
| `nextjs.route_handler.v1` | `javascript`, `typescript` | `route_handler` | `export_statement` | `{"pattern_version":1,"query_family":"framework","framework":"nextjs","router":"app","file_convention":"route","route_path":"/api/users/[id]","normalized_route_template":"/api/users/:id","dynamic_segments":["id"],"verb":"GET","verb_source":"attested","source_kind":"nextjs_route_handler"}` plus optional `route_group_segments`, `parallel_route_segments`, `intercepting_route_markers`, and `intercepted_route_segments` |
| `nuxt.server_route.v1` | `javascript`, `typescript` | `server_route` | `file` | `{"pattern_version":1,"query_family":"framework","framework":"nuxt","router":"server","route_path":"/api/users/[id]","normalized_route_template":"/api/users/:id","dynamic_segments":["id"],"verb":"GET","verb_source":"attested","source_kind":"nuxt_server_route"}` — `verb`/`verb_source` and `normalized_route_template`/`dynamic_segments` are present only when the filename carries a method suffix or the route has dynamic segments |
| `http.client_request.v1` | `javascript`, `jsx`, `typescript`, `tsx`, `vue` | `client_request` | `call_expression` | `{"pattern_version":1,"query_family":"web.http_client","framework":"fetch","client":"fetch","target_path":"/api/users","url_kind":"path","verb":"POST","verb_source":"attested"}` — axios calls additionally carry `"client":"axios"`, `"framework":"axios"`, and `"import_source":"axios"` |

`fetch()` and axios calls emit `http.client_request.v1` only when the first
argument is a plain static string literal (`'...'` or `"..."`). `url_kind` is
`path` for a leading `/`, `absolute` for a URL containing `://`, and `relative`
otherwise. `verb_source` is `attested` when the options object carries a static
string `method:` property (upper-cased into `verb`), or `default` when no
options object or `method` property is present (the clients' spec default `GET`).
Template literals (even without interpolation), identifier/expression URLs,
concatenated URLs, property calls of the bare client name (`obj.fetch(...)`),
and matches inside comments or string literals stay silent. When a `method:`
property is present but its value is not a static string literal, the whole
call emits nothing rather than silently degrading to `GET`. `fetch` is a
global, so no import is required.

Axios calls are import-gated: they emit only when the file imports axios via a
default (`import axios from "axios"`) or namespace (`import * as axios from
"axios"`) import, and call sites are matched on the LOCAL binding, so `import
http from "axios"` gates `http.*` calls. Named imports such as `AxiosError` do
not gate. `axios.get/post/put/patch/delete/head/options("literal")` attests the
verb from the method name (generic type arguments as in
`axios.get<User[]>("/x")` are allowed); direct `axios("literal", { method:
"..." })` resolves the verb like fetch. In Vue SFCs the scan runs over
`<script>`/`<script setup>` section content only — template sections never
produce client-request facts — and the axios import gate is local to the
script section that declares it.

`nextjs.route_handler.v1` emits one fact per exported HTTP-verb handler in an
App Router `route.{js,ts}` file (`app/**/route.js`, `app/**/route.ts`,
including `src/app/**`). Recognized export forms are `export [async] function
GET(...)` and `export const|let|var GET = ...` (a `=` assignment or a `:` typed
binding). Recognized verbs are `GET`, `POST`, `PUT`, `PATCH`, `DELETE`, `HEAD`,
and `OPTIONS`; `verb` is the verb name and `verb_source` is always `attested`
because the export name is the source-of-truth verb. Next.js auto-implements
`OPTIONS` when it is not exported, but that synthesized handler is not attested
source, so only a literally exported `OPTIONS` emits a fact. `route_path`,
`normalized_route_template`, `dynamic_segments`, and the optional
`route_group_segments`/`parallel_route_segments`/`intercepting_route_markers`/`intercepted_route_segments`
keys are derived from the App Router directory segments with the same segment
walk `nextjs.file_route.v1` uses, so a `route` file and a sibling `page` file
resolve identical route paths. The span runs from the `export` keyword through
the handler name so `containing_symbol_id` binds to the handler symbol.
Re-exports (`export { GET } from ...`), default exports, non-verb exports
(`export function helper`), lowercase names (`export const get`), `.jsx`/`.tsx`
route files, `page` files, and route files outside an `app` directory all stay
silent, as do matches inside comments or string literals.

`nuxt.server_route.v1` emits one fact per Nitro server-route file under
`server/api/**` (route prefixed `/api`) or `server/routes/**` (no prefix). The
route path is derived from the file path using the same Nuxt segment
normalization as `nuxt.file_route.v1` — `[id]` -> `:id`, `[[id]]` -> `:id?`,
`[...slug]` -> `:slug*` — and `index.<method>`/`index` files map to their
directory route. The verb comes from the filename method suffix
(`users.get.ts` -> `GET`; also `.post`/`.put`/`.patch`/`.delete`/`.head`/`.options`);
`verb` and `verb_source` (`attested`) are present only when a suffix exists,
because a suffix-less handler answers every method. Emission requires a handler
signal — a `defineEventHandler` or `eventHandler` identifier — OR a method
suffix in the filename. A wrapped custom handler (for example
`defineWrappedResponseHandler`) with neither signal is a documented residual
miss and stays silent. `server/middleware`, `server/plugins`, and `server/utils`
are not routes and are excluded. Server routes are `.js`/`.ts` only; this family
claims the `server/**` space that `nuxt.file_route.v1` deliberately excludes.

Dynamic Vue `:to` bindings, named-route objects, non-literal route paths, spreads,
function-built routes, and lazy component imports are not emitted as static route
facts in this contract version.
Dynamic Nuxt `to` bindings, named-route objects, external `NuxtLink` targets, and
Nuxt named-view page files are not emitted as static route facts in this
contract version.
Dynamic React Router `to`/`path` values, arbitrary local `Link` components, and
Next.js `href` values without a static string or object `pathname` are not
emitted as static route facts in this contract version.

Route reference facts use `target_path`; route definition and file-route facts
use `route_path`, except Vue route definitions keep `target_path` for backward
compatibility with the original Vue fact family. Vue and React child route
definitions may include `parent_route_path` and `effective_route_template`.
Navigation reference facts for Vue, Nuxt, React Router, and Next.js include
`verb="GET"` as an implied navigation verb, not source-attested HTTP evidence.
`htmx.attribute.v1` keeps source-attested request verbs. `data-hx-*` attributes
normalize to canonical `hx-*` `attribute_name` values and include
`data_prefix=true`.

Next.js Pages Router file-route facts require local Next evidence in the file,
such as a `next/*` import or `getStaticProps`, `getServerSideProps`, or
`getStaticPaths`. App Router `app/**/page.*` conventions emit from the file path
alone. Next.js app-route `@slot` segments are excluded from `route_path` and
listed in `parallel_route_segments`; intercepting-route markers are stripped
from `route_path` and recorded in `intercepting_route_markers` with the target
segments in `intercepted_route_segments`.

Nuxt file-route normalization supports optional params (`[[id]]` ->
`:id?`, `dynamic_segments:["id?"]`) and mixed static/dynamic segments such as
`users-[group]` -> `users-:group`.

### `parse_diagnostic`

`record_id`: `diagnostic_id`.

Fields:

- `diagnostic_id`: string
- `file_id`: string
- `path`: root-relative path string
- `language`: string
- `kind`: `error` or `missing`
- `message`: string or `null`
- `span`: span object
- `metadata`: object or `null`

## Null And Empty Values

- Unknown optional values are `null`.
- Empty metadata objects are `{}`.
- Empty arrays are `[]`.
- Required strings must not be empty unless the SQLite contract allows that
  exact field to be empty.

## Streaming Use

The JSONL envelope supports streaming by using `op` values:

- `snapshot`: row emitted by a full export.
- `upsert`: row created or replaced by an incremental producer.
- `delete`: row removed by an incremental producer.

`julie-extract` v3 only guarantees `snapshot` output. A downstream tool may use
the same envelope for its own incremental transport if it preserves the schema
and record kinds.

## Error Handling

JSONL export is all-or-error from the CLI perspective:

- Successful export writes complete JSONL and a report with `status: ok`.
- Failed export writes a report with `status: failed` and does not claim a
  complete output file.
- If `--out -` is used, consumers should treat process exit code plus the final
  JSON report as the completion signal.

## Tradeoffs

- **Envelope over bare rows:** consumers can route records without knowing table
  order or inspecting payload fields.
- **Snake case:** JSONL follows SQLite/report naming rather than old Rust
  camelCase field names.
- **Snapshot first:** old Julie external extract did not expose JSONL output, so
  v3 keeps JSONL as a clean product export instead of a compatibility mode.
- **Open decision before implementation:** whether `julie-extract scan` should
  support direct JSONL streaming without writing SQLite. The current contract
  keeps SQLite as the source of truth and exposes JSONL through `export`.
