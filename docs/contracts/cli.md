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
- SQLite schema: `6`
- JSONL schema: `4`
- Versioned store contract: `1` (unreleased Ph2b)
- Versioned store SQLite schema: `1` (unreleased Ph2b)

The legacy values mirror `EXTRACT_CONTRACT_VERSION` / `SQLITE_SCHEMA_VERSION` in
`crates/julie-extract-artifact/src/schema.rs` and `JSONL_SCHEMA_VERSION` in
`crates/julie-extract-artifact/src/jsonl.rs`. The store values mirror
`STORE_SQLITE_SCHEMA_VERSION` and the frozen contract in
`crates/julie-extract-artifact/src/store/schema.rs`; those constants are the source of truth when
this table drifts.

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
julie-extract scan --root <dir> --db <path> [--force] [--level <symbols|full>] [--ignore-file <path>...] [--jobs <n>] [--spool-dir <path>] [--progress-file <path>] [--parent-pid <pid>] [--strict-schema] [--json]
julie-extract update --root <dir> --db <path> --file <path> [--ignore-file <path>...] [--strict-schema] [--json]
julie-extract delete --root <dir> --db <path> --file <path> [--strict-schema] [--json]
julie-extract info --db <path> [--strict-schema] [--json]
julie-extract export --db <path> --format jsonl --out <path|-> [--strict-schema] [--json]
julie-extract languages [--json]
julie-extract rebind --root <dir> --db <path> [--strict-schema] [--json]

