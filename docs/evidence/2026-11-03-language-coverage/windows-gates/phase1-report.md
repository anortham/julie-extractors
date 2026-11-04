# Windows verification

## Revision and environment

- Synced source: `4aac8d123434167ccbe20b06ac4ea21ed7e493a8` in `C:\work\language-coverage-plan`.
- Sync was performed by root; this worker did not resync until new authorization.
- Identity probe: Windows `10.0.26200`, PowerShell `5.1.26100.9711`, Rust/Cargo `1.98.0` MSVC, Git `2.55.0.windows.3`, .NET `10.0.400`, Node `v24.19.0`.
- Identity output: `target/coverage-windows-identity.log`.

## Verification results on the synced revision

| Command | Result | Host output | win-test log |
|---|---|---|---|
| `cargo xtask test default` | Passed; tier wall clock 333s. Extractor lib tests: 6,832 passed, 0 failed, 7 ignored. | `target/coverage-windows-default.log` | `/home/murphy/.local/share/win-test/logs/20261104T082923Z-language-coverage-plan-3651577.log` |
| `cargo test -p xtask` | Failed 28/30. Two test expectations compared raw temp paths with producer-canonicalized paths (`\\?\C:\...` on Windows). | `target/coverage-windows-xtask-tests.log` | `/home/murphy/.local/share/win-test/logs/20261104T083626Z-language-coverage-plan-3656612.log` |
| `cargo test -p julie-extract-cli --test path_policy --test operations_contract --test rebind_equivalence` | Passed: 9 + 77 + 4 tests. | `target/coverage-windows-cli-focused.log` | `/home/murphy/.local/share/win-test/logs/20261104T083915Z-language-coverage-plan-3657979.log` |
| `cargo xtask test capability` | Passed: capability matrix 51/51 and pending shape contract 1/1. | `target/coverage-windows-capability.log` | `/home/murphy/.local/share/win-test/logs/20261104T084018Z-language-coverage-plan-3658449.log` |
| `cargo xtask test golden` | Passed: 9/9. | `target/coverage-windows-golden.log` | `/home/murphy/.local/share/win-test/logs/20261104T084227Z-language-coverage-plan-3659760.log` |
| `cargo xtask coverage --all --binary target/debug/julie-extract.exe` | Passed: 44 output rows; no failed/unsupported files. The deliberate `rust-malformed-recovery` row has one parse diagnostic. | `target/coverage-windows-coverage-all.log` | `/home/murphy/.local/share/win-test/logs/20261104T084543Z-language-coverage-plan-3661241.log` |

## Disk-space recovery

- Before cleanup, C: had 0 bytes free. Verified the two permitted Julie clone roots on the guest and measured their generated Cargo targets: `C:\work\language-coverage-plan\target` was 6,322,472,716 bytes; `C:\work\julie-extractors\target` was 13,374,341,108 bytes.
- Removed only those two generated target directories. C: then had 18,429,423,616 bytes free. Logs: `target/coverage-windows-target-inventory.log`, `target/coverage-windows-target-cleanup.log`.
- G: is NTFS label `cache`, with 15,335,530,496 bytes free, less than C: had after cleanup; the normal C: target path was retained.
- After the capability feature build, C: had 2,190,450,688 bytes free. Before coverage, removed only the already-passed extractor test executables and matching PDBs: `julie_extractors-b3072168a19a3d37.{exe,pdb}`, `julie_extractors-1cb4a304a6605645.{exe,pdb}`, and `julie_extractors-4cec705ada33d9f5.{exe,pdb}` under `C:\work\language-coverage-plan\target\debug\deps`. Their measured total was 1,372,731,904 bytes. The CLI executable and dependency build outputs were retained. Logs: `target/coverage-windows-test-binary-inventory.log`, `target/coverage-windows-test-binary-cleanup.log`.
- C: had 1,647,579,136 bytes free after coverage.

## Follow-up

- The two `xtask/src/performance/comparison.rs` test expectations were changed on the host to compare against `fs::canonicalize(...)`; source/hash assertions remain unchanged.
- These test edits have not yet been committed or synced to the guest. Rerun `cargo test -p xtask` on a newly synced clean SHA, then shut the guest down and verify it is off.
