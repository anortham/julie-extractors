# SQLite Schema v3

## Scope

SQLite is the primary durable artifact for `julie-extractors`.

This document defines the v3 logical schema. Implementations may add
indexes, views, and internal helper tables, but downstream readers should rely
only on the tables and columns named here.

## Invariants

- One database represents one canonical source root.
- File paths are root-relative Unix-style strings.
- IDs are opaque stable text values. Consumers must not parse ID internals.
- Lines are 1-based. Columns are 0-based. Byte offsets are 0-based offsets into
  the original UTF-8 file content.
- Full source file content is not stored. The artifact stores file metadata,
  hashes, spans, and source-derived extraction facts; consumers that need the
  complete file text must read the matching source tree.
- Enum values are lower-case snake_case strings.
- Booleans are stored as `INTEGER NOT NULL` values `0` or `1`.
- Timestamps are RFC 3339 UTC strings.
- JSON columns store UTF-8 JSON text and must be valid JSON when non-null.
- Tree-sitter node kinds, parser object names, and Rust enum names are internal
  implementation details unless they are explicitly exposed through capability
  metadata.
- The final artifact must include the required indexes in this contract.

## Metadata

### `artifact_metadata`

Key-value metadata for the whole artifact.

Required keys:

- `artifact_id`: generated stable identifier for this artifact.
- `root_path`: canonical source root.
- `schema_version`: `3`.
- `extract_contract_version`: `3`.
- `sqlite_schema_version`: `3`.
- `binary_version`: `julie-extract` version that last wrote the artifact.
- `hash_algorithm`: content hash algorithm name.
- `parser_inventory_fingerprint`: fingerprint of parser package inventory.
- `capability_snapshot_fingerprint`: fingerprint of language capabilities.
- `created_at`: artifact creation timestamp.
- `updated_at`: last successful mutation timestamp.

```sql
CREATE TABLE artifact_metadata (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
```

### `parser_inventory`

Parser dependency rows captured at artifact creation or mutation time.

```sql
CREATE TABLE parser_inventory (
  language TEXT NOT NULL,
  parser_package TEXT NOT NULL,
  parser_version TEXT,
  grammar_version TEXT,
  source TEXT,
  metadata_json TEXT,
  PRIMARY KEY (language, parser_package)
);
```

`parser_package` may be a Rust crate, vendored grammar, or another parser
package identifier. Downstream readers should treat it as evidence, not as an
API they need to load.

## Revisions

### `extraction_revisions`

One row per committed artifact mutation.

```sql
CREATE TABLE extraction_revisions (
  revision_id INTEGER PRIMARY KEY,
  parent_revision_id INTEGER,
  operation TEXT NOT NULL,
  mode TEXT,
  started_at TEXT NOT NULL,
  completed_at TEXT NOT NULL,
  binary_version TEXT NOT NULL,
  extract_contract_version INTEGER NOT NULL,
  sqlite_schema_version INTEGER NOT NULL,
  input_root TEXT,
  counts_json TEXT NOT NULL,
  FOREIGN KEY (parent_revision_id) REFERENCES extraction_revisions(revision_id)
);
```

`operation` values are `scan`, `update`, and `delete`.

`mode` values are operation-specific. `scan` uses `incremental` or `force`.
`update` and `delete` use `single_file`.

### `revision_file_changes`

Files changed by a revision.

```sql
CREATE TABLE revision_file_changes (
  revision_id INTEGER NOT NULL,
  file_id TEXT NOT NULL,
  path TEXT NOT NULL,
  change_kind TEXT NOT NULL,
  PRIMARY KEY (revision_id, file_id),
  FOREIGN KEY (revision_id) REFERENCES extraction_revisions(revision_id)
);
```

`change_kind` values are `inserted`, `updated`, `deleted`, and `unsupported`.

## Files

### `files`

One row per source file currently represented in the artifact.

```sql
CREATE TABLE files (
  file_id TEXT PRIMARY KEY,
  path TEXT NOT NULL UNIQUE,
  language TEXT NOT NULL,
  content_hash TEXT NOT NULL,
  content_bytes INTEGER NOT NULL,
  line_count INTEGER,
  indexed_at TEXT NOT NULL,
  last_revision_id INTEGER NOT NULL,
  status TEXT NOT NULL,
  metadata_json TEXT,
  FOREIGN KEY (last_revision_id) REFERENCES extraction_revisions(revision_id)
);
```

`status` values are `indexed`, `unsupported`, and `failed_preserved`.

Unsupported files normally have no row. `unsupported` is allowed only when a
consumer needs evidence that stale rows were removed for that path.

## Symbols

### `symbols`

Named source entities.

