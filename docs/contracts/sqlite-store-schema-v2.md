# SQLite Store Schema v2

Status: frozen Ph2d lifecycle catalog authority.

All ordinary tables are `STRICT`. `store.db` timestamps are canonical RFC 3339 UTC text
(`YYYY-MM-DDTHH:MM:SS[.fraction]Z`, with one to nine fractional digits when present); `coord.db`
times are injected Unix-millisecond integers. Both databases use `PRAGMA user_version = 2`.
Schema-v1 files are refused with the typed `OlderSchema` result before metadata or tables change.

The executable DDL lives in `julie_extract_artifact::store`. The authority fingerprint normalizes
each non-internal `sqlite_master` row with non-null SQL as
`type|name|tbl_name|compact_whitespace(sql)`, orders by `(type, name)`, joins with newline, and
hashes the UTF-8 bytes with SHA-256.

```text catalog-authority
store-catalog-sha256: 8e3c0f254f06824c93307104063b0fef3dc7dc1eedec49d3665807942d410a2e
coordinator-catalog-sha256: 633e93a3a5d162b56248656410c6e4ce849795e067bff02a31dc85ce4328c02d
```

## Store catalog additions

Schema v2 retains every schema-v1 extraction, log, progress, view, and manifest table and adds:

```text
resolution_bases(base_id, manifest_hash, resolver_output_epoch, state, relative_path, identifier_count, pending_count, file_bytes, file_sha256, request_id, created_at, updated_at)
resolution_base_versions(base_id, version_id)
resolution_deltas(view_id, delta_generation, base_id, manifest_generation, manifest_hash, resolver_output_epoch, identifier_replacements, pending_replacements, pending_tombstones, exact_gap_rows, exact_gap_files, exact_gap_json, request_id, created_at)
resolution_identifier_deltas(view_id, delta_generation, version_id, identifier_id, target_version_id, target_symbol_id, tier, confidence, method, outcome, candidates)
resolution_pending_deltas(view_id, delta_generation, version_id, pending_relationship_id, operation, target_version_id, target_symbol_id, tier, confidence, method)
resolution_pins(pin_id, owner_kind, owner_id, view_id, manifest_generation, base_id, delta_generation, expires_at, created_at)
resolution_scope_batches(transition_id, view_id, previous_transition_id, from_manifest_generation, from_manifest_hash, to_manifest_generation, to_manifest_hash, scope_usable, predecessor_manifest_generation, predecessor_manifest_hash, base_id, delta_generation, resolver_output_epoch, change_count, change_hash, request_id, completed_at)
resolution_scope_journal(transition_id, path, change_kind, old_version_id, new_version_id, touched_names_json)
resolution_scope_state(view_id, predecessor_manifest_generation, predecessor_manifest_hash, base_id, delta_generation, resolver_output_epoch, current_manifest_generation, current_manifest_hash, journal_through_transition_id)
```

`manifest_entries` is now:

```text
manifest_entries(view_id, generation, path, language, version_id, status, observed_content_hash, indexed_at, error_class, error_json)
```

Language is non-empty and participates in the length-delimited `julie-store-manifest-v2` hash.
Version-backed entries must match the language stored on their immutable file version.

`views.resolution_state` is `unbound`, `converging`, or `exact`. Unbound rows have no resolution
binding. Converging rows bind a ready base and view delta but have no exact generation. Exact rows
bind both and require `resolution_exact_at = current_generation`. Deferred foreign keys and tuple
triggers prevent a view, delta, or pin from committing with a mismatched base, manifest, or epoch.

Base rows are `building` with null file identity or `ready` with positive bytes and a non-empty
SHA-256. `resolution_base_versions` roots every source version. A delta references a ready base at
the same resolver epoch and one immutable view manifest. Identifier replacement payloads and
pending replace/tombstone payloads are state-coherent. Pins are owned by `reader` or `resolve`; a
delta-bearing pin must match its view, manifest, and base tuple.

Delta triggers require the recorded manifest hash to match the referenced immutable manifest.
View bindings require that delta's manifest generation to be the current view generation, and a
base referenced by a delta cannot be downgraded from ready or moved to another resolver epoch.

Exact resolution publication is one `BEGIN IMMEDIATE` store transaction. It inserts the
`resolution_deltas` row, copies bounded identifier and pending delta windows, validates every
visible target, advances the view from the expected converging tuple to the exact tuple, and appends
the `resolution_exact_published` store-log effect before commit. Claim and writer-lease fencing live
in `coord.db` and are checked immediately before and again during this transaction; there is still
no cross-database transaction.

Resolution base and scratch files have their own checked schema fingerprints, manifest hash,
resolver-output epoch, completed stamp, semantic counts, integrity checks, and SHA-256 file identity.
They contain `identifier_resolutions` and `pending_resolutions`; those tables are intentionally not
added to `store.db`.

