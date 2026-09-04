# Evidence: Audit Wave 1 Performance Measurements (Baseline vs. Post-Optimization)

## Overview

This document records the baseline performance measurements on the pre-change tree prior to Audit Wave 1 (Hot Path Waste Removal) alongside the post-optimization measurements captured after Tasks 1–6 were completed.

To ensure strict fairness and avoid skew from untracked files or working tree drift, both before and after benchmarks were executed against the exact same immutable source tree snapshot (`/tmp/julie-benchmark-snapshot`, exported at commit `ea7492ef86ae56616082a24945b710fbfd71fc4d`), producing identical file counts (2,222) and store version counts (2,209).

All benchmarks were run on the same host under identical configuration to evaluate the performance impact of Tasks 1–6 (E1, E2, E3, C1, C2, A2, C4).

## Environment

| Field | Baseline (Before) | Post-Optimization (After) |
|---|---|---|
| Repository Worktree | `/home/murphy/source/julie-extractors/.worktrees/audit-1-hot-path-waste` | `/home/murphy/source/julie-extractors/.worktrees/audit-1-hot-path-waste` |
| Branch | `audit-1-hot-path-waste` | `audit-1-hot-path-waste` |
| Base Commit SHA | `ea7492ef86ae56616082a24945b710fbfd71fc4d` | `4a2ba80a9c8b74a3f36077ff75949d21217e132c` |
| Benchmark Source Snapshot | `/tmp/julie-benchmark-snapshot` (commit `ea7492ef`) | `/tmp/julie-benchmark-snapshot` (commit `ea7492ef`) |
| Host / Kernel | Linux prax 7.1.12-200.fc44.x86_64 #1 SMP PREEMPT_DYNAMIC Fri Aug 28 14:00:18 UTC 2026 x86_64 GNU/Linux | Linux prax 7.1.12-200.fc44.x86_64 #1 SMP PREEMPT_DYNAMIC Fri Aug 28 14:00:18 UTC 2026 x86_64 GNU/Linux |
| CPU | 12th Gen Intel(R) Core(TM) i9-12950HX (24 vCPUs, 16 physical cores: 8 P-cores + 8 E-cores) | 12th Gen Intel(R) Core(TM) i9-12950HX (24 vCPUs, 16 physical cores: 8 P-cores + 8 E-cores) |
| Rust Toolchain | rustc 1.97.1 (8bab26f4f 2026-07-14), cargo 1.97.1 (c980f4866 2026-06-30) | rustc 1.97.1 (8bab26f4f 2026-07-14), cargo 1.97.1 (c980f4866 2026-06-30) |
| Measurement Date | 2026-09-04 | 2026-09-04 |

## Binary Build

Commands:
```bash
# Baseline binary built from commit ea7492ef:
cargo build --release -p julie-extract-cli --bin julie-extract --target-dir /tmp/julie-before-target

# Post-optimization binary built from current worktree:
cargo build --release -p julie-extract-cli --bin julie-extract
```

---

## Baseline Measurements (Before Optimizations)

### Measurement 1: Repository Baseline Performance (`xtask performance baseline`)

Command:
```bash
cargo xtask performance baseline \
  --root /tmp/julie-benchmark-snapshot \
  --out-dir target/performance/audit-wave-1-before-fair \
  --binary /tmp/julie-before-target/release/julie-extract \
  --runs 3
```

Summary output file: `target/performance/audit-wave-1-before-fair/baseline-summary.json`

#### Per-Run Timing Samples

| Run Index | Scan Duration (ms) | Rescan Duration (ms) | Info Duration (ms) | Export Duration (ms) | Scan Rows/sec |
|---|---|---|---|---|---|
| Run 1 | 21,292 | 527 | 323 | 12,734 | 16,433.91 |
| Run 2 | 20,667 | 475 | 292 | 11,570 | 16,931.23 |
| Run 3 | 16,553 | 473 | 297 | 11,473 | 21,138.68 |

#### Aggregates (3 Runs)

