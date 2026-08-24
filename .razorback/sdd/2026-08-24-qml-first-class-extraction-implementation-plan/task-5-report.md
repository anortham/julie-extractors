# Task 5 report: per-language and certification gates

## Outcome

- `CommandSpec` now carries deterministic sorted environment entries.
- `run_plan` applies those entries with `std::process::Command::env`, so the
  plan does not depend on shell syntax or platform-specific wrappers.
- `cargo xtask test language <name>` now emits the existing unit-filter
  command plus a `test-golden` command with
  `JULIE_GOLDEN_LANGUAGE=<canonical language>`.
- The golden harness filters the capability matrix to that exact language and
  rejects an unknown filter instead of silently running zero fixtures.
- QML support documentation records the registered targets and the authorized
  KDE Plasma Framework evidence scan.

## Focused verification

- `cargo test -p xtask --test test_tiers`: 23 passed.
- `cargo xtask test language qml`: 127 QML unit tests and 1 filtered golden
  test passed.
- `cargo xtask test language qmldir`: 4 qmldir unit tests and 1 filtered
  golden test passed.
- `cargo build --locked --bin julie-extract`: passed.
- `cargo xtask test golden`: 5 passed; this remains the unfiltered all-language
  gate.
- `node scripts/grammar-freshness-report.mjs --format json`: passed. The
  qmldir grammar is current at `c57e00865a1a6f1cca83340d6dad91f13df55479`;
  qmljs is reported as drift at pinned `606a66b96a13ef30ed5c7ec7e5adc20a9a40157a`
  versus remote `de96ed62abded51fcdfcbeaaa120e0dd0d20c697`.
- `node --test scripts/grammar-freshness-report.test.mjs`: 11 passed.
- `cargo xtask release package-list`: passed.
- Branch-wide certification/contract gates remain lead-owned.

## Real-world evidence

Corpus: `https://github.com/KDE/plasma-framework` at
`0806864a1e7c200ee8872074a4c16be7e1ce3358`, shallow detached checkout.
No project scripts, hooks, or third-party binaries were executed.

- Filesystem counts: 179 `.qml`, 1 `.qmltypes`, and 5 exact `qmldir` files.
- Scan report: `status=ok`, 751 files scanned, 384 changed/indexed, 367
  unsupported, 0 failed, and empty warnings/errors.
- Artifact language rows: QML 180 files (including the `.qmltypes` file) and
  5 qmldir files.
- Artifact evidence: QML 7,195 symbols, 9,884 structural facts, 1,112
  relationships, 2,360 pending relationships, and 121 parse diagnostics;
  qmldir 53 symbols, 53 structural facts, no relationships, no pending
  relationships, and 10 parse diagnostics.
- The temporary checkout and artifact were removed after recording evidence.

## Files owned by this task

- `xtask/src/test_tiers.rs`
- `xtask/tests/test_tiers.rs`
- `crates/julie-extractors/src/tests/golden.rs`
- `docs/languages/qml.md`
- `docs/testing-strategy.md`
- this report
