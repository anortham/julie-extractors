# 2026-06-09 Writer Current-Schema Performance Guard

## Scope

Implement TODO item 6 as the first backlog slice. This slice adds a
non-default performance guard for the SQLite artifact writer against a large
synthetic current-schema workload.

The guard protects the write path used by `julie-extract` without changing the
artifact schema, extractor behavior, JSONL export, or downstream Miller/Eros
contracts.

## Problem

The current writer has the important large-write protections:

- one transaction for scan writes
- cached prepared statements
- WAL/NORMAL SQLite pragmas
- spooled scan input support
- temp-table symbol lookup for requested symbol ids
- inline foreign-key resolution for symbols and identifiers
- deferred foreign-key checks for spooled bulk writes

The remaining gap is evidence. Existing writer batching contract tests are useful
fast tripwires, but they only cover small synthetic workloads and do not
exercise every current v3 child-row domain. A regression in a newly indexed
child table could make large repository artifact writes expensive before Miller
has a chance to index or search the result.

## Design

Add a new `cargo xtask performance writer-current-schema` command.

The command will:

1. Generate a deterministic synthetic artifact workload in memory.
2. Write it through `julie_extract_artifact::writer::ArtifactWriter` into a real
   SQLite database under `target/`.
3. Cover all current v3 row domains:
   `files`, `symbols`, `symbol_annotations`, `identifiers`, `relationships`,
   `pending_relationships`, `type_facts`, `type_argument_usages`,
   `type_arguments`, `literals`, `source_regions`, `structural_facts`,
   `complexity_metrics`, and `parse_diagnostics`.
4. Record row totals, elapsed write time, rows per second, database size, input
   dimensions, and output path in a JSON summary.
5. Exit non-zero only for invalid arguments or failed artifact writes.

The command is report/evidence tooling, not a regular CI gate. It should live
beside the existing `performance baseline` command because both commands are
release-evidence tools and both intentionally write output under `target/`.

## CLI Shape

Initial command:

```bash
cargo xtask performance writer-current-schema \
  --out-dir target/performance/writer-current-schema \
  --files 10000 \
  --symbols-per-file 8 \
  --identifiers-per-file 24 \
  --source-regions-per-file 12
```

Defaults should be large enough to catch obvious scaling mistakes but small
enough to run on a developer laptop when invoked intentionally. The exact
defaults can be adjusted during implementation after a local smoke run.

Supported arguments:

- `writer-current-schema`: subcommand name.
- `--out-dir <path>`: required output directory under `target/` by convention.
- `--files <n>`: optional positive integer.
- `--symbols-per-file <n>`: optional positive integer.
- `--identifiers-per-file <n>`: optional positive integer.
- `--source-regions-per-file <n>`: optional positive integer.

The generated SQLite database should be named `artifact.sqlite`. The summary
should be named `writer-current-schema-summary.json`.

## Fixture Shape

Each generated file should include:

- one file row with stable path, language, content hash, byte count, line count,
  indexed timestamp, and indexed status
- several symbols with body spans and body hashes when the artifact model
  supports them
- symbol annotations tied to symbols
- identifiers that exercise containing-symbol and target-symbol lookup
- relationships between local symbols
- pending relationships for external target names
- type facts attached to symbols
- type argument usages and type arguments
- literals tied to containing symbols
- source regions for comments, doc comments, and string literals
- parse diagnostics

The fixture must be deterministic so repeated runs can compare row totals and
database sizes. It does not need real source text because this guard targets the
writer, not parser extraction quality.

## Module Changes

- `xtask/Cargo.toml`: add a dependency on `julie-extract-artifact` so the
  command can use the public writer and model types directly.
- `xtask/src/performance.rs`: add planning, execution, summary structs, argument
  parsing, metric aggregation, and JSON writing for `writer-current-schema`.
- `xtask/tests/performance_baseline_contract.rs`: extend or split tests to cover
  the new command parser and summary serialization.
- `xtask/tests/commands_contract.rs`: prove `cargo xtask performance
  writer-current-schema` routes through the performance module before the test
  tier parser.
- `docs/testing-strategy.md`: document the manual command, its non-default
  status, and the release baseline matrix.
- `TODO.md`: after implementation and verification, mark item 6 done with the
  command and evidence used.

## Architecture Quality

Affected modules:

- `xtask/src/performance.rs`
- `xtask/tests/performance_baseline_contract.rs`
- `xtask/tests/commands_contract.rs`
- `docs/testing-strategy.md`
- `xtask/Cargo.toml`

Caller-facing interface:

- `cargo xtask performance writer-current-schema ...`

Depth/locality check:

- The change stays in xtask and test/docs surfaces.
- The artifact writer remains the production interface under test.
- No SQLite schema, extractor, JSONL, or CLI artifact contract changes are
  needed.

Test surface:

- Parser and summary unit tests in `cargo test -p xtask`.
- Existing tiny writer batching contract tests remain in the normal artifact test
  suite.
- The large generated workload is invoked manually as release evidence.

Rejected shortcuts:

- Do not add an ignored giant `#[test]`; it gives weaker reporting and is easy
  to skip without capturing evidence.
- Do not rely only on real-repo dogfood baselines; they prove end-to-end
  behavior but do not isolate writer scaling.
- Do not add timing thresholds to regular CI; host variance would make that
  brittle.

Architecture risk: low to medium. The only new dependency is xtask depending on
the artifact crate to exercise the public writer/model API directly.

## Verification Plan

Focused verification:

```bash
cargo test -p xtask performance_baseline_contract
cargo test -p xtask commands_contract
cargo test -p julie-extract-artifact writer_batching_contract
```

Manual evidence run:

```bash
cargo xtask performance writer-current-schema \
  --out-dir target/performance/writer-current-schema
```

Broad verification before closing item 6:

```bash
cargo test -p xtask
cargo xtask test default
cargo xtask test contract
```

Release-evidence matrix:

```bash
cargo build --release -p julie-extract-cli --bin julie-extract
cargo xtask performance baseline --root ~/source/openclaw --out-dir target/performance/openclaw-baseline --binary target/release/julie-extract --runs 3
cargo xtask performance baseline --root ~/source/hermes-agent --out-dir target/performance/hermes-agent-baseline --binary target/release/julie-extract --runs 3
cargo xtask performance baseline --root ~/source/MyraNext --out-dir target/performance/myranext-baseline --binary target/release/julie-extract --runs 3
```

Run the `~/source/eros` baseline when local checkout state and time budget make
it useful for the release evidence.

## Acceptance Criteria

- `cargo xtask performance writer-current-schema --out-dir <path>` creates a
  real SQLite artifact and summary JSON under the requested output directory.
- The generated workload includes rows for every current v3 extraction child
  domain.
- The summary includes input dimensions, row totals by domain, elapsed write
  time, rows per second, artifact size, and output paths.
- Argument parsing rejects unknown arguments and invalid numeric values.
- The command is documented as non-default release-evidence tooling.
- Fast tests cover parser, summary serialization, and command routing.
- Existing default test tiers do not run the large workload.
- `TODO.md` item 6 is updated only after implementation and verification.

## Follow-On Backlog Sequence

After item 6 is implemented, the next slices should remain separate:

1. Item 9: clarify the existing `body_hash` contract before adding new clone
   fingerprint rows.
2. Item 8: design the first cross-language complexity metrics contract.
3. Item 7: design structural tree-sitter query facts after choosing a narrow
   first pattern/use case.