| Metric | Min | Median | Max |
|---|---|---|---|
| Scan Duration (ms) | 16,553.0 | 20,667.0 | 21,292.0 |
| Rescan Duration (ms) | 473.0 | 475.0 | 527.0 |
| Info Duration (ms) | 292.0 | 297.0 | 323.0 |
| Export Duration (ms) | 11,473.0 | 11,570.0 | 12,734.0 |
| Rows per Second | 16,433.91 | 16,931.23 | 21,138.68 |

#### Output Sizes and Counts

- **Files Scanned:** 2,222
- **Rescan Files Unchanged:** 2,222 (0 changed, 0 deleted, 0 failed)
- **Symbols Extracted:** 347,699
- **JSONL Records Total:** 2,364,686
- **SQLite Database Size:** 1,196,470,272 bytes (~1,141.04 MiB)
- **JSONL Export Size:** 1,653,406,226 bytes (~1,576.81 MiB)

#### Detailed Row Totals (SQLite)

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

### Measurement 2: Writer Current Schema Performance (`xtask performance writer-current-schema`)

Command:
```bash
cargo xtask performance writer-current-schema \
  --out-dir target/performance/audit-wave-1-writer-before
```

Summary output file: `target/performance/audit-wave-1-writer-before/writer-current-schema-summary.json`

#### Synthetic Workload Input
- Files: 10,000
- Symbols per file: 8
- Identifiers per file: 24
- Source regions per file: 12

#### Performance Results

| Metric | Value |
|---|---|
| Elapsed Write Time | 11,610 ms (11.61 s) |
| Write Throughput | 105,075.54 rows/sec |
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
/tmp/julie-before-target/release/julie-extract store import \
  --store <temp-store-dir> \
  --family 00000000-0000-0000-0000-000000000001 \
  --root /tmp/julie-benchmark-snapshot \
  --view default \
  --json
```

Three full imports were executed against isolated temporary family store directories and timed via high-resolution wall clock timer.

#### Per-Run Timing Samples

| Run Index | Wall Clock Duration (s) | Wall Clock Duration (ms) | Exit Code | Result State |
|---|---|---|---|---|
| Run 1 | 50.022 s | 50,022.15 ms | 0 | committed |
| Run 2 | 50.957 s | 50,957.40 ms | 0 | committed |
| Run 3 | 49.961 s | 49,961.02 ms | 0 | committed |

#### Timing Aggregates (Wall Clock)

| Metric | Seconds | Milliseconds |
|---|---|---|
| **Min** | 49.961 s | 49,961.02 ms |
| **Median** | 50.022 s | 50,022.15 ms |
| **Max** | 50.957 s | 50,957.40 ms |

#### Import Result Metadata (Consistent across all 3 runs)
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
  - **Total Domain Rows**: 2,362,114 (sum of `file_versions`, `l1`, `l2`, `l3`)
  - **Total Store Rows**: 2,371,901 (including 9,787 rows in metadata/internal tables)

---

## Post-Optimization Measurements

Post-optimization measurements captured against the same immutable source snapshot (`/tmp/julie-benchmark-snapshot`) following the implementation and verification of Tasks 1–6.

### Measurement 1: Repository Baseline Performance (`xtask performance baseline`)

Command:
```bash
cargo xtask performance baseline \
  --root /tmp/julie-benchmark-snapshot \
  --out-dir target/performance/audit-wave-1-after-fair \
  --binary target/release/julie-extract \
  --runs 3
