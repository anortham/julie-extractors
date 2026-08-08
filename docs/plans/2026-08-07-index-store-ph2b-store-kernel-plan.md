# Index Store Ph2b — Store Kernel + Queued Write Verbs Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when
> subagent delegation is available. Fall back to razorback:executing-plans for single-task,
> tightly-sequential, or no-delegation runs.

**Goal:** Add the v4 family-store kernel and real, durable `store import`, `store update`, and
`store delete` CLI behavior without changing the existing v3 artifact commands or exposing
unfinished resolution, migration, retention, or promotion operations.

**Architecture:** The existing `ArtifactWriter` remains the v3 single-artifact writer. A new
deep `julie_extract_artifact::store` module owns the separate v4 store format: layout validation,
connection invariants, schema, immutable file versions, per-level completion stamps, view
manifests, `store_log`, and the durable coordinator protocol. The CLI adds a thin nested `store`
surface and reuses discovery, extraction, spool, progress, and watchdog code; store logic does
not enter the existing 5,000-line `commands.rs`. Every public write is a coordinator request.
The first complete manifest becomes visible once all selected versions have L1, while a `full`
request continues L2 then L3 deepening in background-sized chunks. Failed extraction never
creates a `file_versions` row.

**Approved design:** Miller's frozen v4 contract at
`/Users/murphy/source/miller/docs/plans/2026-08-07-index-store-v4-contract.md` is authoritative.
Its G3b exception was explicitly accepted by the user. Ph2b implements sections 1–5, the
store-side subset of sections 7, 13, and 15, and the V-2/V-3/V-4 purity surgeries from section
16. It does not reopen the frozen architecture.

**Tech stack:** Rust 1.97.1, rusqlite 0.40 bundled SQLite, clap 4, serde/serde_json, blake3,
postcard spool records, existing xtask test tiers.

## Scope Boundary

**In scope:**

- Generation layout creation and `CURRENT` resolution for one family store.
- A separate, versioned v4 `store.db` contract; legacy `SQLITE_SCHEMA_VERSION = 6` stays intact.
- Input-keyed file versions `(path, blake3 content hash, extraction epoch)`.
- Composite version-qualified row identity and exhaustive FK/index classification.
- L1/L2/L3 row partitioning and same-transaction completion stamps.
- Immutable manifest generations, view pointer flips, failed-preserved view state, and manifest
  hashes.
- `store_log` effect records and request chunk/terminal idempotency anchors.
- Durable `coord.db` request queue, time-boxed writer lease, heartbeat/takeover, bounded
  interactive burst scheduling, and crash reconciliation.
- Public `store import`, `store update`, and `store delete` CLI commands and report schema.
- Deterministic row-equivalence, crash/resume, multi-delete, path-deletion, floor, and
  concurrent-request gates for this slice.

**Out of scope, with named follow-ons:**

- **Ph2c — resolution and adapters:** resolution-only `resolve`, own-file bulk resolution,
  immutable bases, per-view deltas/tombstones, convergence/CAS, `store export`, and
  `store import --from-artifact`. Ph2b does not discharge the frozen contract's G3b carry;
  Ph2c must re-measure the G3b ratio in Rust against own-file resolution output before
  resolution is complete.
- **Ph2d — retention and repair:** GC/demotion, purge, vacuum/reindex escalation, capacity
  preflight, generation promotion, full mixed-version matrix, release prep, and Miller pin bump.
- **Ph3 — Miller wiring:** registry family resolution, governor admission, read sessions,
  sidecars, status/health/dashboard, and rollback orchestration.

No unfinished Ph2c/Ph2d subcommand is parsed in Ph2b. Internal schema fields that Ph2c consumes
may exist only when their invariants are already enforced; no placeholder tables or stub results.

## Caller-Facing Contract

The existing top-level commands (`scan`, `update`, `delete`, `info`, `export`, `languages`,
`rebind`) keep byte-compatible parsing and output. Ph2b adds:

```text
julie-extract store import --store <family-dir> --family <uuid> --root <path> --view <id>
                           [--level l1|full] [scan controls] [request controls]
julie-extract store update --store <family-dir> --root <path> --view <id> --file <path>
                           [--level l1|full] [scan controls] [request controls]
julie-extract store delete --store <family-dir> --root <path> --view <id> --file <path>
                           [request controls]
```

- `--store` is explicit. Julie Extractors never derives `~/.miller`, registry, or git-family
  policy; Miller remains the only caller and passes the selected family directory.
- `--family` is required when creating a store and validated when supplied for an existing one.
  The caller mints the UUID; the store persists it. Julie does not infer family identity.
- `store import` creates a missing view and binds its canonical root with resolution `unbound`.
  A later import for that view must match the bound root. `store update` and `store delete`
  require an existing view and return stable `view_not_found` / `view_root_mismatch` errors.
- Request controls are `--request-id`, `--idempotency-key`, and `--request-timeout-seconds`.
  Missing IDs are minted by the CLI and returned in the report. A retry with the same
  idempotency key observes the original request rather than executing again.
- `StoreLevelArg::{L1, Full}` is distinct from the legacy `LevelArg::{Symbols, Full}`. Store L1
  runs the existing `ExtractionLevel::Symbols`. Full is a proof-gated two-wave operation: run
  Symbols across the selected tree, commit every L1, and publish the manifest; then reread each
  source, require the same content hash, run `ExtractionLevel::Full`, compare its L1 projection
  to the stored version by every L1 table's natural key, and commit L2 then L3 only on equality.
  A mismatch returns stable `l1_projection_mismatch`, leaves L2/L3 incomplete, writes none of the
  Full L1 rows, and blocks the affected request as a design defect. A source changed between waves
  keeps the already-published L1 manifest entry, reports `changed_between_waves`, and requires a
  later request to index the new hash; the current request never repoints it. Ph2b rejects this
  wave model unless a multi-language gate proves Symbols L1 rows equal Full L1 rows for every L1
  table. If a later file exposes a mismatch after earlier files were deepened, their committed
  stamps remain valid; the request terminates failed with explicit partial progress and does not
  roll back valid immutable effects.
- Every invocation emits one versioned JSON report on stdout with the request, family, view,
  manifest, level-completion, row-count, and coordinator disposition fields. Human output goes
  through the same report model. Diagnostics never corrupt JSON stdout.

## Store Schema Contract for This Slice

Task 1 writes the exact DDL authority, but these identities are fixed for the plan:

- `store_meta(key PRIMARY KEY, value)` carries store schema/format epochs, family id,
  extraction epoch, reader/writer floors, creator/binary versions, and retention defaults.
- `file_versions(version_id INTEGER PRIMARY KEY, path, content_hash, extraction_epoch, language,
  content_bytes, line_count, metadata_json, complete_l1, complete_l2, complete_l3)` owns immutable
  pure file columns. Uniqueness of `(path, content_hash, extraction_epoch)` is emitted only as
  Task 1's named unique index, never as an inline table constraint. Completion values are
  `store_log.sequence` values and are written with their level's final rows.