```sql
CREATE TABLE symbols (
  symbol_id TEXT PRIMARY KEY,
  file_id TEXT NOT NULL,
  path TEXT NOT NULL,
  language TEXT NOT NULL,
  name TEXT NOT NULL,
  kind TEXT NOT NULL,
  signature TEXT,
  doc_comment TEXT,
  visibility TEXT,
  parent_symbol_id TEXT,
  start_line INTEGER NOT NULL,
  start_column INTEGER NOT NULL,
  end_line INTEGER NOT NULL,
  end_column INTEGER NOT NULL,
  start_byte INTEGER NOT NULL,
  end_byte INTEGER NOT NULL,
  body_start_line INTEGER,
  body_start_column INTEGER,
  body_end_line INTEGER,
  body_end_column INTEGER,
  body_start_byte INTEGER,
  body_end_byte INTEGER,
  body_hash TEXT,
  semantic_group TEXT,
  confidence REAL,
  content_type TEXT,
  is_test INTEGER NOT NULL DEFAULT 0,
  test_container INTEGER NOT NULL DEFAULT 0,
  test_lifecycle INTEGER NOT NULL DEFAULT 0,
  metadata_json TEXT,
  FOREIGN KEY (file_id) REFERENCES files(file_id) ON DELETE CASCADE,
  FOREIGN KEY (parent_symbol_id) REFERENCES symbols(symbol_id) ON DELETE SET NULL
);
```

`body_hash` is present only when all body span columns are present. It is an exact normalized-body fingerprint. The algorithm id is
`julie-normalized-body-md5-v1`: take the source bytes covered by the body span,
tokenize them while preserving quoted string-like tokens, join normalized tokens
with U+001F, and store the lowercase MD5 hex digest. The normalization ignores
whitespace and comments for the symbol language. Equal `body_hash` values are
exact normalized-body match candidates. `body_hash` does not encode duplicate severity,
near-duplicate similarity, or product-level clone ranking; consumers own those
thresholds and presentation choices.

`is_test`, `test_container`, and `test_lifecycle` are integer booleans (`0` or
`1`) derived from extractor test-role metadata.

Artifact producers must keep these first-class columns and the reserved metadata
keys in sync:

- `is_test`: `1` means the extractor identified the symbol as a test case or
  test lifecycle hook.
- `test_container`: `1` means the symbol groups tests, for example `describe`,
  `context`, `suite`, or `group` constructs.
- `test_lifecycle`: `1` means the symbol is setup, teardown, or an equivalent
  lifecycle hook. Lifecycle hooks must also have `is_test = 1`.

These fields are extraction metadata. They are not Julie test linkage, test
quality, or reference-scoring analysis.

### `symbol_annotations`

Annotations, decorators, attributes, or equivalent markers attached to symbols.

```sql
CREATE TABLE symbol_annotations (
  annotation_id TEXT PRIMARY KEY,
  symbol_id TEXT NOT NULL,
  annotation TEXT NOT NULL,
  annotation_key TEXT NOT NULL,
  raw_text TEXT,
  carrier TEXT,
  metadata_json TEXT,
  FOREIGN KEY (symbol_id) REFERENCES symbols(symbol_id) ON DELETE CASCADE
);
```

## Identifiers

### `identifiers`

Usage locations such as calls, variable references, type usages, and member
accesses.

```sql
CREATE TABLE identifiers (
  identifier_id TEXT PRIMARY KEY,
  file_id TEXT NOT NULL,
  path TEXT NOT NULL,
  language TEXT NOT NULL,
  name TEXT NOT NULL,
  kind TEXT NOT NULL,
  containing_symbol_id TEXT,
  target_symbol_id TEXT,
  start_line INTEGER NOT NULL,
  start_column INTEGER NOT NULL,
  end_line INTEGER NOT NULL,
  end_column INTEGER NOT NULL,
  start_byte INTEGER NOT NULL,
  end_byte INTEGER NOT NULL,
  confidence REAL NOT NULL,
  code_context TEXT,
  metadata_json TEXT,
  FOREIGN KEY (file_id) REFERENCES files(file_id) ON DELETE CASCADE,
  FOREIGN KEY (containing_symbol_id) REFERENCES symbols(symbol_id) ON DELETE SET NULL,
  FOREIGN KEY (target_symbol_id) REFERENCES symbols(symbol_id) ON DELETE SET NULL
);
```

## Relationships

### `relationships`

Resolved symbol-to-symbol edges.

```sql
CREATE TABLE relationships (
  relationship_id TEXT PRIMARY KEY,
  from_symbol_id TEXT NOT NULL,
  to_symbol_id TEXT NOT NULL,
  file_id TEXT NOT NULL,
  path TEXT NOT NULL,
  kind TEXT NOT NULL,
  start_line INTEGER,
  start_column INTEGER,
  end_line INTEGER,
  end_column INTEGER,
  start_byte INTEGER,
  end_byte INTEGER,
  confidence REAL NOT NULL,
  metadata_json TEXT,
  FOREIGN KEY (from_symbol_id) REFERENCES symbols(symbol_id) ON DELETE CASCADE,
  FOREIGN KEY (to_symbol_id) REFERENCES symbols(symbol_id) ON DELETE CASCADE,
  FOREIGN KEY (file_id) REFERENCES files(file_id) ON DELETE CASCADE
);
```

### `pending_relationships`

Structured unresolved edges whose target may resolve in another file or a
subsequent extraction pass.

