# Testing strategy

Keep the default test suite fast enough to run after routine edits. Run broader
checks through their named opt-in tiers.

## Current commands

Use `cargo xtask test list` to print the tier names.

- Default: `cargo xtask test default`
- One language: `cargo xtask test language rust`
- Golden fixtures: `cargo xtask test golden`
- Capability and pending-shape contracts: `cargo xtask test capability`
- Extractor contract: `cargo xtask test contract`
- Parser certification: `cargo xtask test certification`
- Writer performance floors: `cargo xtask test perf`
- Changed paths: `cargo xtask test changed <path>...`
- Real-world smoke fixtures: `cargo xtask test real-world-smoke`
- Real-world release fixtures: `cargo xtask test real-world-release`
- Release package manifest: `cargo xtask release package-list`
- Dependency policy: `cargo deny check`
- Language quality and applicability: `node scripts/language-data-quality-report.mjs --strict --check-applicability`
- Real-source coverage samples: `cargo xtask coverage --language <language> --binary <path>` or `cargo xtask coverage --all --binary <path>`
- Coverage matrix: `node scripts/check-language-coverage-matrix.mjs`
- Coverage matrix regression tests: `node --test scripts/check-language-coverage-matrix.test.mjs`
- Report regression tests: `node --test scripts/language-data-quality-report.test.mjs`
- Dogfood this repo: `cargo xtask dogfood repo --root . --out-dir target/dogfood/julie-extractors`

The default command runs `cargo test` for `julie-extractors`,
`julie-extract-artifact`, and `julie-extract-cli`, plus xtask coverage,
comparison, integration-tier, and tier-routing contract tests. It does not
enable the golden or pending-shape features, run the capability matrix,
certify or upgrade parsers, consume downstream smoke fixtures, scan real-world
repositories, scan this repository, run release packaging, or run performance
comparisons. Those checks use named opt-in tiers or workflows.

## Default tier

Target: fast enough for agents to run after normal edits. The warm default-tier
median goal is at most 180 seconds on the recorded reference host. CI reports
the tier wall clock but does not fail on elapsed time. Treat a warm median above
three minutes as a performance defect and fix the cause without dropping tests.

Contains:

- focused unit tests for touched extractor helpers
- CLI argument/report contract tests with tiny fixtures
- schema writer/readback tests with tiny fixtures
- convention tests that enforce test categorization

Does not contain:

- full golden corpus
- parser upgrade matrix
- real-world corpus
- downstream smoke consumers
- release packaging
- large repo scans, including dogfood scans of this repository

## Language tier

Runs one language at a time. It selects that language's unit-test filter and
its registered golden fixtures.

Examples:

- one language unit suite
- one language golden fixture set
- one language parser diagnostics gate

Agents should usually run this tier during extractor work.

Current exact form:

```bash
cargo xtask test language rust
```

For QML and QML module manifests, run both registered targets:

```bash
cargo xtask test language qml
cargo xtask test language qmldir
```

Each target runs its language unit filter and an environment-scoped golden
check. See [the QML support record](languages/qml.md) for fixture and
real-world evidence. This command does not run the full golden suite or parser
certification.

For Go module manifests, `cargo xtask test language gomod` is the target. See
[the Go module manifest support record](languages/gomod.md). For `go.sum`
checksum files, `cargo xtask test language gosum` is the target. See
[the Go checksum support record](languages/gosum.md).

For Qt C++, `cargo xtask test language cpp` is the target. See
[the Qt C++ support record](languages/cpp-qt.md) for the macro pre-pass, the
`qt_header` fixture, and real-world evidence.

## Real-source coverage samples

`cargo xtask coverage` checks pinned source samples through the `julie-extract`
CLI and the SQLite artifact. It validates sample hashes, expected facts,
negative controls, diagnostics, and scan health. The command requires a built
extractor binary. Choose exactly one language or all languages. `--manifest`
is optional and defaults to `fixtures/real-world/coverage-manifest.json`.

