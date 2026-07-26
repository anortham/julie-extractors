# CLI Contract

## Scope

This document defines the first draft contract for the `julie-extract`
process interface.

The CLI is the primary integration surface for non-Rust callers. The Rust crate
can expose richer in-process APIs, but downstream tools must be able to produce
and inspect extraction artifacts by spawning this binary only.

Contract version:

- CLI contract: `1`
- Extraction contract: `4`
- SQLite schema: `5`
- JSONL schema: `4`

These values mirror `EXTRACT_CONTRACT_VERSION` / `SQLITE_SCHEMA_VERSION` in
`crates/julie-extract-artifact/src/schema.rs` and `JSONL_SCHEMA_VERSION` in
`crates/julie-extract-artifact/src/jsonl.rs`; those constants are the source
of truth when this table drifts.

## Invariants

- `julie-extract` owns extraction from source tree to artifact.
- SQLite is the primary durable artifact.
- JSONL is a secondary export and streaming format derived from SQLite.
- Human text output is not a stable contract. JSON reports are stable.
- Commands do not start a daemon, register a workspace, build a search index,
  compute embeddings, run MCP tools, or watch the filesystem.
- Exit codes are coarse. JSON report status and error codes are the precise
  machine contract.

## Commands

```bash
julie-extract scan --root <dir> --db <path> [--force] [--ignore-file <path>...] [--jobs <n>] [--strict-schema] [--json]
julie-extract update --root <dir> --db <path> --file <path> [--ignore-file <path>...] [--strict-schema] [--json]
julie-extract delete --root <dir> --db <path> --file <path> [--strict-schema] [--json]
julie-extract info --db <path> [--strict-schema] [--json]
julie-extract export --db <path> --format jsonl --out <path|-> [--strict-schema] [--json]
julie-extract languages [--json]
```

## Shared Flags

- `--db <path>`: SQLite artifact path. Required for artifact commands.
- `--root <dir>`: source root. Required for `scan`, `update`, and `delete`.
- `--file <path>`: target file. Required for `update` and `delete`.
- `--json`: write the stable JSON report to stdout.
- `--strict-schema`: fail instead of migrating an older compatible artifact.
- `--ignore-file <path>`: extra gitignore-style ignore file. Repeatable.

`--ignore-file` rules take precedence over in-tree `.gitignore` and
`.julieignore` rules. No ignore rule overrides the hard safety exclusions
such as binary files, oversized files, artifact output files, or VCS storage
directories.

## File Selection and Ignore Rules

`scan` and `update` select files through layered ignore rules. All layers use
gitignore pattern syntax. These rules are a stable contract: consumers that
mirror scan behavior (for example a file watcher deciding which change events
to forward) must apply the same layers.

1. **Hard safety exclusions.** Always active and not overridable: VCS storage
   directories (`.git/`, `.hg/`, `.svn/`), dependency and build output
   directories (`node_modules/`, `vendor/`, `target/`, `dist/`, `build/`,
   `.cache/`), `.julie/` and `.memories/`, minified and generated JavaScript
   and TypeScript bundles, files larger than 1 MiB, and the artifact files
   themselves.
2. **`--ignore-file <path>`.** Caller-supplied rules for one invocation.
   Patterns are matched relative to the scan root. These rules are decisive:
   an ignore or whitelist rule here wins over every in-tree rule, so an
   explicit invocation-level exclusion cannot be silently re-included by a
   committed ignore file, and a caller whitelist can re-include a file that
   in-tree rules ignore. This is the integration point for consumer-side
   policy such as vendor-file detection: detect on the consumer side, write
   the result to a file, and pass it here.
3. **In-tree ignore files.** Applied automatically with git semantics. The
   scan honors `.gitignore` and `.julieignore` files in the scan root and in
   subdirectories at any depth (including hidden directories), plus
   `.gitignore` files in ancestor directories up to the enclosing git root
   (so a nested workspace inherits repo-level rules). `.julieignore` carries
   extraction-specific rules a repo owner commits so every consumer gets the
   same exclusions (for example machine-generated files that are tracked in
   git but not worth extracting).

In-tree patterns apply relative to the directory of the ignore file that
declares them, exactly as git treats nested ignore files. Precedence within
the in-tree layer is also git's: a rule in a deeper directory takes
precedence over shallower rules for paths below it, later rules in the same
file win on conflicts, and when `.gitignore` and `.julieignore` exist in the
same directory, `.julieignore` rules win on conflicts. A whitelist pattern
(`!pattern`) can re-include a file a shallower rule ignored, but — exactly as
in git — a file cannot be re-included when one of its parent directories is
excluded, and ignore files inside excluded directories are not read. `scan`
and `update` apply identical selection, so an `update` can never insert rows
for a file a fresh `scan` would not produce.

Symlinked directories are never traversed, for content or for ignore files.
An in-tree ignore file that cannot be read is reported as a non-fatal entry
in the report's `errors` array and its rules are skipped; an unreadable or
invalid `--ignore-file` is a hard CLI error.

## Path Rules

- Input paths use the platform-native path syntax.
- `--root`, `--db`, `--file`, and `--ignore-file` are canonicalized at the CLI
  boundary before artifact operations run.
- Stored file paths are root-relative Unix-style strings.
- `--file` may be absolute or root-relative.
- A file outside `--root` is a typed error.
- One SQLite artifact is bound to one canonical root.
- A root mismatch is a typed error unless `scan --force` rebuilds the artifact.
- `delete --file` does not require the source file to still exist.
- `update --file` requires the source file to exist. Missing files should be
  sent to `delete`.

## Command Semantics

### `scan`

