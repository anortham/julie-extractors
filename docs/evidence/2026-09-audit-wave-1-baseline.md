# Evidence: Audit Wave 1 Performance Measurements (Baseline vs. Post-Optimization)

## Overview

This document records the baseline performance measurements on the pre-change tree prior to Audit Wave 1 (Hot Path Waste Removal) alongside the post-optimization measurements captured in Task 7 after Tasks 1–6 were completed.

All benchmarks were run on the same host under identical configuration to evaluate the performance impact of Tasks 1–6 (E1, E2, E3, C1, C2, A2, C4).

## Environment

| Field | Baseline (Before) | Post-Optimization (After) |
|---|---|---|
| Repository Worktree | `/home/murphy/source/julie-extractors/.worktrees/audit-1-hot-path-waste` | `/home/murphy/source/julie-extractors/.worktrees/audit-1-hot-path-waste` |
| Branch | `audit-1-hot-path-waste` | `audit-1-hot-path-waste` |
| Commit SHA | `ea7492ef86ae56616082a24945b710fbfd71fc4d` | `4a2ba80a9c8b74a3f36077ff75949d21217e132c` |
| Host / Kernel | Linux prax 7.1.12-200.fc44.x86_64 #1 SMP PREEMPT_DYNAMIC Fri Aug 28 14:00:18 UTC 2026 x86_64 GNU/Linux | Linux prax 7.1.12-200.fc44.x86_64 #1 SMP PREEMPT_DYNAMIC Fri Aug 28 14:00:18 UTC 2026 x86_64 GNU/Linux |
| CPU | 12th Gen Intel(R) Core(TM) i9-12950HX (24 vCPUs, 16 physical cores: 8 P-cores + 8 E-cores) | 12th Gen Intel(R) Core(TM) i9-12950HX (24 vCPUs, 16 physical cores: 8 P-cores + 8 E-cores) |
| Rust Toolchain | rustc 1.97.1 (8bab26f4f 2026-07-14), cargo 1.97.1 (c980f4866 2026-06-30) | rustc 1.97.1 (8bab26f4f 2026-07-14), cargo 1.97.1 (c980f4866 2026-06-30) |
| Measurement Date | 2026-09-04 | 2026-09-04 |

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

---

## Post-Optimization Measurements (Task 7)

Post-optimization measurements captured at commit `4a2ba80a9c8b74a3f36077ff75949d21217e132c` following the implementation of Tasks 1–6.

### Measurement 1: Repository Baseline Performance (`xtask performance baseline`)

Command:
```bash
cargo xtask performance baseline --root . --out-dir target/performance/audit-wave-1-after --binary target/release/julie-extract --runs 3
```

Summary output file: `target/performance/audit-wave-1-after/baseline-summary.json`

#### Per-Run Timing Samples
| Run Index | Scan Duration (ms) | Rescan Duration (ms) | Info Duration (ms) | Export Duration (ms) | Scan Rows/sec |
|---|---|---|---|---|---|
| Run 1 | 15,425 | 462 | 281 | 10,886 | 22,685.30 |
| Run 2 | 15,507 | 464 | 271 | 10,749 | 22,565.29 |
| Run 3 | 15,381 | 472 | 287 | 10,840 | 22,750.16 |

#### Aggregates (3 Runs)
| Metric | Min | Median | Max |
|---|---|---|---|
| Scan Duration (ms) | 15,381.0 | 15,425.0 | 15,507.0 |
| Rescan Duration (ms) | 462.0 | 464.0 | 472.0 |
| Info Duration (ms) | 271.0 | 281.0 | 287.0 |
| Export Duration (ms) | 10,749.0 | 10,840.0 | 10,886.0 |
| Rows per Second | 22,565.29 | 22,685.30 | 22,750.16 |

#### Output Sizes and Counts
- **Files Scanned:** 2,224
- **Rescan Files Unchanged:** 2,224 (0 changed, 0 deleted, 0 failed)
- **Symbols Extracted:** 347,718
- **JSONL Records Total:** 2,364,890
- **SQLite Database Size:** 1,196,650,496 bytes (~1,141.22 MiB)
- **JSONL Export Size:** 1,653,564,442 bytes (~1,576.96 MiB)