```bash
cargo build --release -p julie-extract-cli --bin julie-extract
cargo xtask coverage --language sql --binary target/release/julie-extract
cargo xtask coverage --all --binary target/release/julie-extract
```

The manifest records source provenance and licenses with expected positive and
negative facts. This command is a separate opt-in check. It does not run from a
language-tier test or the default tier.

## Contract tier

Runs artifact-facing behavior:

- CLI scan/update/delete/info
- SQLite schema compatibility
- required SQLite indexes and query-plan checks
- batched writer behavior for scan/update/delete with tiny fixtures
- JSON report shape
- downstream smoke consumers

The `julie-extract` CLI and SQLite artifact are the product boundary. SQLite is
the only durable output. JSON reports are command output, not another artifact
store. This tier protects downstream consumers such as code-kb.

Current form:

```bash
cargo xtask test contract
```

This runs golden fixtures, capability matrix checks, pending-shape checks, and
the downstream smoke consumer, plus the SQLite schema and JSON report contract
tests for `julie-extract-artifact`, and the CLI, path-policy, operations, and
Flask route artifact contract tests for `julie-extract-cli`.

The retired resolution tests are not part of this tier. The current
`cargo xtask coverage` sample check is a separate opt-in command.

## Writer performance tier

Runs the feature-gated synthetic writer throughput floors:

```bash
cargo xtask test perf
```

This opt-in tier stays out of the default and contract tiers because wall-clock
floors are intentionally generous and host-sensitive.

The Python SQLite consumer example is a downstream smoke check for non-Rust
artifact readers:

```bash
python3 examples/python/sqlite_consumer.py target/dogfood/julie-extractors/artifact.sqlite
```

## Certification tier

Runs capability matrix checks, pending-shape checks, parser certification, and
parser upgrade checks.

This tier is required before parser dependency changes and release candidates,
not during every edit.

Current form:

```bash
cargo xtask test certification
```

## Changed-path tier

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

## Real-world tiers

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

## Release tier

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

## Dogfood gate

Dogfood scans this repository through the public `julie-extract` CLI, immediately
rescans the same SQLite artifact to prove the incremental `no_change` path,
then validates the generated SQLite artifact, JSON reports, and report-only
performance metrics.

Current form:

```bash
cargo xtask dogfood repo --root . --out-dir target/dogfood/julie-extractors
```

This gate is not part of the default tier. It is release-readiness evidence and
should run intentionally when extraction, CLI, artifact, report, or release
evidence behavior changes. Its hard evidence includes the cold scan
`ok` report and immediate rescan `no_change` report; timings are report-only.
v0.1.0 dogfood evidence is recorded in `docs/release-evidence/v0.1.0-dogfood.md`.

## Performance evidence

### Report-only baseline

The baseline command runs a release binary through the dogfood validator at
least three times. It records min, median, and max metrics, but does not compare
two binaries or enforce performance limits.

```bash
cargo build --release -p julie-extract-cli --bin julie-extract
cargo xtask performance baseline --root . --out-dir target/performance/julie-extractors-baseline --binary target/release/julie-extract --runs 3
```

This local command is outside regular CI. Its results are evidence for a
specific binary and repository tree, not a comparison pass.

### Pinned comparison

The comparison harness needs release binaries with adjacent `.build.json`
metadata sidecars. The build process supplies each sidecar. Its required
SHA-256 binds the binary to source commit and tree, dirty-state hash when
present, toolchain, host and target, profile, features, options, and build
recipe.

Capture a baseline receipt from a pinned binary and manifest:

```bash
cargo xtask performance baseline-receipt \
  --binary <release-binary> \
  --build-metadata-sha256 <sha256> \
  --manifest fixtures/real-world/coverage-manifest.json \
  --manifest-sha256 <sha256> \
  --out-dir <new-directory>
```