```sql
CREATE TABLE pending_relationships (
  pending_relationship_id TEXT PRIMARY KEY,
  from_symbol_id TEXT NOT NULL,
  caller_scope_symbol_id TEXT,
  file_id TEXT NOT NULL,
  path TEXT NOT NULL,
  kind TEXT NOT NULL,
  target_display_name TEXT NOT NULL,
  target_terminal_name TEXT NOT NULL,
  target_receiver TEXT,
  target_namespace_json TEXT NOT NULL,
  target_import_context TEXT,
  start_line INTEGER NOT NULL,
  start_column INTEGER,
  end_line INTEGER,
  end_column INTEGER,
  start_byte INTEGER,
  end_byte INTEGER,
  confidence REAL NOT NULL,
  metadata_json TEXT,
  FOREIGN KEY (from_symbol_id) REFERENCES symbols(symbol_id) ON DELETE CASCADE,
  FOREIGN KEY (caller_scope_symbol_id) REFERENCES symbols(symbol_id) ON DELETE SET NULL,
  FOREIGN KEY (file_id) REFERENCES files(file_id) ON DELETE CASCADE
);
```

`target_namespace_json` is a JSON array of strings.

## Type Facts

### `type_facts`

Type information attached to a symbol.

```sql
CREATE TABLE type_facts (
  type_fact_id TEXT PRIMARY KEY,
  symbol_id TEXT NOT NULL,
  language TEXT NOT NULL,
  resolved_type TEXT NOT NULL,
  generic_params_json TEXT,
  constraints_json TEXT,
  is_inferred INTEGER NOT NULL,
  metadata_json TEXT,
  FOREIGN KEY (symbol_id) REFERENCES symbols(symbol_id) ON DELETE CASCADE
);
```

### `type_argument_usages`

Type argument usage attached to an identifier.

```sql
CREATE TABLE type_argument_usages (
  usage_id TEXT PRIMARY KEY,
  identifier_id TEXT NOT NULL,
  file_id TEXT NOT NULL,
  path TEXT NOT NULL,
  language TEXT NOT NULL,
  metadata_json TEXT,
  FOREIGN KEY (identifier_id) REFERENCES identifiers(identifier_id) ON DELETE CASCADE,
  FOREIGN KEY (file_id) REFERENCES files(file_id) ON DELETE CASCADE
);
```

### `type_arguments`

Normalized nested type arguments for one usage.

```sql
CREATE TABLE type_arguments (
  type_argument_id TEXT PRIMARY KEY,
  usage_id TEXT NOT NULL,
  parent_type_argument_id TEXT,
  ordinal INTEGER NOT NULL,
  type_name TEXT NOT NULL,
  FOREIGN KEY (usage_id) REFERENCES type_argument_usages(usage_id) ON DELETE CASCADE,
  FOREIGN KEY (parent_type_argument_id) REFERENCES type_arguments(type_argument_id) ON DELETE CASCADE
);
```

## Literals

### `literals`

String or scalar literals that carry useful extracted facts such as URLs, SQL,
or other configured carriers. Route is reserved until route carriers are
explicitly configured.

```sql
CREATE TABLE literals (
  literal_id TEXT PRIMARY KEY,
  file_id TEXT NOT NULL,
  path TEXT NOT NULL,
  language TEXT NOT NULL,
  literal_text TEXT NOT NULL,
  kind TEXT NOT NULL,
  carrier TEXT,
  arg_position INTEGER NOT NULL,
  containing_symbol_id TEXT,
  start_line INTEGER NOT NULL,
  start_column INTEGER NOT NULL,
  end_line INTEGER NOT NULL,
  end_column INTEGER NOT NULL,
  start_byte INTEGER NOT NULL,
  end_byte INTEGER NOT NULL,
  confidence REAL NOT NULL,
  metadata_json TEXT,
  FOREIGN KEY (file_id) REFERENCES files(file_id) ON DELETE CASCADE,
  FOREIGN KEY (containing_symbol_id) REFERENCES symbols(symbol_id) ON DELETE SET NULL
);
```

## Source Regions

### `source_regions`

Source spans for comments, doc comments, string literals, and embedded language
regions. These rows give downstream tools precise boundaries without storing
full source text or raw AST nodes.

```sql
CREATE TABLE source_regions (
  source_region_id TEXT PRIMARY KEY,
  file_id TEXT NOT NULL,
  path TEXT NOT NULL,
  language TEXT NOT NULL,
  kind TEXT NOT NULL,
  containing_symbol_id TEXT,
  start_line INTEGER NOT NULL,
  start_column INTEGER NOT NULL,
  end_line INTEGER NOT NULL,
  end_column INTEGER NOT NULL,
  start_byte INTEGER NOT NULL,
  end_byte INTEGER NOT NULL,
  metadata_json TEXT,
  FOREIGN KEY (file_id) REFERENCES files(file_id) ON DELETE CASCADE,
  FOREIGN KEY (containing_symbol_id) REFERENCES symbols(symbol_id) ON DELETE SET NULL
);
```

`kind` values are:

- `comment`
- `doc_comment`
- `string_literal`
- `embedded`

`metadata_json` is optional. Embedded regions may include
`embedded_language` and `host_node_kind`.

## Structural Facts

### `structural_facts`

Parser-backed structural facts that are useful to downstream tools but are not
symbols, identifiers, relationships, literals, or source-region spans.

Rows are pattern-based. `pattern_id` is stable and versioned, so consumers can
depend on the meaning of a row without understanding the tree-sitter grammar
directly. This repo emits extraction facts only; querying, ranking, dashboards,
and product workflows remain downstream.

