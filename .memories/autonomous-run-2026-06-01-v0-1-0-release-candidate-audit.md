# Autonomous Run: v0.1.0 Release Candidate Audit

## Outcome

- Merged PR #7 locally into `main` and based Slice 5 on `1440759`.
- Opened PR #8: https://github.com/anortham/julie-extractors/pull/8.
- Fixed a release-blocking contract mismatch: real SQLite/JSONL artifacts now
  persist parser inventory and language capability snapshot rows.
- Staged v0.1.0 for `aarch64-apple-darwin`.
- Recorded release candidate evidence in
  `docs/release-evidence/2026-06-01-v0-1-0-release-candidate-audit.md`.

## Key Evidence

- Package commit: `c407cde`.
- Package output:
  `target/release-package/v0.1.0-aarch64-apple-darwin-c407cde`.
- Binary version: `julie-extract 0.1.0`.
- Binary SHA-256:
  `c52b86f01c369088fad94da2ca013c9ddcfc840830e787c2f758a06724cf9237`.
- Checksum verification:
  `dist/aarch64-apple-darwin/julie-extract: OK`.
- Capability rows in refreshed baseline: `36` parser inventory, `36` language
  capability, `76` fixture, `17` gap rows.
- Refreshed repeatable baseline: cold scan `6485ms` / `6514ms` / `7550ms`,
  no-change rescan `56ms` / `62ms` / `62ms`, JSONL export `1318ms` /
  `1321ms` / `1328ms`.

## Verification

- `cargo fmt --all -- --check`
- `cargo test -p xtask`
- `cargo xtask test default`
- `cargo xtask test contract`
- `cargo xtask test changed crates/julie-extractors/Cargo.toml crates/julie-extract-artifact/src/writer.rs crates/julie-extract-cli/src/commands.rs docs/release-notes/v0.1.0.md`
- `scripts/check-agent-doc-sync.sh`
- `git diff --check`
- PR #8 Fast Gates

## Next

- Merge PR #8 when ready.
- After merge and CI pass, decide whether to trigger the Release Binaries
  workflow for v0.1.0 or hold the release candidate.

