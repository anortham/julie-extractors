# Evidence: Release Profile Tuning Measurement (Default vs. LTO Thin & Codegen Units = 1)

## Overview

Audit Wave 2, Task 6: Evaluation of release profile tuning (`lto = "thin"`, `codegen-units = 1`) in root `Cargo.toml`.

This document records the empirical measurements comparing the default Rust release profile against a tuned release profile configured with ThinLTO and single code generation unit:

```toml
[profile.release]
lto = "thin"
codegen-units = 1
```

The objective was to evaluate whether the performance gains or binary size reduction justify the build-time overhead in CI and release workflows.

## Evaluation Criteria & Decision Rule

According to the Task 6 contract specification:
- **Adoption Rule**: Adopt the profile only if:
  1. Extraction median improves by **> 5%**, OR
  2. Binary size drops by **> 15%**, AND
  3. Release build time grows by **less than 2x** (< 2.0x).
- **If Winning**: Keep `[profile.release]` in `Cargo.toml` with a short pointer to this evidence document.
- **If Losing**: Revert `Cargo.toml` to default, and document in this evidence file why it was not adopted.

## Environment

| Parameter | Value |
|---|---|
| Worktree | `/home/murphy/source/julie-extractors/.worktrees/audit-2-ci-and-hygiene` |
| Host / Kernel | Linux prax 7.1.12-200.fc44.x86_64 #1 SMP PREEMPT_DYNAMIC Fri Aug 28 14:00:18 UTC 2026 x86_64 GNU/Linux |
| CPU | 12th Gen Intel(R) Core(TM) i9-12950HX (24 vCPUs, 16 physical cores: 8 P-cores + 8 E-cores) |
| Rust Toolchain | `rustc 1.97.1 (8bab26f4f 2026-07-14)`, `cargo 1.97.1 (c980f4866 2026-06-30)` |
| Measurement Date | 2026-09-04 |

---

## Measurements

### 1. Default Profile (Baseline)

Configuration: Default Cargo release profile (no explicit `[profile.release]` in root `Cargo.toml`).

#### A. Clean Build Wall Clock Time
Command:
```bash
cargo clean -p julie-extract-cli -p julie-extract-artifact -p julie-extractors && /usr/bin/time -p cargo build --release -p julie-extract-cli
```
- **Real (Wall Clock)**: `62.36 s`
- **User CPU**: `519.90 s`
- **System CPU**: `27.12 s`

#### B. Binary Size
Command:
```bash
stat -c %s target/release/julie-extract
```
- **Binary Size**: `108,792,032 bytes` (~103.75 MiB / 104 MB)

#### C. Extraction Baseline Performance (`xtask performance baseline`)
Command:
```bash
cargo xtask performance baseline --root . --out-dir target/performance/profile-default --binary target/release/julie-extract --runs 3
```
Summary JSON: `target/performance/profile-default/baseline-summary.json`

| Run Index | Scan Duration (ms) | Rescan Duration (ms) | Info Duration (ms) | Export Duration (ms) | Scan Rows/sec |
|---|---|---|---|---|---|
| Run 1 | 16,108 ms | 457 ms | 291 ms | 11,125 ms | 21,690.07 |
| Run 2 | 16,206 ms | 467 ms | 293 ms | 10,979 ms | 21,558.82 |
| Run 3 | 15,873 ms | 460 ms | 287 ms | 10,643 ms | 22,011.44 |

**Aggregates (3 Runs)**:
- **Scan Duration (ms)**: Min: `15,873.0 ms`, **Median: `16,108.0 ms`**, Max: `16,206.0 ms`
- **Rescan Duration (ms)**: Min: `457.0 ms`, **Median: `460.0 ms`**, Max: `467.0 ms`
- **Info Duration (ms)**: Min: `287.0 ms`, **Median: `291.0 ms`**, Max: `293.0 ms`
- **Export Duration (ms)**: Min: `10,643.0 ms`, **Median: `10,979.0 ms`**, Max: `11,125.0 ms`
- **Scan Throughput**: Min: `21,558.82 rows/s`, **Median: `21,690.07 rows/s`**, Max: `22,011.44 rows/s`
- **Corpus Counts**: 2,213 files, 347,185 symbols, 2,361,805 JSONL records
- **Artifact Sizes**: SQLite: `1,194,971,136 bytes`, JSONL: `1,651,564,941 bytes`

---

### 2. Tuned Profile (`lto = "thin"`, `codegen-units = 1`)

Configuration added to root `Cargo.toml`:
```toml
[profile.release]
lto = "thin"
codegen-units = 1
```

