---
id: julie-extractors-product-completion-focus
title: julie-extractors Product Completion Focus
status: active
created: 2026-06-01T13:24:06.835Z
updated: 2026-06-01T13:24:06.835Z
tags:
  - julie-extractors
  - product-bootstrap
  - performance
  - release
---

# julie-extractors Product Completion Focus

## Current State

- `main` is at `875ee0f` after PR #3: dogfood now captures immediate no-change rescan evidence.
- Completed historical plans: repo bootstrap, old Julie migration, post-bootstrap release readiness, release binaries workflow, incremental scan hash skip, dogfood rescan baseline.
- Primary tracker: `docs/plans/2026-06-01-product-completion-tracker.md`.
- Product boundary remains: source tree -> versioned extraction artifact. SQLite primary, JSONL secondary, `julie-extract` CLI primary, Rust crate secondary.

## Hard Boundaries

- Do not add Julie MCP/server/daemon/search/embedding/watcher/dashboard/editing behavior.
- Do not write to `/Users/murphy/source/julie` unless explicitly asked.
- Do not change public CLI, SQLite, JSONL, or report contracts without a fresh strategy-tier plan.
- Keep default tests fast; dogfood, certification, real-world, and package staging stay specialist gates.

## Current Evidence

- Dogfood evidence: `docs/release-evidence/v0.1.0-dogfood.md`.
- PR #3 evidence: cold scan `18189ms`, no-change rescan `215ms`, export `76771ms`, rescan row writes all `0`.
- Release binary workflow evidence: `docs/release-evidence/2026-06-01-release-binaries-workflow.md`.

## Next Slices In Order

1. Release-binary dogfood evidence: build `target/release/julie-extract`, run dogfood with `--binary`, update evidence.
2. JSONL export performance plan: inspect exporter path before optimizing because export dominates runtime.
3. Repeatable performance baseline: repeated same-machine release-profile runs before thresholds.
4. v0.1.0 release candidate audit.

## Operating Rule

Do not keep rerunning broad tests for status checks. Use existing same-HEAD evidence. Run focused tests after edits, and run default+contract branch gates before merge/push/PR.