Scans the root, extracts supported changed files, deletes artifact rows for
source files that disappeared, and records a new revision only when the artifact
changes. A supported source file larger than 1 MiB is skipped with a
`slow_file_skipped` warning; its existing rows are preserved rather than deleted.

`scan --force` rebuilds the artifact contents in one SQLite transaction. It is
the explicit path for a moved root or full re-extraction.

A scan of an artifact with missing, stale, or failed reference-resolution
metadata re-extracts every supported file before advancing the resolution
contract. A successful upgrade emits `resolution_upgraded`. If any source file
cannot be re-extracted, including an oversized file whose existing rows are
preserved, or if the resolver fails, the scan returns `failed` with
`schema_migration_required` and exit code `3`. This applies to both incremental
and `--force` scans.

`--jobs <n>` (alias `-j`) sets the number of parallel extraction workers. `0`
(the default) auto-detects from available cores. Parallelism only affects the
file read + parse + map phase; the SQLite write stays single-writer. Output is
independent of `--jobs`: the artifact, row ordering, report counts, and per-file
failure handling are identical for any worker count.

With `--json`, successful scan reports include a bounded `counts.file_rows`
summary of the largest source files by attributed artifact rows. Use
`info --json` for the full persisted per-file breakdown.

### `update`

Extracts exactly one file.

Outcomes:

- changed supported file: replaces that file's artifact rows.
- unchanged supported file: returns `no_change`.
- oversized supported file (larger than 1 MiB): skipped with a `slow_file_skipped`
  warning and returns `no_change`. Existing rows for the file are preserved, not
  deleted — this mirrors `scan`, which also skips the file and keeps its rows.
- unsupported or ignored file: deletes stale rows for that path and returns
  `unsupported`.
- missing file: returns `failed` with `file_not_found`; callers should use
  `delete`.
- missing, stale, or failed reference-resolution metadata: returns `failed`
  with `schema_migration_required` and exit code `3`; run
  `julie-extract scan` for the whole workspace before retrying.

### `delete`

Deletes rows for exactly one root-relative file path. Missing rows return
`not_found` with exit code `0`.

Missing, stale, or failed reference-resolution metadata returns `failed` with
`schema_migration_required` and exit code `3`; run `julie-extract scan` for the
whole workspace before retrying.

File watcher integrations should model rename as:

```text
delete old_path
update new_path
```

### `info`

Reads artifact metadata without mutating the database. `info` is the canonical
preflight command for schema version, extraction contract version, root path,
hash algorithm, parser inventory fingerprint, capability snapshot fingerprint,
row totals, and full per-file row attribution.

### `export`

Exports canonical SQLite rows as JSONL.

Only `--format jsonl` is part of this CLI contract. `--out -` writes JSONL
records to stdout. When `--json` and `--out -` are both requested, JSONL uses
stdout and the JSON report uses stderr.

### `languages`

Prints the supported language and capability snapshot. It does not require a
source root or SQLite artifact.

The `--json` report also carries an additive top-level `structural_fact_patterns`
array: the structural-fact pattern registry, content-equivalent to
`docs/contracts/structural-fact-patterns.json` and produced by the same
serializer. Consumers can read it to validate structural-fact metadata payloads
at runtime without vendoring the repo file. The section is additive and unique to
this report; `report_schema_version` stays `3`. See `docs/contracts/reports.md`
for the field description.

## Status Values

`status` is the broad command result in JSON reports:

- `ok`: command completed and any requested mutation/export/read succeeded.
- `no_change`: command completed and no artifact mutation was needed.
- `unsupported`: requested file is ignored or unsupported; stale rows, if any,
  were removed.
- `not_found`: requested delete target had no artifact rows.
- `partial`: scan committed recoverable results but one or more files failed.
- `failed`: command did not complete the requested operation.

Command-specific detail belongs in `operation`, `mode`, counts, and errors, not
in new status strings.

## Exit Codes

- `0`: command completed, including `no_change`, `unsupported`, and `not_found`.
- `1`: command ran but extraction, export, or artifact operation failed.
- `2`: CLI usage error.
- `3`: incompatible artifact, schema, root, or contract version.

## Error Codes

Error codes are defined in [reports.md](reports.md). They are stable API.

The CLI must preserve the original path when a path-specific error is useful,
and it must include the normalized root-relative path when the path was accepted
under the root.

## Data-Loss Guard

The CLI must not replace known-good rows for a parser-backed file with an empty
result caused by parser failure, read failure, or extractor failure. When a
scan can still commit useful rows for other files, the failing file is written
as `failed_preserved`, its previous symbol rows stay intact, and the command
returns `partial` with exit code `1`. If the failure prevents a useful artifact
operation from committing, the command returns `failed`.

During a reference-resolution contract upgrade, preserved rows have not been
re-extracted under the new contract. Any read failure, parse failure, extractor
failure, or oversized-file skip therefore escalates the scan to `failed` with
`schema_migration_required` and exit code `3`, leaving single-file mutations
blocked until a complete whole-workspace scan succeeds.

An intentional empty supported file may still produce zero symbols when the file
hash changed and extraction completed successfully.

## Tradeoffs

- **Kept from old Julie evidence:** full scan, one-file update, one-file delete,
  root-bound artifact, relative Unix paths, schema preflight, JSON reports,
  no-op statuses, and a data-loss guard.
- **Changed from old Julie evidence:** binary name, server nesting, workspace
  IDs, Julie analysis command, search-derived metrics, and Julie-specific
  ignore names are not part of this product contract.
- **Open decision before implementation:** whether to expose a caller-supplied
  artifact ID flag. The draft uses a generated `artifact_id` in SQLite metadata
  and does not add a CLI flag until a downstream caller needs to provide one.