- Every per-version table keys `(version_id, local_id)` and every FK to a per-version row carries
  `version_id`. Existing stable IDs remain the local-id component. No unqualified retained ID.
- `reference_sites.level` is 1 when any relationship/pending row claims the site and 2 only for
  identifier-only sites. Shared evidence is stored once at L1; L2 identifiers may reference it.
- Fingerprint-global parser/capability tables are keyed by extraction epoch, never by view.
- `views` points to one current manifest and carries the future resolution binding fields in a
  state that is already valid (`unbound`, no exact generation).
- `manifests` and `manifest_entries` retain immutable generations. Entries own view-local
  status/observed/error fields (V-2/V-3/V-4); a failed-preserved entry may still point at its
  prior good version. A newly failing path has no version row.
- `store_log.sequence INTEGER PRIMARY KEY AUTOINCREMENT` is the sole log-sequence allocator; no
  counter is mirrored in `store_meta`. It records level completion, manifest flip, and terminal
  request effects. Request progress uses one request-global, monotonically increasing
  `chunk_index` across L1, L2, and L3 waves. `(request_id, chunk_index)` is unique, resume starts
  at `MAX(chunk_index) + 1`, and exactly one terminal entry may exist per request.
- `coord.db` owns requests and the writer lease. It never lives inside a generation and never
  shares a transaction with `store.db`.

The Ph2b catalog allowlist is `store_meta`, extraction-epoch parser/capability tables,
`file_versions`, the 14 per-version child tables, `views`, `manifests`,
`manifest_entries`, `store_log`, and request-chunk progress. It explicitly excludes resolution
bases, deltas, `identifier_resolutions`, and `pending_resolutions`. Nullable resolution binding
columns on `views` are permitted only with CHECK constraints forcing state `unbound` and
`exact_at IS NULL` throughout Ph2b.

Every secondary index in the store DDL is labeled `gc-aligned` or `read-aligned` in the checked-in
contract. The schema test fails on an unlabeled index or a non-version-qualified FK.

## Architecture Quality

- **Affected modules:** `julie-extract-artifact` gains the deep store module; the CLI gains
  nested store args/dispatch/reports and small visibility changes to existing discovery,
  extraction, spool, progress, and watchdog seams. Existing extractor language modules and v3
  writer behavior do not change.
- **Caller-facing interface:** one nested process contract consumed only by Miller. The caller
  supplies family/view/store identity; Julie owns durable execution and reports exact state.
- **Depth/locality:** schema, transactions, manifests, coordinator recovery, and row partitioning
  are hidden behind `Store`, `StoreWriter`, and `StoreCoordinator`. CLI files translate arguments
  and reports only.
- **Test surface:** pure schema/model tests; store writer/manifest/coordinator contract tests;
  public CLI operations; crash-point subprocess tests; multi-language row equivalence; query-plan
  and physical pragma checks; existing default and contract tiers unchanged.
- **Seams:** `ArtifactFile` remains the extraction transport. A fallible store projection rejects
  failed-preserved transport rows and partitions successful rows by level. The public CLI is the
  inter-repo seam; no Miller types or paths enter Rust APIs.
- **Rejected shortcuts:** mutating/expanding `ArtifactWriter`; copying the v3 database per view;
  full-output-first imports that delay L1 visibility; direct CLI writes outside the coordinator;
  cross-WAL transaction claims; output-byte-derived version IDs; view state on immutable version
  rows; unqualified local IDs; exposing future verbs as stubs.
- **Risk:** high. This adds a persistent format, crash recovery, concurrent writers, and
  cross-repo process contracts. The plan therefore requires a Grok adversarial doubt pass before
  approval and crash/equivalence gates before completion.

## Global Constraints

- Legacy artifact schema, JSONL, report, and CLI output are unchanged in Ph2b.
- Store format uses separate constants: `STORE_SQLITE_SCHEMA_VERSION = 1`,
  `STORE_FORMAT_EPOCH = 1`, and `STORE_REPORT_SCHEMA_VERSION = 1`. Do not bump legacy schema 6.
- Promote the compatibility ledger into a real `EXTRACTION_IDENTITY_EPOCH: u32 = 1` beside
  `EXTRACTION_CONTRACT_VERSION`. The v2.30 fixture output is epoch 1. The existing previous-vs-
  current compatibility gate must assert unchanged output when this number is unchanged and
  require an explicit compatible/incompatible epoch-bump classification when it moves.
- All store writer connections assert and read back `foreign_keys=ON`, `journal_mode=WAL`,
  `synchronous=FULL`, `page_size=4096`, and creation-time `auto_vacuum=INCREMENTAL`.
- Bulk import autocheckpoint is 8,000 pages; routine writes use 1,000. Core `secure_delete=ON` is
  reasserted on every writer connection.
- Default batch quantum is 100 versions or a 128 MB observed WAL budget, whichever binds first.
  `MILLER_STORE_CHUNK_VERSIONS=0` means one version per chunk.
- Version dedup trusts only the required non-null completion stamp. Incomplete rows are invisible
  and resumable, never treated as a cache hit.
- Manifest paths are normalized root-relative slash paths and sorted by UTF-8 bytes before
  hashing. The manifest hash includes path, version identity, and view status.
- A content-changing manifest flip leaves resolution `exact_at` behind in the same transaction.
  Ph2b always reports resolution `unbound`; it never invents resolution rows.
- Coordinator state is `queued -> claimed -> committed -> acknowledged | failed`. Every durable
  effect has its own same-transaction `store_log` row; a mid-request L1 manifest flip is a
  non-terminal effect. Only the final request transaction writes the unique terminal row, and
  only that row—not progress/effect rows—is committed-in-fact for the whole request.
- The one-shot process model is explicit: every caller enqueues; a process that acquires the
  lease drains the queue in bounded bursts, while a non-holder polls its request to terminal or
  timeout. When its own request becomes terminal, the holder snapshots the then-queued backlog
  and drains every request in that snapshot to terminal or durable failure. Arrivals during that
  drain are eligible only through a bounded service window
  (`MILLER_STORE_COORDINATOR_SERVICE_MS`, default 1,000 ms). At expiry the holder claims no new
  arrivals, finishes its current safe quantum/request, then releases or explicitly expires its
  lease on every exit path. A requester deadline drops acknowledgment obligations; it never
  cancels durable execution, and a non-holder timeout never deletes its request. A stale holder
  is taken over; a live equal-version holder is not displaced.
- Scheduling classes are interactive (`update`, `delete`) and batch (`import`). The default
  interactive burst is 32 requests or 250 ms before one batch chunk must run.
