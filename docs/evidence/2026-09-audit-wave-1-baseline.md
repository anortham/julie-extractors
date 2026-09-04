# Evidence: Audit Wave 1 Baseline Performance Measurement

## Overview

This document records the baseline performance measurements on the pre-change tree prior to any code changes in Audit Wave 1 (Hot Path Waste Removal). These baseline figures serve as the benchmark against which post-change measurements in Task 7 will be evaluated.

## Environment

| Field | Value |
|---|---|
| Repository Worktree | `/home/murphy/source/julie-extractors/.worktrees/audit-1-hot-path-waste` |
| Branch | `audit-1-hot-path-waste` |
| Baseline Commit SHA | `ea7492ef86ae56616082a24945b710fbfd71fc4d` |
| Host / Kernel | Linux prax 7.1.12-200.fc44.x86_64 #1 SMP PREEMPT_DYNAMIC Fri Aug 28 14:00:18 UTC 2026 x86_64 GNU/Linux |
| CPU | 12th Gen Intel(R) Core(TM) i9-12950HX (24 vCPUs, 16 physical cores: 8 P-cores + 8 E-cores) |
| Rust Toolchain | rustc 1.97.1 (8bab26f4f 2026-07-14), cargo 1.97.1 (c980f4866 2026-06-30) |
| Measurement Date | 2026-09-04 |

## Binary Build

Command:
```bash
cargo build --release -p julie-extract-cli --bin julie-extract
```
Binary: `target/release/julie-extract`

---

## Measurement 1: Repository Baseline Performance (`xtask performance baseline`)

Command:
```bash
cargo xtask performance baseline --root . --out-dir target/performance/audit-wave-1-before --binary target/release/julie-extract --runs 3
```

Summary output file: `target/performance/audit-wave-1-before/baseline-summary.json`

### Per-Run Timing Samples

| Run Index | Scan Duration (ms) | Rescan Duration (ms) | Info Duration (ms) | Export Duration (ms) | Scan Rows/sec |
|---|---|---|---|---|---|
| Run 1 | 16,216 | 476 | 285 | 10,711 | 21,577.72 |
| Run 2 | 15,544 | 467 | 288 | 10,661 | 22,511.27 |
| Run 3 | 15,588 | 454 | 287 | 10,539 | 22,446.69 |

### Aggregates (3 Runs)

| Metric | Min | Median | Max |
|---|---|---|---|
| Scan Duration (ms) | 15,544.0 | 15,588.0 | 16,216.0 |
| Rescan Duration (ms) | 454.0 | 467.0 | 476.0 |
| Info Duration (ms) | 285.0 | 287.0 | 288.0 |
| Export Duration (ms) | 10,539.0 | 10,661.0 | 10,711.0 |
| Rows per Second | 21,577.72 | 22,446.69 | 22,511.27 |

### Output Sizes and Counts

- **Files Scanned:** 2,222
- **Rescan Files Unchanged:** 2,222 (0 changed, 0 deleted, 0 failed)
- **Symbols Extracted:** 347,699
- **JSONL Records Total:** 2,364,686
- **SQLite Database Size:** 1,196,470,272 bytes (~1,141 MiB)
- **JSONL Export Size:** 1,653,406,308 bytes (~1,576 MiB)

### Detailed Row Totals (SQLite)

| Table / Domain | Rows |
|---|---|
| `artifact_metadata` | 12 |
| `complexity_metrics` | 15,949 |
| `extraction_revisions` | 1 |
| `files` | 2,222 |
| `identifiers` | 451,737 |
| `language_capabilities` | 40 |
| `language_capability_fixtures` | 234 |
| `language_capability_gaps` | 21 |
| `literals` | 918 |
| `parse_diagnostics` | 71 |
| `parser_inventory` | 40 |
| `pending_relationships` | 113,847 |
| `relationships` | 16,375 |
| `revision_file_changes` | 2,222 |
| `source_regions` | 470,088 |
| `structural_facts` | 299,705 |
| `symbol_annotations` | 7,651 |
| `symbols` | 347,699 |
| `type_argument_usages` | 9,937 |
| `type_arguments` | 12,914 |
| `type_facts` | 33,249 |

---

## Measurement 2: Writer Current Schema Performance (`xtask performance writer-current-schema`)

Command:
```bash
cargo xtask performance writer-current-schema --out-dir target/performance/audit-wave-1-writer-before
```

Summary output file: `target/performance/audit-wave-1-writer-before/writer-current-schema-summary.json`

### Synthetic Workload Input
- Files: 10,000
- Symbols per file: 8
- Identifiers per file: 24
- Source regions per file: 12

### Performance Results

| Metric | Value |
|---|---|
| Elapsed Write Time | 11,610 ms (11.61 s) |
| Write Throughput | 105,075.54 rows/sec |
| SQLite Artifact Size | 378,384,384 bytes (~360.86 MiB) |
| Transactions Committed | 1 |
| Files Changed | 10,000 |

### Rows Written Breakdown

| Domain | Rows Written |
|---|---|
| `files` | 10,000 |
| `symbols` | 80,000 |
| `symbol_annotations` | 10,000 |
| `identifiers` | 240,000 |
| `relationships` | 70,000 |
| `pending_relationships` | 10,000 |
| `type_facts` | 80,000 |
| `type_argument_usages` | 240,000 |
| `type_arguments` | 240,000 |
| `literals` | 10,000 |
| `source_regions` | 120,000 |
| `structural_facts` | 10,000 |
| `complexity_metrics` | 90,000 |
| `parse_diagnostics` | 10,000 |
| `revision_file_changes` | 10,000 |
| **Total Rows Written** | **1,219,930** |

---

## Measurement 3: Store Import Wall Clock Performance (`julie-extract store import`)

Command:
```bash
target/release/julie-extract store import   --store <temp-store-dir>   --family 00000000-0000-0000-0000-000000000001   --root .   --view default   --json
```

Three full imports were executed against isolated temporary family store directories and timed via high-resolution wall clock timer.

### Per-Run Timing Samples

| Run Index | Wall Clock Duration (s) | Wall Clock Duration (ms) | Exit Code | Result State |
|---|---|---|---|---|
| Run 1 | 115.166 s | 115,165.69 ms | 0 | committed |
| Run 2 | 114.041 s | 114,041.26 ms | 0 | committed |
| Run 3 | 110.587 s | 110,586.92 ms | 0 | committed |

### Timing Aggregates (Wall Clock)

| Metric | Seconds | Milliseconds |
|---|---|---|
| **Min** | 110.587 s | 110,586.92 ms |
| **Median** | 114.041 s | 114,041.26 ms |
| **Max** | 115.166 s | 115,165.69 ms |

### Import Result Metadata (Consistent across all 3 runs)
- `operation`: `import`
- `state`: `committed`
- `requested_level`: `full`
- `completion`: `{"l1": true, "l2": true, "l3": true}`
- `manifest`: `generation: 1`, `disposition: "created"`
- `row_counts`:
  - `file_versions`: 2,209
  - `l1`: 665,063
  - `l2`: 901,280
  - `l3`: 793,562
  - **Total Store Rows**: 2,359,905