`--build-metadata <path>` selects a sidecar when it is not next to the binary;
`--language <name>` limits the receipt to one language. The receipt records
expected semantic rows and diagnostics for `references` and `full` scans across
fresh scans, no-change scans, and single-file updates. The comparison checks
incremental results against fresh output.

Compare the pinned baseline and candidate with:

```bash
cargo xtask performance compare \
  --baseline <baseline-binary> \
  --candidate <candidate-binary> \
  --baseline-expectations <receipt> \
  --baseline-expectations-sha256 <sha256> \
  --baseline-build-metadata-sha256 <sha256> \
  --candidate-build-metadata-sha256 <sha256> \
  --manifest fixtures/real-world/coverage-manifest.json \
  --out-dir <new-directory>
```

If sidecars are not adjacent to their binaries, pass
`--baseline-build-metadata <path>` and
`--candidate-build-metadata <path>`. The comparison can also take
`--language <name>` or `--profile <name>`. The latter must match both pinned
build records.

The comparison runs at `references` and `full` levels for fresh scans,
no-change scans, and single-file updates. Each workload gets one warm-up and
five paired samples. Small workloads are batched to at least two seconds, up to
4,096 operations. A count is accepted only after two consecutive baseline
calibration probes both reach two seconds and satisfy the existing spread
limit. Otherwise the count doubles within the cap; measured baseline samples
still have to meet the floor and spread checks. Comparison summary format v2
records every calibration round and probe time, including rejected counts.
Baseline receipts, build sidecars and row allowlists retain format v1. It records binary, build, manifest, host, storage, cache, and
RSS measurement provenance. Baseline and candidate must use matching toolchain,
profile, operating system, CPU, storage, cache policy, jobs, and memory.

Timing fails only when growth is above both 10% and 100 ms. Peak RSS fails above
both 15% and 32 MiB; artifact size fails above both 10% and 1 MiB. A sample is
inconclusive when its spread exceeds the greater of 10% of its median or 20 ms.
Unexplained semantic row changes fail. `--allowlist <path>` allows exact
versioned row changes tied to `docs/contracts/extraction-output-changes.md`.
Each entry pins the canonical row, hash, count, unique ID, ledger marker, and
optional language, level and workload scope. The manifest and baseline receipt
pin the source states checked by the comparison. The comparison records the
allowlist and ledger hashes and rejects unused entries. The dispatch workflow
also requires the expected allowlist hash. Optional budget approvals require both
`--budget-approvals <path>` and `--budget-approvals-sha256 <sha256>`. Each
approval pins its owner, evidence hash, binary hashes, manifest, language,
level, workload, metric, and exact added rows.

### Writer current-schema guard

This command writes a deterministic synthetic workload through the SQLite
artifact writer for the current schema:

```bash
cargo xtask performance writer-current-schema --out-dir target/performance/writer-current-schema
```

The command writes `artifact.sqlite` and
`writer-current-schema-summary.json` under the requested output directory. The
summary records input dimensions, row totals by domain, write time, rows per
second, and artifact size. Successful artifact creation and non-empty current
schema child-row domains are hard evidence. Timing, throughput, and artifact
size are report-only. This local release-evidence command is outside regular CI
and the default and contract tiers.

## CI policy

The `fast-gates` job runs:

```bash
cargo fmt --check
cargo metadata --format-version 1
cargo deny check --all-features
cargo clippy --workspace --all-targets --all-features --no-deps -- -D warnings
scripts/check-agent-doc-sync.sh
node scripts/language-data-quality-report.mjs --strict
node --test scripts/language-data-quality-report.test.mjs
node --test scripts/check-language-coverage-matrix.test.mjs
node scripts/check-language-coverage-matrix.mjs
node scripts/language-data-quality-report.mjs --strict --check-applicability
cargo test -p xtask
cargo xtask test default
cargo xtask test contract
```

The separate `extractor-compat` job downloads the most recent published release
binary and compares its extraction output with this tree. It needs network
access and fails when no release exists yet.