- Lock order is caller governor admission -> store-writer lease -> future sidecar lease. Ph2b
  owns and tests the store-writer portion; Miller Ph3 owns governor/sidecar integration.
- No new dependency is expected. If implementation needs one, stop that task and revise the plan
  with source/maintenance/CVE evidence before changing Cargo manifests.
- Public path arguments are canonicalized and must stay inside their declared root/store layout.
  Symlink, hard-link, traversal, and file/directory collision behavior is tested on supported
  platforms.

## Verification Strategy

**Source of truth:** `docs/testing-strategy.md`, `docs/release.md`, the frozen v4 contract, and
the target-owned contracts produced by Task 1.

**Baseline evidence:** `RUSTUP_TOOLCHAIN=1.97.1 cargo xtask test default` passed from clean
`84cadd4` in the Ph2 worktree before plan authoring.

**Worker red/green scope:** each task first adds or selects the narrow test named below, proves
the expected failure when behavior is absent, implements the minimum complete behavior, then
reruns the narrow test.

**Worker ceiling:** `RUSTUP_TOOLCHAIN=1.97.1 cargo xtask test default`. Workers do not run the
full contract or crash matrix unless their task owns it.

**Lead affected-change scope:** default tier after every task integration; store contract tests
after every store-module change; CLI store operations after every CLI change.

**Branch gate:**

```bash
RUSTUP_TOOLCHAIN=1.97.1 cargo fmt --check
RUSTUP_TOOLCHAIN=1.97.1 cargo test -p xtask
RUSTUP_TOOLCHAIN=1.97.1 cargo xtask test default
RUSTUP_TOOLCHAIN=1.97.1 cargo xtask test contract
RUSTUP_TOOLCHAIN=1.97.1 cargo test -p julie-extract-artifact --features test-store-crash --test store_crash_contract -- --nocapture
RUSTUP_TOOLCHAIN=1.97.1 cargo test -p julie-extract-cli --features test-store-contract --test store_equivalence -- --nocapture
```

The two new features keep crash/process and multi-language equivalence tests out of the default
tier; convention tests fail if those files become unguarded.

**Hard evidence:** SQLite `quick_check` + `foreign_key_check`; exact catalog hash; required query
plans; separate-process crash matrix; incremental-converged rows equal from-scratch rows; duplicate
requests produce one terminal effect; takeover completes abandoned work; deleted paths disappear
from the next manifest without deleting shared versions.

**Performance evidence:** tiny default-tier tripwires assert transaction counts and no repeated
statement preparation. A feature-gated store harness records rows/s, WAL peak, L1-visible time,
and resume reuse. Timing is report-only in Ph2b except the structural limits (chunk count, WAL
budget response, no full-output-first path).

**Security scopes:**

- `security-secrets`: none declared by the repository.
- `security-deps`: none declared; the plan adds no dependency.
- External model policy: none declared. Per user direction, xAI/Grok is the reviewer; record
  `no external-model policy declared — diff sent to xai` at each dispatch.

**Escalation triggers:** row-equivalence mismatch, an incomplete version accepted as complete,
an unqualified FK/ID, cross-WAL atomicity assumption, duplicate terminal effect, live-holder
displacement, unbounded batch starvation, or a store command changing legacy command bytes.
These are design defects; stop the affected task and revise the approved plan rather than
weakening a gate.

## Parallel Execution Contract

| Task | Batch | File ownership | Serialization | Dependency |
|---|---|---|---|---|
| 1. Contract + schema authority | Serial | Store contract/docs, `store/schema.rs`, schema tests | Yes | Establishes all later identities. |
| 2. Layout + connection invariants | Serial | `store/layout.rs`, `store/connection.rs`, connection tests | Yes | Depends on Task 1 only. |
| 3. Level projection + row writer | Serial | `store/model.rs`, `store/rows.rs`, `store/writer.rs`, writer tests | Yes | Consumes Task 2's connection factory. |
| 4. Manifests + store log | Serial | `store/manifest.rs`, `store/log.rs`, manifest tests | Yes | Consumes Task 3 versions and stamps. |
| 5. Durable coordinator | B | `store/coordinator.rs`, coord schema/tests | No | Consumes Task 1 and terminal log API from Task 4. |
| 6. Internal CLI contract + reports | B | `store/args.rs`, `store/report.rs`, internal contract tests | No | Consumes Task 1 types; exposes no top-level verb. |
| 7. Public queued store import | Serial | top-level dispatch, `cli/src/store/mod.rs`, `import.rs`, shared extraction visibility, import tests | Yes | Consumes Tasks 2–6; first public store command. |
| 8. Public queued update/delete | Serial | `store/args.rs`, `mod.rs`, `update.rs`, `delete.rs`, operation tests | Yes | Consumes Task 7 executor/scheduler. |
| 9. Crash/equivalence/floor gates | C | New feature-gated tests + xtask routing only | No | Production behavior from Tasks 1–8 is frozen. |
| 10. Docs, dogfood, closeout | Serial | Docs map/testing docs/plan ledger only | Yes | All prior gates green. |

Agents are not alone in the codebase. Stay inside assigned files, do not revert another task's
changes, and accommodate already-landed interfaces. A task report must include path, branch,
commit, dirty state, tests, and any plan mismatch. Tasks 5/6 may run in parallel; every other
transition is serialized.

## Tasks

### Task 1: Freeze the target-owned store contract and schema authority

**Files:**

- Create: `docs/contracts/store-v1.md`
- Create: `docs/contracts/sqlite-store-schema-v1.md`
- Create: `docs/architecture/versioned-index-store.md`
- Create: `crates/julie-extract-artifact/src/store/mod.rs`
- Create: `crates/julie-extract-artifact/src/store/schema.rs`
- Modify: `crates/julie-extract-artifact/src/lib.rs`
- Modify: `crates/julie-extractors/src/lib.rs`
- Modify: `crates/julie-extractors/src/tests/api_surface.rs`
- Modify: `xtask/src/compat.rs`
- Create: `crates/julie-extract-artifact/tests/store_schema_contract.rs`
- Modify: `crates/julie-extract-artifact/tests/test_tiers.rs`

**Interfaces:**

- Export `STORE_SQLITE_SCHEMA_VERSION`, `STORE_FORMAT_EPOCH`,
  `create_store_schema(&Connection)`, and `create_coordinator_schema(&Connection)`.
- Export `julie_extractors::EXTRACTION_IDENTITY_EPOCH = 1` and carry it through the existing
  extractor-compat report/classification gate.
- Define exact store/coord table columns, composite FKs, CHECK constraints, and labeled indexes.
- Keep `schema::SQLITE_SCHEMA_VERSION == 6` and the v3 catalog hash unchanged.

**Tests first:** add catalog assertions that fail because the store module/schema does not exist.
The final test hashes normalized `sqlite_master` output into a checked-in authority block in
`sqlite-store-schema-v1.md`, enumerates every per-version FK, and maps every secondary index to
exactly one `gc-aligned`/`read-aligned` classification.

