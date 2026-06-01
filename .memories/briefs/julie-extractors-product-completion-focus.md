---
id: julie-extractors-product-completion-focus
title: julie-extractors Product Completion Focus
status: active
created: 2026-06-01T13:24:06.835Z
updated: 2026-06-01T19:15:34.361Z
tags:
  - julie-extractors
  - product-bootstrap
  - performance
  - release
---

# julie-extractors Product Completion Focus

## Current State

- `main` is at `94f1661` after PR #9 merged the release-blocker fixes and v2.0.0 contract alignment.
- PR #9: https://github.com/anortham/julie-extractors/pull/9.
- Current release target: v2.0.0, chosen because the old Julie in-tree extractor crate had reached v1.22.0 and the standalone product should not publish below that line.
- Primary tracker: `docs/plans/2026-06-01-product-completion-tracker.md`.
- Completed slice plans:
  - `docs/plans/2026-06-01-release-blocker-review-fixes.md`
  - `docs/plans/2026-06-01-v2-0-0-version-and-test-role-contract.md`
- Product boundary remains: source tree -> versioned extraction artifact. SQLite primary, JSONL secondary, `julie-extract` CLI primary, Rust crate secondary.

## Merged In PR #9

- Partial scan semantics: per-file read/extract failures now produce `partial`, exit `1`, `files_failed`, report errors, and `failed_preserved` file rows while valid files still commit.
- Data-loss guard semantics: intentionally empty supported files can replace stale symbols; `failed_preserved` scan updates preserve prior known-good symbols and add failure diagnostics.
- Discovery policy: scan discovery skips symlinks before recursion or file selection, preventing out-of-root symlink injection and loop recursion.
- Metadata fingerprints: parser inventory and capability snapshot fingerprints are deterministic `sha256:<hex>` values derived from canonicalized rows and refreshed on existing metadata reuse.
- Guardrails: member crates inherit workspace lints; CI runs a scoped clippy gate for `julie-extract-artifact`, `julie-extract-cli`, and `xtask` with `--no-deps -D warnings`.
- Versioning: workspace crates and release workflow defaults now target v2.0.0 while artifact contract versions remain v1.
- Test-role contract: SQLite `symbols` now has indexed `is_test`, `test_container`, and `test_lifecycle` booleans; CLI extraction maps existing metadata into those columns; JSONL symbol records expose the booleans and preserve metadata keys.

## Current Evidence

- PR #9 merged as `94f1661` on 2026-06-01 after GitHub Fast Gates passed.
- Local verification before merge:
  - `cargo fmt --all -- --check`
  - `cargo clippy --no-deps -p julie-extract-artifact -p julie-extract-cli -p xtask --lib --bins -- -D warnings`
  - `cargo metadata --format-version 1`
  - `scripts/check-agent-doc-sync.sh`
  - `cargo test -p xtask`
  - `cargo xtask test default`
  - `cargo xtask test contract`
  - `git diff --check`

## Hard Boundaries

- Do not add Julie MCP/server/daemon/search/embedding/watcher/dashboard/editing behavior.
- Do not write to `/Users/murphy/source/julie` unless explicitly asked.
- Do not change public CLI, SQLite, JSONL, or report contracts without a tracked plan.
- Keep default tests fast; dogfood, certification, real-world, and package staging stay specialist gates.

## Next Decision

1. Decide whether to trigger the Release Binaries workflow for `v2.0.0` or hold the release candidate.
2. If proceeding, run the workflow for `2.0.0` and capture the resulting evidence under `docs/release-evidence/`.
3. If holding, create the next focused plan from the remaining validated CC review findings instead of rerunning broad gates.

## Operating Rule

Do not keep rerunning broad tests for status checks. Use same-HEAD evidence after the merge unless code changes. Run focused tests after edits, and run default+contract branch gates before the next merge/push/PR.
