# Erlang real-world corpus delta

## Identity and reproduction

- Worktree: `docs/language-coverage-plan`, HEAD `5afe96223c231548d107ad2194fac1c53a493c2c`; checkout was dirty.
- Prior binary: `target/coverage-baseline/julie-extract-3.11.0`, SHA-256 `79eb21fa95d39445e78b7237a0300fe071649d5671db0fbc4b3baff0ee2fd78d`.
- Comparison binary: `target/debug/julie-extract`, SHA-256 `29a7011b5a06f7f0559aa648b0014f8d32d911a8f9e5e379e19d2d908a73451a`.
- Current parser inputs: `vendor/tree-sitter-erlang/grammar.js` SHA-256 `36fc3e044f84586b2dcb4301710bcc420a025e72edf1b8680d203539235b41cc`; `vendor/tree-sitter-erlang/src/parser.c` SHA-256 `8eeefd9a7a94e5aafd4f1ad9c23b52d3c63a56d268fe24fe37f643a36483e4ea`.
- Both comparison binaries scanned the same ten corpus files from `target/coverage-tmp/erlang-corpus-delta-review/repo`; every copied input matched its corpus file SHA-256 listed below. Both scans exited successfully.
- Comparison receipts: old report `old-report.json` SHA-256 `8ab3ec29b8c4f93fbd59d5a620260172a5019491ebd4739e27c2d3061cbfe2ff`; current report `new-report.json` SHA-256 `d5e3613b30a7ef9f9168679aa580f0a4afba0265f87d9c1de88c33b5ed1ff5ba`; current database SHA-256 `a183ea5c2fea58fcd43f3903990d49f1f49b4a9791c2669b77836c724497c7db`.

## Exact per-file counts

| Corpus path | Input SHA-256 | Symbols old → current | Parse diagnostics old → current | Variable references old → current |
|---|---|---:|---:|---:|
| `certifi-2.15.0/src/certifi.erl` | `b20c2454aeb74c567cf82e4e856129ee5e982848d81e9eb85531944a02d94344` | 5 → 5 | 0 → 0 | 1 → 7 |
| `certifi-2.15.0/src/certifi_pt.erl` | `054e7d4aae07cca40b74677f722e892550f3ee4c60eae16b5a6575ec7eb9deee` | 15 → 15 | 0 → 0 | 1 → 22 |
| `telemetry-1.3.0/src/telemetry.erl` | `faebd2146716eade18c610ce13daedfbfe421fa0b5d5d244d8a47c38ba22a77e` | 52 → 60 | 45 → 0 | 4 → 129 |
| `telemetry-1.3.0/src/telemetry.hrl` | `a1d8c22d81d4615af445fb2134055cd608468ba384f1012d4451deab305a185c` | 13 → 13 | 2 → 2 | 0 → 0 |
| `telemetry-1.3.0/src/telemetry_app.erl` | `436d3951199e51cfc3551490e816b7a339b37a80a44483466d51c09c517645b1` | 6 → 6 | 0 → 0 | 0 → 0 |
| `telemetry-1.3.0/src/telemetry_handler_table.erl` | `16bba537c5bd6a6dfe4db546f3870a5326c18895e6a7e8776adb1f13932033e5` | 40 → 40 | 0 → 0 | 10 → 39 |
| `telemetry-1.3.0/src/telemetry_sup.erl` | `d0c6f28c8b0e6486b4b120be39f67d1e0b405f478aa73800b4db08362e9ef19f` | 6 → 6 | 0 → 0 | 3 → 7 |
| `telemetry-1.3.0/src/telemetry_test.erl` | `67c90971b6cd054c4b48f87a510abe5093f4274e2d4daf114dd18685986b7621` | 12 → 12 | 0 → 0 | 1 → 16 |
| `unicode_util_compat-0.7.1/src/string_compat.erl` | `400d0326f4518c7a39a5584e565a8534710d043106276967a51f532d369011e2` | 729 → 729 | 0 → 0 | 0 → 2,675 |
| `unicode_util_compat-0.7.1/src/unicode_util_compat.erl` | `1934faccd260916bc05b199dfd017f5d1c9424ae212ca5126d470ce56eb06905` | 208 → 208 | 0 → 0 | 0 → 4,430 |

Corpus totals are 1,086 → 1,094 symbols, 47 → 2 parse diagnostics, 2,971 → 10,320 identifiers, 2,974 → 10,323 reference sites, 77 → 80 type facts, 1,580 → 1,590 relationships, and 304 → 316 pending relationships. The new `variable_ref` rows carry source-token spans and `read` or `write` access metadata; the Erlang variable-access tests cover fresh bindings, repeated reads, branches, guards, comprehensions, and closures.

## Eight added telemetry symbols

The four parameters have `metadata.role = "parameter"`; the four local bindings have no parameter role. Byte spans are half-open; columns are zero-based.

| Name | Kind / owner | Source span | Type fact | Access evidence |
|---|---|---|---|---|
| `Measurements` | parameter / `execute/3` | line 160, columns 29–41, bytes 6242–6254 | none | read at line 167 |
| `Handlers` | local / `execute/3` | line 161, columns 4–12, bytes 6320–6328 | none | write at 161; read at 184 |
| `ApplyFun` | local / `execute/3` | line 162, columns 4–12, bytes 6386–6394 | none | write at 162; read at 184 |
| `FailureMetadata` | local / `execute/3` | line 171, columns 20–35, bytes 6747–6762 | none | write at 171; read at 178 |
| `FailureMeasurements` | local / `execute/3` | line 177, columns 20–39, bytes 7123–7142 | none | write at 177; read at 178 |
| `Term` | parameter / `assert_event_names/1` | line 386, columns 19–23, bytes 14355–14359 | explicit `term` | read in the `erlang:error/2` argument at line 387 |
| `Term` | parameter / `assert_event_name/1` | line 408, columns 18–22, bytes 14936–14940 | explicit `term` | read in the `erlang:error/2` argument at line 409 |
| `Ctx` | parameter / `merge_ctx/2` | line 413, columns 20–23, bytes 15132–15135 | explicit `any` | read in the map update on line 413 |

The vendored grammar has a `catch_clause` production for a macro-call head and a macro catch body; `vendor/tree-sitter-erlang/test/corpus/macro_catch.txt` asserts that tree shape. This lets the primary parse retain `telemetry.erl` after `?WITH_STACKTRACE(...)`; the existing export test confirms all eight public exports. The two remaining diagnostics are the `-define` bodies on lines 7 and 9 of `telemetry.hrl`.

## Verification

- Initial release-tier failure is preserved in `target/coverage-final-real-world-release.log`: the old `(52, 45)` expectation disagreed with the current `(60, 0)` result.
- Focused Erlang corpus target: 3 passed, 0 failed; 8.46 seconds, log `target/coverage-final-erlang-corpus-after-update.log`.
- Full `real-world-release` tier: all six commands passed (16 tests total); 13.67 seconds, log `target/coverage-final-real-world-release-after-update.log`.
- The test run rebuilt the CLI as `target/debug/julie-extract`, SHA-256 `9f0d1d726ba63b5a33d143c34c921f5a53d20c2238e7bcdae8cfff8fd9583312`. Between comparison and verification, only the test baseline/comment block and recovery module documentation changed. A direct scan with this verified binary produced 60 telemetry symbols and zero telemetry diagnostics; its report SHA-256 is `b1666f5c75d0000264daf57f737c81ce8b2e1759397bbf34c5c3b2cd1872de9b` and database SHA-256 is `f840a058e770071f318000d6236553af82960e5f344602e4d7daf4695eea67bf`.