**Implementation:** encode the Ph2b schema from this plan and the frozen contract. Use strict
tables where compatible with SQLite targets. Store and coordinator schemas are independently
creatable. Schema creation is idempotent; opening an unknown newer schema is a typed refusal,
not an automatic migration. Enforce the Ph2b catalog allowlist/denylist above; do not create any
Ph2c resolution table or represent a ready/exact resolution generation.

**Exact DDL resolution:** Task 1 owns the first target-side DDL; these rules remove the choices
that are not already mechanical from schema 6:

- `STORE_SQLITE_SCHEMA_VERSION`, `STORE_FORMAT_EPOCH`, and
  `EXTRACTION_IDENTITY_EPOCH` are all integer `1`. Both databases set `PRAGMA user_version=1`.
  Ordinary tables are `STRICT`; booleans remain checked `INTEGER`, timestamps are canonical
  RFC 3339 UTC `TEXT` in `store.db`, and coordinator deadlines/heartbeats are Unix-millisecond
  `INTEGER` values so injected clocks compare without parsing.
- `store_meta` is unrestricted non-empty `TEXT PRIMARY KEY` to non-null `TEXT`; the documented
  keys are `store_sqlite_schema_version`, `store_format_epoch`, `family_id`,
  `extraction_identity_epoch`, `min_reader_version`, `min_writer_version`,
  `created_by_version`, `binary_version`, `retention_window_days`,
  `retention_byte_target`, `retention_byte_ceiling`, and `retention_path_cap`. Schema creation
  seeds the two store versions plus retention defaults `7`, `1.20`, `1.25`, and `24`; Task 2
  atomically binds the identity/version keys.
- `file_versions` uses the columns and unique key fixed by the Store Schema Contract above.
  `version_id` is `INTEGER PRIMARY KEY AUTOINCREMENT`; retained logs may name deleted versions,
  so SQLite rowid reuse is forbidden. Path, hash, epoch, language, and content bytes are required;
  line count and metadata remain nullable as in schema 6.
  Counts are non-negative, stamps are null or positive, L2 implies L1, and L3 implies L2.
- Fingerprint-global tables retain every schema-6 payload column and add leading
  `extraction_epoch INTEGER NOT NULL` to every PK and FK. Gap identity is
  `(extraction_epoch, gap_id)`. The existing gap-status CHECK remains.
- Each of the 14 children retains its schema-6 payload, nullability, and semantic CHECKs, drops
  `file_id`, adds leading `version_id INTEGER NOT NULL`, and changes its PK to
  `(version_id, local_id)`. It keeps denormalized `path`/`language` where schema 6 has them.
  Every child directly cascades from `file_versions(version_id)`; every cross-child/self FK is
  `(version_id, local_id) ON DELETE NO ACTION DEFERRABLE INITIALLY DEFERRED`. Whole-version purge
  is the only physical delete path and removes all children through the direct version FK; an
  individual child delete cannot recursively erase other immutable evidence. This intentionally
  replaces schema 6's optional-link `SET NULL`, which is illegal when the composite's
  `version_id` is non-null. `reference_sites` adds checked level `1|2` and a version-qualified
  form of `reference_sites_identity_guard`: its match predicate includes `version_id`, and its
  mismatch comparison preserves every schema-6 compared payload column plus `level`. V-1
  resolution columns/tables/indexes do not exist. Ph2d demotion must delete L3 before L2 within
  one transaction so deferred cross-level references are satisfied at commit.
- Secondary indexes use an explicit `idx_gc_`/`idx_read_` prefix, or the corresponding
  `uidx_read_` prefix for unique read indexes, as their exhaustive schema classification. Drop
  schema-6 file-id-only indexes, resolution indexes, and the unused
  `type_facts` secondary index. Constraint autoindexes are structural and listed separately.
  The complete named index authority is:

  | Class | Name and ordered columns |
  |---|---|
  | read | `uidx_read_file_versions_identity(path, content_hash, extraction_epoch)` |
  | read | `idx_read_language_capability_gaps_language(extraction_epoch, language)` |
  | gc | `idx_gc_symbols_path(version_id, path)`; `idx_gc_symbols_is_test(version_id, is_test)`; `idx_gc_symbols_test_container(version_id, test_container)`; `idx_gc_symbols_test_lifecycle(version_id, test_lifecycle)` |
  | read | `idx_read_symbols_name_kind(name, kind, version_id)`; `idx_read_symbols_parent(parent_symbol_id, version_id)` |
  | gc | `idx_gc_symbol_annotations_symbol(version_id, symbol_id)` |
  | read | `idx_read_reference_sites_containing_symbol(containing_symbol_id, version_id)` |
  | read | `idx_read_identifiers_name_kind(name, kind, version_id)`; `idx_read_identifiers_containing(containing_symbol_id, version_id)`; `idx_read_identifiers_reference_site(reference_site_id, version_id)` |
  | read | `idx_read_relationships_from(from_symbol_id, version_id)`; `idx_read_relationships_to(to_symbol_id, version_id)`; `idx_read_relationships_kind(kind, version_id)`; `idx_read_relationships_reference_site(reference_site_id, version_id)` |
  | read | `idx_read_pending_terminal(target_terminal_name, version_id)`; `idx_read_pending_from(from_symbol_id, version_id)`; `idx_read_pending_caller_scope(caller_scope_symbol_id, version_id)`; `idx_read_pending_reference_site(reference_site_id, version_id)` |
  | read | `idx_read_type_argument_usages_identifier(identifier_id, version_id)` |
  | gc | `idx_gc_type_arguments_usage(version_id, usage_id)`; `idx_gc_type_arguments_parent(version_id, parent_type_argument_id)` |
  | read | `idx_read_literals_containing_symbol(containing_symbol_id, version_id)` |
  | gc | `idx_gc_source_regions_file_span(version_id, start_byte, end_byte)`; `idx_gc_source_regions_export_order(version_id, path, start_byte, end_byte, kind, source_region_id)` |
  | read | `idx_read_source_regions_kind(kind, version_id, start_byte)`; `idx_read_source_regions_symbol(containing_symbol_id, version_id)` |
  | gc | `idx_gc_structural_facts_file_span(version_id, start_byte, end_byte)`; `idx_gc_structural_facts_export_order(version_id, path, start_byte, end_byte, pattern_id, capture_name, structural_fact_id)` |
  | read | `idx_read_structural_facts_pattern_language_path(pattern_id, language, path, version_id)`; `idx_read_structural_facts_symbol(containing_symbol_id, version_id)` |
  | gc | `idx_gc_complexity_metrics_file_scope(version_id, scope, start_byte)`; `idx_gc_complexity_metrics_export_order(version_id, path, start_byte, end_byte, scope, symbol_id, complexity_metric_id)` |
  | read | `idx_read_complexity_metrics_scope_language(scope, language, path, version_id)`; `idx_read_complexity_metrics_symbol(symbol_id, version_id)` |
  | gc | `idx_gc_diagnostics_path(version_id, path)` |
  | read | `uidx_read_manifests_hash(view_id, manifest_hash)`; `idx_read_manifest_entries_version(version_id, view_id, generation)` |
  | read | `uidx_read_store_log_terminal_request(request_id) WHERE terminal = 1`; `idx_read_store_log_request(request_id, sequence)`; `uidx_read_request_chunks_log_sequence(store_log_sequence)` |
  | read | `uidx_read_requests_idempotency_key(idempotency_key)`; `idx_read_requests_queue(state, created_at, request_id)`; `idx_read_requests_stale(state, claim_heartbeat_at, request_id)` |

  `views.current_generation` joins to `manifest_entries` through the entries PK
  `(view_id, generation, path)`; this is the exact current-view path Ph3 consumes.
