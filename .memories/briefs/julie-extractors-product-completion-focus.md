---
id: julie-extractors-product-completion-focus
title: julie-extractors Product Completion Focus
status: active
created: 2026-06-01T13:24:06.835Z
updated: 2026-06-01T20:27:14.841Z
tags:
  - julie-extractors
  - product-bootstrap
  - performance
  - release
---

# julie-extractors Product Completion Focus

## Current State

- Pre-release audit on 2026-06-01 started from `e9d5601`, after PR #9 merge-status documentation landed.
- PR #9: https://github.com/anortham/julie-extractors/pull/9.
- Current release target: v2.0.0, chosen because the old Julie in-tree extractor crate had reached v1.22.0 and the standalone product should not publish below that line.
- Primary tracker: `docs/plans/2026-06-01-product-completion-tracker.md`.
- Completed slice plans:
  - `docs/plans/2026-06-01-release-blocker-review-fixes.md`
  - `docs/plans/2026-06-01-v2-0-0-version-and-test-role-contract.md`
- Product boundary remains: source tree -> versioned extraction artifact. SQLite primary, JSONL secondary, `julie-extract` CLI primary, Rust crate secondary.
- Full source file contents are intentionally not stored in SQLite, JSONL, or JSON reports. Consumers read the matching source tree when complete file text is needed.

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
- Post-merge status commit `e9d5601` passed GitHub Fast Gates in run `26776385538`.
- No open pull requests at audit time.
- `gh release list` and `git tag --list 'v*'` returned no releases or tags at audit time.
- `cargo xtask release package-list` passed and showed the v2.0.0 package includes the binary, checksum, contracts, release docs, and release notes.
- Pre-release audit docs verification passed locally:
  - `git diff --check`
  - `scripts/check-agent-doc-sync.sh`
  - `cargo xtask release package-list`
  - `cargo xtask test default`
  - `cargo xtask test contract`

## Hard Boundaries

- Do not add Julie MCP/server/daemon/search/embedding/watcher/dashboard/editing behavior.
- Do not write to `/Users/murphy/source/julie` unless explicitly asked.
- Do not change public CLI, SQLite, JSONL, or report contracts without a tracked plan.
- Keep default tests fast; dogfood, certification, real-world, and package staging stay specialist gates.

## Next Decision

1. Decide whether to trigger the Release Binaries workflow for `v2.0.0` or hold the release candidate.
2. If proceeding, run the workflow for `2.0.0` from current `main` and capture the resulting evidence under `docs/release-evidence/`.
3. If public GitHub release assets are required, manually promote the uploaded Actions artifacts into a GitHub Release because the workflow stages artifacts only.

## Operating Rule

Do not keep rerunning broad tests for status checks. Use same-HEAD evidence after the merge unless code changes. Run focused tests after edits, and run default+contract branch gates before the next merge/push/PR.
