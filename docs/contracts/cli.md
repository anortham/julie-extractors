# CLI Contract

## Scope

This document defines the first draft contract for the `julie-extract`
process interface.

The CLI is the primary integration surface for non-Rust callers. The Rust crate
can expose richer in-process APIs, but downstream tools must be able to produce
and inspect extraction artifacts by spawning this binary only.

Contract version:

- CLI contract: `1`
- Extraction contract: `1`
- SQLite schema: `1`
- JSONL schema: `1`

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
julie-extract scan --root <dir> --db <path> [--force] [--ignore-file <path>...] [--strict-schema] [--json]
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

`--ignore-file` only narrows the input set. It does not override hard safety
exclusions such as binary files, oversized files, artifact output files, or VCS
storage directories.

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
changes.

`scan --force` rebuilds the artifact contents in one SQLite transaction. It is
the explicit path for a moved root or full re-extraction.

### `update`

Extracts exactly one file.

Outcomes:

- changed supported file: replaces that file's artifact rows.
- unchanged supported file: returns `no_change`.
- unsupported or ignored file: deletes stale rows for that path and returns
  `unsupported`.
- missing file: returns `failed` with `file_not_found`; callers should use
  `delete`.

### `delete`

Deletes rows for exactly one root-relative file path. Missing rows return
`not_found` with exit code `0`.

File watcher integrations should model rename as:

```text
delete old_path
update new_path
```

### `info`

Reads artifact metadata without mutating the database. `info` is the canonical
preflight command for schema version, extraction contract version, root path,
hash algorithm, parser inventory fingerprint, capability snapshot fingerprint,
and row totals.

### `export`

Exports canonical SQLite rows as JSONL.

Only `--format jsonl` is part of v1. `--out -` writes JSONL records to stdout.
When `--json` and `--out -` are both requested, JSONL uses stdout and the JSON
report uses stderr.

### `languages`

Prints the supported language and capability snapshot. It does not require a
source root or SQLite artifact.

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
result caused by parser failure, read failure, or extractor failure. That case
returns `failed` with a typed error and leaves existing rows intact.

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