#### Detailed Row Totals (SQLite)
| Table / Domain | Rows |
|---|---|
| `artifact_metadata` | 12 |
| `complexity_metrics` | 15,948 |
| `extraction_revisions` | 1 |
| `files` | 2,224 |
| `identifiers` | 451,822 |
| `language_capabilities` | 40 |
| `language_capability_fixtures` | 234 |
| `language_capability_gaps` | 21 |
| `literals` | 918 |
| `parse_diagnostics` | 71 |
| `parser_inventory` | 40 |
| `pending_relationships` | 113,831 |
| `relationships` | 16,395 |
| `revision_file_changes` | 2,224 |
| `source_regions` | 470,062 |
| `structural_facts` | 299,734 |
| `symbol_annotations` | 7,646 |
| `symbols` | 347,718 |
| `type_argument_usages` | 9,939 |
| `type_arguments` | 12,918 |
| `type_facts` | 33,249 |

---

### Measurement 2: Writer Current Schema Performance (`xtask performance writer-current-schema`)

Command:
```bash
cargo xtask performance writer-current-schema --out-dir target/performance/audit-wave-1-writer-after
```

Summary output file: `target/performance/audit-wave-1-writer-after/writer-current-schema-summary.json`

#### Synthetic Workload Input
- Files: 10,000
- Symbols per file: 8
- Identifiers per file: 24
- Source regions per file: 12

#### Performance Results
| Metric | Value |
|---|---|
| Elapsed Write Time | 11,283 ms (11.28 s) |
| Write Throughput | 108,123.91 rows/sec |
| SQLite Artifact Size | 378,384,384 bytes (~360.86 MiB) |
| Transactions Committed | 1 |
| Files Changed | 10,000 |

#### Rows Written Breakdown
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

### Measurement 3: Store Import Wall Clock Performance (`julie-extract store import`)

Command:
```bash
target/release/julie-extract store import \
  --store <temp-store-dir> \
  --family 00000000-0000-0000-0000-000000000001 \
  --root . \
  --view default \
  --json
```

Three full imports were executed against isolated temporary family store directories and timed via high-resolution wall clock timer.

#### Per-Run Timing Samples
| Run Index | Wall Clock Duration (s) | Wall Clock Duration (ms) | Exit Code | Result State |
|---|---|---|---|---|
| Run 1 | 105.645 s | 105,645.31 ms | 0 | committed |
| Run 2 | 104.240 s | 104,239.86 ms | 0 | committed |
| Run 3 | 104.722 s | 104,721.78 ms | 0 | committed |

#### Timing Aggregates (Wall Clock)
| Metric | Seconds | Milliseconds |
|---|---|---|
| **Min** | 104.240 s | 104,239.86 ms |
| **Median** | 104.722 s | 104,721.78 ms |
| **Max** | 105.645 s | 105,645.31 ms |

#### Import Result Metadata (Consistent across all 3 runs)
- `operation`: `import`
- `state`: `committed`
- `requested_level`: `full`
- `completion`: `{"l1": true, "l2": true, "l3": true}`
- `manifest`: `generation: 1`, `disposition: "created"`
- `row_counts`:
  - `file_versions`: 2,211
  - `l1`: 665,084
  - `l2`: 901,450
  - `l3`: 793,571
  - **Total Store Rows**: 2,360,105

---

## Before and After Comparison Tables

### Measurement 1: Repository Baseline Performance Comparison

| Metric | Before Min | Before Median | Before Max | After Min | After Median | After Max | Median Delta | Median Delta (%) |
|---|---|---|---|---|---|---|---|---|
| Scan Duration (ms) | 15,544.0 | 15,588.0 | 16,216.0 | 15,381.0 | 15,425.0 | 15,507.0 | -163.0 ms | **-1.05%** |
| Rescan Duration (ms) | 454.0 | 467.0 | 476.0 | 462.0 | 464.0 | 472.0 | -3.0 ms | **-0.64%** |
| Info Duration (ms) | 285.0 | 287.0 | 288.0 | 271.0 | 281.0 | 287.0 | -6.0 ms | **-2.09%** |
| Export Duration (ms) | 10,539.0 | 10,661.0 | 10,711.0 | 10,749.0 | 10,840.0 | 10,886.0 | +179.0 ms | **+1.68%** |
| Rows per Second | 21,577.72 | 22,446.69 | 22,511.27 | 22,565.29 | 22,685.30 | 22,750.16 | +238.61 rows/s | **+1.06%** |

