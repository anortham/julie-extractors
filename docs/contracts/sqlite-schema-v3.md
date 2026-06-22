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
| `htmx.attribute.v1` | `html`, `razor` | `attribute` | parser-covered attribute span | `frontend_interaction` | An `hx-*` attribute, including request verb and static target path metadata when applicable. |
| `alpine.directive.v1` | `html`, `razor` | `directive` | parser-covered attribute span | `frontend_interaction` | An Alpine `x-*`, `@...`, or `:...` directive with normalized directive metadata. |

Metadata:

- `pattern_version`: integer, currently `1`.
- `query_family`: string matching the table above.
- Framework facts may also include framework-specific keys:
  `framework`, `api_style`, `verb`, `route_template`, `route_source`,
  `handler_kind`, `handler_name`, `attribute_name`, `attribute_value`,
  `target_path`, `directive`, `argument`, `modifiers`, `expression`, and
  `shorthand`.

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
