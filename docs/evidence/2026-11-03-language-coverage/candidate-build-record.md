# Clean final coverage candidate build record

Recorded 2026-11-04T08:51:48.955429+00:00.

- Worktree: /home/murphy/source/julie-extractors/.worktrees/language-coverage-plan
- Branch: docs/language-coverage-plan
- Commit: c4b630d16f31ae1c82d046323f452a4c053f26f7
- Source tree SHA-256 (git-ls-tree-r-v1-sha256): b07c4a4f1ede4bc9b56cb0fa65fe3abc8ab4164eff9955565047eb3fe6aea7f9
- Working tree was clean before and after the build.
- Recipe: cargo build --release --locked --offline -p julie-extract-cli --bin julie-extract
- Recipe SHA-256: bf0ac8dfb7cae289dd3473312f95d7393601b4c4d47d6d45493eb48215454448
- Cargo default release profile; host/target x86_64-unknown-linux-gnu.
- Toolchain: rustc 1.97.1 (8bab26f4f 2026-07-14); cargo 1.97.1 (c980f4866 2026-06-30).
- Normal/build feature graph: 267 sorted package/feature pairs, identical to the pinned baseline vector.
- TMPDIR was /home/murphy/source/julie-extractors/.worktrees/language-coverage-plan/target/coverage-tmp; target output used Cargo's default target directory.
- Compiler/profile overrides were unset: RUSTFLAGS, CARGO_ENCODED_RUSTFLAGS, CARGO_BUILD_TARGET, RUSTC_WRAPPER, CC, CFLAGS, CARGO_TARGET_DIR and CARGO_PROFILE_RELEASE_*.
- Build exited 0 in 0.12 seconds using valid cached outputs; log target/coverage-final-pinned-build.log SHA-256 05343ee5ff14de6d4900a2b6c6d434c95fc912a7f59bd7e6660903cd0f156fcf.
- Binary: target/release/julie-extract; 117126768 bytes; SHA-256 c236e36111785c9ac6bcab6220f07403ad7ad54d02b00590fbe27f6358e40dcc.
- The binary is byte-identical to the earlier clean build at 4aac8d12. The follow-up changed only tests, documentation, audit data and evidence.
- This record follows an actual clean locked/offline build. No historical build invocation is inferred.
