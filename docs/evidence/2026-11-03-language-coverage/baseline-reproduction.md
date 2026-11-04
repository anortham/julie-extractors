# v3.11.0 build reproduction record

Recorded: 2026-11-04 UTC

## Source identity

- Source repository: `/home/murphy/source/julie-extractors`
- Branch: `main`
- Commit: `f9a1b6cfbd6c51e9aecad39a19610695f210ad05`
- Git tree OID: `e666f85c3088f3526a4f808aa18b64336ac90a4d`
- Content identity: `2f7afb3f50ea96fa2046f04eb2f1c75312fe5449926394cec31f52630c290599`, SHA-256 of `git ls-tree -rz --full-tree <commit>` bytes, algorithm `git-ls-tree-r-v1-sha256`.
- The main checkout was clean before the build and remained clean after it. `dirty=false`; `dirty_state_sha256=null`.
- The source tag commit is `ee274586879b8c217911155adc5642b66e4a692a`. The tracked changes from that tag to the recorded commit are release documentation and memory notes; no code or manifest paths changed.
- Input hashes: `Cargo.toml` `8d47c922922d7366a78bf84cf8e7c9bcb304f586c425c1260a6b56f9faf65689`; `Cargo.lock` `fdf4c087d86f6102b6492f01d35436156e27337c1500da6018df7636268e4641`; CLI manifest `842c61de4022bc76edc521b10a34a57056db941ca4dc3502c66d8a9e2e01f9e7`; extractor manifest `ec3839f719d94f6a9d326e7dad113b5437ef92b157e105b49410939b16e5e778`; artifact manifest `c64fc28e8cf0d571e9edc9d148a08b9345992c6bbaa0fc29c29625558c1ba224`; `.cargo/config.toml` `c185a2b859850d94af015a9672c1568e1ef73b12612d820f596ef967b835f637`.
- `Cargo.toml` has no `[profile.release]` override. Compile-time capability and language-policy inputs are included from the source tree. The selected graph builds the local regex grammar build script and bundled SQLite.

## Build recipe

- Executed from the clean source repository root with argv:
  `cargo build --release --locked --offline -p julie-extract-cli --bin julie-extract`
- Recipe SHA-256 over compact `serde_json::to_vec(argv)`: `bf0ac8dfb7cae289dd3473312f95d7393601b4c4d47d6d45493eb48215454448`.
- `CARGO_TARGET_DIR=/home/murphy/source/julie-extractors/.worktrees/language-coverage-plan/target/coverage-baseline-reproduction`.
- `TMPDIR=/home/murphy/source/julie-extractors/.worktrees/language-coverage-plan/target/coverage-tmp`.
- `RUSTFLAGS`, `CARGO_ENCODED_RUSTFLAGS`, `CARGO_BUILD_TARGET`, `RUSTC_WRAPPER`, `CC`, `CFLAGS`, and Cargo release-profile override variables were unset. No custom Cargo home was set. `.cargo/config.toml` contains only the `xtask` alias.
- `rustc 1.97.1 (8bab26f4f 2026-07-14)`; `cargo 1.97.1 (c980f4866 2026-06-30)`.
- Host and target: `x86_64-unknown-linux-gnu`; no explicit `--target` override.
- Profile: Cargo default `release`, reported as optimized. Features are the selected CLI normal/build graph, extracted from `cargo tree --locked --offline -p julie-extract-cli --target x86_64-unknown-linux-gnu -e normal,build,features --prefix none --format '{p} {f}'`: 267 sorted package/feature pairs. The exact vector is in `baseline-features.json` (SHA-256 `d104a3ef7ac48929268995c371a09ea540b1c4a63955457636f3d5da800f1341`). The captured tree is `baseline-feature-tree.txt` (SHA-256 `962322a6fb90a37510a8055f7195779f4787c4e3511a5ed6487bf7978db861c5`).
- Semantic Cargo options: `locked=true`, `offline=true`. No other compiler or profile options were set.
- Build exited 0. Cargo reported `Finished release profile [optimized] target(s) in 1m 17s`.
- Native tools: GCC `16.2.1 20260819 (Red Hat 16.2.1-2)`; GNU ld `2.46.1-1.fc44`.

## Binary comparison

- Saved baseline, read-only: `target/coverage-baseline/julie-extract-3.11.0`.
- Reproduction: `target/coverage-baseline-reproduction/release/julie-extract`.
- Both SHA-256 values: `79eb21fa95d39445e78b7237a0300fe071649d5671db0fbc4b3baff0ee2fd78d`.
- Both sizes: `111129152` bytes. `cmp` reported identical bytes. Both report `julie-extract 3.11.0`.
- The saved baseline was only read and hashed; it was not replaced or modified.
- This records a clean source/build recipe that reproduces the saved binary exactly. The historical invocation and source binding of the saved baseline itself remain unknown.
