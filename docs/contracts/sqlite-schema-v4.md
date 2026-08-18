# SQLite Schema v4

> **Historical.** Superseded by later schema versions. The resolution overlay
> this page describes is retired. See [sqlite-schema-v7.md](sqlite-schema-v7.md)
> and [2026-08-18-resolution-write-path-retirement.md](../decisions/2026-08-18-resolution-write-path-retirement.md).

## Scope

SQLite is the primary durable artifact for `julie-extractors`.

This document defines the v4 logical schema. Implementations may add
indexes, views, and internal helper tables, but downstream readers should rely
only on the tables and columns named here.

v4 adds the **reference-resolution overlay**: two new tables
(`pending_resolutions`, `identifier_resolutions`), a resolver-maintained
`identifiers.target_symbol_id`, and three durable `reference_resolution_*`
metadata keys. The change is purely additive over v3 — every v3 table and column
is unchanged. See [Reference Resolution](#reference-resolution) for the state
model, tiers, outcome vocabulary, and the consumer detection rule. A binary that
opens a v3 artifact creates the overlay tables on the additive `create_schema`
open. A whole-workspace scan backfills the current extraction and resolution
evidence; single-file writes remain blocked until that scan succeeds. See
[`--strict-schema` read preflight](#--strict-schema-read-preflight).

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
- Reference resolution is a **derived overlay**, never a durable extraction fact.
  Pending rows and identifiers are the durable facts; the `pending_resolutions`
  and `identifier_resolutions` tables plus the denormalized
  `identifiers.target_symbol_id` are recomputed by the resolver and kept coherent
  only through the artifact storage primitives (resolve writes the overlay row and
  the denormalized column together; demote clears both). No consumer writes any
  resolution surface directly, and no resolution provenance is ever stored in
  `identifiers.metadata_json`.

## Metadata

### `artifact_metadata`

Key-value metadata for the whole artifact.

Required keys:

- `artifact_id`: generated stable identifier for this artifact.
- `root_path`: canonical source root.
- `schema_version`: `4`.
- `extract_contract_version`: `3`. (Unchanged from v3 — the extraction contract
  did not change; only the additive resolution overlay did.)
- `sqlite_schema_version`: `4`.
- `binary_version`: `julie-extract` version that last wrote the artifact.
- `hash_algorithm`: content hash algorithm name.
- `parser_inventory_fingerprint`: fingerprint of parser package inventory.
- `capability_snapshot_fingerprint`: fingerprint of language capabilities.
- `created_at`: artifact creation timestamp.
- `updated_at`: last successful mutation timestamp.

Reference-resolution keys (schema v4). These are the **only** signal a consumer
may use to decide whether resolution data is present and trustworthy — see
[Consumer detection rule](#consumer-detection-rule):

- `reference_resolution_status`: one of `complete`, `partial`, `failed`, or
  **absent** (the key is missing). Semantics:
  - `complete` — a full pass resolved every applicable reference with no gated
    languages processed this pass.
  - `partial` — resolution ran and committed, but coverage is incomplete: a delta
    pass ran, or a processed language had a gated tier (e.g. import-guided tier 2
    is disabled outside TypeScript/JavaScript). This is the normal steady state.
  - `failed` — the resolver hook errored; the scan still committed its extraction
    rows, but the overlay for the affected rows is stale. The prior
    `reference_resolution_last_full_revision` is preserved. A failed resolver
    hook during `scan`, `update`, or `delete` blocks later single-file mutations
    until a successful whole-workspace scan restores a ready status.
  - **absent** (key not present) — the artifact predates resolution (a v3 artifact
    opened but not yet re-written) or was written before the overlay was
    populated. Consumers must treat absent as "no resolution data", not "no
    references resolved".
- `reference_resolution_version`: the resolution contract version, currently `6`.
- `reference_resolution_last_full_revision`: the `extraction_revisions.revision_id`
  of the last full resolution pass. Preserved across a `failed` write.

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

`target_symbol_id` is a **resolver-maintained denormalized convenience** (schema
v4). Extraction leaves it `NULL`; the reference-resolution pass fills it when an
identifier resolves and clears it on demotion. It is FK-consistent for target
death (`ON DELETE SET NULL`), and the resolver keeps it coherent with
`identifier_resolutions` through the storage primitives. The authoritative
resolution provenance (tier, confidence, method, outcome, candidate count) lives
in [`identifier_resolutions`](#identifier_resolutions); the column is a fast
lookup, not the full record. A `NULL` here means "not resolved" — which the
[outcome vocabulary](#outcome-vocabulary) distinguishes as `ambiguous`,
`missing`, `no_context`, or not-yet-attempted.

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

## Reference Resolution

New in schema v4. Workspace reference resolution fills
`identifiers.target_symbol_id` and records resolved pending relationships through
a deterministic, tiered pass that runs **inside the same writer transaction** as
the scan's row writes. It is a derived overlay: a resolver error never fails or
rolls back a scan — the affected rows simply stay unresolved and the scan report
records a `resolution_failed` diagnostic.

### State model

Pending rows and identifiers are durable facts; resolution is a derived overlay
with FK semantics that make invalidation automatic:

- A pending relationship is **resolved iff it has a `pending_resolutions` row.**
  If the target symbol dies (file rewrite, delete, move — all mint new
  `symbol_id`s), `ON DELETE CASCADE` removes the resolution and the pending row
  reverts to unresolved **with its full context intact**. If the source file is
  rewritten, the pending row itself cascades away and re-extraction re-emits it.
- An identifier's resolution lives in `identifier_resolutions`. Resolved rows
  carry a target and cascade away when it dies (the identifier reverts to
  never-attempted and re-enters the worklist); `ambiguous`/`missing`/`no_context`
  rows carry a `NULL` target and are refreshed by re-resolution.
- `relationships` rows are **not** used to store workspace resolutions — their
  cascade-on-target plus provenance-in-metadata would silently destroy unresolved
  context on file rewrites. Consumers that want a unified edge view read
  `relationships` UNION the `pending_resolutions ⋈ pending_relationships` join.

Invalidation is FK-first (deleted/moved/edited targets are handled entirely by
`CASCADE` / `SET NULL`), name-matched second (after each scan the resolver re-runs
the tier chain for every resolved row whose terminal or receiver name matches a
symbol name inserted or deleted in the files touched by this scan, demoting any
row that no longer yields exactly one candidate).

### `pending_resolutions`

Resolution overlay for pending relationships. One row per resolved pending
relationship; unresolved pendings have no row.

```sql
CREATE TABLE pending_resolutions (
  pending_relationship_id TEXT PRIMARY KEY
    REFERENCES pending_relationships(pending_relationship_id) ON DELETE CASCADE,
  target_symbol_id TEXT NOT NULL
    REFERENCES symbols(symbol_id) ON DELETE CASCADE,
  tier INTEGER NOT NULL,
  confidence REAL NOT NULL,
  method TEXT NOT NULL,
  resolved_at_revision INTEGER NOT NULL
);
```

- `pending_relationship_id`: primary key and FK to the pending row it resolves.
  `ON DELETE CASCADE` — the resolution disappears if the pending row is
  re-extracted.
- `target_symbol_id`: the resolved symbol. `NOT NULL` (this table records
  successes only). `ON DELETE CASCADE` — target death reverts the pending row to
  unresolved.
- `tier`: the resolution tier (1–4) that fired. See [Tiers](#tiers).
- `confidence`: the tier's confidence value (see the tier table).
- `method`: the resolution method string (`tier1_local`, `tier2_import`,
  `tier3_receiver`, `tier3_static_type`, `tier4_global`). `tier3_receiver` and
  `tier3_static_type` both stamp `tier = 3`; they differ in whether a `type_facts`
  row backed the binding.
- `resolved_at_revision`: the `extraction_revisions.revision_id` at which this
  resolution was written.

### `identifier_resolutions`

Resolution overlay for identifiers. Unlike `pending_resolutions`, this table
records **every** attempted outcome, not just successes, so consumers can
distinguish "resolved elsewhere" from "tried and ambiguous/missing".

```sql
CREATE TABLE identifier_resolutions (
  identifier_id TEXT PRIMARY KEY
    REFERENCES identifiers(identifier_id) ON DELETE CASCADE,
  target_symbol_id TEXT
    REFERENCES symbols(symbol_id) ON DELETE CASCADE,
  tier INTEGER,
  confidence REAL,
  method TEXT,
  outcome TEXT NOT NULL,
  candidates INTEGER,
  resolved_at_revision INTEGER NOT NULL,
  CHECK ((outcome = 'resolved') = (target_symbol_id IS NOT NULL))
);
```

- `identifier_id`: primary key and FK to the identifier. `ON DELETE CASCADE`.
- `target_symbol_id`: the resolved symbol, or `NULL` for non-resolved outcomes.
  `ON DELETE CASCADE` — target death drops the row and the identifier re-enters
  the worklist.
- `tier` / `confidence` / `method`: populated when `outcome = 'resolved'`;
  otherwise `NULL`.
- `outcome`: one of `resolved`, `ambiguous`, `missing`, `no_context` — see
  [Outcome vocabulary](#outcome-vocabulary).
- `candidates`: number of candidate symbols the chain considered (useful for
  diagnosing `ambiguous`).
- `resolved_at_revision`: the revision at which this outcome was recorded.
- **`CHECK ((outcome = 'resolved') = (target_symbol_id IS NOT NULL))`** enforces
  outcome/target coherence: a `resolved` row must carry a target, and any
  non-`resolved` row must not.

The denormalized `identifiers.target_symbol_id` mirrors this table's
`target_symbol_id` for resolved rows and is maintained in the same statement
batch. No resolution provenance is written to `identifiers.metadata_json`.

### Tiers

One candidate-matching core drives all tiers. Each tier is an independent filter
over kind-compatible, **same-language** candidates; the edge resolves at the
first tier (in order) whose candidate set is exactly one. If no tier yields
exactly one, the outcome is `ambiguous` when any tier yielded ≥2 candidates and
`missing` when all yielded 0. There is **no best-guess selection** — a wrong edge
is worse than a missing one.

| Tier | Signal | Confidence | Method | Restrictions |
|---|---|---|---|---|
| 1 | Same-file scope (local index result materialized at extraction time) | `0.95` | `tier1_local` | Same language. |
| 2 | Import-guided: candidate reachable through an import symbol in the source file whose name/alias matches the terminal name | `0.85` | `tier2_import` | Same language. **Language-gated:** enabled only where a fixture-tested import contract exists — currently **TypeScript and JavaScript**. Every other language records a `reference_resolution.tier2_import` gap until F4 normalizes import facts. |
| 3 | Receiver-typed: receiver name → scoped symbol → its `type_facts.resolved_type` → type symbol → member with the terminal name | `0.75` (`0.65` when the contributing type fact is inferred) | `tier3_receiver` | Same language. Coverage is bounded by per-language `type_facts` emission; every language records a `reference_resolution.tier3_receiver` gap (broadened by F2). |
| 3 | Static-type receiver: the receiver names a type directly → that type's member with the terminal name. No `type_facts` row participates | `0.70` | `tier3_static_type` | Same language. Refuses a receiver nested inside another type; refuses a non-public type outside its declaring file — a file-scoped homonym of an external type must not answer for references elsewhere; refuses a member that is not statically reachable, meaning `symbols.metadata_json.isStatic` is false or absent and its `signature` lacks `static` as a standalone word (enum members and constants are exempt), because `Type.InstanceMethod()` does not compile; refuses a receiver whose written qualification is not a suffix of the candidate type's declared namespace, so `External.Fixture.Create()` never answers for a workspace `App.Core.Fixture`; and refuses a receiver shadowed by a parameter of an enclosing callable. A namespace or module parent does not count as nesting. |
| 4 | Unique-language-global: exactly one kind-compatible candidate in the same language workspace-wide | `0.55` | `tier4_global` | Same language. Enabled for `type_usage`, `instantiates`, `uses`, `extends`, `implements`, and `calls` to Function/Constructor kinds. **Disabled for `member_access` and method calls** — member names collide too heavily for global uniqueness to be meaningful. |

`tier3_static_type` carries its own `method` string precisely because no type
fact backs it: reporting it as `tier3_receiver` would attribute the binding to
type-fact evidence that does not exist. Both stamp `tier = 3`.

The tier runs for every language, but it can only produce an edge where the
extractor emits type `visibility` and static reachability (`isStatic` metadata
when present, else a standalone `static` member modifier in the signature).
Languages without a fixture-proven contract for both record a
`reference_resolution.tier3_static_type` gap — currently every language except
C#, TypeScript, and JavaScript (`TIER3_STATIC_TYPE_LANGUAGES`).

For TypeScript and JavaScript modules, a cross-file static-type edge also
requires import corroboration: the receiver local name must be imported from
the type's defining file (`module_file_id` match). Same-file receivers still
bind without an import. Module languages only accept runtime value receivers
(`class` / `enum`), not erased interfaces or type aliases.

Kind compatibility: `calls` targets Function/Method/Constructor; `instantiates`
targets Class/Struct/Constructor; `uses`/type edges and identifier `type_usage`
target type-like kinds; identifier `member_access` targets
Property/Field/Method/Constant/EnumMember (tiers 1–3 only) — `Constant` and
`EnumMember` carry static member access such as `SomeEnum.Value` and
`Limits.Max`. Method overloads (same name, same kind) yield >1 candidate and
stay `ambiguous`. Partial classes (multiple same-name class symbols) likewise
stay `ambiguous` at tier 4 and resolve only via tiers 1–3 — coverage loss, never
wrong edges.

Resolution contract 2 also runs tier 1 directly for identifier `variable_ref`
rows. It walks the containing scope outward, then the same file's top-level
values, and never promotes a workspace-global same-name value. Identifier
`member_access` reads receiver/import context from `identifiers.metadata_json`,
runs receiver tier 3 when that context is present, and remains excluded from
tier 4.

All resolved overlays cap confidence at the lower of source extraction
confidence and tier confidence. Direct relationship evidence is tier `1` with
method `extraction_direct`; identifier propagation from that evidence uses
method `tier1_local`.

### Outcome vocabulary

`identifier_resolutions.outcome` is one of:

| Outcome | Meaning | Target |
|---|---|---|
| `resolved` | Exactly one candidate at some tier. | Non-`NULL` |
| `ambiguous` | Some tier yielded ≥2 candidates and none yielded exactly one. | `NULL` |
| `missing` | Every applicable tier yielded 0 candidates. | `NULL` |
| `no_context` | No tier was applicable: identifier `member_access` (no receiver context on identifiers today; F1 adds it), and any identifier kind outside the resolver's supported set, which receives a blanket `no_context` row so it leaves the never-attempted worklist. Consumers counting "attempted" outcomes should expect unsupported kinds here. | `NULL` |

Which surface records which outcome:

- **`pending_resolutions`** records **resolved pendings only.** An unresolved
  pending relationship simply has no row; its unresolved context stays in
  `pending_relationships`.
- **`identifier_resolutions`** records **all four outcomes** for every attempted
  identifier, so `resolved` (elsewhere) is distinguishable from `ambiguous` /
  `missing` / `no_context`.

### Consumer detection rule

**Consumers gate resolution availability on the `reference_resolution_status`
metadata key — never on the schema version, and never by probing for the overlay
tables.** An artifact can be schema v4 (the overlay tables exist via the additive
`create_schema`) while resolution is still `absent` until the first write
backfills it, or `failed` if a resolver error left the overlay stale. Schema
version answers "can this binary read the artifact"; it does **not** answer "is
resolution data present and trustworthy". Only the status key does:

- `absent` (key missing) or `failed` → treat resolution data as not present /
  stale; fall back to unresolved behavior (a `NULL` `target_symbol_id` means
  "unknown", exactly as before v4).
- `partial` → resolution is present but incomplete (normal steady state); use it.
- `complete` → resolution is present and fully applied for this pass.

Read `reference_resolution_version` to confirm the contract version and
`reference_resolution_last_full_revision` to detect staleness against the current
`extraction_revisions` head.

A 2.17-or-newer whole-workspace scan detects a missing or stale resolution
version and re-extracts every supported file before stamping the current
version. Single-file `update` and `delete` return
`schema_migration_required` until that scan completes. The version value records
the resolution pass and is not proof of upgraded row content if metadata was
rewritten outside `julie-extract`. An incomplete extraction or failed resolution
pass records `failed`, returns a non-zero scan result, and keeps single-file
mutations blocked. This also applies when a routine `update` or `delete`
records `failed` after its resolver hook errors; recovery is
`julie-extract scan`.

One caution for dead-code-style consumers that suppress verdicts by unresolved
same-name references: an unresolved reference recorded under a local alias
(e.g. TypeScript `import { X as Y }` used as `Y(...)`) carries
`identifiers.name = "Y"`, so name-based suppression alone will not shield the
aliased-to symbol `X`. Suppression logic should also consult import symbol rows
and `pending_relationships.target_display_name` where alias information
survives.

### `--strict-schema` read preflight

Read commands accept `--strict-schema`. Under it, an artifact whose
`sqlite_schema_version` or `schema_version` does not exactly equal the binary's
supported version (`4`) is **rejected** at preflight with a
`schema_migration_required` report code (exit `3`). A v3 artifact therefore does
**not** transparently upgrade on the read path — resolution backfill happens only
on the **whole-workspace scan** path: `scan` or `scan --force` creates the
overlay tables via the additive `create_schema`, re-extracts every supported
file when the resolution version is missing or stale, and runs a full resolution
pass. Single-file `update` and `delete` return
`schema_migration_required` until that scan completes. Without
`--strict-schema`, reads of an older-but-not-newer artifact are permitted (a
newer-than-supported artifact is always rejected, strict or not).

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
| `laravel.route.v1` | `php` | `route` | parser-covered `Route` facade call span | `framework` | A static Laravel `Route` facade route (`api_style="call_routing"`). Import-gated on `Route::`; AST-driven. `Route::get/post/put/patch/delete/options` set an upper-cased `verb`; `Route::any` omits the verb; `Route::match([verbs], ...)` emits one fact per static verb. `{param}`/`{param?}` normalize to `:param`. Same-file `Route::prefix`/`group` prefixes join into `route_group_prefix`/`effective_route_template`; a non-literal prefix or path stays silent (M2). `controller_action` is captured from a `[Ctrl::class, 'm']` or `'Ctrl@m'` handler when statically resolvable. Metadata payload keys: see the JSON contract linked below. |
| `laravel.resource_route.v1` | `php` | `resource_route` | parser-covered `Route::resource`/`apiResource` call span | `framework` | A Laravel `Route::resource` (`resource_kind="resource"`) or `Route::apiResource` (`resource_kind="api_resource"`) declaration with a static URI literal and, when present, the controller class. |
| `laravel.route_prefix.v1` | `php` | `route_prefix` | parser-covered `Route::prefix`/`group` prefix site | `framework` | A static same-file Laravel group prefix (`Route::prefix('x')->group(...)` or `Route::group(['prefix'=>'x'], ...)`) emitted at its own site with `mount_path` (raw literal) and `normalized_mount_path` (including enclosing group scope). Cross-file `RouteServiceProvider` prefixes are out of scope. |
| `symfony.route.v1` | `php` | `request_mapping` | class or method declaration carrying a static `#[Route]` attribute | `framework` | A Symfony `#[Route]` attribute route (`api_style="annotation_routing"`). Import-gated on `Symfony\Component\Routing\`. Class-level `#[Route]` emits `attribute_kind="class_route"` and joins into method `class_route_template`/`effective_route_template`. Method-level routes with `methods=` emit `attribute_kind="http_method"` (one fact per verb); without `methods=` emit `attribute_kind="request_mapping"` with verb omitted. Static-literal path guarding only (M2). Distinct from `laravel.route.v1`. |
| `ktor.route.v1` | `kotlin` | `route` | parser-covered bare verb call span inside `routing{}`/`route{}` | `framework` | A Ktor server verb call under the restricted lexical gate (design §4.6): bare `get`/`post`/`put`/`patch`/`delete`/`head`/`options` identifier, trailing lambda, static `string_literal` first arg (Braces), lexically inside `routing{}`/`route{}`. Gated on a server-side import (`io.ktor.server.*`, or Ktor 1.x `io.ktor.routing.*`/`io.ktor.application.*`); client-only `io.ktor.client.*` files stay silent. Enclosing `route("/prefix")` scopes join with the verb path into `effective_route_template` (accumulating when nested); the raw literal stays in `route_template`. `client.get`/`map.get` stay silent. |
| `phoenix.route.v1` | `elixir` | `route` | parser-covered router verb-macro call span | `framework` | A static Phoenix router verb-macro route (`api_style="dsl_routing"`). Import-gated on a `Phoenix.Router`/`:router` module; AST-driven. `get/post/put/patch/delete/head/options "/path", Ctrl, :action` set an upper-cased `verb`, the controller module `alias` (`controller`), and action atom (`action`). Phoenix `:id` segments are already normalized `:param`. Same-file `scope "/api" do ... end` prefixes join into `route_group_prefix`/`effective_route_template` (accumulating when nested); an interpolated/`~r`/concatenated/`@attr` path stays silent (M2). Metadata payload keys: see the JSON contract linked below. |
| `phoenix.resource_route.v1` | `elixir` | `resource_route` | parser-covered `resources` macro call span | `framework` | A Phoenix `resources "/x", Ctrl` RESTful resource declaration with a static path literal (`resource_path`/`normalized_resource_path`), the controller `alias` when present, and the enclosing same-file `route_group_prefix`. |
| `phoenix.forward.v1` | `elixir` | `forward` | parser-covered `forward` macro call span | `framework` | A static same-file Phoenix `forward "/lit", Plug` prefix registration emitted at its own site with `mount_path` (raw literal), `normalized_mount_path` (including enclosing scope), and `mount_target` (the forwarded plug alias). Cross-file scope prefixes are out of scope. |
| `axum.route.v1` | `rust` | `route` | parser-covered `.route` call span | `framework` | A static axum `Router::new().route("/x", get(h))` route (`api_style="call_routing"`), one fact per method-router verb. Import-gated on `axum`; AST-driven. The second `.route` argument must be a bare-identifier verb chain (`get(h)`, `get(a).post(b)`, with `.layer(...)` middleware transparent), which rejects actix's `web::get().to(h)` shape on the shared `rust` arm. `any`/`any_service` omit `verb`/`verb_source`. axum 0.8 `{id}` brace captures normalize to `:id` (a 0.7 `:id` template joins but under-reports `dynamic_segments`; no version-sniff). The receiver is single-assignment traced: a `Router::new()` chain or an unknown receiver (parameter/return) emits; a variable reassigned a conflicting non-router value is poisoned and stays silent. `format!`/concat/`const` paths stay silent (M2). |
| `axum.nest.v1` | `rust` | `nest` | parser-covered `.nest` call span | `framework` | A static same-file axum `Router::new().nest("/lit", sub_router)` prefix registration at its own site with `mount_path` (raw literal), `normalized_mount_path`, and `mount_target` (source text of the nested sub-router expression). The nested target is cross-file, so no route join is guessed (Miller's job). A poisoned receiver stays silent. |
| `actix.attribute_route.v1` | `rust` | `attribute_route` | parser-covered handler `function_item` span | `framework` | A static actix-web attribute-macro route (`api_style="attribute"`), one fact per verb. `#[get]`/`#[post]`/… map to their verb; `#[route("/x", method = "GET", method = "POST")]` emits one fact per `method =` value. `verb`/`verb_source` are ALWAYS present (attribute-macro verbs are always explicit). Registration is cross-file, so there are NO `route_group_prefix`/`effective_route_template` keys. The fact anchors on the handler `function_item` (a following sibling of the attribute), so its `containing_symbol_id` binds to the handler, not the enclosing module. Import-gated on `actix_web`; `{id}` normalizes to `:id`. Non-literal macro arguments (`const`/identifier) stay silent (M2). |
| `actix.scope_route.v1` | `rust` | `scope_route` | parser-covered `web::scope(...).route` call span | `framework` | A static actix-web scope-chained route `web::scope("/api").route("/x", web::post().to(h))` (`api_style="call_routing"`). The scope prefix is read same-file by walking the `.route` receiver chain to its base `web::scope(literal)`, so it flows into `route_group_prefix` + `effective_route_template` (both ALWAYS — scope routes are always scoped). The verb comes from the `web::<verb>()` method router (OPT — omitted for the method-agnostic `web::route()`). The `web::<verb>().to(h)` shape (a `scoped_identifier` base) is how axum's bare-identifier `get(h)` is rejected on the shared `rust` arm, and axum's `Router::new()` receiver is not a `web::scope`. `{id}` normalizes to `:id`. A non-static scope prefix, `format!`/concat/`const` paths, variable-bound scopes, and `web::resource().route()` guard forms stay silent (M2, documented `open_gaps`). |
| `actix.mount.v1` | `rust` | `mount` | parser-covered `web::scope(...).configure`/`.service` call span | `framework` | A static actix-web `web::scope("/lit").configure(fn)`/`.service(sub)` mount at its registration site with `mount_path` (raw literal), `normalized_mount_path`, and `mount_target` (source text of the configure/service argument). The delegated routes live in a cross-file target, so no route join is guessed (Miller's job). Import-gated on `actix_web`; a non-static scope prefix stays silent. |
| `htmx.attribute.v1` | `html`, `razor`, `javascript`, `jsx`, `tsx`, `vue` | `attribute` | parser-covered attribute span | `frontend_interaction` | An `hx-*` or `data-hx-*` attribute, including request verb and static target path metadata when applicable. |
| `alpine.directive.v1` | `html`, `razor` | `directive` | parser-covered attribute span | `frontend_interaction` | An Alpine `x-*`, `@...`, or `:...` directive with normalized directive metadata. |
| `razor.page_directive.v1` | `razor` | `page_directive` | `razor_page_directive` | `component_routing` | A Razor `@page` directive with route-template metadata. |
| `razor.code_block.v1` | `razor` | `code_block` | `razor_block` | `component_code` | A Razor `@code` or `@functions` block. |
| `razor.template_expression.v1` | `razor` | `template_expression` | `razor_implicit_expression`, `razor_explicit_expression` | `component_template` | A Razor template expression such as `@name` or `@(expr)`. |
| `css.selector_rule.v1` | `css`, `vue`, `html` | `rule_set` | `rule_set` | `stylesheet_structure` | A CSS selector rule set with selector kind and declaration-count metadata. |
| `css.custom_property.v1` | `css`, `vue`, `html` | `custom_property` | `property_name` | `stylesheet_structure` | A CSS custom property declaration. |
| `css.media_query.v1` | `css`, `vue`, `html` | `media_query` | `media_statement` | `responsive_design` | A CSS `@media` query. |
| `css.keyframes.v1` | `css`, `vue`, `html` | `keyframes` | `keyframes_statement` | `animation` | A CSS `@keyframes` animation. |
| `css.supports.v1` | `css`, `vue`, `html` | `supports` | `supports_statement` | `feature_query` | A CSS `@supports` feature query. |
| `css.container.v1` | `css`, `vue`, `html` | `container` | `at_rule` | `responsive_design` | A CSS `@container` query. |
| `css.font_face.v1` | `css`, `vue`, `html` | `font_face` | `at_rule` | `stylesheet_structure` | A CSS `@font-face` rule. |
| `css.layer.v1` | `css`, `vue`, `html` | `layer` | `at_rule` | `stylesheet_structure` | A CSS `@layer` rule. |
| `css.charset.v1` | `css`, `vue`, `html` | `charset` | `charset_statement` | `stylesheet_structure` | A CSS `@charset` rule. |
| `css.namespace.v1` | `css`, `vue`, `html` | `namespace` | `namespace_statement` | `stylesheet_structure` | A CSS `@namespace` rule. |
| `html.link.v1` | `html` | `link` | `element` | `document_navigation` | An HTML anchor link with an `href` target. |
| `html.area_link.v1` | `html` | `area_link` | `element` | `document_navigation` | An HTML image-map area link (`<area href>`). |
| `html.media.v1` | `html` | `media` | `element` | `document_assets` | An HTML media reference (`img`/`source`/audio/video/track with `src`). |
| `html.landmark.v1` | `html` | `landmark` | `element` | `document_landmarks` | An HTML landmark element or element with a landmark role. |
| `html.data_attribute.v1` | `html` | `data_attribute` | `element` | `document_attributes` | A generic HTML `data-*` attribute (excluding htmx/Alpine reserved prefixes). |
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
| `http.client_request.v1` | `javascript`, `jsx`, `typescript`, `tsx`, `vue`, `python`, `csharp`, `razor`, `go`, `java`, `kotlin`, `php`, `ruby`, `elixir`, `rust` | `client_request` | parser-covered call span (Java builder chains anchor the enclosing statement) | `web.http_client` | A supported outbound HTTP client call whose URL argument is a static string literal. Kotlin covers Ktor (`client="ktor"`), OkHttp builders (`client="okhttp"`), Retrofit annotations (`client="retrofit"`), Spring WebClient (`client="spring_webclient"`), and RestTemplate (`client="spring_resttemplate"`), each import-gated. PHP covers Guzzle verb and `request`/`requestAsync` forms (`client="guzzle"`), Laravel `Http` (`client="laravel_http"`), Symfony HttpClient (`client="symfony_http_client"`), and cURL (`client="curl"`). Elixir covers Req (`client="req"`), Tesla (`client="tesla"`), HTTPoison (`client="httpoison"`), Finch.build (`client="finch"`), and OTP `:httpc` (`client="httpc"`). Rust covers reqwest (`client="reqwest"`), hyper builders (`client="hyper"`), and ureq (`client="ureq"`). Metadata payload keys: see the JSON contract linked below. |
| `blazor.component_reference.v1` | `razor` | `component_reference` | parser-covered Razor tag-name span | `component_reference` | A Blazor/Razor component tag reference captured from a Razor markup element name. |
| `razor.route_reference.v1` | `csharp`, `razor` | `route_reference` | parser-covered call/attribute-value span | `frontend_navigation` | A static Razor/Blazor route reference from a NavigationManager call or route attribute value. |

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
blocks do not pop enclosing `namespace`/`scope` prefixes early. Laravel routes
come from an AST-driven collector import-gated on the `Route::` facade; same-file
`Route::prefix`/`group` prefixes are lexical-containment (joined into
`route_group_prefix`/`effective_route_template` and emitted as a
`laravel.route_prefix.v1` fact at the prefix site, poisoned by a non-literal
prefix). Symfony `#[Route]` attributes emit the separate `symfony.route.v1`
pattern (not Laravel). Cross-file `RouteServiceProvider` prefixes remain out of
scope, so Laravel `route_template` is not guaranteed to be the absolute public
path when such a prefix applies. Ktor server routes emit `ktor.route.v1` under
a restricted lexical gate inside `routing{}`/`route{}`: the raw literal stays in
`route_template`, and enclosing static `route("/prefix")` scopes join into
`effective_route_template`.
Phoenix routes come from an
AST-driven collector import-gated on a `Phoenix.Router`/`:router` module: the
bare verb macros emit `phoenix.route.v1`, `resources` emits
`phoenix.resource_route.v1`, and `forward` emits a `phoenix.forward.v1` prefix
registration; same-file `scope` blocks are lexical-containment prefixes
(joined/poisoned like the Rails `scope_stack`), and `pipe_through`/`live`/
`socket`/`channel` plus cross-file scope prefixes are out of scope. axum routes
come from an AST-driven collector import-gated on `axum`: `.route("/x",
get(h).post(c))` emits one `axum.route.v1` per method-router verb (bare-verb
chain only, rejecting actix's `web::get().to(h)`; `any`/`any_service` omit the
verb), and `.nest("/lit", sub)` emits an `axum.nest.v1` prefix registration at
its own site. The receiver is single-assignment traced (a poisoned receiver
stays silent), axum 0.8 `{id}` normalizes to `:id` (0.7 `:id` is an honest
`dynamic_segments` under-report), the nest target is cross-file so no join is
guessed, and `format!`/concat/`const` paths stay silent (M2). actix-web routes
run on the same shared `rust` arm, import-gated on `actix_web`, split (like
aspnet) into two route pattern ids plus a mount: attribute macros `#[get("/x")]`/
`#[route("/x", method = "GET")]` emit `actix.attribute_route.v1` (verb ALWAYS, no
prefix keys, bound to the handler `function_item`), scope chains
`web::scope("/api").route("/x", web::post().to(h))` emit `actix.scope_route.v1`
(same-file scope prefix → `route_group_prefix`/`effective_route_template`, verb
OPT from `web::<verb>()`), and `web::scope("/lit").configure(fn)`/`.service(sub)`
emits an `actix.mount.v1` prefix registration. The `web::get().to(h)` /
`web::scope` shapes keep actix and axum from double-emitting on the shared arm;
non-static scope prefixes, `format!`/concat/`const` paths, variable-bound scopes,
and `web::resource().route()` guard forms stay silent (M2, documented `open_gaps`).

Backend-language client collectors emit the same `http.client_request.v1`
metadata shape for static string URL arguments: Python module-qualified
`requests`/`httpx` calls, C# `HttpClient` method calls and
`HttpRequestMessage`, Go `net/http` package calls, Java `HttpRequest` builder
chains, PHP Guzzle/Laravel/Symfony/cURL families, Ruby
`Net::HTTP` calls with literal `URI(...)`/`URI.parse(...)` arguments, Elixir
Req/Tesla/HTTPoison/Finch/`:httpc` families, and Rust reqwest/hyper/ureq
families (static URLs only; import- and shape-gated per collector). Java builder-chain facts span the enclosing statement (the URL and
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
(`crates/julie-extractors/src/base/structural_fact_registry/`); treat it as the
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

`kind_coverage_json` stores the same additive capability object, including the
`test_detection` domain. That domain uses the fixed units `test_case`,
`test_container`, and `test_lifecycle`; each unit appears in `supported`,
`not_applicable`, or `open_gaps`. See
[Test Evidence v1](test-evidence-v1.md) for the evidence and consumer rules.

Adding `test_detection` inside this existing JSON object does not change the
SQLite v4 table shape or the extraction contract version.

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
- Lower-durability settings for benchmarks are not part of the v4 product
  contract.
- Reference resolution runs inside the same writer transaction as the scan's row
  writes (before revision-count updates and before commit, in every writer path
  including the spooled deferred-FK transaction). A resolver failure is
  non-fatal: the scan commits and the report records `resolution_failed`.

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
CREATE INDEX idx_source_regions_export_order ON source_regions(path, start_byte, end_byte, kind, source_region_id);
CREATE INDEX idx_source_regions_kind_file ON source_regions(kind, file_id, start_byte);
CREATE INDEX idx_source_regions_symbol ON source_regions(containing_symbol_id);
CREATE INDEX idx_structural_facts_file_span ON structural_facts(file_id, start_byte, end_byte);
CREATE INDEX idx_structural_facts_export_order ON structural_facts(path, start_byte, end_byte, pattern_id, capture_name, structural_fact_id);
CREATE INDEX idx_structural_facts_pattern_language_path ON structural_facts(pattern_id, language, path);
CREATE INDEX idx_structural_facts_symbol ON structural_facts(containing_symbol_id);
CREATE INDEX idx_complexity_metrics_file_scope ON complexity_metrics(file_id, scope, start_byte);
CREATE INDEX idx_complexity_metrics_export_order ON complexity_metrics(path, start_byte, end_byte, scope, symbol_id, complexity_metric_id);
CREATE INDEX idx_complexity_metrics_scope_language ON complexity_metrics(scope, language, path);
CREATE INDEX idx_complexity_metrics_symbol ON complexity_metrics(symbol_id);
CREATE INDEX idx_diagnostics_path ON parse_diagnostics(path);
CREATE INDEX idx_diagnostics_file ON parse_diagnostics(file_id);
CREATE INDEX idx_identifiers_file_line_name ON identifiers(file_id, start_line, name);
CREATE INDEX idx_pending_resolutions_target ON pending_resolutions(target_symbol_id);
CREATE INDEX idx_identifier_resolutions_target ON identifier_resolutions(target_symbol_id);
```

The final three indexes are new in v4: `idx_identifiers_file_line_name` backs the
resolver's `(file_id, start_line, name)` propagation fallback, and the two
`*_resolutions_target` indexes back FK cascade and reverse-lookup by target
symbol.

These indexes protect the v4 access patterns. Implementations may add more
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
- No old Julie schema compatibility tables as a v4 requirement.

## Tradeoffs

- **Stable opaque IDs:** downstream readers get durable references without
  depending on old fixture key or MD5 mechanics.
- **Structured pending only:** v4 stores the richer unresolved target shape and
  does not expose Julie's legacy flat pending queue as a separate contract.
- **Resolution is an overlay, not an extraction fact:** v4 keeps unresolved
  context durable and layers resolution as FK-governed overlay tables, so target
  death and candidate-set changes invalidate automatically instead of corrupting
  `trace`/`impact` with stale edges.
- **Capability rows in SQLite:** consumers can validate language evidence from
  the artifact without also reading repository fixtures.
- **Indexes are required, not advisory:** write cost is acceptable because this
  product is an artifact producer for downstream tools that need predictable
  lookup performance.
- **Test role flags are first-class:** extractor metadata is also exposed as
  indexed SQLite booleans because downstream test filtering should not depend on
  JSON expression scans.
- **Source regions are spans, not search:** v4 exposes AST-bounded source
  ranges for downstream products, but it does not create lexical indexes,
  vector indexes, or store complete source text.
- **Open decision before implementation:** exact parser version fields depend on
  what each parser package exposes. The required contract is a parser inventory
  table plus fingerprint; missing package-level versions must be represented as
  null, not guessed.