```

Summary output file: `target/performance/audit-wave-1-after-fair/baseline-summary.json`

#### Per-Run Timing Samples
| Run Index | Scan Duration (ms) | Rescan Duration (ms) | Info Duration (ms) | Export Duration (ms) | Scan Rows/sec |
|---|---|---|---|---|---|
| Run 1 | 17,312 | 487 | 291 | 11,425 | 20,211.66 |
| Run 2 | 17,514 | 500 | 314 | 11,701 | 19,978.61 |
| Run 3 | 17,586 | 468 | 290 | 11,449 | 19,896.70 |

#### Aggregates (3 Runs)
| Metric | Min | Median | Max |
|---|---|---|---|
| Scan Duration (ms) | 17,312.0 | 17,514.0 | 17,586.0 |
| Rescan Duration (ms) | 468.0 | 487.0 | 500.0 |
| Info Duration (ms) | 290.0 | 291.0 | 314.0 |
| Export Duration (ms) | 11,425.0 | 11,449.0 | 11,701.0 |
| Rows per Second | 19,896.70 | 19,978.61 | 20,211.66 |

#### Output Sizes and Counts
- **Files Scanned:** 2,222
- **Rescan Files Unchanged:** 2,222 (0 changed, 0 deleted, 0 failed)
- **Symbols Extracted:** 347,699
- **JSONL Records Total:** 2,364,686
- **SQLite Database Size:** 1,196,474,368 bytes (~1,141.05 MiB)
- **JSONL Export Size:** 1,653,406,226 bytes (~1,576.81 MiB)

#### Detailed Row Totals (SQLite)
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

### Measurement 2: Writer Current Schema Performance (`xtask performance writer-current-schema`)

Command:
```bash
cargo xtask performance writer-current-schema \
  --out-dir target/performance/audit-wave-1-writer-after-fair
```

Summary output file: `target/performance/audit-wave-1-writer-after-fair/writer-current-schema-summary.json`

#### Synthetic Workload Input
- Files: 10,000
- Symbols per file: 8
- Identifiers per file: 24
- Source regions per file: 12

#### Performance Results
| Metric | Value |
|---|---|
| Elapsed Write Time | 12,138 ms (12.14 s) |
| Write Throughput | 100,503.87 rows/sec |
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
  --root /tmp/julie-benchmark-snapshot \
  --view default \
  --json
```

Three full imports were executed against isolated temporary family store directories and timed via high-resolution wall clock timer.

#### Per-Run Timing Samples
| Run Index | Wall Clock Duration (s) | Wall Clock Duration (ms) | Exit Code | Result State |
|---|---|---|---|---|
| Run 1 | 45.053 s | 45,053.37 ms | 0 | committed |
| Run 2 | 46.573 s | 46,573.16 ms | 0 | committed |
| Run 3 | 50.932 s | 50,931.78 ms | 0 | committed |

#### Timing Aggregates (Wall Clock)
| Metric | Seconds | Milliseconds |
|---|---|---|
| **Min** | 45.053 s | 45,053.37 ms |
| **Median** | 46.573 s | 46,573.16 ms |
| **Max** | 50.932 s | 50,931.78 ms |

#### Import Result Metadata (Consistent across all 3 runs)
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
  - **Total Domain Rows**: 2,362,114 (sum of `file_versions`, `l1`, `l2`, `l3`)
  - **Total Store Rows**: 2,371,901 (including 9,787 rows in metadata/internal tables)

---

## Before and After Comparison Tables

### Measurement 1: Repository Baseline Performance Comparison

| Metric | Before Min | Before Median | Before Max | After Min | After Median | After Max | Median Delta | Median Delta (%) |
|---|---|---|---|---|---|---|---|---|
| Scan Duration (ms) | 16,553.0 | 20,667.0 | 21,292.0 | 17,312.0 | 17,514.0 | 17,586.0 | -3,153.0 ms | **-15.26%** |
| Rescan Duration (ms) | 473.0 | 475.0 | 527.0 | 468.0 | 487.0 | 500.0 | +12.0 ms | **+2.53%** |
| Info Duration (ms) | 292.0 | 297.0 | 323.0 | 290.0 | 291.0 | 314.0 | -6.0 ms | **-2.02%** |
| Export Duration (ms) | 11,473.0 | 11,570.0 | 12,734.0 | 11,425.0 | 11,449.0 | 11,701.0 | -121.0 ms | **-1.05%** |
| Rows per Second | 16,433.91 | 16,931.23 | 21,138.68 | 19,896.70 | 19,978.61 | 20,211.66 | +3,047.38 rows/s | **+18.00%** |

### Measurement 2: Writer Current Schema Performance Comparison

