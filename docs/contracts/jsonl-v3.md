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
Structural-fact metadata is validated as a JSON object and emitted from the
stored SQLite JSON text, so object member order is preserved even though
consumers must treat JSON object order as semantically insignificant.

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
  "updated_at": "2026-05-31T16:05:00Z",
  "reference_resolution_status": "complete",
  "reference_resolution_version": 2,
  "reference_resolution_last_full_revision": 7
}
```

The three `reference_resolution_*` fields (additive, schema v4 artifacts) mirror
the SQLite `artifact_metadata` keys of the same names. All three are `null` when
the artifact has never run a resolution pass. **JSONL consumers apply the same
detection rule as SQLite consumers:** treat resolution data (including
`identifier.target_symbol_id`, which the resolution pass populates) as present
and trustworthy only when `reference_resolution_status` is `complete` or
`partial`; on `null` or `"failed"`, treat a `null` `target_symbol_id` as
"unknown", exactly as before resolution existed. Never gate on
`sqlite_schema_version`. See `sqlite-schema-v4.md` § Reference Resolution.

Resolution version 2 adds occurrence spans, receiver metadata, confidence
provenance, and span-derived relationship identities. A 2.17-or-newer
whole-workspace scan detects a missing or stale resolution version and
re-extracts every supported file before stamping version 2. Single-file
`update` and `delete` refuse the stale artifact with
`schema_migration_required`. An incomplete extraction or failed resolution pass
leaves the upgrade failed and those mutations blocked. The version stamp records
the resolution pass; it must not be treated as proof of upgraded row content if
metadata was rewritten outside `julie-extract`.

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
  `body_spans`, `structural_facts`, `complexity_metrics`, `annotations`,
  `doc_comments`, `literals`, `source_regions`, and `test_detection` domains

Each `kind_coverage` domain has `supported`, `not_applicable`, and `open_gaps`.
`test_detection` uses the fixed units `test_case`, `test_container`, and
`test_lifecycle`. It is additive inside the existing object, so JSONL remains
version 3 and the extraction contract version does not change. See
[Test Evidence v1](test-evidence-v1.md) for evidence and consumer rules.

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
- `target_symbol_id`: string or `null` — extraction leaves this `null`; on
  schema v4 artifacts the workspace resolution pass fills it for resolved
  references. `null` means "unknown", never "definitely unresolvable". Gate on
  the artifact record's `reference_resolution_status` before treating populated
  values as workspace-resolution output. Resolution provenance (tier,
  confidence, outcome) is SQLite-only (`identifier_resolutions`); JSONL exports
  the denormalized target only.
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

`containing_symbol_id` binds each fact to the innermost byte-containing
scope-bearing symbol. `variable`, `constant`, `enum_member`, and `import`
symbols are value holders, not scopes, so they are never containment candidates.
When no byte-containing candidate exists (for example, a fact whose span starts
on an `export const` head that sits outside its value symbol), a line-containment
fallback selects the narrowest line-spanning candidate whose byte span is not
contained by the fact, with deterministic tie-breaks (narrowest byte span, then
earliest start byte). Module-scope facts with no enclosing scope-bearing symbol
are `null`.

Supported patterns are advertised in `language_capability` records under
`kind_coverage.structural_facts.supported`.

The table below lists the structural-fact patterns and where they fire. The full
per-pattern metadata payload — every key each `pattern_id` can carry, with its
JSON value type and presence rule — is published as a machine-readable contract
at [`structural-fact-patterns.json`](./structural-fact-patterns.json), generated
from the in-process pattern registry
(`crates/julie-extractors/src/base/structural_fact_registry/`). Treat that file
as the source of truth for structural-fact metadata payloads. Regenerate the
checked-in file after an intentional registry change with:

```
UPDATE_CONTRACT_JSON=1 cargo test -p julie-extractors structural_fact_registry
```

Every fact carries the base keys `pattern_version` (integer, currently `1`) and
`query_family` (string); framework and web route/http facts additionally carry a
`framework` key.

| Pattern ID | Language | Capture | Node Kind(s) |
| --- | --- | --- | --- |
| `rust.unsafe_block.v1` | `rust` | `unsafe_block` | `unsafe_block` |
| `go.goroutine_launch.v1` | `go` | `go_statement` | `go_statement` |
| `go.defer_statement.v1` | `go` | `defer_statement` | `defer_statement` |
| `python.decorated_definition.v1` | `python` | `decorated_definition` | `decorated_definition` |
| `javascript.await_expression.v1` | `javascript` | `await_expression` | `await_expression` |
| `jsx.await_expression.v1` | `jsx` | `await_expression` | `await_expression` |
| `typescript.await_expression.v1` | `typescript` | `await_expression` | `await_expression` |
| `tsx.await_expression.v1` | `tsx` | `await_expression` | `await_expression` |
| `c.preprocessor_definition.v1` | `c` | `preprocessor_definition` | `preproc_def`, `preproc_function_def` |
| `cpp.preprocessor_definition.v1` | `cpp` | `preprocessor_definition` | `preproc_def`, `preproc_function_def` |
| `aspnet.minimal_api.route.v1` | `csharp` | `route_call` | parser-covered invocation span |
| `aspnet.minimal_api.route_group.v1` | `csharp` | `route_group` | parser-covered invocation span |
| `aspnet.attribute_route.v1` | `csharp` | `attribute_route` | `attribute` |
| `express.route.v1` | `javascript`, `jsx`, `typescript`, `tsx` | `route_call` | parser-covered call span |
| `express.router_mount.v1` | `javascript`, `jsx`, `typescript`, `tsx` | `router_mount` | parser-covered call span |
| `fastify.route.v1` | `javascript`, `jsx`, `typescript`, `tsx` | `route_call` | parser-covered call span |
| `nestjs.route.v1` | `javascript`, `typescript` | `route_decorator` | handler method declaration span |
| `fastapi.route.v1` | `python` | `route` | decorated function declaration span |
| `fastapi.include_router.v1` | `python` | `include_router` | parser-covered call span |
| `flask.route.v1` | `python` | `route` | decorated function declaration span |
| `flask.blueprint_registration.v1` | `python` | `blueprint_registration` | parser-covered call span |
| `django.url_pattern.v1` | `python` | `url_pattern` | parser-covered call span |
| `django.url_include.v1` | `python` | `url_include` | parser-covered call span |
| `spring.request_mapping.v1` | `java`, `kotlin` | `request_mapping` | class or method declaration line (Kotlin anchors the handler `function_declaration`) |
| `go.net_http.route.v1` | `go` | `route_call` | parser-covered call span |
| `gin.route.v1` | `go` | `route_call` | parser-covered call span |
| `echo.route.v1` | `go` | `route_call` | parser-covered call span |
| `rails.route.v1` | `ruby` | `route` | parser-covered DSL call span |
| `rails.resource_route.v1` | `ruby` | `resource_route` | parser-covered DSL call span |
| `rails.mount.v1` | `ruby` | `mount` | parser-covered DSL call span |
| `laravel.route.v1` | `php` | `route` | parser-covered `Route` facade call span |
| `laravel.resource_route.v1` | `php` | `resource_route` | parser-covered `Route::resource`/`apiResource` call span |
| `laravel.route_prefix.v1` | `php` | `route_prefix` | parser-covered `Route::prefix`/`group` prefix site |
| `symfony.route.v1` | `php` | `request_mapping` | class or method declaration carrying a static `#[Route]` attribute |
| `ktor.route.v1` | `kotlin` | `route` | parser-covered bare verb call span inside `routing{}`/`route{}` |
| `phoenix.route.v1` | `elixir` | `route` | parser-covered router verb-macro call span |
| `phoenix.resource_route.v1` | `elixir` | `resource_route` | parser-covered `resources` macro call span |
| `phoenix.forward.v1` | `elixir` | `forward` | parser-covered `forward` macro call span |
| `axum.route.v1` | `rust` | `route` | parser-covered `.route` call span |
| `axum.nest.v1` | `rust` | `nest` | parser-covered `.nest` call span |
| `actix.attribute_route.v1` | `rust` | `attribute_route` | parser-covered handler `function_item` span |
| `actix.scope_route.v1` | `rust` | `scope_route` | parser-covered `web::scope(...).route` call span |
| `actix.mount.v1` | `rust` | `mount` | parser-covered `web::scope(...).configure`/`.service` call span |
| `htmx.attribute.v1` | `html`, `razor`, `javascript`, `jsx`, `tsx`, `vue` | `attribute` | parser-covered attribute span |
| `alpine.directive.v1` | `html`, `razor` | `directive` | parser-covered attribute span |
| `razor.page_directive.v1` | `razor` | `page_directive` | `razor_page_directive` |
| `razor.code_block.v1` | `razor` | `code_block` | `razor_block` |
| `razor.template_expression.v1` | `razor` | `template_expression` | `razor_implicit_expression`, `razor_explicit_expression` |
| `css.selector_rule.v1` | `css`, `vue`, `html` | `rule_set` | `rule_set` |
| `css.custom_property.v1` | `css`, `vue`, `html` | `custom_property` | `property_name` |
| `css.media_query.v1` | `css`, `vue`, `html` | `media_query` | `media_statement` |
| `css.keyframes.v1` | `css`, `vue`, `html` | `keyframes` | `keyframes_statement` |
| `css.supports.v1` | `css`, `vue`, `html` | `supports` | `supports_statement` |
| `css.container.v1` | `css`, `vue`, `html` | `container` | `at_rule` |
| `css.font_face.v1` | `css`, `vue`, `html` | `font_face` | `at_rule` |
| `css.layer.v1` | `css`, `vue`, `html` | `layer` | `at_rule` |
| `css.charset.v1` | `css`, `vue`, `html` | `charset` | `charset_statement` |
| `css.namespace.v1` | `css`, `vue`, `html` | `namespace` | `namespace_statement` |
| `html.link.v1` | `html` | `link` | `element` |
| `html.area_link.v1` | `html` | `area_link` | `element` |
| `html.media.v1` | `html` | `media` | `element` |
| `html.landmark.v1` | `html` | `landmark` | `element` |
| `html.data_attribute.v1` | `html` | `data_attribute` | `element` |
| `html.script.v1` | `html` | `script` | `script_element` |
| `html.form.v1` | `html` | `form` | `element` |
| `html.form_control.v1` | `html` | `form_control` | `element` |
| `vue.sfc_section.v1` | `vue` | `section` | `sfc_section` |
| `vue.template_directive.v1` | `vue` | `directive` | `template_attribute` |
| `vue.route_reference.v1` | `vue` | `route_reference` | `template_attribute` |
| `vue.route_definition.v1` | `javascript`, `jsx`, `typescript`, `tsx`, `vue` | `route_definition` | `object` |
| `nuxt.route_reference.v1` | `vue` | `route_reference` | `template_attribute` |
| `nuxt.file_route.v1` | `javascript`, `jsx`, `typescript`, `tsx`, `vue` | `file_route` | `file` |
| `react.route_reference.v1` | `javascript`, `jsx`, `tsx` | `route_reference` | `jsx_attribute` |
| `react.route_definition.v1` | `javascript`, `jsx`, `typescript`, `tsx` | `route_definition` | `object`, `jsx_element` |
| `nextjs.route_reference.v1` | `javascript`, `jsx`, `tsx` | `route_reference` | `jsx_attribute` |
| `nextjs.file_route.v1` | `javascript`, `jsx`, `typescript`, `tsx` | `file_route` | `file` |
| `nextjs.route_handler.v1` | `javascript`, `typescript` | `route_handler` | `export_statement` |
| `nuxt.server_route.v1` | `javascript`, `typescript` | `server_route` | `file` |
| `http.client_request.v1` | `javascript`, `jsx`, `typescript`, `tsx`, `vue`, `python`, `csharp`, `razor`, `go`, `java`, `kotlin`, `php`, `ruby`, `elixir`, `rust` | `client_request` | parser-covered call span (Java builder chains anchor the enclosing statement) |

