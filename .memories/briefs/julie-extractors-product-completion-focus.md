---
id: julie-extractors-product-completion-focus
title: julie-extractors Product Completion Focus
status: active
created: 2026-06-01T13:24:06.835Z
updated: 2026-06-01T13:51:25.717Z
tags:
  - julie-extractors
  - product-bootstrap
  - performance
  - release
---

# julie-extractors Product Completion Focus

## Current State

- `main` was at `a3038ee` when Slice 1 started.
- Active branch: `codex/release-dogfood-evidence`.
- Completed historical plans: repo bootstrap, old Julie migration, post-bootstrap release readiness, release binaries workflow, incremental scan hash skip, dogfood rescan baseline.
- Primary tracker: `docs/plans/2026-06-01-product-completion-tracker.md`.
- Product boundary remains: source tree -> versioned extraction artifact. SQLite primary, JSONL secondary, `julie-extract` CLI primary, Rust crate secondary.

## Hard Boundaries

- Do not add Julie MCP/server/daemon/search/embedding/watcher/dashboard/editing behavior.
- Do not write to `/Users/murphy/source/julie` unless explicitly asked.
- Do not change public CLI, SQLite, JSONL, or report contracts without a fresh strategy-tier plan.
- Keep default tests fast; dogfood, certification, real-world, and package staging stay specialist gates.

## Current Evidence

- Debug dogfood evidence: `docs/release-evidence/v0.1.0-dogfood.md`.
- Release-binary dogfood evidence: `docs/release-evidence/2026-06-01-release-binary-dogfood.md`.
- Release dogfood metrics at `a3038ee`: cold scan `7607ms`, no-change rescan `52ms`, export `68983ms`, rescan row writes all `0`.
- Release binary: `target/release/julie-extract`, version `julie-extract 0.1.0`, SHA-256 `af51b3792e10eb54f6aab5d94cd04c257801b183be0fb23f08db96ba23f441ce`.
- Release binary workflow evidence: `docs/release-evidence/2026-06-01-release-binaries-workflow.md`.

## Next Slices In Order

1. JSONL export performance plan: inspect exporter path before optimizing because export dominates runtime.
2. Repeatable performance baseline: repeated same-machine release-profile runs before thresholds.
3. v0.1.0 release candidate audit.

## Operating Rule

Do not keep rerunning broad tests for status checks. Use existing same-HEAD evidence. Run focused tests after edits, and run default+contract branch gates before merge/push/PR.
