# Remove the versioned family store and JSONL export (3.0.0)

Date: 2026-09-15. Status: proposed, not started.

## Why

`code-kb` is the only consumer of `julie-extract`. It calls `scan`, `update`,
and `delete` and reads the SQLite artifact. It never calls `store` or `export`.
Miller, the consumer those were built for, is retired. Every release still
builds, tests, documents, and packages them. The store is about 65k lines of
source and tests, and its tests are the slowest part of the default gate.

Removing them is a breaking CLI change, so the release is `3.0.0`.

## What goes

### Versioned family store

- `crates/julie-extract-artifact/src/store/` (coordinator, maintenance,
  generation, connection, and the rest).
- `crates/julie-extract-cli/src/store/` (executor and command dispatch).
- `Command::Store` and `StoreArgs` in `crates/julie-extract-cli/src/args.rs`;
  the `Store` arm in `commands.rs`.
- Tests: every `store_*` file under `crates/julie-extract-artifact/tests/` and
  `crates/julie-extract-cli/tests/` (36 files).
- Env vars `MILLER_STORE_CHUNK_VERSIONS` and `MILLER_ALLOW_EXTRACTOR_DOWNGRADE`.
- CI: the `windows-capacity-store` job in `.github/workflows/ci.yml`.
- xtask: store tiers in `xtask/src/test_tiers.rs`, store parts of
  `xtask/src/performance.rs`, `xtask/src/dogfood.rs`, and
  `xtask/src/release.rs` (package manifest lists store docs).
- Docs: `docs/contracts/store-v1.md`, `docs/contracts/sqlite-store-schema-v1.md`
  and `-v2.md`, `docs/architecture/versioned-index-store.md`, the store section
  of `docs/contracts/cli.md`, the store paragraph and commands in `README.md`,
  `docs/README.md`, and `docs/site/index.html`.
- Dependencies to re-check after removal: `fs4`, `same-file`, `getrandom`,
  `time`, `rayon` in the CLI crate and `getrandom` in the artifact crate. Drop
  each one that has no remaining user.

### JSONL export

- `crates/julie-extract-artifact/src/jsonl.rs`.
- `Command::Export`, `ExportArgs`, and the `export` function in the CLI.
- Tests: `crates/julie-extract-artifact/tests/jsonl_contract.rs`,
  `crates/julie-extractors/src/tests/jsonl_pipeline.rs`,
  `crates/julie-extractors/src/tests/jsonl_invariants.rs`, the JSONL parts of
  `crates/julie-extractors/tests/downstream_smoke.rs` and
  `crates/julie-extract-cli/tests/cli_contract.rs` and
  `operations_contract.rs`.
- Docs: `docs/contracts/jsonl-v1.md` through `jsonl-v5.md`, the export rows in
  `docs/contracts/cli.md`, `docs/contracts/reports.md`, `README.md`,
  `docs/README.md`, and the JSONL export step in the dogfood evidence.
- `examples/` consumers that read JSONL, if any remain.

### Miller references

- 362 files under `crates/*/src` and `docs` mention Miller. Source comments and
  contract text that say "Miller consumes" or "Miller computes" change to say
  what the consumer does, or name `code-kb`. Historical release notes,
  decisions, and evidence stay as written.

## What stays

- `scan`, `update`, `delete`, `info`, `languages`, `rebind`.
- SQLite schema v7 unchanged. The `reference_sites`, `identifiers`,
  `source_regions`, and every other table stay; `code-kb` chooses what it reads
  through `--level`.
- The Rust crate API and the `syntax-api` feature.
- Old contract docs for schema v1 to v6 and extracted-data v1 to v3 (history).

## Order of work

Each step is one commit and leaves the default gate green.

1. Write `docs/decisions/2026-09-15-store-and-jsonl-retirement.md` (ADR):
   code-kb is the sole consumer; store and JSONL are retired; 3.0.0.
2. Remove the store CLI command, its executor, and its CLI tests. Remove the
   CI job. Update `cli.md` and `README.md`.
3. Remove the artifact-crate store modules and their tests. Remove
   `MILLER_*` env vars. Drop unused dependencies.
4. Remove the export command, `jsonl.rs`, and JSONL tests. Update `cli.md`,
   `reports.md`, `README.md`, `docs/README.md`.
5. Update xtask: test tiers, performance, dogfood, release package list.
   `cargo test -p xtask` and `cargo xtask release preflight --version 3.0.0`
   must pass.
6. Sweep Miller mentions in source and current contract docs.
7. Bump all three crates to `3.0.0`. Write `docs/release-notes/v3.0.0.md`
   (classification: breaking, CLI commands removed, artifact schema unchanged).
8. Release per `docs/release.md`. Then bump the `code-kb` pin.

## Acceptance

- `julie-extract --help` lists no `store` or `export` command.
- `rg -n "store|jsonl|MILLER" crates xtask --glob '*.rs'` returns only
  unrelated words (for example "restore").
- `cargo xtask test default` and `cargo xtask test contract` pass, on Linux and
  on Windows through `win-test`.
- `code-kb` suite passes against the 3.0.0 binary with no code-kb change other
  than the pin.
- Default gate wall time drops; record before and after in the release
  evidence.

## Estimate

Three to four agent sessions: one for the store CLI and CI, one for the
artifact-crate store and dependencies, one for JSONL, xtask, docs, and the
Miller sweep, and part of one for the release.