ASP.NET route facts emit `normalized_route_template` as the server-side
cross-family join key. Minimal API route calls compute it from
`effective_route_template` when a same-file `MapGroup` prefix is resolved, else
from `route_template`; `MapGroup` route-group facts compute it from
`route_prefix`; attribute-routing facts compute it from
`effective_route_template` when present, else `route_template`. The raw
`route_template`, `route_prefix`, and `effective_route_template` values remain
the source-shaped ASP.NET strings.

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

Backend route facts share these gates. Express and Fastify routes are
import-gated and receiver-traced in-file; a Fastify plugin parameter named
`fastify` attests the framework by itself, while a generic `app` parameter
counts only when the file also imports fastify. A verb-method call whose only
argument is a string literal (Express's `app.get('setting')` getter) is not a
route. Spring `@RequestMapping`-family templates come only from the positional
value or `value =`/`path =` annotation elements; `produces`/`consumes`/
`params`/`headers` literals never become routes. Method-level shortcut
annotations (`@GetMapping`, ...) emit `attribute_kind="http_method"`;
`@RequestMapping` on a method emits `attribute_kind="request_mapping"` with
`verb` present only when a `method =` element names it. Each class declaration
resets the class-level template, so one controller's prefix cannot leak into
the next. Kotlin Spring controllers emit the same `spring.request_mapping.v1`
shape from a separate AST-driven collector (import-gated on
`org.springframework.web.bind.annotation`): the class `@RequestMapping` prefix
resets per `class`/`object`/`companion object`; multi-path values use Kotlin's
bracket arrays (`["/a", "/b"]`, not Java's `{...}`); `$`-interpolated,
concatenated, and identifier route arguments stay silent; and each fact is
anchored on the handler `function_declaration` so `containing_symbol_id` binds
to the handler function, not the enclosing class. Go `net/http` patterns follow Go 1.22 `[METHOD ][HOST]/[PATH]`
parsing: `route_template` carries the path part, `verb` the method token, and
`host` the host part when present. gin/echo routes emit
`api_style="call_routing"` (`mux_routing` is reserved for `go.net_http.route.v1`);
nested `Group` calls compose literal prefixes, and a non-literal prefix poisons
the chain so its routes emit `route_template` only. The echo import gate
accepts any major version of `github.com/labstack/echo`. Rails DSL facts
require `config/routes.rb` routes to sit inside a `routes.draw do ... end`
block; split files under `config/routes/` allow top-level DSL. Every
`do ... end` block is depth-tracked, so `member`/`collection`/`constraints`
blocks do not pop enclosing `namespace`/`scope` prefixes early. Laravel routes
come from an AST-driven collector import-gated on the `Route::` facade:
`Route::get/post/put/patch/delete/options` emit `api_style="call_routing"` with
an upper-cased `verb`, `Route::any` omits the verb (not verb-restricted), and
`Route::match(['get','post'], ...)` emits one fact per static verb. Laravel
`{param}`/`{param?}` segments normalize to `:param`. `Route::resource`/
`apiResource` emit `laravel.resource_route.v1` (`resource_kind` `resource` or
`api_resource`). Same-file `Route::prefix('x')->group(closure)` and
`Route::group(['prefix'=>'x'], closure)` are lexical-containment prefixes: they
join into `route_group_prefix`/`effective_route_template` on the contained
routes and emit a `laravel.route_prefix.v1` fact (`mount_path`/
`normalized_mount_path`) at the prefix site; a non-literal prefix poisons the
group so contained routes emit `route_template` only. Interpolated,
concatenated, `self::CONST`, and variable route arguments stay silent (M2).
Symfony `#[Route]` attributes emit the separate `symfony.route.v1` pattern.
Cross-file `RouteServiceProvider` prefixes remain out of scope, so Laravel
`route_template` is not guaranteed to be the absolute public path when such a
prefix applies. Ktor server routes emit `ktor.route.v1` under a restricted
lexical gate inside `routing{}`/`route{}`, gated on a server-side import
(`io.ktor.server.*`, or Ktor 1.x `io.ktor.routing.*`/`io.ktor.application.*`) so
client-only `io.ktor.client.*` files stay silent; enclosing `route("/prefix")`
scopes join with the verb path into `effective_route_template` (accumulating when
nested), while the raw literal stays in `route_template`.

Phoenix routes come from an AST-driven collector import-gated on a
`Phoenix.Router`/`:router` router module: the bare verb macros
`get/post/put/patch/delete/head/options "/path", Ctrl, :action` emit
`phoenix.route.v1` (`api_style="dsl_routing"`) with an upper-cased `verb`, the
controller module `alias` (`controller`) and action atom (`action`). Phoenix
`:id` segments are already the normalized `:param` flavor. Same-file
`scope "/api", Ctrl do ... end` blocks are lexical-containment prefixes (the
Rails `scope_stack` shape over the AST): they join into
`route_group_prefix`/`effective_route_template` on the contained routes and
resources, accumulate when nested, and an interpolated prefix poisons the scope
so contained routes emit `route_template` only. An options-only or alias-only
scope adds no path segment. `resources "/x", Ctrl` emits
`phoenix.resource_route.v1` (`resource_path`/`normalized_resource_path`), and
`forward "/lit", Plug` emits a `phoenix.forward.v1` prefix registration
(`mount_path`/`normalized_mount_path`/`mount_target`) at its own site.
Interpolated (`"/u/#{id}"`), concatenated (`"/a/" <> id`), `~r` regex-sigil,
`~S`-with-literal-`#{}`, and `@attr`/identifier route arguments stay silent
(M2). The `pipe_through`, `live`, `socket`, and `channel` macros, and cross-file
scope prefixes, are out of scope, so `route_template` is not guaranteed to be
the absolute public path when a cross-file prefix applies.

axum routes come from an AST-driven collector import-gated on an `axum`
reference. A `Router::new().route("/path", get(h).post(c))` call emits one
`axum.route.v1` (`api_style="call_routing"`) per method-router verb; the second
argument must be a bare-identifier verb chain (`get(h)`, `get(a).post(b)`, with
non-verb middleware like `.layer(...)` transparent), which is how actix's
`web::get().to(h)` argument shape is rejected on the shared `rust` dispatch arm.
`any`/`any_service` omit the `verb`/`verb_source` keys (not verb-restricted).
axum 0.8 `{id}` brace captures normalize to the shared `:id` join key; a 0.7
`:id` template joins correctly but under-reports `dynamic_segments` (an honest
under-report — the extractor does not version-sniff). The `.route`/`.nest`
receiver is single-assignment traced same-file: a `Router::new()` chain (inline
or via a variable) or an unknown receiver (a function parameter/return) emits,
but a variable also reassigned a conflicting non-router value is poisoned and
its `.route`/`.nest` calls stay silent (Go poison model). `.nest("/lit", sub)`
emits an `axum.nest.v1` prefix registration
(`mount_path`/`normalized_mount_path`/`mount_target`) at its own site; the
nested sub-router is a cross-file target, so no route join is guessed (Miller's
job). `format!`, concatenated, and `const`/identifier route arguments stay
silent (M2).

actix-web routes come from an AST-driven collector on the same shared `rust`
dispatch arm, import-gated on an `actix_web` reference (which, with the arg
shapes below, keeps axum and actix from ever double-emitting). actix registers
routes through two provenance models, mirroring the `aspnet.attribute_route.v1`
vs `aspnet.minimal_api.route.v1` split, so it emits two route pattern ids plus a
mount. (1) Attribute macros `#[get("/x")]`/`#[post("/x")]`/… and `#[route("/x",
method = "GET")]` on a handler `fn` emit `actix.attribute_route.v1`
(`api_style="attribute"`), one fact per verb — the verb is ALWAYS known (from the
macro name, or one per `method = "VERB"` argument). Registration is cross-file,
so there are no `route_group_prefix`/`effective_route_template` keys, and the
fact anchors on the handler `function_item` (a following sibling of the
attribute) so its binding resolves to the handler, not the enclosing module. (2)
Scope-chained routes `web::scope("/api").route("/x", web::post().to(h))` emit
`actix.scope_route.v1` (`api_style="call_routing"`): the scope prefix is read
same-file by walking the `.route` receiver chain to its base `web::scope(literal)`,
so it flows into `route_group_prefix` + `effective_route_template`; the verb comes
from the `web::<verb>()` method router (OPT — omitted for the method-agnostic
`web::route()`). A `web::<verb>().to(h)` method router bottoms out at a
`scoped_identifier`, which is how axum's bare-identifier verb chain (`get(h)`) is
rejected here; conversely axum's `Router::new()` receiver is not a `web::scope`,
so actix rejects it. `web::scope("/lit").configure(fn)`/`.service(sub)` emits an
`actix.mount.v1` prefix registration (`mount_path`/`normalized_mount_path`/
`mount_target`) at the scope site; the delegated routes are a cross-file target,
so no route join is guessed. `{id}` brace captures normalize to the shared `:id`
join key. A non-static scope prefix (making the absolute path unknowable),
`format!`/concat/`const` route arguments, variable-bound scopes, and
`web::resource().route()` guard forms all stay silent (M2) and are recorded as
documented `open_gaps`.

Backend-language client collectors emit the same `http.client_request.v1`
metadata shape for static string URL arguments: Python module-qualified
`requests`/`httpx` calls, C# `HttpClient` method calls and
`HttpRequestMessage`, Go `net/http` package calls, Java `HttpRequest` builder
chains, Kotlin Ktor `client.get("...")`-style verb calls (`client="ktor"`,
import-gated on `io.ktor.client`; only a `receiver.verb(...)` navigation callee
qualifies, so the server-side routing DSL `get("/x")` is not a client request),
PHP Guzzle `$client->get('...')` (`client="guzzle"`, import-gated on
`GuzzleHttp`) and the Laravel `Http` facade `Http::get('...')`
(`client="laravel_http"`, import-gated on `Facades\Http`, including chained
`Http::withToken(...)->get(...)`), Ruby `Net::HTTP` calls with literal
`URI(...)`/`URI.parse(...)` arguments, and Elixir Req `Req.get("...")` verb
calls (`client="req"`, import-gated on `Req.`; bang variants `Req.get!(...)`
share the verb; the keyword-list form `Req.get(url: "...")` has no positional
string URL and stays silent), and Rust reqwest calls — the scoped convenience
free function `reqwest::get("...")` and the builder verb form
`client.get("...")` / `reqwest::Client::new().get("...")` (`client="reqwest"`,
import-gated on `reqwest`). For the builder form only, the URL must be url-like
(absolute `scheme://` or `/`-rooted) so a `HashMap::get("key")` lookup stays
silent; the scoped form is unambiguous. Java builder-chain facts span the enclosing statement (the URL and
verb are resolved statement-locally), so their `node_kind` is the statement
node rather than a call node. Instance/session clients and dynamic URL
expressions stay silent.

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

`aspnet.attribute_route.v1` emits definition facts for attribute-routed ASP.NET
controllers, using tree-sitter attribution (each attribute node is bound to its
owning class or method declaration, not raw text association). Three
`attribute_kind` shapes are emitted:

- `controller_route` — a class-level `[Route("...")]` with a literal template
  (e.g. `[Route("api/[controller]")]`). Carries `route_template` (as written)
  and `effective_route_template`.
- `http_method` — a method-level `[HttpGet]`/`[HttpPost]`/`[HttpPut]`/`[HttpPatch]`/`[HttpDelete]`/`[HttpHead]`/`[HttpOptions]`
  attribute. Carries `verb` (upper-cased from the attribute name), `route_template`
  when the attribute has a literal argument, `controller_route_template` when the
  enclosing class has a literal `[Route]`, and `effective_route_template`. Bare
  `[HttpPost]` (no argument) inherits the controller-level effective template.
  Multiple `Http*` attributes on one method emit one fact each.
- `route` — a method-level `[Route("...")]` on an action method that has **no**
  `Http*` verb attribute (no `verb`). When a method carries both a verb attribute
  and a sibling `[Route]`, only the verb attribute emits; the `[Route]` does not
  produce a separate `route` fact.

`effective_route_template` is formed by joining the controller template and
method template, substituting the `[controller]` token (the class name minus a
trailing `Controller`) and the `[action]` token (the method name) with their
lowercased values, then normalizing a single leading `/` (e.g.
`UsersController` + `[HttpGet("{id}")]` -> `/api/users/{id}`). The
cross-family join key is `normalized_route_template`, which converts ASP.NET
parameters such as `{id}` and `{id:int}` to `:id` while preserving trailing
slashes. `route_tokens` lists the tokens that were substituted (e.g.
`["controller"]`), so consumers know a substitution occurred.

Attributes whose route argument is not a plain string literal (interpolation,
concatenation, `nameof`, constant references) stay silent. A `[ApiController]`
without any route attributes emits nothing. `[HttpGet]` on a class is invalid
routing and is ignored (only `[Route]` is read at class level). Conventional
(non-attribute, `MapControllerRoute`-style) routing is out of scope for this
family.

`nestjs.route.v1` emits one fact per HTTP-method decorator
(`@Get`/`@Post`/`@Put`/`@Patch`/`@Delete`/`@Options`/`@Head`/`@All`) on a NestJS
controller method, joined same-file to the class `@Controller('base')` prefix
(the Spring class+method join model). `api_style` is `decorator_routing`. `verb`
is the upper-cased method name with `verb_source="attested"`; `@All` accepts any
method, so it omits both `verb` and `verb_source`. `route_template` is the raw
method sub-path (empty for a bare `@Get()`). `class_route_template` carries the
static `@Controller` prefix — a plain string, the `path` of a `{ path }` object,
or each element of a string array (arrays, and array method paths, cross-product
into one fact per combination). `effective_route_template` joins the class prefix
with the method sub-path (an empty method path resolves to the class prefix
alone, avoiding a trailing slash), and `normalized_route_template` — the
cross-family join key — converts NestJS `:id` segments and applies a single
leading `/`. The fact span is anchored to the handler `method_definition` (not
the decorator, which in tree-sitter-typescript is a preceding sibling outside the
method symbol range) so `containing_symbol_id` binds to the handler method.
Emission requires a `@nestjs/common` import, and only a plain string-literal
decorator argument emits: template literals (`` `/a/${x}` ``), concatenation
(`'/a/' + x`), identifier/const references (`PATHS.USER`), and other computed
arguments stay silent (whole-argument static check). `app.setGlobalPrefix(...)`
(cross-file, no safe mount consumer), `RouterModule.register(...)` dynamic
composition, and versioned routes are out of scope for this family.

Dynamic Vue `:to` bindings, named-route objects, non-literal route paths, spreads,
function-built routes, and lazy component imports are not emitted as static route
facts in this contract version.
Dynamic Nuxt `to` bindings, named-route objects, external `NuxtLink` targets, and
Nuxt named-view page files are not emitted as static route facts in this
contract version.
Dynamic React Router `to`/`path` values, arbitrary local `Link` components, and
Next.js `href` values without a static string or object `pathname` are not
emitted as static route facts in this contract version.

Route reference facts, `html.form.v1`, and `http.client_request.v1` use
`target_path`; route definition and file-route facts use `route_path`, except
Vue route definitions keep `target_path` for backward compatibility with the
original Vue fact family. The HTTP method is always carried as `verb`
(upper-cased), never `http_method`. Vue and React child route definitions may
include `parent_route_path` and `effective_route_template`. Navigation reference
facts for Vue, Nuxt, React Router, and Next.js include `verb="GET"` as an
implied navigation verb, not source-attested HTTP evidence.
`htmx.attribute.v1` keeps source-attested request verbs. `data-hx-*` attributes
normalize to canonical `hx-*` `attribute_name` values and include
`data_prefix=true`. Beyond `html`/`razor`, the family also covers JSX/TSX
component markup (`javascript`, `jsx`, `tsx`) and Vue single-file-component
`<template>` sections (`vue`). On these component surfaces only static string
attribute values emit: JSX brace-expression values (`hx-post={url}`) and Vue
dynamic bindings (`:hx-post`, `v-bind:hx-post`) stay silent, and Vue scanning is
restricted to `<template>` so htmx attributes inside `<script>` strings are not
reported. `typescript` is intentionally excluded because the plain TypeScript
grammar cannot parse JSX (it reads `<Ident ...>` as a type expression).

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
