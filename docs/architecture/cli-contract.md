# CLI Contract Draft

The CLI is the primary integration surface. It must be stable, scriptable, and
pleasant for non-Rust consumers.

## Command Sketch

```bash
julie-extract scan --root <dir> --db <path> [--force] [--json]
julie-extract update --root <dir> --db <path> --file <path> [--json]
julie-extract delete --root <dir> --db <path> --file <path> [--json]
julie-extract info --db <path> [--json]
julie-extract export --db <path> --format jsonl --out <path|-> [--json]
julie-extract languages [--json]
```

## Path Rules

- `--root`, `--db`, and `--file` accept platform-native input paths.
- The CLI canonicalizes input paths at the boundary.
- Stored file paths are relative Unix-style paths.
- A database is bound to one canonical root.
- A root mismatch is a typed error unless an explicit rebuild path is chosen.
- A file outside the root is a typed error.

## Command Statuses

Every command should return a JSON report in `--json` mode. Reports should
include:

- `status`
- `schema_version`
- `extract_contract_version`
- `binary_version`
- `hash_algorithm`
- counts for scanned, changed, skipped, deleted, failed, and total rows
- `errors[]` with code, message, path, and recoverability

Status values should distinguish:

- `ok`
- `no_change`
- `unsupported`
- `not_found`
- `partial`
- `failed`

## Exit Codes

Draft model:

- `0`: command completed, including no-op statuses.
- `1`: command ran but extraction/artifact operation failed.
- `2`: CLI usage error.
- `3`: incompatible artifact/schema/root.

Exit codes are coarse. JSON error codes are the precise contract.

## Generalization From Miller

Miller has proven these usage patterns matter:

- full scan for a repo
- single-file update after a watcher event
- delete after a watcher event
- read-only info check
- version gating before opening the artifact
- stable machine-readable failures

The new CLI should preserve those usage patterns while removing Julie-specific
server and schema coupling.
