# SQLite Store Schema v1

Status: frozen Ph2b catalog authority.

All ordinary tables are `STRICT`. `store.db` timestamps are canonical RFC 3339 UTC text (`YYYY-MM-DDTHH:MM:SS[.fraction]Z`, with one to nine fractional digits when present); `coord.db` times are Unix-millisecond integers. Both databases use `PRAGMA user_version = 1`.

The executable DDL lives in `julie_extract_artifact::store`. The authority fingerprint normalizes each non-internal `sqlite_master` row with non-null SQL as `type|name|tbl_name|compact_whitespace(sql)`, orders by `(type, name)`, joins with newline, and hashes the UTF-8 bytes with SHA-256.

```text catalog-authority
store-catalog-sha256: 1897879e3cdccc86c7a90bd94e583ea71838e05982c9f218980eb41fa04d4659
coordinator-catalog-sha256: ab35421934961f1caba2404b8638c667cbc4a6525ac3143e3cbd68d414d2ed56
```

## Store table authority

`store.db` contains exactly these 25 ordinary tables:

```text
store_meta(key, value)
file_versions(version_id, path, content_hash, extraction_epoch, language, content_bytes, line_count, metadata_json, complete_l1, complete_l2, complete_l3)
parser_inventory(extraction_epoch, language, parser_package, parser_version, grammar_version, source, metadata_json)
language_capabilities(extraction_epoch, language, parser_package, extensions_json, dependency_status, target_symbols, target_relationships, target_pending_relationships, target_identifiers, target_types, actual_symbols, actual_relationships, actual_pending_relationships, actual_identifiers, actual_types, kind_coverage_json)
language_capability_fixtures(extraction_epoch, language, fixture_name, source_path, expected_path)
language_capability_gaps(extraction_epoch, gap_id, language, capability, status, reason, required_closure, evidence_json)
symbols(version_id, symbol_id, path, language, name, kind, signature, doc_comment, visibility, parent_symbol_id, start_line, start_column, end_line, end_column, start_byte, end_byte, body_start_line, body_start_column, body_end_line, body_end_column, body_start_byte, body_end_byte, body_hash, semantic_group, confidence, content_type, is_test, test_container, test_lifecycle, metadata_json)
symbol_annotations(version_id, annotation_id, symbol_id, annotation, annotation_key, raw_text, carrier, metadata_json)
reference_sites(version_id, reference_site_id, path, language, containing_symbol_id, start_line, start_column, end_line, end_column, start_byte, end_byte, is_exact, provenance, level)
identifiers(version_id, identifier_id, reference_site_id, path, language, name, kind, containing_symbol_id, start_line, start_column, end_line, end_column, start_byte, end_byte, confidence, code_context, metadata_json)
relationships(version_id, relationship_id, reference_site_id, from_symbol_id, to_symbol_id, path, kind, start_line, start_column, end_line, end_column, start_byte, end_byte, confidence, metadata_json)
pending_relationships(version_id, pending_relationship_id, reference_site_id, from_symbol_id, caller_scope_symbol_id, path, kind, target_display_name, target_terminal_name, target_receiver, target_namespace_json, target_import_context, start_line, start_column, end_line, end_column, start_byte, end_byte, confidence, metadata_json)
type_facts(version_id, type_fact_id, symbol_id, language, resolved_type, generic_params_json, constraints_json, is_inferred, metadata_json)
type_argument_usages(version_id, usage_id, identifier_id, path, language, metadata_json)
type_arguments(version_id, type_argument_id, usage_id, parent_type_argument_id, ordinal, type_name)
literals(version_id, literal_id, path, language, literal_text, kind, carrier, arg_position, containing_symbol_id, start_line, start_column, end_line, end_column, start_byte, end_byte, confidence, metadata_json)
source_regions(version_id, source_region_id, path, language, kind, containing_symbol_id, start_line, start_column, end_line, end_column, start_byte, end_byte, metadata_json)
structural_facts(version_id, structural_fact_id, path, language, pattern_id, capture_name, node_kind, containing_symbol_id, start_line, start_column, end_line, end_column, start_byte, end_byte, confidence, metadata_json)
complexity_metrics(version_id, complexity_metric_id, path, language, scope, symbol_id, algorithm_id, covered_lines, covered_bytes, decision_count, loop_count, max_nesting_depth, parameter_count, start_line, start_column, end_line, end_column, start_byte, end_byte, metadata_json)
parse_diagnostics(version_id, diagnostic_id, path, language, kind, message, start_line, start_column, end_line, end_column, start_byte, end_byte, metadata_json)
views(view_id, root, current_generation, resolution_state, resolution_base_id, resolution_delta_generation, resolution_exact_at, created_at, updated_at)
manifests(view_id, generation, manifest_hash, request_id, created_at)
manifest_entries(view_id, generation, path, version_id, status, observed_content_hash, indexed_at, error_class, error_json)
store_log(sequence, request_id, event_kind, view_id, generation, version_id, level, terminal, payload_json, created_at)
request_chunks(request_id, chunk_index, store_log_sequence, level, payload_json, created_at)
```

