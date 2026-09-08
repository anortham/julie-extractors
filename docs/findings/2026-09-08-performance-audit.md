# Performance Audit Findings — 2026-09-08

Code-level audit for repeated database calls, repeated file reads, and quadratic
passes across the artifact store crate, the CLI store commands, and the
extraction pipeline. Every change below carries a before and after number from
the same workload. Findings without a change are listed with the measurement
that bounds them.

## Workload

- Root: this repository at `0725ae23`, 3,032 tracked files, 2,316 extractable.
- Machine: 24 cores, Linux, scratch on tmpfs.
- Commands: fresh `scan`, fresh `store import --level full`, `store export`.
- Method: one warm-up run, then three timed runs; the median is reported.

## Baseline

| Operation | Wall | Notes |
| --- | ---: | --- |
| fresh `scan` | 17.7 s | user 52 s, sys 1.5 s; reads 0.65 GB, writes 1.5 GB |
| no-change `scan` | 0.6 s | |
| fresh `store import` | 55.2 s | user 66 s, sys 13.6 s; reads 21.8 GB, writes 24.8 GB for a 1.2 GB store |
| `store export` | 42.7 s | user 33 s, sys 9.6 s |
| `store import --from-artifact` | 6.6 s | |

The import read 33 times and wrote 16 times the bytes the scan did for the same
rows. The read syscall count (5.3 million, 4 KB each) showed SQLite paging index
pages through its default 2 MB page cache on every quantum.

## Changes kept

### 1. One in-memory staging schema per quantum

`StoreWriter::l1_projection_matches_in_transaction` opened a fresh in-memory
database and ran the full store schema (about 99 DDL statements) for every file
in the deep wave. It now takes an `L1ProjectionStaging` that the executor
creates once per quantum; each check stages rows in a transaction that rolls
back on drop.

| Metric | Before | After |
| --- | ---: | ---: |
| fresh `store import` | 55.2 s | 52.3 s |

### 2. Bulk page cache for the store writer

The store writer used SQLite's default page cache. The artifact writer already
sizes its cache from physical memory (`bulk_cache_size_kib`). The bulk writer
profile in `store/pragmas.rs` now applies the same cache and `temp_store =
MEMORY`; the routine profile keeps the default cache and default `temp_store`.
Scope: the coordinator applies the bulk profile to every quantum it executes,
including interactive update and delete requests; a direct
`StoreWriter::write_level` call keeps the routine cache and only switches the
WAL autocheckpoint for a bulk request. The cache is per connection, and the
coordinator opens a fresh connection per quantum, so the cache is discarded
between quanta.

| Metric | Before | After |
| --- | ---: | ---: |
| fresh `store import` | 52.3 s | 42.0 s |
| import bytes read | 21.8 GB | 10.2 GB |
| import bytes written | 24.8 GB | 9.9 GB |
| import sys time | 13.6 s | 6.3 s |

### 3. Export uses cached statements and the bulk cache

`store export` compiled a fresh `INSERT` for every row of every table and wrote
through the default page cache. `copy_files` and `copy_version_table` now use
`prepare_cached`, and the export target applies `bulk_cache_size_kib` and
`temp_store = MEMORY`. `bulk_cache_size_kib` is now a public function of
`julie_extract_artifact`.

| Metric | Before | After statements | After cache |
| --- | ---: | ---: | ---: |
| `store export` | 42.7 s | 29.6 s | 23.0 s |
| export sys time | 9.6 s | 9.6 s | 2.2 s |

## Changes tried and reverted

- **`mmap_size` on the bulk writer.** Halved bytes read (10.2 GB to 5.5 GB) but
  wall time did not move (42.0 s to 43.1 s). Reverted.
- **Cached statements in the per-file store write path** (`delete_level_rows`,
  version lookups, the level stamp, store-log append, manifest entries). Removed
  about 90,000 uncached `prepare` calls per import. Six alternating paired runs
  showed no difference (39.9 s versus 40.0 s). Reverted.

## Findings measured but not changed

### Deep-wave quantum size

The deep wave runs 8 files per quantum, each quantum a serial extract-then-write
step. With 24 cores, 8 files cannot fill the machine and one large file
dominates each quantum. `MILLER_STORE_CHUNK_VERSIONS` shows the cost:

| Deep chunk | fresh `store import` |
| ---: | ---: |
| 8 (default) | 41 s |
| 32 | 33 s |
| 64 | 31 s |
| 128 | 27 s |

The default of 8 dates from the phase-2b dogfood run, where a large quantum
outran the lease before overruns became resumable. That constraint no longer
applies to imports: a heartbeat thread renews the lease during a drain, import
is a renewable-quantum kind, and the 4 s quantum limit excludes it. What a
larger chunk still costs is the non-preemptible interval for interactive
requests behind the import, the work lost on a crash before the next commit,
and peak memory, because every chunk's extracted rows are held before the
write and the 128 MB budget is a projection from source bytes, not a measure.

