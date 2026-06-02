---
id: julie-extractors-product-completion-focus
title: julie-extractors Product Completion Focus
status: active
created: 2026-06-01T13:24:06.835Z
updated: 2026-06-02T19:39:28.214Z
tags:
  - julie-extractors
  - product-bootstrap
  - performance
  - release
---

# julie-extractors Product Completion Focus

## Current State

- v2.0.0 is published: https://github.com/anortham/julie-extractors/releases/tag/v2.0.0.
- v2.0.1 release candidate is prepared locally on `main` but not yet published.
- v2.0.1 candidate scope fixes the Eros SQLite write-path blocker, adds report profiling, prunes noisy external pending relationships, and adds a small artifact-writer prepared-statement cache tune.
- Primary v2.0.1 evidence: `docs/release-evidence/2026-06-02-v2-0-1-release-candidate.md`.
- v2.0.1 release notes: `docs/release-notes/v2.0.1.md`.
- Release Binaries workflow defaults now target `2.0.1`; README still points at the published v2.0.0 assets until v2.0.1 is actually published and asset checksums exist.
- Product boundary remains: source tree -> versioned extraction artifact. SQLite primary, JSONL secondary, `julie-extract` CLI primary, Rust crate secondary.
- Full source file contents are intentionally not stored in SQLite, JSONL, or JSON reports. Consumers read the matching source tree when complete file text is needed.

## Completed Release Scope

- Standalone contracts, repo bootstrap, Julie code migration, release readiness, release blocker fixes, v2.0.0 version alignment, and test-role contract alignment are complete.
- v2.0.0 release workflow publishing upgrade is complete: four platform-specific release archives are attached to the GitHub Release.
- v2.0.1 release-candidate verification is complete locally except the final commit/push/publish flow.

## Current Evidence

- v2.0.1 force-scan matrix with `julie-extract 2.0.1`:
  - Eros: ok, 611 files, 80,494 symbols, 5.10s wall time.
  - openclaw: ok, 12,781 supported files, 640,317 symbols, 87.88s wall time.
  - hermes-agent: ok, 2,588 files, 261,296 symbols, 30.35s wall time.
  - Newtonsoft.Json: ok, 981 files, 21,286 symbols, 6.62s wall time.
  - julie-extractors: ok, 1,035 files, 33,686 symbols, 6.79s wall time.
- v2.0.1 no-change rescans are cheap: openclaw 1.17s, hermes-agent 0.19s, Eros 0.05s.
- Local v2.0.1 gates passed: format, diff check, agent-doc sync, metadata, xtask tests, package-list, local package staging, default, contract, changed-path with certification trigger, real-world smoke, dogfood repo.
- Openclaw cold scan remains expensive because it extracts/writes roughly two million rows; defer deeper cold-scan optimization until after v2.0.1 unless the user reopens it as release-blocking.

## Hard Boundaries

- Do not add Julie MCP/server/daemon/search/embedding/watcher/dashboard/editing behavior.
- Do not write to `/Users/murphy/source/julie` unless explicitly asked.
- Do not change public CLI, SQLite, JSONL, or report contracts without a tracked plan.
- Keep default tests fast; dogfood, certification, real-world, and package staging stay specialist gates.

## Operating Rule

Do not keep rerunning broad tests for status checks. Use same-HEAD evidence after the v2.0.1 release-candidate commit unless code changes. Run focused tests after edits, and run default+contract branch gates before the next merge/push/PR.
