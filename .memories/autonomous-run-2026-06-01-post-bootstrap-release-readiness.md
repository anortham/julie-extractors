# Autonomous Run Report: Post-Bootstrap Release Readiness

## Status

Ready for review.

## Plan

- Plan: `docs/plans/2026-06-01-post-bootstrap-stabilization-release-readiness.md`
- Branch: `codex/post-bootstrap-release-readiness`
- Base branch: `main`
- Base commit: `300d1d925a218392662dd8a6d256df42d72bdbfa`
- Verified head before first push: `5d8e73d2d8b77236992328070c42acc1727b50bf`
- Pull request: https://github.com/anortham/julie-extractors/pull/1

## What Shipped

- Added the post-bootstrap stabilization and release-readiness plan.
- Added dev-only project MCP configuration in `.mcp.json` pointing Julie MCP tooling at this repo.
- Split `xtask` command routing into focused modules.
- Added `cargo xtask dogfood repo` to produce repeatable dogfood extraction evidence.
- Added `cargo xtask release package` to stage release package manifests, checksums, docs, and binary payloads.
- Added CI and manually dispatched specialist release-readiness gates.
- Added v0.1.0 release evidence, release documentation, and release notes.
- Added a Python SQLite consumer example with contract tests.
- Fixed SQLite writer and CLI issues uncovered during dogfood:
  - duplicate extractor identifiers are deduplicated before artifact writes;
  - child rows with missing required references are skipped instead of aborting valid extraction rows.

## Verification

Fresh branch gate on `5d8e73d2d8b77236992328070c42acc1727b50bf`:

- `cargo fmt --check` passed.
- `cargo test -p xtask` passed.
- `cargo xtask test default` passed.
- `cargo xtask test contract` passed.
- `git diff --check` passed.

Specialist and release-readiness evidence run earlier on this branch:

- `cargo metadata --format-version 1 --no-deps` passed.
- `cargo xtask test certification` passed.
- `cargo xtask test real-world-smoke` passed.
- `cargo xtask test real-world-release` passed.
- `cargo xtask dogfood repo --root . --out-dir target/dogfood/julie-extractors` passed.
- `cargo xtask release package --version 0.1.0 --target aarch64-apple-darwin --out-dir target/release-package/v0.1.0-aarch64-apple-darwin --binary target/release/julie-extract` passed.
- `python3 examples/python/sqlite_consumer.py target/dogfood/julie-extractors/artifact.sqlite` passed.

## Dogfood Evidence

Recorded in `docs/release-evidence/v0.1.0-dogfood.md`:

- Files: `1006`
- Symbols: `32708`
- JSONL records: `213222`
- SQLite bytes: `136564736`
- JSONL bytes: `184001656`
- Scan: `18189` ms
- Info: `6` ms
- Export: `76175` ms
- Export throughput: `1853.483539132093` rows/sec

## Boundary Checks

- No generated dogfood or release package artifacts are tracked.
- Product code remains focused on `source tree -> versioned extraction artifact`.
- No MCP server, daemon, search, embedding, watcher, dashboard, or editing behavior was added as product behavior.
- The `.mcp.json` file is project tooling only.
- `/Users/murphy/source/julie` remained read-only.

## Diff Summary

`git diff --stat 300d1d925a218392662dd8a6d256df42d72bdbfa..HEAD` before the report commit:

- 40 files changed.
- 3940 insertions.
- 77 deletions.

Primary touched areas:

- `.github/workflows/`
- `.mcp.json`
- `crates/julie-extract-artifact/`
- `crates/julie-extract-cli/`
- `docs/`
- `examples/python/`
- `xtask/`

## Blockers Hit

None.

## Next Steps

- Review https://github.com/anortham/julie-extractors/pull/1.
- Merge after review.