The only explicit store indexes are:

```text
read: uidx_read_file_versions_identity(path, content_hash, extraction_epoch)
read: idx_read_language_capability_gaps_language(extraction_epoch, language)
gc: idx_gc_symbols_path(version_id, path)
gc: idx_gc_symbols_is_test(version_id, is_test)
gc: idx_gc_symbols_test_container(version_id, test_container)
gc: idx_gc_symbols_test_lifecycle(version_id, test_lifecycle)
read: idx_read_symbols_name_kind(name, kind, version_id)
read: idx_read_symbols_parent(parent_symbol_id, version_id)
gc: idx_gc_symbol_annotations_symbol(version_id, symbol_id)
read: idx_read_reference_sites_containing_symbol(containing_symbol_id, version_id)
read: idx_read_identifiers_name_kind(name, kind, version_id)
read: idx_read_identifiers_containing(containing_symbol_id, version_id)
read: idx_read_identifiers_locator_line(version_id, name, start_line, identifier_id)
read: idx_read_identifiers_locator_span(version_id, name, start_byte, end_byte, identifier_id)
read: idx_read_identifiers_reference_site(reference_site_id, version_id)
read: idx_read_relationships_from(from_symbol_id, version_id)
read: idx_read_relationships_to(to_symbol_id, version_id)
read: idx_read_relationships_kind(kind, version_id)
read: idx_read_relationships_reference_site(reference_site_id, version_id)
read: idx_read_pending_terminal(target_terminal_name, version_id)
read: idx_read_pending_from(from_symbol_id, version_id)
read: idx_read_pending_caller_scope(caller_scope_symbol_id, version_id)
read: idx_read_pending_reference_site(reference_site_id, version_id)
read: idx_read_type_argument_usages_identifier(identifier_id, version_id)
gc: idx_gc_type_arguments_usage(version_id, usage_id)
gc: idx_gc_type_arguments_parent(version_id, parent_type_argument_id)
read: idx_read_literals_containing_symbol(containing_symbol_id, version_id)
gc: idx_gc_source_regions_file_span(version_id, start_byte, end_byte)
gc: idx_gc_source_regions_export_order(version_id, path, start_byte, end_byte, kind, source_region_id)
read: idx_read_source_regions_kind(kind, version_id, start_byte)
read: idx_read_source_regions_symbol(containing_symbol_id, version_id)
gc: idx_gc_structural_facts_file_span(version_id, start_byte, end_byte)
gc: idx_gc_structural_facts_export_order(version_id, path, start_byte, end_byte, pattern_id, capture_name, structural_fact_id)
read: idx_read_structural_facts_pattern_language_path(pattern_id, language, path, version_id)
read: idx_read_structural_facts_symbol(containing_symbol_id, version_id)
gc: idx_gc_complexity_metrics_file_scope(version_id, scope, start_byte)
gc: idx_gc_complexity_metrics_export_order(version_id, path, start_byte, end_byte, scope, symbol_id, complexity_metric_id)
read: idx_read_complexity_metrics_scope_language(scope, language, path, version_id)
read: idx_read_complexity_metrics_symbol(symbol_id, version_id)
gc: idx_gc_diagnostics_path(version_id, path)
read: uidx_read_manifests_hash(view_id, manifest_hash)
read: idx_read_manifest_entries_version(version_id, view_id, generation)
read: uidx_read_store_log_terminal_request(request_id) WHERE terminal = 1
read: idx_read_store_log_request(request_id, sequence)
read: uidx_read_request_chunks_log_sequence(store_log_sequence)
```

Primary-key and unique-constraint autoindexes are structural; they are not secondary indexes and carry no read/GC label.

## Coordinator table authority

`coord.db` contains exactly:

```text
requests(request_id, idempotency_key, kind, payload_json, state, requester_id, requester_deadline, claim_owner, claim_heartbeat_at, terminal_log_sequence, result_json, error_json, created_at, updated_at)
writer_lease(resource, holder_id, holder_version, holder_pid, heartbeat_at, expires_at, fencing_token)
```

Its only explicit indexes are:

```text
read: uidx_read_requests_idempotency_key(idempotency_key)
read: idx_read_requests_queue(state, created_at, request_id)
read: idx_read_requests_stale(state, claim_heartbeat_at, request_id)
```

## Foreign-key authority

- Every extraction child directly references `file_versions(version_id) ON DELETE CASCADE`.
- Every reference to a retained child or self row is `(version_id, local_id) ON DELETE NO ACTION DEFERRABLE INITIALLY DEFERRED`.
- Fingerprint fixture/gap rows reference their epoch-qualified language capability with `ON DELETE CASCADE`.
- A view's nullable current generation targets `(view_id, generation)` and is deferred.
- Manifest entries reference their manifest and use `ON DELETE RESTRICT` for a nullable version.
- `store_log`, `request_chunks`, and `coord.db` contain no cross-database or prunable-row FKs.

The Ph2b denylist is `pending_resolutions`, `identifier_resolutions`, resolution base/delta tables, and reader-pin tables. Resolution state cannot be `ready` or `exact`.