Two caveats on the table above. `MILLER_STORE_CHUNK_VERSIONS` sets both waves,
so the 32 and 64 rows also shrank the L1 chunk from 100; the deep-only effect
is at least as large as shown. And two per-quantum costs confound the result:
the coordinator opens a fresh store connection per quantum, which discards the
page cache, and the executor builds a new rayon pool with 16 MiB worker stacks
per quantum. Reusing both across quanta needs no contract change and should be
measured before the chunk default moves. Raising the default is a contract
change (`docs/contracts/cli.md`) and needs a decision. The design that keeps
small commits while filling the cores is a bounded prefetch: extract chunk N+1
on the pool while chunk N writes, and validate the frozen file identity when
the prefetched result is consumed.

### Every file is extracted twice

The L1 wave extracts symbols only, then the deep wave re-extracts every file at
full level (4,632 extractions for 2,316 files); the second read may be served
from the snapshot cache when the file still fits in it. The L1 wave costs
about 9 s of the 41 s import. This is the two-wave design, not a defect; a
change would cache the full extraction from the L1 wave, at the cost of memory
or spool space.

### Containing-symbol binders

`base/containing_symbol.rs` and `base/source_regions.rs` scan every symbol for
every fact or region. Disabling both binders in an experiment build cut scan
CPU from 52.7 s to 50.0 s and wall from 17.0 s to 16.4 s, so the upper bound
of any fix is about 5% of scan CPU on this repository. Large generated files
with thousands of literals pay more. `ContainingSymbolIndex` exists and is
unused.

### From-artifact import

`load_artifact_file` opens a connection per file and deserializes each child
row twice (`json_object` text to `Value` to the typed struct). The whole
from-artifact import of this repository takes 6.6 s, so the per-file cost is
bounded here; it grows linearly with file count.

## Second review (Codex, read-only)

A second model reviewed the diff and this document. It confirmed the staging
rollback semantics, the export statement reuse, and the busy classification.
It found that the first cut applied `temp_store = MEMORY` to the routine
profile too (fixed: bulk only), that the chunk table conflates both waves
(noted above), and that the lease rationale for 8 was outdated (corrected
above). It added three verified defects, listed first below: the extraction
pool is rebuilt per quantum, the store connection and its cache are reopened
per quantum, and `validate_full` clones the whole extraction per file. It also
noted that `bulk_cache_size_kib` accepts any `i64` while SQLite reads a 32-bit
value, so an out-of-range override fails the store's read-back check.

## Findings verified by reading, not measured

- `store/executor.rs` builds a new rayon pool (16 MiB stacks per worker) for
  every quantum, including quanta with no work.
- `store/coordinator.rs` opens a fresh store writer connection per quantum, so
  the bulk page cache and statement cache start cold every 8 files.
- `store/executor.rs` `validate_full` returns `full.clone()`, copying every
  extracted row of the file once per deep file.

Each item names the loop and the input that drives it. None showed up as a
measurable share of the workloads above.

- `store/executor.rs` re-parses the request payload and rebuilds the chunk plan
  on every quantum, and reloads every prior progress row for the request.
  Payload here is 432 KB over 313 quanta.
- `store/from_artifact.rs` re-hashes the whole source artifact on every quantum
  (`verify_source_identity`).
- `store/coordinator.rs` drain loop re-fetches every pending request per quantum
  and checks `allowed_ids` with a `Vec::contains`. Only matters with many queued
  requests.
- `src/writer.rs` (artifact) loads the whole `files` table, then still queries
  per file; `ensure_data_loss_guard` runs a `COUNT(*)` per file whose result is
  ignored unless the file is `FailedPreserved`.
- `store/generation.rs` `copy_manifest_entries` and `store/manifest.rs`
  `publish_transaction` insert manifest entries with an uncached `execute`.
- `store/maintenance.rs` clones a `String` per manifest entry for a binary
  search and re-canonicalizes the scratch directory per entry.
- `discovery.rs` walks the tree twice (ignore scopes, then files) and re-checks
  every ancestor prefix per file.
- `extraction.rs` copies every scanned file into the snapshot cache during a
  scan, though only `update` and `store import` read it back.
- `.h` files are parsed three times: a C probe, a C++ probe, then extraction.
  This repository has no `.h` files.
- `NormalizedSpan::from_content_range` counts newlines from byte zero per call;
  markup collectors call it once per attribute.
- Markup files are attribute-scanned two or three times by different
  collectors.

## Tooling note

`perf` and `strace` are not installed, and `samply` needs
`perf_event_paranoid` lowered. Phase splits came from `--progress-file`, and
byte and syscall counts from `/proc/<pid>/io`.
