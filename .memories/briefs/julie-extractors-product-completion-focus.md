---
id: julie-extractors-product-completion-focus
title: julie-extractors Product Completion Focus
status: active
created: 2026-06-01T13:24:06.835Z
updated: 2026-06-01T20:52:23.564Z
tags:
  - julie-extractors
  - product-bootstrap
  - performance
  - release
---

# julie-extractors Product Completion Focus

## Current State

- Pre-release audit on 2026-06-01 started from `e9d5601`, after PR #9 merge-status documentation landed; workflow publishing upgrade is now in progress after the user asked to mirror Julie's working release setup.
- PR #9: https://github.com/anortham/julie-extractors/pull/9.
- Current release target: v2.0.0, chosen because the old Julie in-tree extractor crate had reached v1.22.0 and the standalone product should not publish below that line.
- Primary tracker: `docs/plans/2026-06-01-product-completion-tracker.md`.
- Completed slice plans:
  - `docs/plans/2026-06-01-release-blocker-review-fixes.md`
  - `docs/plans/2026-06-01-v2-0-0-version-and-test-role-contract.md`
- Product boundary remains: source tree -> versioned extraction artifact. SQLite primary, JSONL secondary, `julie-extract` CLI primary, Rust crate secondary.
- Full source file contents are intentionally not stored in SQLite, JSONL, or JSON reports. Consumers read the matching source tree when complete file text is needed.

## Release Workflow Direction

- Julie's working release workflow builds and publishes four assets: Linux x86_64, macOS Apple Silicon, macOS Intel, and Windows x86_64.
- `julie-extractors` should mirror that shape for `julie-extract` only, without Julie server/daemon/plugin behavior.
- The `Release Binaries` workflow now needs to be treated as a publishing workflow: build four target-specific binaries, stage packages with `cargo xtask release package`, archive each staged package, create or update GitHub Release `v{version}`, and upload release assets.
- Manual dispatch is retained for the first release path; tag pushes matching `v*` remain supported.

## Current Evidence

- PR #9 merged as `94f1661` on 2026-06-01 after GitHub Fast Gates passed.
- Post-merge status commit `e9d5601` passed GitHub Fast Gates in run `26776385538`.
- No open pull requests at audit time.
- `gh release list` and `git tag --list 'v*'` returned no releases or tags at audit time.
- `cargo xtask release package-list` passed and showed the v2.0.0 package includes the binary, checksum, contracts, release docs, and release notes.
- Release workflow upgrade local verification passed:
  - red test: `cargo test -p xtask --test commands_contract workflow_commands_keep_release_binary_workflow_explicit` failed before workflow changes on missing `contents: write`;
  - green test: same focused command passed after workflow/docs changes;
  - `ruby -e 'require "yaml"; YAML.load_file(".github/workflows/release-binaries.yml")'` passed;
  - `cargo fmt --all -- --check` passed;
  - `cargo test -p xtask` passed;
  - `cargo metadata --format-version 1` passed;
  - `cargo clippy --no-deps -p julie-extract-artifact -p julie-extract-cli -p xtask --lib --bins -- -D warnings` passed;
  - `scripts/check-agent-doc-sync.sh` passed;
  - `cargo xtask release package-list` passed;
  - `cargo xtask test default` passed;
  - `cargo xtask test contract` passed.

## Hard Boundaries

- Do not add Julie MCP/server/daemon/search/embedding/watcher/dashboard/editing behavior.
- Do not write to `/Users/murphy/source/julie` unless explicitly asked.
- Do not change public CLI, SQLite, JSONL, or report contracts without a tracked plan.
- Keep default tests fast; dogfood, certification, real-world, and package staging stay specialist gates.

## Next Decision

1. Land the release workflow publishing upgrade and confirm GitHub Fast Gates pass on `main`.
2. Trigger the Release Binaries workflow for `2.0.0` from current `main` or create/push tag `v2.0.0`.
3. Capture resulting four-platform release asset evidence under `docs/release-evidence/`.

## Operating Rule

Do not keep rerunning broad tests for status checks. Use same-HEAD evidence after the merge unless code changes. Run focused tests after edits, and run default+contract branch gates before the next merge/push/PR.