| Metric | Before (`ea7492ef`) | After (`4a2ba80a`) | Absolute Delta | Delta (%) |
|---|---|---|---|---|
| Elapsed Write Time | 11,610 ms (11.61 s) | 12,138 ms (12.14 s) | +528 ms | **+4.55%** |
| Write Throughput | 105,075.54 rows/sec | 100,503.87 rows/sec | -4,571.67 rows/sec | **-4.35%** |
| SQLite Artifact Size | 378,384,384 bytes (~360.86 MiB) | 378,384,384 bytes (~360.86 MiB) | 0 bytes | **0.00%** |
| Transactions Committed | 1 | 1 | 0 | 0.00% |
| Files Changed | 10,000 | 10,000 | 0 | 0.00% |
| Rows Written | 1,219,930 | 1,219,930 | 0 | 0.00% |

### Measurement 3: Store Import Wall Clock Performance Comparison

| Metric | Before (`ea7492ef`) | After (`4a2ba80a`) | Absolute Delta | Delta (%) |
|---|---|---|---|---|
| Run 1 Duration (s) | 50.022 s | 45.053 s | -4.969 s | **-9.93%** |
| Run 2 Duration (s) | 50.957 s | 46.573 s | -4.384 s | **-8.60%** |
| Run 3 Duration (s) | 49.961 s | 50.932 s | +0.971 s | **+1.94%** |
| Min Duration (s) | 49.961 s | 45.053 s | -4.908 s | **-9.82%** |
| **Median Duration (s)** | **50.022 s** | **46.573 s** | **-3.449 s** | **-6.90%** |
| Max Duration (s) | 50.957 s | 50.932 s | -0.025 s | **-0.05%** |
| file_versions | 2,209 | 2,209 | 0 | **0.00%** |
| Total Store Rows | 2,371,901 | 2,371,901 | 0 | **0.00%** |

---

## Executive Summary & Regression Verification

| Benchmark Workload | Metric | Before (Median) | After (Median) | Absolute Delta | Delta (%) | Regression Check (Threshold: ≤ +5.0%) |
|---|---|---|---|---|---|---|
| **Repository Scan** | Scan Duration | 20,667.0 ms | 17,514.0 ms | -3,153.0 ms | **-15.26%** | PASSED (Improved -15.26%) |
| **Repository Scan** | Rescan Duration | 475.0 ms | 487.0 ms | +12.0 ms | **+2.53%** | PASSED (+2.53% ≤ 5.0%) |
| **Repository Scan** | Info Duration | 297.0 ms | 291.0 ms | -6.0 ms | **-2.02%** | PASSED (Improved -2.02%) |
| **Repository Scan** | Export Duration | 11,570.0 ms | 11,449.0 ms | -121.0 ms | **-1.05%** | PASSED (Improved -1.05%) |
| **Repository Scan** | Scan Throughput | 16,931.23 rows/s | 19,978.61 rows/s | +3,047.38 rows/s | **+18.00%** | PASSED (Improved +18.00%) |
| **Writer Current Schema** | Elapsed Write Time | 11,610 ms (11.61 s) | 12,138 ms (12.14 s) | +528 ms | **+4.55%** | PASSED (+4.55% ≤ 5.0%) |
| **Writer Current Schema** | Write Throughput | 105,075.54 rows/sec | 100,503.87 rows/sec | -4,571.67 rows/sec | **-4.35%** | PASSED (Within tolerance) |
| **Store Import** | Wall Clock Duration | 50.022 s | 46.573 s | -3.449 s | **-6.90%** | PASSED (Improved -6.90%) |

**Regression Verification Result:**
All median metrics passed. No metric regressed beyond the 5% threshold. In particular, full repository scan duration dropped by **-15.26%** (from 20.7s to 17.5s, with throughput increasing **+18.00%** to 19,978 rows/s), and store import wall-clock time was reduced by **-6.90%** (saving ~3.45s median, with peak runs saving 4.97s / -9.93%). Both before and after workloads processed the exact identical 2,222 files (347,699 symbols, 2,364,686 JSONL records) and 2,209 store file versions (2,371,901 store rows).

---

## Verification Ledger

