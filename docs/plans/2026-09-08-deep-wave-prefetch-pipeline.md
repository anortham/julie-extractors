# Deep-Wave Prefetch Pipeline — Design

Status: proposed, not started. Follows the 2026-09-08 performance audit
(`docs/findings/2026-09-08-performance-audit.md`), which measured the store
import on this repository at 35 s after connection reuse and core-scaled
chunks, against 17 s for a plain scan of the same files.

## Problem

A deep-wave quantum is a serial pair: extract the chunk's files on the worker
pool, then write and commit them on the coordinator thread. While the writer
runs, every worker idles. While the workers run, the writer idles. The chunk
size now equals the worker count, so the extraction half fills the machine,
but the write half still runs alone, and one large file in a chunk holds the
whole chunk.

Raising the chunk size further trades away the properties the quantum exists
for: an interactive update or delete waits behind the whole quantum, a crash
loses the whole quantum's extraction, and every chunk's extracted rows sit in
memory before the write.

## Goal

Overlap extraction of chunk N+1 with the write of chunk N, while keeping:

- the quantum as the unit of commit, progress, and crash recovery;
- the frozen chunk schedule in the durable request;
- the file-identity check on every extraction (`changed_between_waves`);
- bounded memory: at most one prefetched chunk in flight beyond the one
  being written;
- SQLite access on the coordinator thread only.

## Design

### Where the pipeline lives

`StoreRequestExecutor` owns the pipeline, next to the pool it already keeps.
The coordinator stays unchanged: it still calls `execute_quantum` once per
chunk inside one store transaction and commits after it returns.

```text
quantum k                 quantum k+1
├─ take prefetched(k) ───►├─ take prefetched(k+1)
├─ spawn prefetch(k+1)    ├─ spawn prefetch(k+2)
├─ write chunk k          ├─ write chunk k+1
└─ commit                 └─ commit
```

### State

```rust
struct PrefetchedChunk {
    request_id: String,
    chunk_index: u64,
    plan_fingerprint: String,
    files: Vec<PlannedImportFile>,
    extracted: Vec<Result<StoreFileVersion, String>>,
}

struct Prefetch {
    key: (String, u64),
    handle: std::thread::JoinHandle<PrefetchedChunk>,
}
```

The executor holds `Option<Prefetch>`. A prefetch runs on one spawned thread
that drives the existing rayon pool (`map_with_pool`), so the pool is shared
and the coordinator thread never blocks on extraction.

### Quantum flow

1. Resolve the chunk for `next_chunk_index` exactly as today.
2. If a prefetch exists with the same `(request_id, chunk_index)` and the
   same plan fingerprint, join it and use its results. Otherwise discard the
   prefetch and extract inline as today.
3. Before writing, start the prefetch for `chunk_index + 1` if that chunk is
   a deep chunk of the same request. Skip files already complete in the
   store (the same lookup the write loop does today) is not possible before
   the write commits, so the prefetch extracts every planned file in the next
   chunk and the consumer drops results for files that turn out complete.
4. Write and validate as today. `validate_full` compares the extracted L1
   projection against the stored one, so a stale prefetch cannot publish rows
   that disagree with the committed L1 wave.
5. Return the quantum. The coordinator commits.

### Invalidation

A prefetched result is used only when all of these hold at consume time:

- same request id and chunk index;
- same plan fingerprint (the payload hash the executor already validates);
- the request is still claimed by this holder (the coordinator checks the
  lease before commit; the executor does not need its own check);
- each file's content hash still equals the frozen hash. The prefetch reads
  the file and hashes it, so `changed_between_waves` is produced by the
  prefetch itself and surfaces as a per-file failure exactly as today.

Anything else drops the prefetch and extracts inline. A dropped prefetch is
joined and discarded; it is never left running past the request.

### Failure and shutdown

- If the quantum fails, the executor joins and drops the prefetch before
  returning the error. Nothing is committed from a prefetch.
- If the process is killed, the prefetch dies with it. Recovery reads
  committed `request_chunks` and restarts at the next index, as today.
- The parent watchdog abort point stays between quanta; a prefetch in flight
  is joined at the abort.

### Memory bound

At most two chunks of extracted rows exist at once: the one being written and
the one being prefetched. The deep chunk is `max(8, workers)` files, bounded
by the 128 MB projected WAL budget. The prefetch adds one chunk of that size.

### What does not change

- Chunk boundaries, progress rows, terminal effects, manifest publication.
- The L1 wave. Prefetch applies to deep chunks only; the L1 wave already runs
  100 files per quantum and is 9 s of the 35 s here.
- The `changed_between_waves` and `l1_projection_mismatch` contracts.

## Expected gain and how to measure

The deep wave on this repository is about 26 s: roughly 17 s of extraction
and 9 s of writes, serialized. Full overlap would bound the deep wave by the
larger half, about 17 s, so the import would approach 26 s total. Measure
with the audit's workload: paired alternating runs of `store import` against
the previous binary, plus peak RSS from `/usr/bin/time -v` and bytes written
from `/proc/<pid>/io`. Guard the result with the existing
`store_import_contract` chunk tests plus one new test that kills the process
mid-prefetch and proves recovery restarts at the committed chunk.

## Risks

- A prefetch thread holds file content and extraction results across a commit
  boundary; a bug that consumes a stale prefetch would write rows for the
  wrong chunk. The `(request_id, chunk_index, plan_fingerprint)` key and the
  per-file hash check are the guard, and `validate_full` is the backstop.
- Windows: the prefetch reads source files while the writer runs. No handle
  on a store file is involved, so no unlink or rename conflict arises.
- Interactive fairness does not improve: the write half still holds the
  coordinator. It does not get worse either, because the chunk size is
  unchanged.

## Tasks

1. Add `Prefetch` state and `map_with_pool` on a spawned thread; join on
   drop.
2. Consume a matching prefetch in the deep-wave path; extract inline
   otherwise.
3. Start the next prefetch before the write loop.
4. Contract test: prefetch consumed, prefetch invalidated by a changed file,
   crash mid-prefetch recovers at the committed chunk.
5. Measure with the audit workload and record numbers in the findings doc.
