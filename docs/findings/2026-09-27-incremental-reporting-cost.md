# Incremental report-count cost

## Finding

The 22 whole-artifact `COUNT(*)` queries are measurable but small beside a single-file command on both tested artifacts. Their measured p95 was 0.393 ms on a 48,776-row, 20,119,552-byte artifact and 10.472 ms on a 1,688,470-row, 720,732,160-byte artifact. On the larger artifact, that was 1.92% of update p95 and 1.96% of delete p95. This does not justify changing public report totals, introducing approximate values, or persisting a cache.

| Dataset | Count-loop p95 | Update p95 | Count share | Delete p95 | Count share |
| --- | ---: | ---: | ---: | ---: | ---: |
| CLI `src` (14 files, 48,776 artifact rows) | 0.393 ms | 216.113 ms | 0.18% | 213.481 ms | 0.18% |
| Extractor crate (1,315 files, 1,688,470 artifact rows) | 10.472 ms | 544.041 ms | 1.92% | 534.918 ms | 1.96% |

The count share divides the p95 of the 22-query loop by the corresponding command p95. Each command p95 is the maximum of three measured runs; each count-loop p95 is the maximum of five measured runs (nearest-rank p95 at these sample sizes).

## Workload and method

The source snapshot and release CLI came from commit `e11fe30a016a27a466d67153a7a31582ebd0fafa`. The release binary SHA-256 is `de815345b82921072fa062624ec262bdd90807ff2836453f3991960d9f15ca09`. Both source trees were archived from that commit into scratch storage and scanned with `--force --json` at the default full extraction level. These are actual project source files, not generated or empty rows.

The small root was `crates/julie-extract-cli/src` (14 scanned files); its update target was `args.rs` (201 lines). The large root was `crates/julie-extractors` (1,315 scanned files); its update target was `src/lib.rs` (150 lines). For each update sample, the corresponding scratch source file had one trailing newline appended after the seed scan, causing a real one-file content-hash change. Each timed update and delete used a fresh copy of its seed database; database copying was outside the timed interval. Delete removed that same target from its fresh artifact copy. Each command had one discarded warmup followed by three measured runs.

Commands, with the fixed scratch roots and target paths substituted, were:

```text
$BIN scan --root <root> --db <seed.sqlite> --force --json
$BIN update --root <root> --db <fresh-copy.sqlite> --file <target> --json
$BIN delete --root <root> --db <fresh-copy.sqlite> --file <target> --json
```

The count loop uses the same 22 table names and `SELECT COUNT(*)` statements as `table_totals`, through `rusqlite` 0.40.2 with bundled SQLite 3.53.2. It discards one warmup loop and records five measured loops on each seeded artifact. `EXPLAIN QUERY PLAN` showed covering-index scans for 21 tables and a table scan for the single-row `extraction_revisions` table. On the large artifact, the slowest counts were `reference_sites` (3.67 ms), `identifiers` (3.57 ms), and `pending_relationships` (1.20 ms) in the inspected loop.

| Dataset/action | Measured command wall times (ms) |
| --- | --- |
| Small update | 216.113, 215.884, 215.780 |
| Small delete | 206.070, 213.481, 205.353 |
| Large update | 527.368, 544.041, 535.987 |
| Large delete | 500.181, 529.292, 534.918 |

The count-loop samples (ms) were `0.393, 0.249, 0.242, 0.238, 0.288` for small and `9.634, 9.826, 9.942, 10.472, 9.786` for large. The timed runs took about 32 seconds; no builds or tests ran during the timed samples. The host was Linux x86_64 on an Intel i9-12950HX. A post-run snapshot showed 8 GiB swap in use and 17 GiB available memory, so residual machine noise is possible; the narrow run spreads are the available stability check.

## Limits and reuse

This measures project source artifacts up to 1.69 million rows, not the larger `code-kb` consumer repository. It measures wall time only, not memory. A future comparison can reuse the same source snapshots, commands, count harness, and baseline logs. Rebuild and force-scan the same roots with the changed binary before timing its update/delete path, since the producer identity and extracted rows may change. No after measurement or performance improvement is claimed here.

Scratch assets are under `/home/murphy/source/julie-extractors/target/receiver-facts-tmp/reporting/`:

- `julie-extract-e11fe30a` and `build-baseline.log`: baseline binary and build log.
- `small-cli-scan.json`, `large-scan.json`, and the two selected `*-base.sqlite` files: extraction reports and seed artifacts.
- `run-measurements.py`, `measurement-results.json`, `measurement-driver.log`, and `*-run-*.stdout.json` / `*.stderr.log`: timed runner and command samples.
- `count-bench/`, `count-bench-build.log`, and `*-count-queries.tsv`: disposable exact-query harness, build log, query plans, counts, and timings.