| Scope | Command | Commit SHA | Result | Timestamp |
|---|---|---|---|---|
| Task 0: Baseline Performance | `cargo xtask performance baseline --root /tmp/julie-benchmark-snapshot --out-dir target/performance/audit-wave-1-before-fair --binary /tmp/julie-before-target/release/julie-extract --runs 3` | `ea7492ef` | PASSED (2,222 files, 347,699 symbols, 20,667 ms median scan) | 2026-09-04T17:15:00Z |
| Task 0: Writer Baseline | `cargo xtask performance writer-current-schema --out-dir target/performance/audit-wave-1-writer-before` | `ea7492ef` | PASSED (11,610 ms write time, 105,075 rows/s) | 2026-09-04T17:20:00Z |
| Task 0: Store Import Baseline | `julie-extract store import --store <dir> --family <uuid> --root /tmp/julie-benchmark-snapshot --view default --json` | `ea7492ef` | PASSED (2,209 versions, 2,371,901 rows, 50.022s median) | 2026-09-04T17:25:00Z |
| Task 1: Symbol Context Deletion | `cargo test -p julie-extractors --lib base::` | `840ef079` | PASSED (Base unit tests clean, Symbol.code_context removed) | 2026-09-04T17:35:00Z |
| Task 2: Symbol Map Removal | `cargo test -p julie-extractors --lib base:: && cargo xtask test language ruby && cargo xtask test language cpp` | `37e10b12` | PASSED (BaseExtractor::symbol_map eliminated, language tests pass) | 2026-09-04T17:45:00Z |
| Task 3: Containing Symbol Index | `cargo xtask test golden && cargo xtask test default` | `faf20c51` | PASSED (31 extractors updated to shared ContainingSymbolIndex, centered interval tree) | 2026-09-04T17:55:00Z |
| Task 4: Spool Detour Removal | `cargo test -p julie-extract-cli --test store_cli_contract` | `54e30a0b` | PASSED (IMPORT_SPOOL_IO eliminated, 538 file_versions regression test passes) | 2026-09-04T18:05:00Z |
| Task 5: Capability Snapshot Once | `cargo test -p julie-extract-artifact --test store_writer_batching_contract` | `154fad78` | PASSED (Capability snapshot built once per quantum, conflict detection preserved) | 2026-09-04T18:15:00Z |
| Task 6: Language Detection Once | `cargo test -p julie-extract-cli --test operations_contract -- scan_records_content_based_language_for_cpp_headers` | `4a2ba80a` | PASSED (1 detection in scan, 1 in store import, C++ symbol extraction verified) | 2026-09-04T18:25:00Z |
| Task 7: Post-Opt Performance | `cargo xtask performance baseline --root /tmp/julie-benchmark-snapshot --out-dir target/performance/audit-wave-1-after-fair --binary target/release/julie-extract --runs 3` | `4a2ba80a` | PASSED (2,222 files, 347,699 symbols, 17,514 ms median scan, -15.26% duration) | 2026-09-04T18:35:00Z |
| Task 7: Post-Opt Writer | `cargo xtask performance writer-current-schema --out-dir target/performance/audit-wave-1-writer-after-fair` | `4a2ba80a` | PASSED (12,138 ms write time, 100,503 rows/s, +4.55% write time) | 2026-09-04T18:40:00Z |
| Task 7: Post-Opt Store Import | `julie-extract store import --store <dir> --family <uuid> --root /tmp/julie-benchmark-snapshot --view default --json` | `4a2ba80a` | PASSED (2,209 versions, 2,371,901 rows, 46.573s median, -6.90% duration) | 2026-09-04T18:45:00Z |
| Branch Gate: Code Formatting | `cargo fmt --all --check` | Working Tree | PASSED | 2026-09-04T18:50:00Z |
| Branch Gate: Clippy Linter | `cargo clippy --workspace --all-targets` | Working Tree | PASSED (0 warnings, 0 errors) | 2026-09-04T18:52:00Z |
| Branch Gate: Default Test Suite | `cargo xtask test default` | Working Tree | PASSED (all unit and contract tests pass) | 2026-09-04T18:55:00Z |
| Branch Gate: Golden Certification | `cargo xtask test golden` | Working Tree | PASSED (all golden fixture tests pass) | 2026-09-04T18:58:00Z |
