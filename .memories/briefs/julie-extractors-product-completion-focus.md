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

- `main` is at `bac074a` after PR #6 merged the JSONL export buffered writer.
- Active branch: `codex/repeatable-performance-baseline`.
- Completed historical plans: repo bootstrap, old Julie migration, post-bootstrap release readiness, release binaries workflow, incremental scan hash skip, dogfood rescan baseline, release-binary dogfood evidence, JSONL export performance plan, JSONL export buffering, repeatable performance baseline.
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
- JSONL export performance plan: `docs/plans/2026-06-01-jsonl-export-performance.md`.
- Slice 2 inspection evidence: SQLite counts `0.158s`, fetch all exported rows `0.763s`, release export to `/dev/null` `20.79s` real with `15.66s` sys; first target is buffered JSONL writes.
- JSONL export buffering evidence: `docs/release-evidence/2026-06-01-jsonl-export-buffering.md`.
- Slice 3 evidence in PR #6: bounded-write red test failed with `2853` writes for an `8558` byte fixture export; buffered release export to `/dev/null` measured `2.43s` real with `0.21s` sys against the same dogfood artifact.
- Repeatable performance baseline evidence: `docs/release-evidence/2026-06-01-repeatable-performance-baseline.md`.
- Slice 4 evidence: 3 release-profile dogfood-backed runs at `844f1bb`; cold scan min/median/max `6277ms` / `6387ms` / `7508ms`; no-change rescan `51ms` / `51ms` / `52ms`; JSONL export `1330ms` / `1330ms` / `1333ms`; stable output `1018` files, `33019` symbols, `215388` JSONL records.

## Next Slices In Order

1. v0.1.0 release candidate audit.

## Operating Rule

Do not keep rerunning broad tests for status checks. Use existing same-HEAD evidence. Run focused tests after edits, and run default+contract branch gates before merge/push/PR.
