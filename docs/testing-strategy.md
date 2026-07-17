# Testing Strategy

The default test suite must stay fast. Julie's current pain point is not that it
has too much verification; it is that expensive verification became too easy to
run accidentally.

## Current Commands

Use `cargo xtask test list` to print the tier names.

- Default: `cargo xtask test default`
- One language: `cargo xtask test language rust`
- Golden fixtures: `cargo xtask test golden`
- Capability and pending-shape contracts: `cargo xtask test capability`
- Extractor contract: `cargo xtask test contract`
- Parser certification: `cargo xtask test certification`
- Changed paths: `cargo xtask test changed <path>...`
- Real-world smoke fixtures: `cargo xtask test real-world-smoke`
- Real-world release fixtures: `cargo xtask test real-world-release`
- Release package manifest: `cargo xtask release package-list`
- Dependency policy: `cargo deny check`
- Dogfood this repo: `cargo xtask dogfood repo --root . --out-dir target/dogfood/julie-extractors`

The default command expands to fast package tests for `julie-extractors`,
`julie-extract-artifact`, and `julie-extract-cli`. Slow and contract-heavy
gates are selected by Cargo features or named test targets, so plain default
runs do not include golden fixtures, capability matrix scans, pending-shape
checks, parser-upgrade checks, downstream smoke consumers, dogfood repo scans,
release packaging, or real-world fixtures.

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
- large repo scans, including dogfood scans of this repository

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

This runs golden fixtures, capability matrix checks, pending-shape checks, and
the downstream smoke consumer, plus the SQLite schema, JSON report, and JSONL
contract tests for `julie-extract-artifact` and the CLI contract, path-policy,
and operations contract tests for `julie-extract-cli`.

The Python SQLite consumer example is a downstream smoke check for non-Rust
artifact readers:

```bash
python3 examples/python/sqlite_consumer.py target/dogfood/julie-extractors/artifact.sqlite
```

## Certification Tier

Runs capability matrix checks, pending-shape checks, parser certification, and
parser upgrade checks.

This tier is required before parser dependency changes and release candidates,
not during every edit.

Current form:

```bash
cargo xtask test certification
```

## Changed-Path Tier

Runs the default tier and adds the full certification tier when changed files
can affect parser dependency behavior.

Current parser dependency triggers:

- `Cargo.lock`
- `crates/julie-extractors/Cargo.toml`
- `crates/julie-extractors/src/language_spec/**`
- `crates/julie-extractors/src/registry*`
- `crates/julie-extractors/src/tests/capability_matrix*`
- `crates/julie-extractors/src/tests/pending_shape_contract*`
- `fixtures/extraction/**`

Current form:

```bash
cargo xtask test changed crates/julie-extractors/Cargo.toml
```

## Real-World Tiers

Runs selected real-world repositories.

Use this for release confidence and extractor quality audits. Keep smoke and
release profiles separate.

Current smoke form:

```bash
cargo xtask test real-world-smoke
```

Current release form:

```bash
cargo xtask test real-world-release
```

`cargo xtask test real-world` is kept as an alias for the release profile.

## Release Tier

Runs:

- default tier
- changed language tiers
- contract tier
- certification tier
- real-world smoke or release profile based on release type
- packaging checks for all target platforms

The release package manifest is:

```bash
cargo xtask release package-list
```

The package list is constrained to `julie-extract` binaries, checksums,
contract and architecture docs, testing strategy docs, and versioned release
notes.

## Dogfood Gate

Dogfood scans this repository through the public `julie-extract` CLI, immediately
rescans the same SQLite artifact to prove the incremental `no_change` path,
then validates the generated SQLite artifact, JSON reports, JSONL export, and
report-only performance metrics.

Current form:

```bash
cargo xtask dogfood repo --root . --out-dir target/dogfood/julie-extractors
```

This gate is not part of the default tier. It is release-readiness evidence and
should run intentionally when extraction, CLI, artifact, JSONL, report, or
release evidence behavior changes. Its hard evidence includes the cold scan
`ok` report and immediate rescan `no_change` report; timings are report-only.
v0.1.0 dogfood evidence is recorded in `docs/release-evidence/v0.1.0-dogfood.md`.

Repeatable performance baselines run the release binary through the same dogfood
validator multiple times and aggregate report-only min/median/max metrics:

```bash
cargo build --release -p julie-extract-cli --bin julie-extract
cargo xtask performance baseline --root . --out-dir target/performance/julie-extractors-baseline --binary target/release/julie-extract --runs 3
```

This command is release-evidence tooling. It is not part of regular CI, and it
does not define hard timing thresholds.

The writer current-schema performance guard exercises the SQLite artifact writer
directly with a deterministic synthetic v2-schema workload:

```bash
cargo xtask performance writer-current-schema --out-dir target/performance/writer-current-schema
```

The command writes `artifact.sqlite` and
`writer-current-schema-summary.json` under the requested output directory. The
summary records input dimensions, row totals by domain, write elapsed time,
rows per second, and artifact size. Successful artifact creation and non-empty
current-schema child-row domains, including `source_regions`,
`structural_facts`, and `complexity_metrics`, are hard evidence; timing, rows
per second, and artifact size are report-only metrics. This guard is local
release-evidence tooling, not part of regular CI or the default/contract tiers.

## CI Policy

Regular CI runs only fast gates:

```bash
cargo fmt --check
cargo metadata --format-version 1
cargo test -p xtask
cargo xtask test default
cargo xtask test contract
```

Specialist gates are manual through `workflow_dispatch`:

```bash
cargo xtask test certification
cargo xtask test real-world-smoke
cargo xtask test real-world-release
cargo xtask dogfood repo --root . --out-dir target/dogfood/julie-extractors
cargo xtask release package --version <version> --target <target> --out-dir <path> --binary <path>
```

Repeatable performance baselines are local release-evidence runs unless a future
plan explicitly adds a dedicated workflow:

```bash
cargo xtask performance baseline --root . --out-dir target/performance/julie-extractors-baseline --binary target/release/julie-extract --runs 3
cargo xtask performance writer-current-schema --out-dir target/performance/writer-current-schema
```

## Guardrails

- Enforce the default-suite wall-clock budget (90s, `xtask/src/test_tiers.rs`)
  as implementation work grows.
- Add a tiny-fixture writer budget before the SQLite writer lands.
- Add convention tests that fail if slow tests enter default.
- Add contract tests that fail when required schema indexes are missing.
- Add a performance gate that detects per-row commits in the SQLite writer.
- Require every slow test to carry a category marker.
- Keep exact per-language commands documented.
- Workers run narrow tests; lead sessions own broad gates.
- Keep `crates/julie-extractors/src/tests/test_tiers.rs` green; it fails if
  known slow gates are no longer feature-gated out of the default suite.