```sql
CREATE TABLE structural_facts (
  structural_fact_id TEXT PRIMARY KEY,
  file_id TEXT NOT NULL,
  path TEXT NOT NULL,
  language TEXT NOT NULL,
  pattern_id TEXT NOT NULL,
  capture_name TEXT NOT NULL,
  node_kind TEXT NOT NULL,
  containing_symbol_id TEXT,
  start_line INTEGER NOT NULL,
  start_column INTEGER NOT NULL,
  end_line INTEGER NOT NULL,
  end_column INTEGER NOT NULL,
  start_byte INTEGER NOT NULL,
  end_byte INTEGER NOT NULL,
  confidence REAL NOT NULL,
  metadata_json TEXT,
  FOREIGN KEY (file_id) REFERENCES files(file_id) ON DELETE CASCADE,
  FOREIGN KEY (containing_symbol_id) REFERENCES symbols(symbol_id) ON DELETE SET NULL
);
```

`containing_symbol_id` binds each fact to the innermost byte-containing
scope-bearing symbol. `variable`, `constant`, `enum_member`, and `import`
symbols are value holders, not scopes, so they are never containment
candidates. When no byte-containing candidate exists (for example, a fact whose
span starts on an `export const` head that sits outside its value symbol), a
line-containment fallback selects the narrowest line-spanning candidate whose
byte span is not contained by the fact, with deterministic tie-breaks (narrowest
byte span, then earliest start byte). Module-scope facts with no enclosing
scope-bearing symbol are `NULL`.

Supported patterns are advertised in
`language_capabilities.kind_coverage_json` under
`kind_coverage.structural_facts.supported`.

