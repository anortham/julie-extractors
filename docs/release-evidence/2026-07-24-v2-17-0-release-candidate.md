# v2.17.0 release-candidate evidence

Date: 2026-07-24

Status: locally verified and awaiting explicit merge, push, and release approval.
No tag, workflow dispatch, package publication, or GitHub Release was created by
this preparation.

## Candidate

- Branch: `codex/miller-takeover-resolution`
- Rust packages: `julie-extract-artifact`, `julie-extract-cli`, and
  `julie-extractors` at `2.17.0`
- SQLite schema: `4`
- Extract contract: `3`
- JSONL schema: `3`
- Resolution evidence contract: `2`
- Release targets: `aarch64-apple-darwin`, `x86_64-apple-darwin`,
  `x86_64-unknown-linux-gnu`, and `x86_64-pc-windows-msvc`

## Reference-resolution evidence

- Languages: `36`
- Exact coverage cells: `689`
- Evidence rows: `3268`
- Attempted rows: `1021`
- Resolved rows: `227`
- Unresolved pending rows: `794`
- Silent cells: `0`
- Quality-bar debts: `0`
- Identifier spans: `2212/2212`
- Direct relationship spans: `224/224`
- Pending relationship spans: `394/832`

The upgrade path is fail-closed. A whole-workspace scan automatically
re-extracts every supported file when the resolution version is missing, stale,
or failed. Single-file update and delete operations return
`schema_migration_required` until that scan succeeds. Empty artifacts advance
their metadata without fabricating rows; incomplete extraction or resolver
failure keeps the artifact unavailable to single-file mutation.
The same gate covers `scan --force` and preserved oversized files. A resolver
failure during a routine `update` or `delete` also records a failed status and
requires a successful whole-workspace scan before further single-file
mutations.

## Verification

Passed locally with Rust 1.96:

- `cargo fmt --all -- --check`
- `cargo xtask test default`
- `cargo xtask test contract`
- `cargo xtask test golden`
- `node scripts/reference-resolution-coverage-report.mjs --strict`
- `node scripts/language-data-quality-report.mjs --strict`
- `cargo clippy --workspace --all-targets --all-features --no-deps -- -D warnings`
- `cargo build --workspace --release`
- `cargo xtask release package-list`
- `cargo xtask release preflight --version 2.17.0`
- `scripts/check-agent-doc-sync.sh`
- `git diff --check`

The preflight reported four targets and 23 package inputs. Live archive
checksums and downloaded-binary evidence remain intentionally absent until the
approval-gated four-platform release workflow completes.

## Independent review

A fresh Claude final review verified the forced-scan gate, oversized-file gate,
post-finalization totals, CLI contract, and steady-state failed-status contract.
It returned `approve` with zero findings after rerunning the affected suites and
release gates.
