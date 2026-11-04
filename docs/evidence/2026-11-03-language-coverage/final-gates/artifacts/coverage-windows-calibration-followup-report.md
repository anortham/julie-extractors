# Windows calibration follow-up

- Synced `bd7369f043dd2c1c4ddd3a76541c1c7ef372cb56` to `C:\work\language-coverage-plan`; sync evidence: `target/coverage-windows-calibration-followup-sync.log`.
- `cargo test -p xtask` passed: xtask lib 36/36; all other package targets passed. Log: `target/coverage-windows-calibration-followup-xtask.log`; win-test log `/home/murphy/.local/share/win-test/logs/20261104T093746Z-language-coverage-plan-3704958.log`.
- Focused T-SQL test passed: 1 passed, 0 failed, 6,839 filtered. Log: `target/coverage-windows-calibration-followup-tsql-c-retry.log`; win-test log `/home/murphy/.local/share/win-test/logs/20261104T094444Z-language-coverage-plan-3708507.log`.
- First T-SQL link attempt failed with `LNK1106` when C: filled. The failed run is preserved in `target/coverage-windows-calibration-followup-tsql.log`.
- Removed the previously passing extractor test exe/PDB pair at `C:\work\language-coverage-plan\target\debug\deps\julie_extractors-b3072168a19a3d37.{exe,pdb}` (279,027,712 bytes total); this raised C: from 0 to 258,560,000 bytes.
- After root approval, removed only the ordinary generated cache `C:\work\language-coverage-plan\target\debug\incremental` (7,885 files, 8,155,271,631 bytes; no reparse point). C: rose to 7,402,950,656 bytes free. The focused test then rebuilt on C: and passed.
- C: had 5,522,800,640 bytes free after the passing test. G: has 12,752,089,088 bytes free; the canceled task-specific build at `G:\codex-target\language-coverage-plan-calibration-followup` is left isolated as requested. No matching Cargo/rustc process remained before the C: retry.
- Guest shutdown verified: `target/coverage-windows-calibration-followup-guest-status-final.log` says `shut off`.
- Host task worktree: `/home/murphy/source/julie-extractors/.worktrees/language-coverage-plan`, branch `docs/language-coverage-plan`, HEAD `bd7369f043dd2c1c4ddd3a76541c1c7ef372cb56`. No source changes from this verification. Root's six performance-evidence files were dirty at handoff; other inventoried worktrees were clean.