| Pattern ID | Language | Capture | Node Kind(s) | Query Family | Meaning |
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
| `aspnet.attribute_route.v1` | `csharp` | `attribute_route` | `attribute` | `framework` | An attribute-routed ASP.NET controller class or action method (tree-sitter attribution of attribute -> owning declaration). One fact per routing attribute (`attribute_kind` is `controller_route`, `http_method`, or `route`). Non-literal templates, `[ApiController]` without routes, and conventional (non-attribute) routing stay silent. Metadata payload keys: see the JSON contract linked below. |
| `express.route.v1` | `javascript`, `jsx`, `typescript`, `tsx` | `route_call` | parser-covered call span | `framework` | A static Express route registration on an import-gated, in-file traced receiver. |
| `express.router_mount.v1` | `javascript`, `jsx`, `typescript`, `tsx` | `router_mount` | parser-covered call span | `framework` | A static Express `app.use`/`router.use` mount point. |
| `fastify.route.v1` | `javascript`, `jsx`, `typescript`, `tsx` | `route_call` | parser-covered call span | `framework` | A static Fastify shorthand or object-form route registration. |
| `nestjs.route.v1` | `javascript`, `typescript` | `route_decorator` | handler method declaration span | `framework` | A static NestJS HTTP-method decorator (`@Get`…`@All`) joined same-file to its `@Controller` class prefix. `verb` is upper-cased (omitted for `@All`); `class_route_template`/`effective_route_template` carry the joined class prefix; `normalized_route_template` is the `:param` join key. Requires a `@nestjs/common` import; only plain string-literal decorator arguments emit. Metadata payload keys: see the JSON contract linked below. |
| `fastapi.route.v1` | `python` | `route` | decorated function declaration span | `framework` | A FastAPI path-operation decorator on a traced FastAPI/APIRouter receiver. |
| `fastapi.include_router.v1` | `python` | `include_router` | parser-covered call span | `framework` | A FastAPI `include_router` mount call. |
| `flask.route.v1` | `python` | `route` | decorated function declaration span | `framework` | A Flask route decorator on a traced Flask/Blueprint receiver. |
| `flask.blueprint_registration.v1` | `python` | `blueprint_registration` | parser-covered call span | `framework` | A Flask `register_blueprint` mount call. |
| `django.url_pattern.v1` | `python` | `url_pattern` | parser-covered call span | `framework` | A Django `path` or `re_path` URL pattern. |
| `django.url_include.v1` | `python` | `url_include` | parser-covered call span | `framework` | A Django `include` mount inside a `path` URL pattern. |
| `spring.request_mapping.v1` | `java`, `kotlin` | `request_mapping` | class or method declaration line (Kotlin anchors the handler `function_declaration`) | `framework` | A Spring MVC request-mapping annotation on a class or method. Java and Kotlin share this pattern id (`api_style="annotation_routing"`); the Kotlin collector is AST-driven, import-gated on `org.springframework.web.bind.annotation`, resets the class `@RequestMapping` prefix per `class`/`object`/`companion object`, reads Kotlin bracket-array multi-paths, and keeps `$`-interpolated / concatenated / identifier route arguments silent (M2). |
| `go.net_http.route.v1` | `go` | `route_call` | parser-covered call span | `framework` | A Go `net/http` route registration through package-level or ServeMux calls. |
| `gin.route.v1` | `go` | `route_call` | parser-covered call span | `framework` | A gin route registration on a traced router or group receiver. |
| `echo.route.v1` | `go` | `route_call` | parser-covered call span | `framework` | An echo route registration on a traced Echo or group receiver. |
| `rails.route.v1` | `ruby` | `route` | parser-covered DSL call span | `framework` | A Rails routes DSL handler route. |
| `rails.resource_route.v1` | `ruby` | `resource_route` | parser-covered DSL call span | `framework` | A Rails `resources` or `resource` declaration. |
| `rails.mount.v1` | `ruby` | `mount` | parser-covered DSL call span | `framework` | A Rails `mount` route for a Rack app or engine. |
| `htmx.attribute.v1` | `html`, `razor`, `javascript`, `jsx`, `tsx`, `vue` | `attribute` | parser-covered attribute span | `frontend_interaction` | An `hx-*` or `data-hx-*` attribute, including request verb and static target path metadata when applicable. |
| `alpine.directive.v1` | `html`, `razor` | `directive` | parser-covered attribute span | `frontend_interaction` | An Alpine `x-*`, `@...`, or `:...` directive with normalized directive metadata. |
| `razor.page_directive.v1` | `razor` | `page_directive` | `razor_page_directive` | `component_routing` | A Razor `@page` directive with route-template metadata. |
| `razor.code_block.v1` | `razor` | `code_block` | `razor_block` | `component_code` | A Razor `@code` or `@functions` block. |
| `razor.template_expression.v1` | `razor` | `template_expression` | `razor_implicit_expression`, `razor_explicit_expression` | `component_template` | A Razor template expression such as `@name` or `@(expr)`. |
| `css.selector_rule.v1` | `css` | `rule_set` | `rule_set` | `stylesheet_structure` | A CSS selector rule set with selector kind and declaration-count metadata. |
| `css.custom_property.v1` | `css` | `custom_property` | `property_name` | `stylesheet_structure` | A CSS custom property declaration. |
| `css.media_query.v1` | `css` | `media_query` | `media_statement` | `responsive_design` | A CSS `@media` query. |
| `css.keyframes.v1` | `css` | `keyframes` | `keyframes_statement` | `animation` | A CSS `@keyframes` animation. |
| `html.link.v1` | `html` | `link` | `element` | `document_navigation` | An HTML anchor link with an `href` target. |
| `html.script.v1` | `html` | `script` | `script_element` | `document_assets` | An HTML script element with inline/external metadata. |
| `html.form.v1` | `html` | `form` | `element` | `document_forms` | An HTML form with action, method, and control-count metadata. |
| `html.form_control.v1` | `html` | `form_control` | `element` | `document_forms` | An HTML form control and its resolved owner-form metadata when available. |
| `vue.sfc_section.v1` | `vue` | `section` | `sfc_section` | `component_structure` | A Vue single-file component section (`template`, `script`, or `style`). |
| `vue.template_directive.v1` | `vue` | `directive` | `template_attribute` | `component_template` | A Vue template directive such as `v-bind`, `v-on`, `v-if`, or shorthand forms. |
| `vue.route_reference.v1` | `vue` | `route_reference` | `template_attribute` | `frontend_navigation` | A static Vue Router link target such as `<RouterLink to="/calendar">`. |
| `vue.route_definition.v1` | `javascript`, `jsx`, `typescript`, `tsx`, `vue` | `route_definition` | `object` | `frontend_navigation` | A static Vue Router route-table entry with a literal `path`, including `vue-router` JS/TS modules. |
| `nuxt.route_reference.v1` | `vue` | `route_reference` | `template_attribute` | `frontend_navigation` | A static Nuxt `NuxtLink` or `nuxt-link` target with a literal `to` path. |
| `nuxt.file_route.v1` | `javascript`, `jsx`, `typescript`, `tsx`, `vue` | `file_route` | `file` | `frontend_navigation` | A Nuxt `app/pages/**` or `pages/**` page route derived from the file path. |
| `react.route_reference.v1` | `javascript`, `jsx`, `tsx` | `route_reference` | `jsx_attribute` | `frontend_navigation` | A static React Router `Link` or `NavLink` target imported from React Router. |
| `react.route_definition.v1` | `javascript`, `jsx`, `typescript`, `tsx` | `route_definition` | `object`, `jsx_element` | `frontend_navigation` | A static React Router route object or `<Route>` element with a literal `path` or `index`. |
| `nextjs.route_reference.v1` | `javascript`, `jsx`, `tsx` | `route_reference` | `jsx_attribute` | `frontend_navigation` | A static `next/link` target from a string `href` or object `pathname`. |
| `nextjs.file_route.v1` | `javascript`, `jsx`, `typescript`, `tsx` | `file_route` | `file` | `frontend_navigation` | A Next.js App Router or Pages Router page route derived from the file path. |
| `nextjs.route_handler.v1` | `javascript`, `typescript` | `route_handler` | `export_statement` | `framework` | An exported HTTP-verb handler (`GET`/`POST`/`PUT`/`PATCH`/`DELETE`/`HEAD`/`OPTIONS`) in an App Router `route.{js,ts}` file. One fact per exported verb. Route paths are derived with the same segment walk as `nextjs.file_route.v1`. Metadata payload keys: see the JSON contract linked below. |
| `nuxt.server_route.v1` | `javascript`, `typescript` | `server_route` | `file` | `framework` | A Nitro server route under `server/api/**` (route prefixed `/api`) or `server/routes/**` (no prefix). One fact per file; `verb`/`verb_source` are present only when the filename carries a method suffix (`users.get.ts`). Emission requires a `defineEventHandler`/`eventHandler` identifier or a method suffix; a wrapped custom handler with neither is a documented residual miss. `server/middleware`, `server/plugins`, and `server/utils` are excluded. Claims the `server/**` space `nuxt.file_route.v1` excludes. Metadata payload keys: see the JSON contract linked below. |
| `http.client_request.v1` | `javascript`, `jsx`, `typescript`, `tsx`, `vue`, `python`, `csharp`, `go`, `java`, `kotlin`, `ruby` | `client_request` | parser-covered call span (Java builder chains anchor the enclosing statement) | `web.http_client` | A supported outbound HTTP client call whose URL argument is a static string literal. Kotlin covers the Ktor client (`client="ktor"`, import-gated on `io.ktor.client`, `receiver.verb(...)` calls only). Metadata payload keys: see the JSON contract linked below. |

