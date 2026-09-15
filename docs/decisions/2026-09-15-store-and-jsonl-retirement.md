# Store and JSONL Retirement

Date: 2026-09-15
Status: accepted

## Decision

`code-kb` is the sole consumer of `julie-extract`. It calls `scan`, `update`,
and `delete` and reads the SQLite artifact. Miller, the consumer the versioned
family store and the JSONL export were built for, is retired.

The versioned family store (`store` command, the artifact-crate `store`
modules, the `MILLER_*` environment variables, and the store contracts) and
the JSONL export (`export` command, `jsonl.rs`, and the `jsonl-v*` contracts)
are removed. Removing CLI commands is a breaking change, so the release that
ships this is `3.0.0`.

## What stays

- `scan`, `update`, `delete`, `info`, `languages`, `rebind`.
- SQLite schema v7 unchanged. Every table stays; `code-kb` chooses what it
  reads through `--level`.
- The Rust crate API and the `syntax-api` feature.
- Contract docs for schema v1 to v6 and extracted-data v1 to v3 stay as
  history. The removed store and JSONL contract docs are recoverable from git.

## Why now

Every release still built, tested, documented, and packaged the store and the
export. The store is about 65k lines of source and tests, and its tests are the
slowest part of the default gate. Nothing consumes them.

## Consequences

- Downstream consumers that need row streaming read the SQLite artifact
  directly. There is no secondary export format.
- The default gate loses the store tiers; wall time is recorded before and
  after in the 3.0.0 release evidence.
- `code-kb` bumps its `julie-extract` pin to 3.0.0 with no other change.

Plan: `docs/plans/2026-09-15-remove-store-and-jsonl.md`.
