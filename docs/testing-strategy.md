# Testing Strategy

The default test suite must stay fast. Julie's current pain point is not that it
has too much verification; it is that expensive verification became too easy to
run accidentally.

## Default Tier

Target: fast enough for agents to run after normal edits.

Contains:

- focused unit tests for touched extractor helpers
- CLI argument/report contract tests with tiny fixtures
- schema writer/readback tests with tiny fixtures
- convention tests that enforce test categorization

Does not contain:

- full golden corpus
- parser upgrade matrix
- real-world corpus
- release packaging
- large repo scans

## Language Tier

Runs one language or one capability area at a time.

Examples:

- one language unit suite
- one language golden fixture set
- one language parser diagnostics gate

Agents should usually run this tier during extractor work.

## Contract Tier

Runs artifact-facing behavior:

- CLI scan/update/delete/info
- SQLite schema compatibility
- JSON report shape
- JSONL export shape
- downstream smoke consumers

This tier protects Miller/Eros-style users.

## Certification Tier

Runs capability matrix, parser certification, and parser upgrade checks.

This tier is required before parser dependency changes and release candidates,
not during every edit.

## Real-World Tier

Runs selected real-world repositories.

Use this for release confidence and extractor quality audits. Keep smoke and
release profiles separate.

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
- Add convention tests that fail if slow tests enter default.
- Require every slow test to carry a category marker.
- Keep exact per-language commands documented.
- Workers run narrow tests; lead sessions own broad gates.