ASP.NET route facts emit `normalized_route_template` as the server-side
cross-family join key. Raw `route_template`, `route_prefix`, and
`effective_route_template` values remain source-shaped ASP.NET strings; the
normalized key converts route parameters such as `{id}` or `{id:int}` to `:id`
and preserves trailing slashes.

`fetch()` and axios calls emit `http.client_request.v1` only when the first
argument is a plain static string literal (`'...'` or `"..."`). Template
literals (even without interpolation), identifier/expression URLs, concatenated
URLs, property calls of the bare client name (`obj.fetch(...)`), and matches
inside comments or string literals stay silent. When a `method:` property is
present but its value is not a static string literal, the whole call emits
nothing rather than silently degrading to `GET`. `fetch` is a global, so no
import is required. Axios calls are import-gated on a default or namespace
axios import and matched on the LOCAL binding (`import http from "axios"`
gates `http.*`). In Vue SFCs the scan covers `<script>`/`<script setup>`
section content only, and the axios import gate is local to the declaring
script section.

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
the next. Go `net/http` patterns follow Go 1.22 `[METHOD ][HOST]/[PATH]`
parsing: `route_template` carries the path part, `verb` the method token, and
`host` the host part when present. gin/echo routes emit
`api_style="call_routing"` (`mux_routing` is reserved for `go.net_http.route.v1`);
nested `Group` calls compose literal prefixes, and a non-literal prefix poisons
the chain so its routes emit `route_template` only. The echo import gate
accepts any major version of `github.com/labstack/echo`. Rails DSL facts
require `config/routes.rb` routes to sit inside a `routes.draw do ... end`
block; split files under `config/routes/` allow top-level DSL. Every
`do ... end` block is depth-tracked, so `member`/`collection`/`constraints`
blocks do not pop enclosing `namespace`/`scope` prefixes early.

Backend-language client collectors emit the same `http.client_request.v1`
metadata shape for static string URL arguments: Python module-qualified
`requests`/`httpx` calls, C# `HttpClient` method calls and
`HttpRequestMessage`, Go `net/http` package calls, Java `HttpRequest` builder
chains, and Ruby `Net::HTTP` calls with literal `URI(...)`/`URI.parse(...)`
arguments. Java builder-chain facts span the enclosing statement (the URL and
verb are resolved statement-locally), so their `node_kind` is the statement
node rather than a call node. Instance/session clients and dynamic URL
expressions stay silent.

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
`data_prefix=true` in metadata JSON. Beyond `html`/`razor`, the family also
covers JSX/TSX component markup (`javascript`, `jsx`, `tsx`) and Vue
single-file-component `<template>` sections (`vue`). On these component surfaces
only static string attribute values emit: JSX brace-expression values
(`hx-post={url}`) and Vue dynamic bindings (`:hx-post`, `v-bind:hx-post`) stay
silent, and Vue scanning is restricted to `<template>` so htmx attributes inside
`<script>` strings are not reported. `typescript` is intentionally excluded
because the plain TypeScript grammar cannot parse JSX.

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

### Structural-fact metadata payload

The full per-pattern metadata payload — every key each `pattern_id` can carry,
with its JSON value type and presence rule — is published as a machine-readable
contract at
[`structural-fact-patterns.json`](./structural-fact-patterns.json). That file is
generated from the in-process pattern registry
(`crates/julie-extractors/src/base/structural_fact_registry.rs`); treat it as the
source of truth for structural-fact metadata payloads. Regenerate the checked-in
file after an intentional registry change with:

```
UPDATE_CONTRACT_JSON=1 cargo test -p julie-extractors structural_fact_registry
```

Every fact carries the base keys `pattern_version` (integer, currently `1`) and
`query_family` (string, matching the table above); framework and web
route/http facts additionally carry a `framework` key. The `route_path` vs
`target_path` and `verb` naming policy above is stable across the payload
contract and stays documented here as prose, not in the JSON.

## Complexity Metrics

### `complexity_metrics`

Versioned parser-backed metrics for file and symbol scopes. Rows are primitive
facts, not an extractor-owned quality score. Downstream tools own ranking,
thresholds, dashboards, and risk labels.