`store_meta.generation_state` is seeded as `serving` and may contain only `serving` or `retired`.
Existing-generation opens validate without running schema initialization or changing `user_version`;
the state is the durable store-side half of the root-owned maintenance fence.

`store_meta.resolution_scope_journal_version` is `1`. Its absence is valid on a read-only legacy
schema-v2 store and means no partial scope may be trusted. The first mutating open or manifest
publication installs the additive feature without changing `PRAGMA user_version`. Promotion upgrades
the source before catalog comparison and copies the feature metadata with the rest of `store_meta`.

`resolution_scope_batches.transition_id` is the only transition identity and is never derived from
reusable manifest generations. Each non-no-op flip appends one header linked to the previous header
for that view. A usable header repeats the first exact predecessor manifest/base/delta/epoch tuple,
records a binary-path-ordered child count, and hashes the canonical child payload under
`julie-resolution-scope-changes-v1`. Each child stores one touched path, its `path_added`,
`path_deleted`, or `content_replaced` kind, nullable old/new file-version IDs, and canonical
binary-sorted, deduplicated JSON symbol names from both versions. A missing predecessor, missing
feature history, journal/head discontinuity, or more
than 512 changes writes a scope-unusable header with zero children and the canonical empty-payload
hash. Same-generation no-op reuse writes no header.

## Coordinator lifecycle additions

The root-owned coordinator catalog adds:

```text
request_receipts(request_id, idempotency_key, kind, payload_json, terminal_result_json, terminal_generation_name, terminal_log_sequence, completed_at)
consumer_cursors(consumer_id, generation_name, store_log_sequence, updated_at)
maintenance_intent(resource, run_id, action, source_generation_name, owner_id, owner_pid, fencing_token, heartbeat_at, expires_at, started_at, plan_fingerprint, source_min_writer_version)
family_allocator_marks(allocator_kind, scope_id, high_water, updated_at)
```

Receipts are immutable and independently reserve both request ID and idempotency key. Consumer cursor
sequence/time and family allocator high-water/time values cannot regress. The singleton maintenance
intent uses resource `store-maintenance`, a coherent heartbeat/expiry window, and actions `gc`,
`repair`, `promote`, or `rollback`. Global `file_version` and `store_log` allocator marks use the
empty scope; `manifest_generation` and `resolution_delta_generation` marks use a non-empty view ID.

The schema-v2-only explicit indexes are:

```text
read: uidx_read_resolution_bases_identity(manifest_hash, resolver_output_epoch)
read: idx_read_symbols_symbol(symbol_id, version_id)
read: idx_read_symbols_parent_name(version_id, parent_symbol_id, name, symbol_id)
read: idx_read_type_facts_symbol(version_id, symbol_id, type_fact_id)
read: idx_read_resolution_base_versions_version(version_id, base_id)
read: idx_read_resolution_deltas_base(base_id, view_id, delta_generation)
read: idx_read_resolution_identifier_deltas_target(target_version_id, target_symbol_id, view_id, delta_generation)
gc: idx_gc_resolution_identifier_deltas_version(version_id, view_id, delta_generation, identifier_id)
read: idx_read_resolution_pending_deltas_target(target_version_id, target_symbol_id, view_id, delta_generation)
gc: idx_gc_resolution_pending_deltas_version(version_id, view_id, delta_generation, pending_relationship_id)
read: idx_read_resolution_pins_owner_expiry(owner_kind, owner_id, expires_at, pin_id)
read: idx_read_resolution_pins_bound(view_id, manifest_generation, base_id, delta_generation)
read: idx_read_resolution_scope_batches_view(view_id, transition_id)
read: idx_read_resolution_scope_journal_versions(old_version_id, new_version_id, transition_id)
read: idx_read_resolution_scope_journal_kind(change_kind, transition_id, path)
```

Every schema-v1 index remains present. Primary-key and unique-constraint autoindexes are structural
and are not classified secondary indexes.

## Coordinator catalog

The coordinator retains `requests` and `writer_lease`. Request kinds are now:

```text
import | update | delete | resolve | export | from_artifact
```

Schema v2 adds exactly one coordinator index:

```text
coord: uidx_coord_one_claimed_resolve(kind) WHERE kind = 'resolve' AND state = 'claimed'
```

It permits at most one claimed resolve request per family coordinator. Existing request-state,
idempotency, writer-lease, queue, and stale-claim constraints remain unchanged.

## Exclusions

Resolved semantic rows remain in immutable base files and cumulative delta files; `store.db` does
not add `identifier_resolutions` or `pending_resolutions`. Schema v2 performs no in-place migration,
general GC, network operation, or cross-database transaction.
Ph2d retention must treat `resolution_base_versions`, current view bindings, and unexpired
`resolution_pins` as roots and must not reap request-owned scratch while its claim is live.
