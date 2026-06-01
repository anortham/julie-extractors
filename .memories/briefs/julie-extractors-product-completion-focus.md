---
id: julie-extractors-product-completion-focus
title: julie-extractors Product Completion Focus
status: active
created: 2026-06-01T13:24:06.835Z
updated: 2026-06-01T21:12:41.587Z
tags:
  - julie-extractors
  - product-bootstrap
  - performance
  - release
---

# julie-extractors Product Completion Focus

## Current State

- v2.0.0 is published: https://github.com/anortham/julie-extractors/releases/tag/v2.0.0.
- Release Binaries workflow run `26781742834` passed from `main` commit `a1f5069a36975e446c6a533e60bdcd3a9d3c11fa`.
- Remote tag `v2.0.0` points at `a1f5069a36975e446c6a533e60bdcd3a9d3c11fa`.
- Four GitHub Release assets are published: Linux x86_64, macOS Apple Silicon, macOS Intel, and Windows x86_64.
- Primary tracker: `docs/plans/2026-06-01-product-completion-tracker.md`.
- Release evidence: `docs/release-evidence/2026-06-01-v2-0-0-release.md`.
- Product boundary remains: source tree -> versioned extraction artifact. SQLite primary, JSONL secondary, `julie-extract` CLI primary, Rust crate secondary.
- Full source file contents are intentionally not stored in SQLite, JSONL, or JSON reports. Consumers read the matching source tree when complete file text is needed.

## Completed Release Scope

- Standalone contracts, repo bootstrap, Julie code migration, release readiness, release blocker fixes, v2.0.0 version alignment, and test-role contract alignment are complete.
- Release workflow publishing upgrade is complete: four platform-specific release archives are attached to the GitHub Release.
- The v2.0.0 release decision has been executed. There is no active pre-release implementation branch.

## Current Evidence

- PR #9 merged as `94f1661` on 2026-06-01 after GitHub Fast Gates passed.
- Post-merge status commit `e9d5601` passed GitHub Fast Gates in run `26776385538`.
- Release workflow upgrade commit `a1f5069` passed local gates before push: `cargo fmt --all -- --check`, `cargo test -p xtask`, `cargo metadata --format-version 1`, scoped `cargo clippy`, `scripts/check-agent-doc-sync.sh`, `cargo xtask release package-list`, `cargo xtask test default`, and `cargo xtask test contract`.
- GitHub Fast Gates passed for `a1f5069` before release in run `26781385166`.
- Release Binaries workflow run `26781742834` passed and published `v2.0.0`.
- GitHub release metadata confirms `v2.0.0` is published, non-draft, non-prerelease, and targets `a1f5069a36975e446c6a533e60bdcd3a9d3c11fa`.

## Hard Boundaries

- Do not add Julie MCP/server/daemon/search/embedding/watcher/dashboard/editing behavior.
- Do not write to `/Users/murphy/source/julie` unless explicitly asked.
- Do not change public CLI, SQLite, JSONL, or report contracts without a tracked plan.
- Keep default tests fast; dogfood, certification, real-world, and package staging stay specialist gates.

## Operating Rule

Do not keep rerunning broad tests for status checks. Use same-HEAD evidence after the merge unless code changes. Run focused tests after edits, and run default+contract branch gates before the next merge/push/PR.
