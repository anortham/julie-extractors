# JSON Reports

## Scope

JSON reports are the stable machine-readable result for `julie-extract`
commands. They summarize command status, artifact versions, counts, and typed
errors.

Reports do not include complete source file contents. They may report file paths,
metadata, counts, warnings, and typed errors.

Every command accepts `--json`. Without `--json`, output is human-facing and not
part of this contract.

## Report Shape

```json
{
  "report_schema_version": 3,
  "status": "ok",
  "operation": "scan",
  "mode": "incremental",
  "input": {
    "db_path": "/tmp/code.sqlite",
    "root_path": "/repo",
    "file_path": null,
    "root_relative_path": null,
    "format": null,
    "output_path": null
  },
  "artifact": {
    "db_path": "/tmp/code.sqlite",
    "root_path": "/repo",
    "artifact_id": "01hz...",
    "schema_version": 4,
    "extract_contract_version": 3,
    "sqlite_schema_version": 4,
    "jsonl_schema_version": null,
    "hash_algorithm": "blake3",
    "parser_inventory_fingerprint": "sha256:...",
    "capability_snapshot_fingerprint": "sha256:..."
  },
  "tool": {
    "binary_name": "julie-extract",
    "binary_version": "2.0.0"
  },
  "revision": {
    "latest_revision_id": 7,
    "created_revision_id": 7
  },
  "counts": {
    "files_scanned": 10,
    "files_changed": 2,
    "files_unchanged": 8,
    "files_unsupported": 0,
    "files_deleted": 0,
    "files_failed": 0,
    "rows_written": {
      "artifact_metadata": 10,
      "parser_inventory": 36,
      "language_capabilities": 36,
      "language_capability_fixtures": 72,
      "language_capability_gaps": 0,
      "extraction_revisions": 1,
      "revision_file_changes": 2,
      "files": 2,
      "symbols": 12,
      "symbol_annotations": 0,
      "identifiers": 30,
      "relationships": 4,
      "pending_relationships": 2,
      "type_facts": 3,
      "type_argument_usages": 0,
      "type_arguments": 0,
      "literals": 1,
      "source_regions": 4,
      "structural_facts": 1,
      "complexity_metrics": 2,
      "parse_diagnostics": 0
    },
    "totals": {
      "artifact_metadata": 10,
      "parser_inventory": 36,
      "language_capabilities": 36,
      "language_capability_fixtures": 72,
      "language_capability_gaps": 0,
      "extraction_revisions": 7,
      "revision_file_changes": 24,
      "files": 100,
      "symbols": 2400,
      "symbol_annotations": 120,
      "identifiers": 12000,
      "relationships": 900,
      "pending_relationships": 80,
      "type_facts": 14,
      "type_argument_usages": 5,
      "type_arguments": 8,
      "literals": 6,
      "source_regions": 300,
      "structural_facts": 12,
      "complexity_metrics": 200,
      "parse_diagnostics": 0
    },
    "file_rows_truncated": false,
    "file_rows": [
      {
        "path": "src/main.rs",
        "language": "rust",
        "status": "indexed",
        "total_rows": 61,
        "rows": {
          "artifact_metadata": 0,
          "parser_inventory": 0,
          "language_capabilities": 0,
          "language_capability_fixtures": 0,
          "language_capability_gaps": 0,
          "extraction_revisions": 0,
          "revision_file_changes": 0,
          "files": 1,
          "symbols": 12,
          "symbol_annotations": 0,
          "identifiers": 30,
          "relationships": 4,
          "pending_relationships": 2,
          "type_facts": 3,
          "type_argument_usages": 0,
          "type_arguments": 0,
          "literals": 1,
          "source_regions": 4,
          "structural_facts": 1,
          "complexity_metrics": 2,
          "parse_diagnostics": 1
        }
      }
    ]
  },
  "profile": {
    "total_duration_ms": 1234,
    "phases": {
      "existing_artifact": 4,
      "discovery": 18,
      "extraction_spool": 621,
      "writer_open": 2,
      "artifact_write": 581
    },
    "languages": {
      "rust": {
        "files": 10,
        "changed_files": 2,
        "unchanged_files": 8,
        "failed_files": 0,
        "bytes": 42122,
        "read_duration_ms": 5,
        "extract_duration_ms": 509,
        "spool_write_duration_ms": 7
      }
    }
  },
  "errors": [],
  "warnings": []
}
```