- The target-owned names `manifests` plus `manifest_entries` supersede the frozen design's
  conceptual `view_manifest`. `views` is `(view_id TEXT PRIMARY KEY, root TEXT,
  current_generation INTEGER NULL, resolution_state TEXT DEFAULT 'unbound',
  resolution_base_id TEXT NULL, resolution_delta_generation INTEGER NULL,
  resolution_exact_at INTEGER NULL, created_at TEXT, updated_at TEXT)` with non-empty identity/
  root checks and one CHECK forcing the only Ph2b resolution state to `unbound` with all three
  binding fields null. Its nullable `(view_id, current_generation)` FK targets `manifests` and
  is `DEFERRABLE INITIALLY DEFERRED`; publication inserts the manifest before flipping the view.
- `manifests` is keyed `(view_id, generation)` with positive generation, non-empty
  `manifest_hash` and `request_id`, canonical `created_at`, FK to `views`, and unique
  `(view_id, manifest_hash)` enforced only by `uidx_read_manifests_hash`, never a duplicate inline
  UNIQUE. `manifest_entries` is keyed `(view_id, generation, path)`, FKs to the manifest and
  nullable version, and carries `status`, `observed_content_hash`, `indexed_at`,
  `error_class`, and `error_json`. The version FK is `ON DELETE RESTRICT`, never cascade or set
  null, so every live or historical manifest is a GC root. Status is exactly
  `indexed|failed_preserved|failed`; indexed requires a version/no error, failed-preserved
  requires a prior version/error, and a new failed entry requires no version/an error.
- `store_log` has sole allocator `sequence INTEGER PRIMARY KEY AUTOINCREMENT`, non-empty
  `request_id`/`event_kind`, nullable view/generation/version/level fields, checked
  `terminal INTEGER`, non-null `payload_json`, and canonical `created_at`. A classified partial
  unique index permits one terminal row per request. `request_chunks` is a separate table keyed
  `(request_id, chunk_index)` with non-negative global chunk index, its effect's log sequence,
  nullable checked level, payload, and created time; a classified unique index makes one chunk
  own one log sequence. Log/progress rows deliberately do not FK to prunable log or retained
  version/manifest rows. Ph2b does not prune `store_log`. Ph2d may prune a terminal row only after
  its coordinator request is no longer a reconciliation root and after every sidecar cursor plus
  safety window has passed it; non-terminal coordinator requests root terminal lookup by
  `request_id`, including the phase-1/phase-2 tear where `terminal_log_sequence` is still null.
- `coord.db.requests` uses the exact frozen field list plus nullable
  `terminal_log_sequence INTEGER`: text request/idempotency/kind/requester/owner/result/error,
  integer deadlines/heartbeats/created/updated times, checked kinds `import|update|delete`, and
  checked states `queued|claimed|committed|acknowledged|failed`. State-coherence CHECKs require
  claim owner/heartbeat only while claimed; committed/acknowledged require a terminal sequence,
  result, and no error; failed requires an error, no result, and permits a null terminal sequence
  when validation or execution failed before any terminal store effect. `idempotency_key` is not
  declared inline UNIQUE; only `uidx_read_requests_idempotency_key` enforces it.
  Classified queue-order and stale-claim indexes are `(state, created_at, request_id)` and
  `(state, claim_heartbeat_at, request_id)`.
- `coord.db.writer_lease` is one optional row keyed by checked resource `store-writer`, with
  non-empty holder id/version, positive PID, integer heartbeat/expiry, and positive fencing token.
  Acquire inserts, heartbeat/takeover CAS-updates, and release deletes it; equal-version live
  displacement remains a Task 5 logic rule, not a schema trigger. Reader pins belong to Ph3 and
  are not in the Ph2b catalog.

**Acceptance:**

- [x] Store and coordinator catalogs match the checked-in authority exactly.
- [x] Every per-version FK is composite; no retained local ID is globally unique by accident.
- [x] Every index is classified; required unique/partial indexes enforce one terminal effect.
- [x] Legacy v3 catalog/version tests remain byte-identical and green.
- [x] Same-epoch compatibility is executable policy, not a documentation-only ledger entry.

### Task 2: Implement store layout and connection invariants

**Files:**

- Create: `crates/julie-extract-artifact/src/store/layout.rs`
- Create: `crates/julie-extract-artifact/src/store/connection.rs`
- Create: `crates/julie-extract-artifact/tests/store_connection_contract.rs`

**Interfaces:**

- `StoreLayout::create(root, family_id, creator_version)` creates `gen-001/store.db`, external
  `coord.db`, `spool/`, `scratch/`, `bases/`, and atomically publishes `CURRENT` last.
- `StoreLayout::open(root)` resolves and validates one generation without following a path
  outside the store root.
- `StoreConnectionFactory` exposes read-only and writer connections with typed floor/schema/
  pragma errors.

**Tests first:** missing/partial `CURRENT`, traversal generation names, symlink escape, wrong
family, unknown schema, reader/writer floors, late `auto_vacuum` no-op detection, and pragma
read-back. Use platform-conditional assertions only where filesystem semantics genuinely differ.

**Implementation:** creation writes a temporary generation name, verifies store pragmas, commits
metadata, fsyncs the database/directory where supported, and renames `CURRENT.partial` to
`CURRENT`. Recovery reaps only scaffolding not named by `CURRENT`; later promotion rules remain
Ph2d.

**Acceptance:**

- [x] No valid open resolves outside the supplied family directory.
- [x] Every writer reasserts FULL durability, FK enforcement, and secure delete.
- [x] A torn first creation is either absent or reopenable; never half-published.
- [x] Below-writer-floor opens read-only; below-reader-floor returns a typed not-ready reason.

### Task 3: Persist immutable versions and per-level rows

**Files:**