### Measurement 2: Writer Current Schema Performance Comparison

| Metric | Before (`ea7492ef`) | After (`4a2ba80a`) | Absolute Delta | Delta (%) |
|---|---|---|---|---|
| Elapsed Write Time | 11,610 ms (11.61 s) | 11,283 ms (11.28 s) | -327 ms | **-2.82%** |
| Write Throughput | 105,075.54 rows/sec | 108,123.91 rows/sec | +3,048.37 rows/sec | **+2.90%** |
| SQLite Artifact Size | 378,384,384 bytes (~360.86 MiB) | 378,384,384 bytes (~360.86 MiB) | 0 bytes | **0.00%** |
| Transactions Committed | 1 | 1 | 0 | 0.00% |
| Files Changed | 10,000 | 10,000 | 0 | 0.00% |
| Rows Written | 1,219,930 | 1,219,930 | 0 | 0.00% |

### Measurement 3: Store Import Wall Clock Performance Comparison

| Metric | Before (`ea7492ef`) | After (`4a2ba80a`) | Absolute Delta | Delta (%) |
|---|---|---|---|---|
| Run 1 Duration (s) | 115.166 s | 105.645 s | -9.521 s | **-8.27%** |
| Run 2 Duration (s) | 114.041 s | 104.240 s | -9.801 s | **-8.60%** |
| Run 3 Duration (s) | 110.587 s | 104.722 s | -5.865 s | **-5.30%** |
| Min Duration (s) | 110.587 s | 104.240 s | -6.347 s | **-5.74%** |
| **Median Duration (s)** | **114.041 s** | **104.722 s** | **-9.319 s** | **-8.17%** |
| Max Duration (s) | 115.166 s | 105.645 s | -9.521 s | **-8.27%** |
| Total Store Rows | 2,359,905 | 2,360,105 | +200 | +0.008% |

---

## Executive Summary & Regression Verification

| Benchmark Workload | Metric | Before (Median) | After (Median) | Absolute Delta | Delta (%) | Regression Check (Threshold: ≤ +5.0%) |
|---|---|---|---|---|---|---|
| **Repository Scan** | Scan Duration | 15,588.0 ms | 15,425.0 ms | -163.0 ms | **-1.05%** | PASSED (Improved) |
| **Repository Scan** | Rescan Duration | 467.0 ms | 464.0 ms | -3.0 ms | **-0.64%** | PASSED (Improved) |
| **Repository Scan** | Info Duration | 287.0 ms | 281.0 ms | -6.0 ms | **-2.09%** | PASSED (Improved) |
| **Repository Scan** | Export Duration | 10,661.0 ms | 10,840.0 ms | +179.0 ms | **+1.68%** | PASSED (+1.68% ≤ 5%) |
| **Repository Scan** | Scan Throughput | 22,446.69 rows/s | 22,685.30 rows/s | +238.61 rows/s | **+1.06%** | PASSED (Improved) |
| **Writer Current Schema** | Elapsed Write Time | 11,610 ms (11.61 s) | 11,283 ms (11.28 s) | -327 ms | **-2.82%** | PASSED (Improved) |
| **Writer Current Schema** | Write Throughput | 105,075.54 rows/s | 108,123.91 rows/s | +3,048.37 rows/s | **+2.90%** | PASSED (Improved) |
| **Store Import** | Wall Clock Duration | 114.041 s | 104.722 s | -9.319 s | **-8.17%** | PASSED (Improved -8.17%) |

**Regression Verification Result:**
All median metrics passed. No metric regressed beyond the 5% threshold. In particular, store import saw a **-8.17%** reduction in wall-clock time (~9.32s savings per run) and writer throughput increased by **+2.90%**.