Fields:

- `report_schema_version`: report shape version, always `3` for this contract.
- `status`: one of the CLI status values.
- `operation`: command name.
- `mode`: operation-specific mode such as `incremental`, `force`, `single_file`,
  `read_only`, or `export`.
- `input`: normalized command inputs. Paths are absolute except
  `root_relative_path`.
- `artifact`: artifact path and version metadata when a database is involved.
- `tool`: binary identity.
- `revision`: latest and created revision IDs when known.
- `counts`: command-local counts and artifact totals.
- `profile`: optional command timing data. `scan` emits it once extraction
  profiling is available.
- `errors`: typed errors.
- `warnings`: typed warnings that did not change the exit code.
- `structural_fact_patterns`: additive top-level array published only by the
  `languages` report (omitted from every other command's report). It is the
  structural-fact pattern registry, produced by the same serializer as the
  checked-in `docs/contracts/structural-fact-patterns.json` contract, so
  consumers can validate structural-fact metadata payloads at runtime without
  vendoring the repo file. See that file and
  `crates/julie-extractors/src/base/structural_fact_registry/` for the object
  shape and key catalog (source of truth). This is additive:
  `report_schema_version` stays `3`.

- `languages`: additive top-level object emitted only by artifact-mutating
  commands (`scan`, `update`, `delete`) when a reference-resolution pass ran or
  failed; omitted everywhere else so other report shapes are byte-unchanged.
  This is additive: `report_schema_version` stays `3`. See
  [Reference Resolution Section](#reference-resolution-section).

Commands that do not use an artifact, such as `languages`, set `artifact` and
`revision` to `null`.

## Reference Resolution Section

Schema v4 artifacts run a workspace reference-resolution pass inside the writer
transaction of every mutating command (see `sqlite-schema-v4.md` § Reference
Resolution). Those commands report it under the top-level `languages` key:

```json
"languages": {
  "reference_resolution": {
    "status": "complete",
    "version": 2,
    "last_full_revision": 7,
    "counts": {
      "pending_resolutions": 4618,
      "identifier_resolutions": 62189
    },
    "gated_languages": ["python", "rust"],
    "failed": null,
    "totals": {
      "total": 9000,
      "attempted": 7220,
      "resolved": 6000,
      "ambiguous": 20,
      "missing": 700,
      "no_context": 280,
      "unresolved_pending": 500,
      "unattempted": 1500,
      "span_present": 8500,
      "span_missing": 500
    },
    "origin_totals": {
      "identifier": {
        "total": 7000,
        "attempted": 5220,
        "resolved": 4500,
        "ambiguous": 20,
        "missing": 700,
        "no_context": 280,
        "unresolved_pending": 0,
        "unattempted": 1500,
        "span_present": 6600,
        "span_missing": 400
      },
      "pending_relationship": {
        "total": 1000,
        "attempted": 1000,
        "resolved": 500,
        "ambiguous": 0,
        "missing": 0,
        "no_context": 0,
        "unresolved_pending": 500,
        "unattempted": 0,
        "span_present": 900,
        "span_missing": 100
      },
      "relationship": {
        "total": 1000,
        "attempted": 1000,
        "resolved": 1000,
        "ambiguous": 0,
        "missing": 0,
        "no_context": 0,
        "unresolved_pending": 0,
        "unattempted": 0,
        "span_present": 1000,
        "span_missing": 0
      }
    },
    "by_language": [
      {
        "language": "rust",
        "origin": "identifier",
        "raw_kind": "call",
        "canonical_kind": "calls",
        "tier": 1,
        "method": "tier1_local",
        "outcome": "resolved",
        "span_present": true,
        "count": 8479
      }
    ]
  }
}
```

- `status`: durable resolution status (`complete`, `partial`, or `failed`) —
  the same value written to the `reference_resolution_status` metadata key.
- `version`: resolution contract version; `0` when the pass failed before
  producing a report.
- `last_full_revision`: revision of the last Full pass; `0` when unknown.
- `counts`: overlay rows written by this pass (`pending_resolutions`,
  `identifier_resolutions`).
- `gated_languages`: languages in this pass gated out of tier-2 import
  resolution (sorted).
- `failed`: `null` on success, or the non-fatal resolver error message. A
  failure also emits a `resolution_failed` warning and durable status `failed`;
  the scan itself still commits.
- `totals`: whole-artifact evidence-row totals for total/attempted/resolved/
  ambiguous/missing/`no_context`/unresolved-pending/unattempted rows and span
  presence. They are not unique source-reference counts: one source occurrence
  can produce both an identifier row and a pending-relationship row.
- `origin_totals`: the same totals split by `identifier`, `relationship`, and
  `pending_relationship`, so consumers can select one evidence domain without
  double-counting a source occurrence.
- `attempted`: excludes identifier rows with no resolution attempt and pending
  `imports`/`references` rows, whose canonical evidence is reported but whose
  symbol tier chain is intentionally not run.
- `by_language`: whole-artifact cells grouped by language, evidence origin, raw
  kind, canonical kind, outcome, tier, method, and span presence. Origins are
  `identifier`, `relationship`, and `pending_relationship`. Unresolved pending
  rows and never-attempted identifiers are included in the denominator.

`counts.rows_written` and `counts.totals` are exhaustive for SQLite schema v4
row domains. Commands must emit every key with `0` when that row kind is not
written or not present.

## Per-File Row Attribution

`counts.file_rows` attributes durable extraction rows to source files. Entries
are sorted by descending `total_rows`, then ascending root-relative `path`.

Fields:

- `path`: root-relative source path using `/` separators.
- `language`: canonical language name from the `files` row.
- `status`: file status from SQLite, such as `indexed`, `unsupported`, or
  `failed_preserved`.
- `total_rows`: sum of file-attributed row domains for that file.
- `rows`: exhaustive row-domain counts for the file. Artifact-level and
  revision-level domains are present with `0` because they are not owned by a
  single source file.
- `counts.file_rows_truncated`: `true` when the report includes only the largest
  files. Use `info --json` for the full persisted breakdown.

File-attributed domains are `files`, `symbols`, `symbol_annotations`,
`identifiers`, `relationships`, `pending_relationships`, `type_facts`,
`type_argument_usages`, `type_arguments`, `literals`, `source_regions`,
`structural_facts`, `complexity_metrics`, and `parse_diagnostics`.

## Profile Shape

`profile` is diagnostic data for performance investigation, not a pass/fail
threshold.

Fields:

- `total_duration_ms`: wall-clock time in milliseconds for the profiled command
  span.
- `phases`: command-specific phase timings in milliseconds. `scan` phase keys
  include `existing_artifact`, `discovery`, `extraction_spool`, `writer_open`,
  and `artifact_write` when those phases run.
- `languages`: per-language scan timing and volume data keyed by canonical
  language name.
- `languages.*.files`: supported files considered for that language.
- `languages.*.changed_files`: files extracted and spooled as changed.
- `languages.*.unchanged_files`: files spooled from the incremental unchanged
  path without parser work.
- `languages.*.failed_files`: files represented by failure rows.
- `languages.*.bytes`: source bytes read for the language.
- `languages.*.read_duration_ms`: source read, hash, and UTF-8 decode time.
- `languages.*.extract_duration_ms`: parser and extraction time.
- `languages.*.spool_write_duration_ms`: JSONL spool write time.

## Status Values

- `ok`
- `no_change`
- `unsupported`
- `not_found`
- `partial`
- `failed`

`partial` is an error status for exit-code purposes. It exists so callers can
distinguish a consistent artifact with per-file extraction failures from a
command that failed before producing useful rows.

## Error Shape

```json
{
  "code": "root_mismatch",
  "message": "database root does not match requested root",
  "path": null,
  "root_relative_path": null,
  "recoverable": false,
  "details": {
    "expected_root": "/old",
    "requested_root": "/new"
  }
}
```

Fields:

- `code`: stable machine-readable code.
- `message`: human-readable explanation.
- `path`: original path when a path caused the error.
- `root_relative_path`: normalized relative path when known.
- `recoverable`: whether retrying after caller action can succeed.
- `details`: structured JSON object with code-specific fields.

## Error Codes

Stable report codes:

- `usage_error`: invalid CLI arguments.
- `invalid_path`: path cannot be normalized.
- `file_outside_root`: accepted file path is outside the requested root.
- `file_not_found`: `update` target does not exist.
- `root_mismatch`: artifact is bound to a different root.
- `schema_migration_required`: artifact schema is older under
  `--strict-schema`, or single-file `update`/`delete` requires a
  successful whole-workspace scan because reference evidence is missing, stale,
  or failed. The diagnostic is recoverable and its `details.action` is
  `julie-extract scan`.
- `schema_incompatible`: artifact is newer or otherwise incompatible.
- `contract_incompatible`: extraction contract version is incompatible.
- `db_open_failed`: SQLite artifact could not be opened.
- `db_write_failed`: SQLite transaction failed. Concurrent-writer contention
  surfaces here rather than as a distinct lock-timeout code.
- `unsupported_format`: requested export or output format is unsupported.
- `unsupported_file`: file is ignored or unsupported.
- `read_failed`: source file could not be read.
- `parse_failed`: parser failed for a supported file.
- `data_loss_guard`: preserving known-good rows blocked replacement.
- `export_failed`: JSONL export failed.
- `internal_error`: unexpected implementation failure.

Warnings use the same shape and may use warning-only codes such as
`metadata_missing`, `capability_gap`, `slow_file_skipped`, or
`resolution_upgraded`.
`slow_file_skipped` is emitted by `scan` for otherwise-supported source files
that exceed the extractor's oversized-file limit and are excluded from
extraction.
`resolution_upgraded` is emitted when a whole-workspace scan automatically
re-extracts every supported file to advance the reference-resolution evidence
contract.

Any `reference_resolution_status = failed` blocks later single-file `update`
and `delete` operations with `schema_migration_required`, including a failed
resolver hook during an otherwise routine single-file mutation. Recovery is a
successful `julie-extract scan` of the whole workspace.

## Command Report Requirements

### `scan`

- `operation`: `scan`
- `mode`: `incremental` or `force`
- Must include file counts, row counts, totals, latest revision, and created
  revision when a mutation happened.
- Must include `profile` on successful reports. Write failures after extraction
  should include the partial scan profile available at the failure point.
- Successful reports include a top-N `counts.file_rows` offender summary for
  the persisted artifact.
- Mutating reports include the `languages.reference_resolution` section when a
  resolution pass ran or failed (see
  [Reference Resolution Section](#reference-resolution-section)); `update` and
  `delete` reports include it under the same rule.

### `update`

- `operation`: `update`
- `mode`: `single_file`
- Must include requested paths in `input.file_path` and
  `input.root_relative_path` when path normalization succeeds.
- Unsupported or ignored files return `status: unsupported` and exit `0` after
  stale rows for the path are removed.

### `delete`

- `operation`: `delete`
- `mode`: `single_file`
- Must include requested paths in `input.file_path` and
  `input.root_relative_path` when path normalization succeeds.
- Missing rows return `status: not_found` and exit `0`.

### `info`

- `operation`: `info`
- `mode`: `read_only`
- Must not mutate the artifact.
- Must include metadata, totals, and missing metadata warnings.
- Must include a full `counts.file_rows` breakdown for every persisted file.

### `export`

- `operation`: `export`
- `mode`: `jsonl`
- Must include exported record counts by kind.
- `artifact.jsonl_schema_version` is `4`.

### `languages`

- `operation`: `languages`
- `mode`: `capability_snapshot`
- `artifact`: `null`
- The report includes capability counts and may include the full snapshot under
  a `languages` field.
- Includes the additive top-level `structural_fact_patterns` array (the
  structural-fact pattern registry). It is content-equivalent to
  `docs/contracts/structural-fact-patterns.json` and produced by the same
  registry serializer. This section is unique to the `languages` report;
  `report_schema_version` remains `3`.

## stdout And stderr

- Successful `--json` reports are written to stdout.
- Failed `--json` reports are written to stdout when no other machine stream is
  using stdout.
- `export --out - --json` writes JSONL to stdout and the final report to stderr.
- Human diagnostics may be written to stderr, but machine consumers should rely
  on JSON reports and exit codes.

## Tradeoffs

- **Broad statuses, precise codes:** status remains easy to branch on, while
  error codes carry detail.
- **No Julie analysis fields:** report totals cover artifact rows only. Search,
  embeddings, reference scores, and test quality are downstream concerns.
- **Artifact metadata nested under `artifact`:** avoids old Julie field names
  such as `julie_version` and `workspace_id`.
- **Open decision before implementation:** whether every usage error can return
  JSON when `--json` appears after the invalid token. The target behavior is
  JSON for recognized `--json`; argument-parser failures before that point may
  be text plus exit code `2`.
