# Clean fixed-margin coverage candidate build record

Recorded 2026-11-04T11:14:14.733249+00:00.

- Worktree: /home/murphy/source/julie-extractors/.worktrees/language-coverage-plan
- Branch: docs/language-coverage-plan
- Commit: e1c18a1eb6b76eb930117235fa19cfce9477e32a
- Source tree SHA-256 (git-ls-tree-r-v1-sha256): da9fc6639e6a70bfcf4c6a7e0bf36ed23e865c385dff2cbbf48534407485ebc8
- Working tree was clean before and after the actual build.
- Recipe: cargo build --release --locked --offline -p julie-extract-cli --bin julie-extract
- Recipe SHA-256: bf0ac8dfb7cae289dd3473312f95d7393601b4c4d47d6d45493eb48215454448
- Cargo default release profile; host/target x86_64-unknown-linux-gnu.
- Toolchain: rustc 1.97.1 (8bab26f4f 2026-07-14); cargo 1.97.1 (c980f4866 2026-06-30).
- Normal/build graph: 267 sorted package/feature pairs, identical to the pinned baseline.
- TMPDIR was /home/murphy/source/julie-extractors/.worktrees/language-coverage-plan/target/coverage-tmp; Cargo's default target directory was used.
- Compiler/profile overrides were unset: RUSTFLAGS, CARGO_ENCODED_RUSTFLAGS, CARGO_BUILD_TARGET, RUSTC_WRAPPER, CC, CFLAGS, CARGO_TARGET_DIR and CARGO_PROFILE_RELEASE_*.
- Build exited 0. Log target/coverage-margin-candidate-build.log, SHA-256 dcbf7b694571154a9bf26c16a5edd0a32901e1e2fe832cfa81580a53d30c2014.
- Binary: target/release/julie-extract; 117126768 bytes; SHA-256 c236e36111785c9ac6bcab6220f07403ad7ad54d02b00590fbe27f6358e40dcc.
- CLI bytes match the prior clean build and untimed row preflight. The follow-up changes calibration, tests, documentation and evidence; CLI/extractor production source is unchanged.
- This record follows an actual clean locked/offline build, using valid cached outputs.
