# Progress File Contract v1

`julie-extract scan --progress-file <path>` appends live progress records while a
scan runs. This document is the consumer contract for that file.

The flag is opt-in. A scan without it opens no progress file, writes nothing, and
behaves exactly as it did before the flag existed.

## Why it exists

A scan spends its long phase growing a temporary extraction spool and only opens
the artifact database near the end. A supervisor that decides a scan is hung by
watching artifact file sizes therefore sees no movement at all for that whole
window, and a healthy scan of a large tree looks identical to a wedged one. On a
74k-file tree the blind window is minutes. This file is the signal for it.

## Path

The file name MUST be one of:

- any name whose extension is `progress` — `scan.progress`, `workspace.progress`;
- the bare dotfile `.progress`.

Case is ignored, so `scan.PROGRESS` and `.Progress` are accepted too. Any other
name is refused at argument time with report code `invalid_path`, before the file
is opened.

This is a data-loss guard, not a naming preference. Creating the progress file
truncates whatever is already at the path, so `--progress-file $ROOT/src/lib.rs`
— one templating slip against the wrong variable — would empty a source file at
argument time. The path is then excluded from discovery, so `files_scanned` stays
internally consistent and the report is `ok`: the file would be destroyed with no
diagnostic at all. Requiring a name nothing else uses rules out the whole class
rather than guarding one instance of it.

The name rule is checked against what the path RESOLVES to, not the spelling
passed on the command line, so a `report.progress` symlink pointing at a source
file is refused rather than followed. A path whose final component is a symbolic
link is then refused outright, even when the target satisfies the name rule: the
link is a second name for a file the caller did not spell out, and creating the
progress file writes straight through it.

The path may also not BE the artifact database or either of its `-wal` / `-shm`
sidecars. That check compares **file identity, not path spelling**: when both
paths exist their device and inode decide it, so a hard link
(`ln artifact.sqlite scan.progress`) is refused even though the two names are not
equal and both satisfy the name rule. The check runs again immediately after the
progress file is created, because when neither file existed beforehand there was
no identity to compare — and on a case-insensitive volume creating
`INDEX.PROGRESS` is what makes it the artifact `index.progress`. A progress file
created by a run that is then refused is removed again, so a rejected argv leaves
nothing at the artifact's path.

**Platform limit.** File identity is exact on Unix (macOS, Linux). On Windows it
is not available: the pinned Rust toolchain gates
`std::os::windows::fs::MetadataExt::volume_serial_number` and `file_index` behind
the unstable `windows_by_handle` feature, and the extractor workspace sets
`unsafe_code = "forbid"`, so the Win32 call behind them cannot be reached
directly either. On Windows the check falls back to a case-insensitive path
comparison, which still refuses `INDEX.PROGRESS` against `index.progress` but
does NOT see a Windows hard link. A `--progress-file` hard-linked to the artifact
on Windows will truncate it.

## Format

Append-only JSONL, UTF-8, one JSON object per line, `\n` terminated. The file is
created (and truncated if it already exists) at argument-parse time, before any
scanning starts, and is appended to for the rest of the scan. It is never
renamed and never rewritten in place. Within one scan it never shrinks;
truncation happens only when a NEW scan is given the same path.

Each record is serialized and written with a single unbuffered write followed by
a flush.

## Guarantees

- **Length is monotonically non-decreasing within a single scan.** A consumer may
  treat "the file got longer" as "the scan advanced" without parsing anything.
  Across scans it is not: the next scan to be handed the same path truncates the
  file at argument time, so the length restarts at zero.
- **A length DECREASE means a new scan truncated the file.** It is never a stall
  and never a regression. A consumer that sums or diffs lengths across polls must
  treat a decrease as a fresh baseline AND as progress — a supervisor that reuses
  one progress path per workspace will see this on every rescan, and reading it
  as "no movement" is the exact false stall this file exists to prevent.
- **A trailing line without a terminating newline is an incomplete tail.**
  Parsers must drop it and read it again on the next poll.
- **A blank line, or one malformed line in the middle of the file, may follow a
  failed write.** A write that fails part-way through a record leaves that record
  truncated; the next record opens with a newline that closes it. Parsers must
  skip lines that do not parse rather than stopping at the first one. Every
  record after a failure still parses.
- **At most one record per second**, and a throttled record is written only when
  a counter actually advanced. Phase entries are unthrottled; a scan enters at
  most six phases.
- **Counters never decrease** across records within one file.
- **The progress file is excluded from discovery.** Writing it inside `--root`
  does not change `counts.files_scanned` or any other report count.
- **A mid-scan write failure never fails the scan.** An unusable path, a path
  whose name is not an accepted `.progress` spelling, and a path that collides
  with the artifact all fail at argument time instead, with report code
  `invalid_path`.

## Record schema

```json
{
  "progress_schema_version": 1,
  "pid": 48213,
  "phase": "extraction_spool",
  "elapsed_ms": 4231,
  "files_discovered": 1786,
  "files_supported": 1786,
  "files_extracted": 1024,
  "files_spooled": 1024
}
```

| Field | Type | Meaning |
| --- | --- | --- |
| `progress_schema_version` | integer | `1` for this contract. |
| `pid` | integer | Process id of the scan writing the file. |
| `phase` | string | Phase the scan has entered. |
| `elapsed_ms` | integer | Milliseconds since the scan started. |
| `files_discovered` | integer | Directory entries walked during discovery. Advances in steps of 256 while the walk runs and is exact once discovery ends. |
| `files_supported` | integer | Supported source files discovery selected. |
| `files_extracted` | integer | Files the extraction workers have finished. |
| `files_spooled` | integer | Files written to the extraction spool. |

Unknown fields may be added in a later version. Consumers must ignore fields they
do not recognize and must not require field order.

## Phases

The phase strings are the same keys the final report uses in `profile.phases`,
so the live signal and the finished report describe the same phases:
`existing_artifact`, `discovery`, `force_metadata`, `extraction_spool`,
`writer_open`, `artifact_write`. The field's initial value before the first phase
entry is `starting`; a scan enters `existing_artifact` before anything can
advance a counter, so `starting` is not observed in practice.

## Limitations in v1

`artifact_write` emits its phase-entry record and nothing more; it is not
row-instrumented. That phase writes continuously to the artifact database, so a
consumer that samples the artifact's `.db`/`-wal`/`-shm` sizes already sees it
move. A scan that wedges strictly inside the artifact write is therefore detected
by the consumer's existing stall window, which is the correct outcome for a
genuine hang.
