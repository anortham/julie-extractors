# Testing Strategy

The default test suite must stay fast. Julie's current pain point is not that it
has too much verification; it is that expensive verification became too easy to
run accidentally.

## Current Commands

Use `cargo xtask test list` to print the tier names.

- Default: `cargo xtask test default`
- One language: `cargo xtask test language rust`
- Golden fixtures: `cargo xtask test golden`
- Capability matrix: `cargo xtask test capability`
- Extractor contract: `cargo xtask test contract`
- Parser certification: `cargo xtask test certification`
- Real-world fixtures: `cargo xtask test real-world`

The default command expands to fast package tests for `julie-extractors`,
`julie-extract-artifact`, and `julie-extract-cli`. Slow and contract-heavy
gates are selected by Cargo features or named test targets, so plain default
runs do not include golden fixtures, capability matrix scans, parser-upgrade
checks, downstream smoke consumers, or real-world fixtures.

## Default Tier

Target: fast enough for agents to run after normal edits.

Contains:

- focused unit tests for touched extractor helpers
- CLI argument/report contract tests with tiny fixtures
- schema writer/readback tests with tiny fixtures
- tiny-fixture writer performance tripwires for obvious regressions
- convention tests that enforce test categorization

Does not contain:

- full golden corpus
- parser upgrade matrix
- real-world corpus
- downstream smoke consumers
- release packaging
- large repo scans

## Language Tier

Runs one language or one capability area at a time.

Examples:

- one language unit suite
- one language golden fixture set
- one language parser diagnostics gate

Agents should usually run this tier during extractor work.

Current exact form:

```bash
cargo xtask test language rust
```

## Contract Tier

Runs artifact-facing behavior:

- CLI scan/update/delete/info
- SQLite schema compatibility
- required SQLite indexes and query-plan checks
- batched writer behavior for scan/update/delete with tiny fixtures
- JSON report shape
- JSONL export shape
- downstream smoke consumers

This tier protects Miller/Eros-style users.

Current form:

```bash
cargo xtask test contract
```

This runs golden fixtures, capability matrix checks, and the downstream smoke
consumer, plus the SQLite schema, JSON report, and JSONL contract tests for
`julie-extract-artifact` and the CLI contract tests for `julie-extract-cli`.

## Certification Tier

Runs capability matrix, parser certification, and parser upgrade checks.

This tier is required before parser dependency changes and release candidates,
not during every edit.

Current parser-upgrade form:

```bash
cargo xtask test certification
```

## Real-World Tier

Runs selected real-world repositories.

Use this for release confidence and extractor quality audits. Keep smoke and
release profiles separate.

Current fixture-backed form:

```bash
cargo xtask test real-world
```

## Release Tier

Runs:

- default tier
- changed language tiers
- contract tier
- certification tier
- real-world smoke or release profile based on release type
- packaging checks for all target platforms

## Guardrails

- Add a default-suite wall-clock budget before implementation work grows.
- Add a tiny-fixture writer budget before the SQLite writer lands.
- Add convention tests that fail if slow tests enter default.
- Add contract tests that fail when required schema indexes are missing.
- Add a performance gate that detects per-row commits in the SQLite writer.
- Require every slow test to carry a category marker.
- Keep exact per-language commands documented.
- Workers run narrow tests; lead sessions own broad gates.
- Keep `crates/julie-extractors/src/tests/test_tiers.rs` green; it fails if
  known slow gates are no longer feature-gated out of the default suite.
