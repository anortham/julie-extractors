# Clean coverage candidate build record

Recorded 2026-11-04T08:18:40.270400+00:00.

- Worktree: /home/murphy/source/julie-extractors/.worktrees/language-coverage-plan
- Branch: docs/language-coverage-plan
- Commit: 4aac8d123434167ccbe20b06ac4ea21ed7e493a8
- Source tree SHA-256 (git-ls-tree-r-v1-sha256): 195783c00bb38c4adf69c4777aafbda2dd68da1d3178d39eca8959a54306d911
- Working tree was clean before and after the build.
- Recipe: cargo build --release --locked --offline -p julie-extract-cli --bin julie-extract
- Recipe SHA-256: bf0ac8dfb7cae289dd3473312f95d7393601b4c4d47d6d45493eb48215454448
- Cargo default release profile; host/target x86_64-unknown-linux-gnu.
- Toolchain: rustc 1.97.1 (8bab26f4f 2026-07-14); cargo 1.97.1 (c980f4866 2026-06-30).
- Normal/build feature graph: 267 sorted package/feature pairs, identical to the pinned baseline vector.
- TMPDIR was /home/murphy/source/julie-extractors/.worktrees/language-coverage-plan/target/coverage-tmp; target output used Cargo's default target directory.
- Compiler/profile overrides were unset: RUSTFLAGS, CARGO_ENCODED_RUSTFLAGS, CARGO_BUILD_TARGET, RUSTC_WRAPPER, CC, CFLAGS, CARGO_TARGET_DIR and CARGO_PROFILE_RELEASE_*.
- Build exited 0 in 34.57 seconds; log target/coverage-clean-candidate-build.log SHA-256 90b26d5bcc4beaa9bb175d2d8122c4d27067bcdfd1ed32cbd2d6ad95438c9fde.
- Binary: target/release/julie-extract; 117126768 bytes; SHA-256 c236e36111785c9ac6bcab6220f07403ad7ad54d02b00590fbe27f6358e40dcc.
- This record was captured after an actual clean locked/offline build, before writing tracked measurement evidence. No historical build invocation is inferred.