The `Performance Floor` workflow is manual through `workflow_dispatch`. Its
writer-floor job always runs `cargo xtask test perf`. The pinned extractor
comparison runs only when `run_comparison` is enabled and its binary, metadata,
manifest, receipt, and optional allowlist or budget approvals pass hash checks.
The comparison runs on the workflow's Linux host. No final comparison metrics
are claimed here.

The `Specialist Gates` workflow runs certification, real-world smoke/release,
and dogfood gates nightly and on manual dispatch. Release package staging runs
on manual dispatch only. Their local commands are:

```bash
cargo xtask test certification
cargo xtask test real-world-smoke
cargo xtask test real-world-release
cargo xtask dogfood repo --root . --out-dir target/dogfood/julie-extractors
cargo xtask release package --version <version> --target <target> --out-dir <path> --binary <path>
```

## Guardrails

- Keep test-tier pass/fail independent of machine-dependent wall-clock time.
  The default and `changed` tiers print their wall clock as an informational
  line (`test tier wall clock: <seconds>s`) at the end of execution; a run with
  any failed command is suffixed
  `(partial; <failed> of <total> command(s) failed)` so an aborted tier is not
  mistaken for a full one. This timing is report-only and never affects the
  exit code. A tier runs every command and reports all failures rather than
  stopping at the first one. Treat growth past 3 minutes warm as a defect to
  fix, not a gate.
- Measure default-tier timing locally on a stable machine, with the same
  toolchain and warmed build cache, using three runs such as:

  ```bash
  for run in 1 2 3; do /usr/bin/time -p cargo xtask test default; done
  ```

  Record the local min/median/max when timing is an acceptance criterion; do
  not use GitHub Actions duration as a performance gate.
- Keep the writer performance floor opt-in because it uses host-sensitive
  wall-clock limits; it detects collapse, not moderate regressions.
- Add convention tests that fail if slow tests enter default.
- Add contract tests that fail when required schema indexes are missing.
- Add a performance gate that detects per-row commits in the SQLite writer.
- Require every slow test to carry a category marker.
- Keep exact per-language commands documented.
- Workers run narrow tests; lead sessions own broad gates.
- Keep `crates/julie-extractors/src/tests/test_tiers.rs` green; it fails if
  known slow gates are no longer feature-gated out of the default suite.

## Coverage evidence and applicability checks

`--strict` fails on silent capability cells and quality-bar debt.
`--strict --check-applicability` also requires a classification for each
language/domain pair and useful golden evidence for each advertised supported
kind. It rejects conflicting classifications and unsupported capability
claims. Test-detection roles are checked per unit by the capability contract
tests. The eleven retained consumer/context gaps remain visible without
becoming strict-mode failures.

CI runs both report commands and the report's Node regression tests. Run those
tests when changing fixture classification or proof predicates. The
[coverage evidence decision](decisions/2026-11-03-language-coverage-evidence.md)
records the manifest/schema representation boundaries.

The matrix validator's fast mutation tests run with
`node --test scripts/check-language-coverage-matrix.test.mjs`; regular CI runs
these tests and validates the complete matrix. The validator reads the registry
languages and the canonical observed-domain list. It checks matrix completeness, evidence pointers, named
Rust tests, source and fixture paths, golden registrations, retained-gap
identities, and local-defect dispositions.

Run the complete matrix check with:

```bash
node scripts/check-language-coverage-matrix.mjs
```

The default input is `fixtures/extraction/validation-matrix.json`. The full
matrix has one cell for each of 42 registry languages and 15 observed domains,
for 630 cells. The checker fails while the matrix is missing, incomplete, or has
invalid evidence. `--matrix <path>` selects a different JSON file.
`--markdown <path>` writes a deterministic table after validation succeeds;
the output path must differ from the input.
CI regenerates the committed Markdown view and fails if it differs.

For example:

```bash
node scripts/check-language-coverage-matrix.mjs \
  --matrix fixtures/extraction/validation-matrix.json \
  --markdown target/validation-matrix.md
```