#### A. Clean Build Wall Clock Time
Command:
```bash
cargo clean -p julie-extract-cli -p julie-extract-artifact -p julie-extractors && /usr/bin/time -p cargo build --release -p julie-extract-cli
```
- **Real (Wall Clock)**: `101.61 s`
- **User CPU**: `422.26 s`
- **System CPU**: `24.36 s`

#### B. Binary Size
Command:
```bash
stat -c %s target/release/julie-extract
```
- **Binary Size**: `105,832,792 bytes` (~100.93 MiB / 101 MB)

#### C. Extraction Baseline Performance (`xtask performance baseline`)
Command:
```bash
cargo xtask performance baseline --root . --out-dir target/performance/profile-lto-thin --binary target/release/julie-extract --runs 3
```
Summary JSON: `target/performance/profile-lto-thin/baseline-summary.json`

| Run Index | Scan Duration (ms) | Rescan Duration (ms) | Info Duration (ms) | Export Duration (ms) | Scan Rows/sec |
|---|---|---|---|---|---|
| Run 1 | 16,098 ms | 460 ms | 288 ms | 9,292 ms | 21,703.72 |
| Run 2 | 16,061 ms | 465 ms | 282 ms | 9,178 ms | 21,754.41 |
| Run 3 | 16,465 ms | 472 ms | 293 ms | 9,322 ms | 21,220.80 |

**Aggregates (3 Runs)**:
- **Scan Duration (ms)**: Min: `16,061.0 ms`, **Median: `16,098.0 ms`**, Max: `16,465.0 ms`
- **Rescan Duration (ms)**: Min: `460.0 ms`, **Median: `465.0 ms`**, Max: `472.0 ms`
- **Info Duration (ms)**: Min: `282.0 ms`, **Median: `288.0 ms`**, Max: `293.0 ms`
- **Export Duration (ms)**: Min: `9,178.0 ms`, **Median: `9,292.0 ms`**, Max: `9,322.0 ms`
- **Scan Throughput**: Min: `21,220.80 rows/s`, **Median: `21,703.72 rows/s`**, Max: `21,754.41 rows/s`
- **Corpus Counts**: 2,213 files, 347,188 symbols, 2,361,812 JSONL records
- **Artifact Sizes**: SQLite: `1,194,930,176 bytes`, JSONL: `1,651,570,000 bytes`

---

## Comparison Table

| Metric | Default Profile | Tuned Profile (`lto="thin", codegen-units=1`) | Delta | Delta (%) | Decision Threshold | Threshold Met? |
|---|---|---|---|---|---|---|
| **Clean Build Time** | `62.36 s` | `101.61 s` | `+39.25 s` | `+62.94%` (1.63x) | `< 2.0x` | **YES** (1.63x < 2x) |
| **Binary Size** | `108,792,032 bytes` | `105,832,792 bytes` | `-2,959,240 bytes` | `-2.72%` | `> 15% reduction` | **NO** (-2.72% vs -15%) |
| **Median Scan Duration** | `16,108.0 ms` | `16,098.0 ms` | `-10.0 ms` | `-0.06%` | `> 5% improvement` | **NO** (-0.06% vs -5%) |
| Median Rescan Duration | `460.0 ms` | `465.0 ms` | `+5.0 ms` | `+1.09%` | N/A | N/A |
| Median Info Duration | `291.0 ms` | `288.0 ms` | `-3.0 ms` | `-1.03%` | N/A | N/A |
| Median Export Duration | `10,979.0 ms` | `9,292.0 ms` | `-1,687.0 ms` | `-15.37%` | N/A | N/A |
| Median Scan Throughput | `21,690.07 rows/s` | `21,703.72 rows/s` | `+13.65 rows/s` | `+0.06%` | N/A | N/A |

---

## Decision & Rationale

**Decision**: **NOT ADOPTED (REVERTED TO DEFAULT)**

### Rationale:
1. **Extraction Median**: The scan duration improved by only **0.06%** (-10 ms out of ~16.1 seconds), which is well below the required **5%** threshold. In a typical multi-threaded parser workload, parsing performance is dominated by C runtime tree-sitter grammars and I/O, where Rust ThinLTO produces negligible differences.
2. **Binary Size**: Binary size decreased by **2.72%** (from 108.8 MB to 105.8 MB, saving ~2.96 MB). This is far short of the required **15%** reduction threshold.
3. **Build Time Cost**: While the build time growth of **+62.94%** (1.63x, from 62.36s to 101.61s) was under the 2.0x ceiling, the significant 39-second build-time penalty per clean build would slow down release builds and CI verification with almost zero tangible performance or size benefit.

### Action Taken:
`Cargo.toml` has been reverted to default without `[profile.release]` overrides, keeping clean release builds fast while preserving the empirical evidence in this document.
