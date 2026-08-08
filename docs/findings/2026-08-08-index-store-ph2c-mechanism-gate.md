# Ph2c-a Store Resolution Mechanism Gate

## Verdict

GO. Every fixed Miller-scale pair in every run passed G1, G2, G3a, G3b, G3c, G4, and G5. No average is used by the verdict.

Command:

```text
RUSTUP_TOOLCHAIN=1.97.1 cargo xtask performance store-resolution --runs 3
```

Summary artifact: `target/performance/store-resolution/store-resolution-summary.json`.

## Environment

- Machine: Apple M2 Ultra, 24 logical CPUs, 64 GiB RAM.
- OS: Darwin 25.6.0 arm64.
- Toolchain: rustc 1.97.1, cargo 1.97.1.
- Source HEAD at measurement: `d0cc5849ff3063dd70e50c0beb35bc33bd5eede3` plus the Task 5 working-tree implementation.
- Fixed corpus: 1,538 files, 392,134 live identifier inputs, 89,538 pending relationship inputs, and 10,412 resolved pending outputs before pair-specific rows.
- Window size: 300.

## Per-run results

| Pair | Run | Compute ms | Fresh ms | Diff ms | Delta write ms | Exact ms | Identifier rows | Pending rows | Rows/sec | Delta/fresh | Peak RSS bytes | Base bytes | Delta bytes | Integrity ms |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| miller-mutated | 1 | 5,841 | 10,234 | 352 | 813 | 11,399 | 392,142 | 10,420 | 67,136.11 | 0.19945 | 28,770,304 | 69,554,176 | 11,390,976 | 4,393 |
| miller-unchanged | 1 | 5,812 | 10,197 | 339 | 16 | 10,552 | 392,154 | 10,424 | 67,473.16 | 0.06108 | 18,972,672 | 69,554,176 | 61,440 | 4,385 |
| miller-mutated | 2 | 5,860 | 10,201 | 338 | 795 | 11,334 | 392,142 | 10,420 | 66,918.43 | 0.19334 | 28,721,152 | 69,554,176 | 11,390,976 | 4,341 |
| miller-unchanged | 2 | 5,870 | 10,224 | 342 | 15 | 10,581 | 392,154 | 10,424 | 66,806.47 | 0.06082 | 19,496,960 | 69,554,176 | 61,440 | 4,354 |
| miller-mutated | 3 | 5,788 | 10,178 | 337 | 773 | 11,288 | 392,142 | 10,420 | 67,750.86 | 0.19178 | 28,737,536 | 69,554,176 | 11,390,976 | 4,390 |
| miller-unchanged | 3 | 5,808 | 10,163 | 328 | 17 | 10,508 | 392,154 | 10,424 | 67,519.63 | 0.05940 | 19,513,344 | 69,554,176 | 61,440 | 4,355 |

Every row also recorded `publish_ms=0`, `semantic_differences=0`, `applied_differences=0`, `exact_gap_mismatches=0`, and `foreground_identifier_work=0`. Foreground binding took 1–2 ms. `background_pipeline_ms` equals the listed exact time and remained below the frozen 24,390 ms control.

## Gate ledger

- G1: PASS. Repeat exact builds had zero semantic differences in both tables for all six samples.
- G2: PASS. Persisted base plus delta had zero differences from fresh exact output for all six samples.
- G3a: PASS. The minimum sample was 66,806.47 identifier rows/sec, above 50,000.
- G3b: PASS. The maximum sample ratio was 0.19945, below 0.50.
- G3c: PASS. The maximum time-to-exact was 11,399 ms, below 30,000 ms.
- G4: PASS. Every sample had zero exact-gap mismatches.
- G5: PASS. Foreground identifier work was zero and the maximum background time was 11,399 ms, below 24,390 ms.

## Diagnostic and correction

The first full run was a valid NO-GO: G1, G2, and G4 passed, but throughput was 12,485–12,597 rows/sec, delta/fresh was 0.867–0.895, and exact time was 62,734–63,801 ms. The fixed corpus concentrates 392,134 identifiers under one version. Disjunctive keyset predicates of the form `version_id > ? OR (version_id = ? AND local_id > ?)` made SQLite restart at the beginning of that version for every 300-row page.

All composite-key cursors now use row-value predicates `(version_id, local_id) > (?, ?)`. The correction covers Store candidate, phase, and source-freeze windows plus base and scratch resolution cursors. It does not change policy, ordering, thresholds, denominators, or semantic output.

The Store schema also gains exact locator indexes for line and span lookups. The canonical schema-v1 catalog hash is `1897879e3cdccc86c7a90bd94e583ea71838e05982c9f218980eb41fa04d4659`.

## Hard stop

Task 5 is the Ph2c-a stop point. This finding records the GO verdict; Task 6 was not started in this run.