- Create: `crates/julie-extract-artifact/src/store/model.rs`
- Create: `crates/julie-extract-artifact/src/store/rows.rs`
- Create: `crates/julie-extract-artifact/src/store/writer.rs`
- Create: `crates/julie-extract-artifact/tests/store_writer_contract.rs`
- Create: `crates/julie-extract-artifact/tests/store_writer_performance.rs`

**Interfaces:**

- `StoreFileVersion::try_from_artifact_file(epoch, &ArtifactFile)` accepts only successful,
  content-hashed extraction and exposes L1/L2/L3 row projections.
- `StoreWriter::write_level(request, version, level)` returns created/reused/incomplete state,
  counts, and the completion log sequence.
- `StoreWriter::lookup_version(path, hash, epoch, required_level)` returns only stamped-complete
  versions for dedup.

**Tests first:** one multi-language fixture covers every row family; shared reference-site level
classification; same local IDs in two versions; composite parent/FK behavior; crash before the
stamp; stamp/last-row atomicity; failed-preserved refusal; existing incomplete-version resume;
statement preparation once per transaction; Symbols-vs-Full L1 projection equality; and one
copy of every parser/capability row per extraction epoch regardless of view count.

**Implementation:** do not reuse v3 SQL or mutate `ArtifactWriter`. Reuse pure serialization and
canonical JSON helpers by extracting them only when both writers need identical value mapping;
SQL statements remain format-specific. `ArtifactFile` metadata JSON is already canonicalized by
the v2.30 extraction mapping; the store projection persists those canonical strings and must not
deserialize into unordered maps and reserialize them. L1 writes files-pure rows, symbols, annotations,
relationships, pending relationships, type facts, complexity, diagnostics, and L1 reference
sites. L2 writes identifiers and remaining reference sites. L3 writes regions, structural facts,
type-argument usages/arguments, and literals. Site level is derived from the complete
`ArtifactFile`: any site claimed by a relationship or pending relationship is L1; only
identifier-only sites are L2. L1 inserts only level-1 sites; L2 inserts only level-2 sites and
never rewrites a previously stored L1 site. The first successful write for an extraction epoch
upserts `parser_inventory` and all `language_capability*` rows once for that epoch.

**Acceptance:**

- [x] Equal `(path, hash, epoch)` reuses one `version_id`; changed path/hash/epoch does not.
- [x] A killed level transaction leaves its stamp null; resume rewrites only the incomplete level.
- [x] Two retained versions may reuse every legacy local ID without PK/FK collision.
- [x] Full store rows, qualified by one version, equal the v3 writer's extraction rows before
      resolution and view bookkeeping.
- [x] Symbols and Full extraction produce identical L1 projections on the multi-language gate;
      any mismatch blocks the two-wave import model.
- [x] L1-stamped/L2-incomplete resume inserts only identifier-only reference sites and leaves L1
      reference sites byte-identical.

### Task 4: Add immutable manifests and the store log

**Files:**

- Create: `crates/julie-extract-artifact/src/store/manifest.rs`
- Create: `crates/julie-extract-artifact/src/store/log.rs`
- Create: `crates/julie-extract-artifact/tests/store_manifest_contract.rs`

**Interfaces:**

- `ManifestBuilder` canonicalizes/sorts entries and computes the deterministic manifest hash.
- `ManifestStore::ensure_view(view_id, root)` creates an unbound view once and rejects root drift.
- `ManifestStore::publish(view_id, expected_generation: Option<u64>, entries, request_id)` creates
  one immutable generation and CAS-flips the view pointer in the same transaction. `None` is the
  only valid cold-publish expectation; later publishes compare the current generation.
- `StoreLog` appends effect/progress/terminal records and queries committed-in-fact by request id.

**Tests first:** identical sets hash equally regardless of discovery order; status changes affect
the hash; CAS loses honestly; failed-preserved points at the old version; new failed paths own no
version; update/delete invalidate `resolution_exact_at`; duplicate terminal entry is impossible;
multi-delete removes exactly the named paths from the next generation while older generations
remain readable; import creates a missing view; root mismatch and unknown update/delete view are
stable errors; store-log sequence allocation remains unique and monotonic without a mirrored meta
counter; and two concurrent cold imports yield one first generation plus one bounded loser
recompute.

**Implementation:** manifest rows are immutable after publication. An identical-set publication
may return the existing generation; in Ph2b the view remains `unbound` with
`resolution_exact_at IS NULL`. Every content/status change creates a generation and leaves any
future resolution binding behind. A CAS loser reloads the new head, recomputes its entry delta,
and retries within a fixed bound. Store-log entries share each effect transaction; level stamps,
manifest flips, and the final terminal are separate effects when they occur in separate
transactions.

**Acceptance:**

- [x] A reader pinning an old manifest sees a complete, unchanged entry set after a new publish.
- [x] Each manifest flip is atomic with its non-terminal effect log row; only the final request
      transaction writes the unique terminal row.
- [x] V-2/V-3/V-4 data exists only on manifest entries, never `file_versions`.
- [x] Synthetic path-deletion and multi-delete fixtures pass.

### Task 5: Implement the durable one-shot coordinator

**Files:**

- Create: `crates/julie-extract-artifact/src/store/coordinator.rs`
- Create: `crates/julie-extract-artifact/tests/store_coordinator_contract.rs`
- Create: `crates/julie-extract-artifact/tests/store_coordinator_takeover.rs`
- Modify: `crates/julie-extract-artifact/Cargo.toml` only to add the feature-gated takeover test,
  not a dependency

**Interfaces:**

- `StoreCoordinator::enqueue(request)` deduplicates by idempotency key.
- `try_acquire_or_takeover(holder, version, now)` implements floor-aware lease CAS.
- `drain(executor, policy)` runs interactive bursts and one batch chunk, heartbeating between
  quanta.
- `reconcile(request_id)` treats a terminal `store_log` row as committed-in-fact and progress
  rows as resumable only.

**Tests first:** two submitters/one effect; stale-owner takeover; live equal-version no
displacement; newer compatible writer queues rather than killing; terminal-log/coord tear in
both directions; request-global chunk indices are `0..N` across level waves and resume at `N+1`;
progress-without-terminal resume; requester timeout without request deletion; 32-request/250-ms
burst cap; batch makes progress under an infinite interactive producer; the backlog present when
the holder's own request terminates drains fully while later arrivals are capped by the service
window; clean exit releases the lease; and process A exiting with process B queued either drains B
first or allows takeover within the heartbeat SLA without a duplicate terminal effect.

**Implementation:** `coord.db` transactions and `store.db` transactions remain ordered and
separate. Recovery checks the store terminal anchor before claiming execution. Use injected clock,
pid-liveness, and executor traits for deterministic unit tests; the feature-gated test kills real
processes only within its temporary directory. After its own terminal state, a holder first
snapshots and completes the existing backlog. It then accepts new arrivals only until the bounded
service window expires, stops claiming, finishes its current safe quantum/request, and
releases/expires the lease in a finally-equivalent guard on success, failure, panic, or watchdog
exit. Requester deadlines affect acknowledgment only.

