# v2.0.1 Release Candidate Evidence

Date: 2026-06-02

Candidate binary: `julie-extract 2.0.1`.

Generated artifacts are under `target/performance/2.0.1-rc/` and are not
committed.

## Fixes Under Test

- Large symbol lookup no longer uses giant dynamic `IN (...)` SQL.
- Same-batch symbol IDs are resolved in memory before SQLite is queried.
- Remaining unresolved symbol IDs are resolved through a temporary-table join.
- Hot artifact-writer statements use rusqlite's prepared statement cache.
- Scan JSON reports include optional profiling data.

## Force Scan Matrix

Command shape:

```bash
target/release/julie-extract scan --root <repo> --db target/performance/2.0.1-rc/<name>.sqlite --force --json
```

| Repo | Status | Files | Symbols | Identifiers | Pending relationships | Wall time | Report total | Extraction/spool | Artifact write |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `/Users/murphy/source/julie-extractors` | `ok` | 1,035 | 33,686 | 101,506 | 60,489 | 6.79s | 5,635ms | 2,851ms | 2,751ms |
| `/Users/murphy/source/openclaw` | `ok` | 12,781 | 640,317 | 1,102,973 | 119,945 | 87.88s | 87,804ms | 51,056ms | 36,601ms |
| `/Users/murphy/source/Newtonsoft.Json` | `ok` | 981 | 21,286 | 111,147 | 44,716 | 6.62s | 6,604ms | 4,103ms | 2,483ms |
| `/Users/murphy/source/hermes-agent` | `ok` | 2,588 | 261,296 | 336,060 | 181,821 | 30.35s | 30,316ms | 17,487ms | 12,776ms |
| `/Users/murphy/source/eros` | `ok` | 611 | 80,494 | 39,817 | 20,595 | 5.10s | 5,083ms | 2,767ms | 2,298ms |

## No-Change Rescan Matrix

Command shape:

```bash
target/release/julie-extract scan --root <repo> --db target/performance/2.0.1-rc/<name>.sqlite --json
```

| Repo | Status | Wall time | Report total | Extraction/spool | Artifact write |
| --- | --- | ---: | ---: | ---: | ---: |
| `/Users/murphy/source/openclaw` | `no_change` | 1.17s | 1,166ms | 417ms | 56ms |
| `/Users/murphy/source/hermes-agent` | `no_change` | 0.19s | 184ms | 115ms | 5ms |
| `/Users/murphy/source/eros` | `no_change` | 0.05s | 44ms | 25ms | 1ms |
| `/Users/murphy/source/julie-extractors` | `no_change` | 0.23s | 228ms | 37ms | 4ms |
| `/Users/murphy/source/Newtonsoft.Json` | `no_change` | 0.05s | 51ms | 32ms | 2ms |

## Regression Evidence

- `cargo fmt --check`: passed.
- `git diff --check`: passed.
- `scripts/check-agent-doc-sync.sh`: passed.
- `cargo metadata --format-version 1`: passed.
- `cargo test -p xtask`: passed.
- `cargo xtask release package-list`: passed.
- `cargo xtask release package --version 2.0.1 --target aarch64-apple-darwin --out-dir target/release-package/v2.0.1-aarch64-apple-darwin --binary target/release/julie-extract`:
  passed.
- `cargo xtask test default`: passed.
- `cargo xtask test contract`: passed after stale JSX/TSX golden fixtures were
  updated to match the pending-noise pruning behavior.
- `cargo xtask test changed .github/workflows/release-binaries.yml .github/workflows/specialist-gates.yml Cargo.lock crates/julie-extract-artifact/Cargo.toml crates/julie-extract-artifact/src/writer.rs crates/julie-extract-artifact/tests/writer_performance.rs crates/julie-extract-cli/Cargo.toml crates/julie-extractors/Cargo.toml fixtures/extraction/jsx/basic/expected.json fixtures/extraction/tsx/basic/expected.json`:
  passed.
- `cargo xtask test real-world-smoke`: passed.
- `cargo test -p julie-extract-artifact --test writer_performance child_row_batch_avoids_per_file_statement_prepare_overhead -- --nocapture`
  - Before cached writer statements: failed at 1.389674833s with a 900ms
    tripwire.
  - After cached writer statements and a stable 1.25s tripwire: passed.
- `cargo test -p julie-extract-artifact`: passed.

## Dogfood Evidence

Command:

```bash
cargo xtask dogfood repo --root . --out-dir target/dogfood/julie-extractors-2.0.1 --binary target/release/julie-extract
```

Result:

- status: passed
- files: 1,035
- symbols: 33,646
- SQLite bytes: 142,569,472
- JSONL records: 220,786
- scan duration: 5,492ms
- no-change rescan duration: 64ms
- info duration: 6ms
- export duration: 1,508ms

## Release Judgment

The v2.0.1 candidate fixes the Eros blocker and avoids major release-blocking
performance regressions across the measured matrix. Openclaw cold scan remains
expensive because it writes roughly two million extracted rows from 12,781
supported files, mostly TypeScript. The no-change rescan path is cheap enough
for normal incremental use. Deeper cold-scan optimization should continue after
v2.0.1.
