# Clean calibrated coverage candidate build record

Recorded 2026-11-04T09:38:05.690686+00:00.

- Worktree: /home/murphy/source/julie-extractors/.worktrees/language-coverage-plan
- Branch: docs/language-coverage-plan
- Commit: bd7369f043dd2c1c4ddd3a76541c1c7ef372cb56
- Source tree SHA-256 (git-ls-tree-r-v1-sha256): bd8794bd18e2c9ba771592d0ef4e3f187768123781101cce5ed6ae374a149262
- Working tree was clean before and after the build.
- Recipe: cargo build --release --locked --offline -p julie-extract-cli --bin julie-extract
- Recipe SHA-256: bf0ac8dfb7cae289dd3473312f95d7393601b4c4d47d6d45493eb48215454448
- Cargo default release profile; host/target x86_64-unknown-linux-gnu.
- Toolchain: rustc 1.97.1 (8bab26f4f 2026-07-14); cargo 1.97.1 (c980f4866 2026-06-30).
- Normal/build graph: 267 sorted package/feature pairs, identical to the pinned baseline.
- TMPDIR was /home/murphy/source/julie-extractors/.worktrees/language-coverage-plan/target/coverage-tmp; Cargo's default target directory was used.
- Compiler/profile overrides were unset: RUSTFLAGS, CARGO_ENCODED_RUSTFLAGS, CARGO_BUILD_TARGET, RUSTC_WRAPPER, CC, CFLAGS, CARGO_TARGET_DIR and CARGO_PROFILE_RELEASE_*.
- Build exited 0: Finished `release` profile [optimized] target(s) in 0.19s. Log target/coverage-calibration-candidate-build.log, SHA-256 a2e0c7ac8f1b623bae62b66b324c508334e55a410674c2bb52b835a5fe13accc.
- Binary: target/release/julie-extract; 117126768 bytes; SHA-256 c236e36111785c9ac6bcab6220f07403ad7ad54d02b00590fbe27f6358e40dcc.
- Binary bytes match both earlier clean builds. The follow-up changes benchmark calibration, tests, documentation and evidence; CLI/extractor production source is unchanged.
- This record follows an actual clean locked/offline build, using valid cached outputs.