**Acceptance:**

- [x] Every completed request has one terminal store effect and one converged coord state.
- [x] No crash point causes duplicate manifest flips or lost committed effects.
- [x] Interactive and batch maximum-wait invariants are asserted, not inferred from timing logs.
- [x] Store-writer lease exclusivity, stale takeover, and live equal-version non-displacement are
      asserted. The full governor -> store -> sidecar lock-order gate remains Ph3-owned.

### Task 6: Add internal store CLI models and versioned reports

**Files:**

- Modify: `crates/julie-extract-cli/src/main.rs`
- Modify: `crates/julie-extract-cli/src/lib.rs`
- Create: `crates/julie-extract-cli/src/store/mod.rs`
- Create: `crates/julie-extract-cli/src/store/args.rs`
- Create: `crates/julie-extract-cli/src/store/report.rs`
- Create: `crates/julie-extract-cli/tests/store_cli_contract.rs`

**Interfaces:**

- Define internal clap models for `StoreArgs` + `StoreCommand::Import` and shared request controls.
- `StoreCommandOutcome` renders versioned JSON/human output and stable exit codes.

**Tests first:** exact import form through a test-only parser root; required create-family behavior;
IDs/timeouts; unknown future subcommands rejected; report serialization; stdout/stderr purity; path
and identifier length bounds; and legacy command help/parse/report snapshots unchanged.

**Implementation:** compile the store args/report modules but do not add `Command::Store` to the
top-level CLI in Task 6. This avoids a parsed public command without a durable executor. Task 7
wires import only when its production path exists; Task 8 adds update/delete only with theirs.

**Acceptance:**

- [x] The final import form parses through the internal contract; update/delete/future verbs do
      not yet parse and no top-level store verb exists.
- [x] Legacy CLI contract tests pass unchanged.
- [x] Reports always name request, family, view, state, and exact failure class.
- [x] No public success or not-implemented stub exists.

### Task 7: Expose and implement queued `store import`

**Files:**

- Create: `crates/julie-extract-cli/src/store/import.rs`
- Create: `crates/julie-extract-cli/src/store/executor.rs`
- Modify: `crates/julie-extract-cli/src/store/mod.rs` (module wiring and production dispatch after
  Task 6; no placeholder declarations)
- Modify: `crates/julie-extract-cli/src/args.rs` (`Command::Store` + import-only nested surface)
- Modify: `crates/julie-extract-cli/src/commands.rs` (one dispatch-only match arm)
- Modify narrowly: `crates/julie-extract-cli/src/discovery.rs`
- Modify narrowly: `crates/julie-extract-cli/src/extraction.rs`
- Modify narrowly: `crates/julie-extract-cli/src/spool.rs`
- Modify narrowly: `crates/julie-extract-cli/src/progress.rs`
- Modify narrowly: `crates/julie-extract-cli/src/watchdog.rs`
- Create: `crates/julie-extract-cli/tests/store_import_contract.rs`

**Interfaces:**

- Wire the top-level `store import` command only after its production executor exists.
- `StoreRequestExecutor` dispatches coordinator request kinds and commits one scheduling quantum.
- Import discovery hashes the tree, asks the store for complete versions, and extracts only
  missing required levels.
- Import first calls `ensure_view`; only import may create a view.
- The global L1 wave runs `ExtractionLevel::Symbols`. The final L1 chunk publishes the manifest.
  A full request then performs a hash-guarded `ExtractionLevel::Full` wave, discards its proven-
  equal L1 projection, and commits L2 then L3. Equality is checked at runtime for that version by
  every L1 table's natural key; mismatch returns `l1_projection_mismatch`, writes no Full L1 rows,
  and leaves L2/L3 incomplete. It does not republish an identical manifest.

**Tests first:** cold L1 import; full import with observable L1-before-L2/L3 commits; unchanged
reuse with zero parser calls; epoch change re-extracts; 101-version chunk boundary; WAL-budget
boundary; one failed file preserves prior version; first-time failed path has no version; crash at
each chunk/stamp/manifest/terminal boundary; retry reuses all stamped work; crash after L1
manifest flip but before terminal resumes deepening without re-flipping; source change between
waves keeps the published L1 entry, reports `changed_between_waves`, leaves that version's L2/L3
stamps null, and requires a later request for the new hash; L1 projection mismatch writes no Full
L1 rows and returns the stable error; missing-view creation and root-mismatch refusal.

**Implementation:** extract through existing `ArtifactFile`/spool transport. Scan controls keep
their current semantics. The request executor owns progress records and heartbeats. It publishes
only when every discovered supported path has a usable L1 entry or an honest failed view state.
The two-wave cost is deliberate: it buys extractor-cost L1 visibility. Full deepening rereads
source through `SourceSnapshot` and requires `SourceSnapshot.content_hash` to equal
`file_versions.content_hash` for the target path/version/extraction epoch. A changed snapshot
cannot deepen or repoint the published L1 version in the current request. The Full result's L1
projection is compared to the stored version before any deeper rows are committed. A mid-request
manifest effect is resumable progress, never the terminal idempotency anchor.

**Acceptance:**

- [x] Cold import produces a readable L1 manifest before any L2/L3 completion event.
- [x] Resume reuse equals the number of previously stamped levels at every crash point.
- [x] Full import eventually stamps all requested levels without duplicate version rows.
- [x] Multi-language fixture has non-empty evidence in every applicable row family.
- [x] Full deepening consumes only hash-matching snapshots and cannot mutate the already-stamped
      L1 projection.
- [x] `commands.rs` contains one store dispatch arm and no store business logic.

### Task 8: Implement queued `store update` and `store delete`

**Files:**

- Create: `crates/julie-extract-cli/src/store/update.rs`
- Create: `crates/julie-extract-cli/src/store/delete.rs`
- Modify: `crates/julie-extract-cli/src/store/args.rs` (add Update/Delete only with execution)
- Modify: `crates/julie-extract-cli/src/store/mod.rs` (nested dispatch)
- Create: `crates/julie-extract-cli/tests/store_operations_contract.rs`

**Interfaces:**

- Update appends/reuses one version, publishes one manifest generation, then deepens by policy.
- Delete publishes one manifest generation without touching any version rows.
- Both execute as interactive coordinator requests and use generation CAS.
- Both require an existing view whose bound root matches the request.
- A Full update follows the import wave contract. If the target version lacks L1, it runs Symbols,
  commits/stamps L1, and publishes the new manifest; otherwise it reuses complete L1. It then
  rereads the source, requires its snapshot hash to match the target version, runs Full, compares
  every L1 natural-key set to the stored version, and commits L2 then L3 without rewriting L1.
  `l1_projection_mismatch` leaves L2/L3 incomplete and writes no Full L1 rows. A source changed
  between waves keeps the current L1 manifest entry, reports `changed_between_waves`, and requires
  a later update for the new hash.