# Unreleased Ph2b store commands
julie-extract store import --store <family-dir> --family <uuid> --root <dir> --view <id> [--level <l1|full>] [--json]
julie-extract store update --store <family-dir> [--family <uuid>] --root <dir> --view <id> --file <path> [--level <l1|full>] [--json]
julie-extract store delete --store <family-dir> [--family <uuid>] --root <dir> --view <id> --file <path>... [--json]
```

The nested `store` surface is an unreleased Ph2b implementation slice. Miller does not use it
yet. Ph2c still owns resolution bases/deltas and exact-generation binding; Ph2d still owns
retention, garbage collection, and repair.

## Shared Flags

- `--db <path>`: SQLite artifact path. Required for artifact commands.
- `--root <dir>`: source root. Required for `scan`, `update`, `delete`, and
  `rebind`. For `rebind` it names the root the artifact is retargeted at rather
  than the root it already records.
- `--file <path>`: target file. Required for `update` and `delete`.
- `--json`: write the stable JSON report to stdout.
- `--strict-schema`: fail instead of migrating an older compatible artifact.
- `--ignore-file <path>`: extra gitignore-style ignore file. Repeatable.

`scan` additionally accepts three opt-in process-lifecycle flags — `--spool-dir`,
`--progress-file`, and `--parent-pid`. They exist for supervisors that run many
concurrent scans, and each one is inert when absent: an invocation without a flag
opens no extra file, starts no thread, and produces the same artifact, report, and
exit code it produced before the flag existed. They are documented under `scan`
below and are not accepted by any other command.

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
- `--root`, `--db`, `--file`, `--ignore-file`, `--spool-dir`, and
  `--progress-file` are canonicalized at the CLI boundary before artifact
  operations run. `--spool-dir` is created when it does not exist;
  `--progress-file` requires an existing parent directory, exactly as `--db` does,
  must use the `.progress` extension, may not itself be a symbolic link, and may
  not BE `--db` or one of its `-wal`/`-shm` sidecars. That last check compares file
  identity rather than path spelling — device and inode on Unix, volume serial and
  file index on Windows — so a hard link to the artifact is refused on every
  supported platform (see [progress-file-v1.md](progress-file-v1.md)).
- Stored file paths are root-relative Unix-style strings.
- `--file` may be absolute or root-relative.
- A file outside `--root` is a typed error.
- One SQLite artifact is bound to one canonical root.
- A root mismatch is a typed error unless `scan --force` rebuilds the artifact
  or `rebind` retargets it. `rebind` is the sanctioned retarget path: it is the
  one command that does not run the root gate, because rewriting the binding is
  what it exists for. After a successful `rebind` the artifact records the new
  root, so every later command naming that root passes the gate normally.
- `delete --file` does not require the source file to still exist.
- `update --file` requires the source file to exist. Missing files should be
  sent to `delete`.

## Command Semantics

### `scan`

Scans the root, extracts supported changed files, deletes artifact rows for
source files that disappeared, and records a new revision only when the artifact
changes. A supported source file larger than 1 MiB is skipped with a
`slow_file_skipped` warning, and any rows it already had are deleted so the
artifact never serves stale symbols for a file that grew past the limit.

`scan --force` rebuilds the artifact contents in one SQLite transaction. It is
the explicit path for a moved root or full re-extraction.

A scan of an artifact with missing, stale, or failed reference-resolution
metadata re-extracts every supported file before advancing the resolution
contract. A successful upgrade emits `resolution_upgraded`. If any source file
cannot be re-extracted, including an oversized file whose rows were removed, or
if the resolver fails, the scan returns `failed` with
`schema_migration_required` and exit code `3`. This applies to both incremental
and `--force` scans.

`--level <symbols|full>` chooses the extraction level for a NEW artifact.
`full` (the default) is the complete extraction — every invocation without the
flag behaves exactly as it did before the flag existed. `symbols` builds the
progressive-indexing symbol core: the identifier walks and text/facts collectors
never run, so `identifiers`, `identifier_resolutions`, `literals`,
`type_argument_usages`, `type_arguments`, `source_regions`, and
`structural_facts` stay empty, uniformly across every supported language, while
`files`, `symbols`, `symbol_annotations`, `relationships`,
`pending_relationships`, `type_facts`, `complexity_metrics`, and
`parse_diagnostics` are identical to a full extraction. The resolution hook
still runs (pending relationships resolve). The chosen level is recorded in the
`index_level` artifact-metadata key and in `artifact.index_level` on every
report.

An artifact's level is fixed when it is first built. A rescan or `update`
without `--level` inherits the recorded level; passing a conflicting `--level`
for an existing artifact — with or without `--force` — is a `usage_error`
(exit 2) whose details carry `artifact_index_level` and
`requested_index_level`. To change level, rebuild into a fresh artifact.
Artifacts written before this flag existed read as `full`.

`--jobs <n>` (alias `-j`) sets the number of parallel extraction workers. `0`
(the default) auto-detects from available cores. Parallelism only affects the
file read + parse + map phase; the SQLite write stays single-writer. Output is
independent of `--jobs`: the artifact, row ordering, report counts, and per-file
failure handling are identical for any worker count.

`--spool-dir <path>` places this scan's extraction spool in the named directory
instead of the system temporary directory, and enables removal of spool files in
that directory that no live scan owns. The directory is created when missing.

A running scan holds an advisory lock for its spool's lifetime on a sibling
`<spool>.lock` sentinel, created and locked before the spool file exists. The
lock never covers the spool's own bytes: file locks are mandatory on Windows, so
a lock taken over the spool would fail the spool's own writer. Removal is decided
by that lock and never by file age or by the process id in the file name: a
locked spool is always kept, and a spool the lock cannot prove unowned is always
kept. Removal runs once at startup, before this scan creates its own spool, and
is best effort — a read-only or foreign-owned directory never fails the scan.

Only a spool with a matching sentinel is ever a removal candidate, so a spool
written without the flag, or by a scan whose sentinel could not be locked, can
never be removed by anyone. Directories may therefore be shared with flagless
scans safely; the cost is that those spools are never cleaned up. On a filesystem
that cannot take the lock at all (some NFS and FUSE scratch mounts return
`ENOLCK`) the scan falls back to a non-candidate spool name in the requested
directory rather than failing: the flag makes concurrent scans safer, and
refusing to run would trade a leak for an outage.

The spool directory is excluded from discovery, so placing it inside `--root`
does not change `counts.files_scanned` — a surviving spool would otherwise be
detected as JSON and extracted as if it were source. The exclusion covers the
whole directory, so a `--spool-dir` inside `--root` drops that subtree from the
scan; when that directory holds anything other than spool files and their
sentinels, the report carries a `spool_dir_excluded` warning naming it, because
the directory is created when missing and the counts alone would look healthy. A
dedicated scratch directory such as `$ROOT/.spool` excludes nothing and is
silent. A scan whose spool directory could not carry an ownership lock warns with
`spool_lock_unavailable`. Both warnings appear on every report a scan emits,
including a failed one, because they describe how the scan was configured rather
than what it found.

Accepted limit: `flock` is node-local, and network filesystems emulate it per
node rather than across the cluster. Two machines sharing one `--spool-dir` over
NFS can each believe they own a sentinel, leaving the minimum-age veto as the
only guard. Give each machine its own spool directory.

Without the flag the spool goes to the system temporary directory with no
sentinel and nothing is ever removed, which is what a scan did before the flag
existed.

`--progress-file <path>` appends live progress records to the named file while the
scan runs, so a supervisor can tell a healthy long scan from a wedged one during
the phase before the artifact database is opened. The file name MUST either have
the extension `progress` (`scan.progress`) or be the bare dotfile `.progress`;
case is ignored, and anything else is refused at argument time with
`invalid_path`. Creating the progress file truncates it, so without that rule a
templating slip against the wrong variable — `--progress-file $ROOT/src/lib.rs` —
would silently destroy the file it named and still report `ok`. The format is
append-only JSONL:
at most one record per second, written only when a counter or phase advanced, and
each record is one unbuffered write. Within one scan the file length is therefore
monotonically non-decreasing and "the length grew" is a sound advance signal on
its own. A new scan handed the same path truncates it, so a length DECREASE means
a fresh scan started and must be read as a new baseline and as progress, never as
a stall. A trailing line without a newline is an incomplete tail and must be
dropped by parsers; a failed write can also leave one malformed line mid-file,
which parsers must skip rather than stop at. `artifact_write` emits a phase-entry
record only and is not row-instrumented; a consumer watching artifact file sizes
already sees that phase. The progress file is excluded from discovery, so writing
it inside `--root` does not change `counts.files_scanned`. An unusable path fails
at argument time with `invalid_path` before any scanning starts, as does a path
that IS the artifact or one of its `-wal`/`-shm` sidecars — creating the progress
file truncates it, so the collision would destroy the artifact before the scan had
even validated that it could run. That check compares file identity, not path
spelling, so a symlink, a hard link, and a case-variant spelling on a
case-insensitive volume are all refused rather than followed on every supported
platform — device and inode on Unix, volume serial and file index on Windows. A
write failure
mid-scan is swallowed rather than failing the scan. The record schema is
[progress-file-v1.md](progress-file-v1.md).

`--parent-pid <pid>` aborts the scan when the named process stops being this
process's parent. The value MUST be the DIRECT parent's process id; a value that
is not already the direct parent aborts on the first probe, which is a loud
deterministic failure rather than a silent no-op. The kernel is asked who the
parent is now rather than whether a recorded id is still alive, so process-id
reuse cannot defeat it. The poll interval is about two seconds and the abort is
cooperative: the scan stops between extraction chunks or before it opens the
artifact, unwinds normally so its spool file is removed, and returns `failed` with
error code `parent_exited` and exit code `1`. The diagnostic's `details` carry
`expected_parent_pid` and `observed_parent_pid`. Once the artifact write
transaction has started the scan runs to completion; that write is atomic and the
spool must survive until the writer has read it. The flag is Unix-only — `std`
exposes no Windows equivalent — and is accepted and ignored on other platforms so
one caller argv works everywhere.

With `--json`, successful scan reports include a bounded `counts.file_rows`
summary of the largest source files by attributed artifact rows. Use
`info --json` for the full persisted per-file breakdown.

### `update`

Extracts exactly one file.

Outcomes:

- changed supported file: replaces that file's artifact rows.
- unchanged supported file: returns `no_change`.
- oversized supported file (larger than 1 MiB): skipped with a `slow_file_skipped`
  warning and returns `unsupported`. Existing rows for the path are deleted —
  this mirrors `scan`, which also skips the file and removes its rows.
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

### `rebind`

Retargets an existing artifact at a new source root. It rewrites recorded root
and identity metadata only: nothing is copied, nothing is extracted, and no
extracted row is read, written, or deleted.

The intended caller flow is three steps: copy an artifact to a staging path,
`rebind` the copy at the new root, then run an ordinary incremental `scan`
against that root. The scan passes its own root gate because the artifact now
records the new root, and reconciles whatever differs between the two
checkouts. A `rebind` onto a byte-identical checkout leaves that scan with
nothing to do (`no_change`, `counts.files_changed` of `0`).

Metadata effects of a successful retarget:

- `root_path` is rewritten to the canonicalized `--root`.
- `artifact_id` is rewritten to a freshly minted `artifact-<32 lowercase hex>`
  identity. It is random rather than clock-derived, so two artifacts rebound
  from the same copy in the same instant cannot collide, and a consumer keying
  cache invalidation on `artifact_id` sees the change.
- `updated_at` is refreshed to the retarget time. `created_at` is preserved:
  the artifact still dates from the extraction that built it.
- Three additive provenance keys are written: `rebound_from_root`,
  `rebound_from_artifact_id`, and `rebound_at`. `rebound_at` always equals the
  refreshed `updated_at`, so the two never disagree about when the retarget
  happened. Like `index_level`, these are optional metadata: an artifact that
  was never rebound carries none of them, and a reader that does not know them
  reads the artifact exactly as before.
- Everything else is untouched — `binary_version`, both capability
  fingerprints, the reference-resolution keys, `index_level`, every other
  metadata key, and every data table.

All six writes land in one SQLite transaction, so an interrupted `rebind`
leaves the artifact either fully retargeted or metadata-identical, never
half-renamed with a stale identity.

That transaction re-verifies the identity the validation gates ran against: it
re-reads `root_path` and `artifact_id` before writing, and refuses with
`artifact_changed` and exit code `1` if either differs from the validated
value. The staging protocol leaves nothing able to interleave, so this is a
guard for a direct caller racing a scan, a second `rebind`, or a path
replacement rather than a step in the intended flow. For the same reason the
write opens the artifact read-write without SQLite's create flag: an artifact
that vanished between validation and the write fails as `db_open_failed`, never
as a silent re-creation.

Validation order, each step refusing before the next runs:

1. Argument parsing. A missing `--root` or `--db` is a `usage_error` with exit
   code `2`.
2. Path canonicalization of `--root` and `--db`, at the CLI boundary as for
   every other command. An unusable path fails with `invalid_path` before the
   artifact is opened.
3. Artifact open. A missing or unopenable artifact fails with `db_open_failed`
   and exit code `1`; a refused `rebind` never creates an artifact.
4. The version gates every artifact command runs: `schema_migration_required`
   for an older schema under `--strict-schema`, `schema_incompatible`, and
   `contract_incompatible`, each with exit code `3`.
5. The capability-fingerprint gate. An artifact whose recorded
   `parser_inventory_fingerprint` or `capability_snapshot_fingerprint`
   disagrees with the running binary fails with `fingerprint_mismatch` and exit
   code `3`. It was built by a different extractor, so retargeting it would
   serve rows this binary would not produce; the fix is a fresh
   `julie-extract scan`, which the diagnostic's `details.action` names.
6. The committed-revision gate. An artifact with no committed extraction
   revision is a metadata-only shell rather than an index, and fails with
   `no_committed_revision` and exit code `3`. This runs after the fingerprint
   gate on purpose: a shell that fails both should name the more fundamental
   refusal.

The root gate is deliberately not part of that order.

Outcomes:

- New root: `status: ok`, exit code `0`, and `rebind.changed` is `true`.
- Requested root already the recorded root: `status: no_change`, exit code `0`,
  and `rebind.changed` is `false`. Not a single metadata row is written — no
  refreshed `updated_at`, no provenance keys. Asking for the root the artifact
  already records succeeds so a caller that cannot cheaply tell whether the
  copy it just made needs retargeting can ask unconditionally.
- Write failure: `status: failed` with `db_open_failed`, `db_write_failed`, or
  `artifact_changed` and exit code `1`. The transaction rolls back, so the
  artifact's metadata is byte-identical to what it was before. A new identity
  that cannot be generated fails the same way with `internal_error` and exit
  code `1`, before any write.

`rebind` is additive: it introduces no new table, column, or JSONL record, so
the extraction, SQLite, and JSONL versions pinned above are unchanged, and the
CLI contract version stays `1`.

### `store import`, `store update`, and `store delete` (unreleased Ph2b)

These commands target a family store, not a legacy `--db` artifact. `store import` creates the
store when absent, creates a missing view, and binds that view to the canonical root. The caller
must supply the family UUID on creation. `store update` and `store delete` require an existing
store and view; `--family` is optional for them, but a supplied value must match the stored family.

`store import` discovers the whole declared root. `store update` plans exactly one canonical
root-relative file and never rediscovers the tree. `store delete` plans exactly the repeatable
`--file` arguments and does not delete immutable extraction rows. An update whose content hash is
already current and a delete whose path is already absent are semantic no-ops: neither creates a
manifest generation nor duplicates a version or terminal effect.

`--level l1` publishes the symbol/relationship core before deep evidence. `--level full` requires
L1, L2, and L3 completion. A later Full request may deepen immutable versions published by an L1
request without creating a new manifest generation. Every content-changing Ph2b result reports
`resolution.state: "unbound"` and `resolution.exact_at_matches: false`.

All three verbs enqueue a durable coordinator request and wait up to
`--request-timeout-seconds` (default `30`) for acknowledgment. `--request-id` and
`--idempotency-key` are optional; omitted values are minted by the executor. Reusing an
idempotency key with the same canonical request replays its terminal report. Reusing it for a
different request or operation returns `idempotency_conflict`. A requester timeout does not cancel
a lease holder that is safely draining the request.

Store imports default to 100 versions per L1 quantum and 8 versions per Full-deepening quantum;
both remain bounded by the 128 MB projected WAL budget. `MILLER_STORE_CHUNK_VERSIONS=N` applies to
both waves of a newly enqueued request, with `0` meaning one version. Those limits are stored in the
request, so a retry uses the original schedule even when a successor process has different settings.

Store JSON uses its own `report_schema_version: 1`. Stable fields include `operation`, request
identity, family/view/root identity, coordinator state, requested/completed levels, manifest
generation/hash/disposition, row counts, resolution state, failure class, and a nullable error.
The physical format and recovery invariants are frozen in [store-v1.md](store-v1.md) and
[sqlite-store-schema-v1.md](sqlite-store-schema-v1.md).

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

An oversized file is a policy skip rather than a failure, so the guard does not
apply: its rows are removed on both `scan` and `update`. Preserving them would
serve symbols from a version of the file the extractor can no longer read.

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