```sql
CREATE TABLE complexity_metrics (
  complexity_metric_id TEXT PRIMARY KEY,
  file_id TEXT NOT NULL,
  path TEXT NOT NULL,
  language TEXT NOT NULL,
  scope TEXT NOT NULL,
  symbol_id TEXT,
  algorithm_id TEXT NOT NULL,
  covered_lines INTEGER NOT NULL,
  covered_bytes INTEGER NOT NULL,
  decision_count INTEGER NOT NULL,
  loop_count INTEGER NOT NULL,
  max_nesting_depth INTEGER NOT NULL,
  parameter_count INTEGER,
  start_line INTEGER NOT NULL,
  start_column INTEGER NOT NULL,
  end_line INTEGER NOT NULL,
  end_column INTEGER NOT NULL,
  start_byte INTEGER NOT NULL,
  end_byte INTEGER NOT NULL,
  metadata_json TEXT,
  FOREIGN KEY (file_id) REFERENCES files(file_id) ON DELETE CASCADE,
  FOREIGN KEY (symbol_id) REFERENCES symbols(symbol_id) ON DELETE SET NULL
);
```

`scope` values are `file` and `symbol`. File-scope rows use
`symbol_id = NULL`; symbol-scope rows link to `symbols.symbol_id` when the
symbol is still present.

The initial algorithm id is `julie-ast-complexity-v1`. It counts parser node
kinds for decisions, loops, and maximum decision/loop nesting depth, records
covered lines/bytes, and emits `parameter_count` only when the language parser
shape is clear for callable symbols.

Supported scopes are advertised in `language_capabilities.kind_coverage_json`
under `kind_coverage.complexity_metrics.supported`.

## Diagnostics

### `parse_diagnostics`

Tree-sitter parse errors and missing-node diagnostics normalized into stable
artifact rows.

```sql
CREATE TABLE parse_diagnostics (
  diagnostic_id TEXT PRIMARY KEY,
  file_id TEXT NOT NULL,
  path TEXT NOT NULL,
  language TEXT NOT NULL,
  kind TEXT NOT NULL,
  message TEXT,
  start_line INTEGER NOT NULL,
  start_column INTEGER NOT NULL,
  end_line INTEGER NOT NULL,
  end_column INTEGER NOT NULL,
  start_byte INTEGER NOT NULL,
  end_byte INTEGER NOT NULL,
  metadata_json TEXT,
  FOREIGN KEY (file_id) REFERENCES files(file_id) ON DELETE CASCADE
);
```

`kind` values are `error` and `missing`.

## Language Capabilities

### `language_capabilities`

One row per language in the capability snapshot.

```sql
CREATE TABLE language_capabilities (
  language TEXT PRIMARY KEY,
  parser_package TEXT NOT NULL,
  extensions_json TEXT NOT NULL,
  dependency_status TEXT NOT NULL,
  target_symbols INTEGER NOT NULL,
  target_relationships INTEGER NOT NULL,
  target_pending_relationships INTEGER NOT NULL,
  target_identifiers INTEGER NOT NULL,
  target_types INTEGER NOT NULL,
  actual_symbols INTEGER NOT NULL,
  actual_relationships INTEGER NOT NULL,
  actual_pending_relationships INTEGER NOT NULL,
  actual_identifiers INTEGER NOT NULL,
  actual_types INTEGER NOT NULL,
  kind_coverage_json TEXT NOT NULL
);
```

### `language_capability_fixtures`

Fixture evidence rows referenced by a capability snapshot.

```sql
CREATE TABLE language_capability_fixtures (
  language TEXT NOT NULL,
  fixture_name TEXT NOT NULL,
  source_path TEXT NOT NULL,
  expected_path TEXT NOT NULL,
  PRIMARY KEY (language, fixture_name),
  FOREIGN KEY (language) REFERENCES language_capabilities(language) ON DELETE CASCADE
);
```

### `language_capability_gaps`

Declared gaps with typed evidence.

```sql
CREATE TABLE language_capability_gaps (
  gap_id TEXT PRIMARY KEY,
  language TEXT NOT NULL,
  capability TEXT NOT NULL,
  status TEXT NOT NULL,
  reason TEXT NOT NULL,
  required_closure TEXT NOT NULL,
  evidence_json TEXT NOT NULL,
  FOREIGN KEY (language) REFERENCES language_capabilities(language) ON DELETE CASCADE
);
```

## Performance Contract

The writer must optimize for extraction throughput and predictable downstream
queries.

Required writer behavior:

- Use one explicit SQLite transaction per committed `scan`, `update`, `delete`,
  or `scan --force` operation.
- Use prepared statements for repeated inserts, updates, and deletes.
- Replace one file by deleting existing rows through `file_id` or indexed
  `path`, then inserting the new normalized rows in batches.
- Avoid per-row commits and per-row schema or metadata reads.
- Compute hashes before extraction writes so unchanged files skip row churn.
- Run the data-loss guard before deleting known-good parser-backed rows.
- Leave the artifact with all required indexes present before reporting success.

Permitted implementation optimizations:

- `scan --force` may write into a new database file and atomically replace the
  old artifact after the transaction succeeds.
- Secondary indexes may be created after a bulk load when that is faster, as
  long as readers never observe a successful artifact without required indexes.
- Temporary or staging tables may be used inside a transaction. They are not
  part of the public schema.

SQLite mode requirements:

- Writers should use WAL mode for normal incremental operation.
- Readers must tolerate WAL sidecar files.
- Lower-durability settings for benchmarks are not part of the v3 product
  contract.

## Required Indexes

```sql
CREATE INDEX idx_files_path ON files(path);
CREATE INDEX idx_files_language ON files(language);
CREATE INDEX idx_symbols_path ON symbols(path);
CREATE INDEX idx_symbols_file ON symbols(file_id);
CREATE INDEX idx_symbols_name_kind ON symbols(name, kind);
CREATE INDEX idx_symbols_parent ON symbols(parent_symbol_id);
CREATE INDEX idx_symbols_is_test ON symbols(is_test);
CREATE INDEX idx_symbols_test_container ON symbols(test_container);
CREATE INDEX idx_symbols_test_lifecycle ON symbols(test_lifecycle);
CREATE INDEX idx_identifiers_path ON identifiers(path);
CREATE INDEX idx_identifiers_file ON identifiers(file_id);
CREATE INDEX idx_identifiers_name_kind ON identifiers(name, kind);
CREATE INDEX idx_identifiers_containing ON identifiers(containing_symbol_id);
CREATE INDEX idx_identifiers_target ON identifiers(target_symbol_id);
CREATE INDEX idx_relationships_from ON relationships(from_symbol_id);
CREATE INDEX idx_relationships_to ON relationships(to_symbol_id);
CREATE INDEX idx_relationships_kind ON relationships(kind);
CREATE INDEX idx_relationships_file ON relationships(file_id);
CREATE INDEX idx_pending_terminal ON pending_relationships(target_terminal_name);
CREATE INDEX idx_pending_file ON pending_relationships(file_id);
CREATE INDEX idx_pending_from ON pending_relationships(from_symbol_id);
CREATE INDEX idx_pending_caller_scope ON pending_relationships(caller_scope_symbol_id);
CREATE INDEX idx_type_facts_symbol ON type_facts(symbol_id);
CREATE INDEX idx_symbol_annotations_symbol ON symbol_annotations(symbol_id);
CREATE INDEX idx_type_argument_usages_identifier ON type_argument_usages(identifier_id);
CREATE INDEX idx_type_argument_usages_file ON type_argument_usages(file_id);
CREATE INDEX idx_type_arguments_usage ON type_arguments(usage_id);
CREATE INDEX idx_type_arguments_parent ON type_arguments(parent_type_argument_id);
CREATE INDEX idx_literals_file ON literals(file_id);
CREATE INDEX idx_source_regions_file_span ON source_regions(file_id, start_byte, end_byte);
CREATE INDEX idx_source_regions_kind_file ON source_regions(kind, file_id, start_byte);
CREATE INDEX idx_source_regions_symbol ON source_regions(containing_symbol_id);
CREATE INDEX idx_structural_facts_file_span ON structural_facts(file_id, start_byte, end_byte);
CREATE INDEX idx_structural_facts_pattern_language_path ON structural_facts(pattern_id, language, path);
CREATE INDEX idx_structural_facts_symbol ON structural_facts(containing_symbol_id);
CREATE INDEX idx_complexity_metrics_file_scope ON complexity_metrics(file_id, scope, start_byte);
CREATE INDEX idx_complexity_metrics_scope_language ON complexity_metrics(scope, language, path);
CREATE INDEX idx_complexity_metrics_symbol ON complexity_metrics(symbol_id);
CREATE INDEX idx_diagnostics_path ON parse_diagnostics(path);
CREATE INDEX idx_diagnostics_file ON parse_diagnostics(file_id);
```

These indexes protect the v3 access patterns. Implementations may add more
indexes, but removing one requires a schema-versioned contract change.

## Performance Budgets

Exact timing budgets belong in tests and release gates, not in this prose
contract. The first implementation must still provide measurable gates for:

- tiny-fixture writer throughput in the default or contract tier
- query-plan checks for required indexes in the contract tier
- real-world scan throughput in the real-world or release tier

## Deliberate Exclusions

- No search index tables.
- No embedding tables.
- No MCP, daemon, watcher, or workspace registry tables.
- No Julie analysis tables for reference scoring, test linkage, or test quality.
- No old Julie schema compatibility tables as a v3 requirement.

## Tradeoffs

- **Stable opaque IDs:** downstream readers get durable references without
  depending on old fixture key or MD5 mechanics.
- **Structured pending only:** v3 stores the richer unresolved target shape and
  does not expose Julie's legacy flat pending queue as a separate contract.
- **Capability rows in SQLite:** consumers can validate language evidence from
  the artifact without also reading repository fixtures.
- **Indexes are required, not advisory:** write cost is acceptable because this
  product is an artifact producer for downstream tools that need predictable
  lookup performance.
- **Test role flags are first-class:** extractor metadata is also exposed as
  indexed SQLite booleans because downstream test filtering should not depend on
  JSON expression scans.
- **Source regions are spans, not search:** v3 exposes AST-bounded source
  ranges for downstream products, but it does not create lexical indexes,
  vector indexes, or store complete source text.
- **Open decision before implementation:** exact parser version fields depend on
  what each parser package exposes. The required contract is a parser inventory
  table plus fingerprint; missing package-level versions must be represented as
  null, not guessed.