**Tests first:** update content change; same-hash no-op; symbols/full policy; concurrent updates to
different files; same-file loser retries from the new generation; delete existing/missing;
delete then branch-style re-add reuses retained version; path rename modeled as delete+update;
failed update preserves old version/status; resolution exactness invalidation; duplicate request
has one effect; Full update exposes L1 before L2/L3, reuses already-complete L1, rejects an L1
projection mismatch without Full L1 writes, and freezes the published L1 entry when source bytes
change between waves.

**Implementation:** share executor/report helpers with import; no copy of discovery/extraction or
manifest code. A CAS loser recomputes its manifest delta against the current generation before a
bounded retry; it never overwrites another request's change.

**Acceptance:**

- [x] Version rows are append-only across update/delete.
- [x] Concurrent disjoint updates converge to a manifest containing both effects.
- [x] Delete never physically removes extraction rows.
- [x] Every content-changing result reports resolution unbound and exact-at mismatch honestly.
- [x] Full update obeys the same hash guard, natural-key equality proof, and no-L1-rewrite rule as
      Full import.

### Task 9: Close Ph2b with crash, equivalence, and floor matrices

**Files:**

- Create: `crates/julie-extract-artifact/tests/store_crash_contract.rs`
- Create: `crates/julie-extract-cli/tests/store_equivalence.rs`
- Create: `crates/julie-extract-cli/tests/store_mixed_version.rs`
- Modify: `crates/julie-extract-artifact/Cargo.toml`
- Modify: `crates/julie-extract-cli/Cargo.toml`
- Modify: `crates/julie-extract-artifact/tests/test_tiers.rs`
- Modify: `crates/julie-extract-cli/tests/perf_gate_convention.rs`
- Modify: `xtask/src/test_tiers.rs`

**Interfaces:** test-only subprocess crash hooks at named durable boundaries; no production
environment variable or hidden public flag.

**Tests first/implementation:** build the matrix around public CLI processes. For equivalence,
run incremental import/update/delete sequences and a from-scratch import of the final tree, then
compare every visible pre-resolution row by natural key and payload, ignoring version surrogate
integers and bookkeeping sequences. Include Rust, C#, TypeScript, Python, SQL, JSON/YAML/Markdown,
and Razor fixtures with at least one multi-delete/path-reuse scenario.

The v3 oracle is an extraction-only `ArtifactWriter` path with no resolution hook/pass. Compare
only the per-version extraction tables; exclude v3 revisions, view bookkeeping, and every
resolution overlay/column explicitly.

**Acceptance:**

- [x] Every crash point reopens with `quick_check=ok` and `foreign_key_check` empty.
- [x] Incremental-converged visible rows equal from-scratch rows for every table in Ph2b scope.
- [x] Older writers are read-only; older readers fail honestly; the downgrade escape hatch is
      explicit and tested without lowering stored floors.
- [x] Feature-gate convention keeps the default tier free of process/crash/large fixtures.
- [x] Existing legacy artifact determinism and compatibility gates remain green.

### Task 10: Document, dogfood, and close the slice

**Files:**

- Modify: `docs/contracts/cli.md`
- Modify: `docs/testing-strategy.md`
- Create: `docs/README.md` (the required base had no documentation map; Task 10 records and closes
  this plan mismatch rather than dropping the file)
- Modify: `docs/plans/2026-08-07-index-store-ph2b-store-kernel-plan.md`
- Create: `docs/release-evidence/2026-08-07-index-store-ph2b/README.md`

**Work:** run the branch gate, then dogfood a temporary store over this repo plus the Miller repo.
Exercise two views with shared files, L1-first full import, 20 mixed updates/deletes, one killed
batch with takeover, and a final from-scratch equivalence comparison. Record commands, commits,
row/version reuse, request counts, manifest generations, WAL peak, L1-visible time, full time,
and all gate results. Generated databases/logs stay under `target/`.

**Completion evidence (2026-08-08):** the runtime under dogfood was
`6a61b6e8832ab935830cd8bd0e1a19aa6f57f7a6`; that commit also contains the test-only parallel
crash-fixture isolation repair found by this task's exact default-parallel feature command. The
dogfood used Julie Extractors `6a61b6e8832ab935830cd8bd0e1a19aa6f57f7a6` and Miller
`b7df7db2f775657912c90df5067ceb7fee985db0` through disposable `git archive` roots. It finished
with 25 committed terminal requests, one honestly failed and replaced Full request, zero
nonterminal requests, zero duplicate terminal effects/chunks, and zero visible-row mismatches
across 21 normalized groups in each of two views. Full commands and persisted facts are recorded
in [the Ph2b release evidence](../release-evidence/2026-08-07-index-store-ph2b/README.md).
The original file ledger said to modify `docs/README.md`, but that file did not exist at the
required base. Lead review authorized creating the missing documentation map; no planned file was
silently omitted.

**Acceptance:**

- [x] Branch gate and both feature-gated store gates pass from the final commit.
- [x] Dogfood records zero duplicate terminal effects and zero equivalence mismatches.
- [x] Docs state that Ph2b is an unreleased implementation slice and name Ph2c/Ph2d remaining
      work; no README/install claim says Miller uses the store yet.
- [x] Worktree state is clean at closeout and the plan ledger records actual commits/tests/mismatches.
- [x] No version bump, tag, push, release, Miller pin bump, or worktree cleanup occurs without the
      user's separate approval.

## Plan Review Record

- Local architecture review classified the slice as high risk and required a pre-execution doubt
  pass against the frozen v4 contract.
- Grok 4.5 cycle 1 returned 12 findings; each was verified and folded into the plan.
- Grok 4.5 cycle 2 confirmed those closures and returned eight precision findings; each was
  verified and folded into the plan.
- Grok 4.5 cycle 3 approved the corrected plan with no findings and no remaining design decision.
  Review session: `019fde23-5b1e-70a0-b79e-4ea6889c4e3e`.
- Repository guidance declares no external-model policy; the plan contains no secrets or private
  customer data and was sent to xAI for these read-only reviews.

## Definition of Done

Ph2b is complete only when a public Julie Extractors CLI process can create/open one family
store, queue and durably execute import/update/delete requests, serve immutable L1-first view
manifests over deduplicated version-qualified rows, resume every named crash point without a
duplicate effect, and prove visible extraction-row equivalence to a fresh build across the
multi-language fixture. Resolution remains honestly unbound. Ph2 remains open until Ph2c and
Ph2d ship and the final release is adopted by Miller. In particular, Ph2b does not satisfy the
frozen header's G3b carry; Ph2c must re-measure it on Rust own-file resolution output.
