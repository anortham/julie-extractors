# Migration Inventory

This inventory classifies source from `/Users/murphy/source/julie` for the new
`/Users/murphy/source/julie-extractors` product.

The goal is not to copy Julie wholesale. The goal is to move extraction-owned
code and redesign product-facing artifact layers around `julie-extract`.

## Move Mostly Intact

### Extractor Crate

Source:

- `/Users/murphy/source/julie/crates/julie-extractors/`

Includes:

- `Cargo.toml`
- `README.md`
- `src/base/`
- all language extractor modules
- `src/factory.rs`
- `src/language.rs`
- `src/language_spec/`
- `src/manager.rs`
- `src/pipeline.rs`
- `src/registry.rs`
- `src/test_calls.rs`
- `src/test_detection.rs`
- `src/utils/`
- `src/capability_snapshot.rs`
- `src/routing_identifiers.rs`
- `src/routing_relationships.rs`
- `src/routing_symbols.rs`
- `examples/extract_file.rs`
- `tests/downstream_smoke.rs`

Reason: this is already the reusable extraction engine and public Rust API.

### Extractor Tests

Source:

- `/Users/murphy/source/julie/crates/julie-extractors/src/tests/`

Reason: per-language unit, golden, capability, and parser-upgrade tests belong
with the extraction engine.

### Extraction Fixtures

Source:

- `/Users/murphy/source/julie/fixtures/extraction/`

Includes:

- `capabilities.json`
- `tree-sitter-real-world-corpus.toml`
- all `source.*` and `expected.json` fixtures

Reason: fixtures are product evidence. Keep the same relative path initially or
update all `include_str!` references in the same migration task.

### Language Configuration

Source:

- `/Users/murphy/source/julie/languages/*.toml`

Reason: extractor-adjacent features such as literal carrier classification and
test-role classification need language policy. Move or split the subset that is
artifact-producing extraction policy.

## Carry And Rewrite Docs

Source docs to import as references, then rewrite for the standalone product:

- `/Users/murphy/source/julie/docs/EXTERNAL_EXTRACT.md`
- `/Users/murphy/source/julie/docs/EXTRACTION_CONTRACT.md`
- `/Users/murphy/source/julie/docs/ADDING_NEW_LANGUAGES.md`
- `/Users/murphy/source/julie/docs/TREE_SITTER_QUALITY_BAR.md`
- `/Users/murphy/source/julie/docs/TREE_SITTER_UPGRADES.md`
- `/Users/murphy/source/julie/docs/LANGUAGE_CERTIFICATION_REPORT.md`
- `/Users/murphy/source/julie/docs/LANGUAGE_REAL_WORLD_EVIDENCE.md`
- `/Users/murphy/source/julie/docs/LANGUAGE_REAL_WORLD_EVIDENCE.json`
- `/Users/murphy/source/julie/docs/plans/2026-05-18-external-extractor-cli-design.md`
- `/Users/murphy/source/julie/docs/plans/2026-05-18-external-extractor-cli-implementation-plan.md`
- `/Users/murphy/source/julie/docs/plans/2026-05-11-julie-extractors-best-in-class-data-model.md`
- `/Users/murphy/source/julie/docs/plans/2026-05-29-extraction-enrichments-for-miller-bridge.md`
- `/Users/murphy/source/julie/docs/release-notes/v7.10.0.md`
- `/Users/murphy/source/julie/docs/release-notes/v7.13.2.md`

Rewrite goals:

- remove Julie MCP/server assumptions
- make `julie-extract` the product name
- define SQLite and JSONL as product artifacts
- preserve language capability evidence
- document migration differences from old Julie extract

## Redesign Into Product Modules

### CLI And Reports

Source:

- `/Users/murphy/source/julie/src/external_extract/*.rs`
- `/Users/murphy/source/julie/src/cli.rs`
- `/Users/murphy/source/julie/src/main.rs`

Do not copy blindly. Current code depends on Julie database, indexing core,
analysis, search config, and generic CLI output formatting. Extract behavior and
rewrite dependencies around the new product modules.

Product target:

- `julie-extract scan/update/delete/info/export/languages`
- stable JSON reports
- typed error codes
- stable exit code policy

### Artifact Writer

Source:

- `/Users/murphy/source/julie/src/database/schema.rs`
- `/Users/murphy/source/julie/src/database/migrations.rs`
- `/Users/murphy/source/julie/src/database/files.rs`
- `/Users/murphy/source/julie/src/database/identifiers.rs`
- `/Users/murphy/source/julie/src/database/relationships.rs`
- `/Users/murphy/source/julie/src/database/revisions.rs`
- `/Users/murphy/source/julie/src/database/revision_changes.rs`
- `/Users/murphy/source/julie/src/database/schema_enrichments.rs`
- `/Users/murphy/source/julie/src/database/types.rs`
- `/Users/murphy/source/julie/src/database/bulk/`
- `/Users/murphy/source/julie/src/database/symbols/`

Do not move the database crate wholesale. Extract only artifact-owned tables and
writers:

- files
- symbols
- symbol annotations
- identifiers
- relationships
- structured pending relationships
- types
- type arguments
- literals
- parse diagnostics
- repair/failure state if retained
- revisions
- metadata

Product target: a clean `extract-v1` SQLite schema, not Julie's internal schema.

### Indexing Layer

Source:

- `/Users/murphy/source/julie/src/indexing_core/batch.rs`
- `/Users/murphy/source/julie/src/indexing_core/discovery.rs`
- `/Users/murphy/source/julie/src/indexing_core/extraction.rs`
- `/Users/murphy/source/julie/src/indexing_core/paths.rs`
- `/Users/murphy/source/julie/src/indexing_core/persistence.rs`
- `/Users/murphy/source/julie/src/indexing_core/analysis.rs`
- `/Users/murphy/source/julie/src/tools/workspace/indexing/file_policy.rs`

Product target: source discovery, path policy, extraction batching, and artifact
persistence without Julie workspace/search coupling.

### Analysis

Source:

- `/Users/murphy/source/julie/src/analysis/literals.rs`
- `/Users/murphy/source/julie/src/analysis/test_linkage.rs`
- `/Users/murphy/source/julie/src/analysis/test_quality.rs`

Decision needed: decide which analysis outputs are extractor artifact facts and
which belong to downstream products. Literal carrier classification likely
belongs here because Miller/Eros consume it as extraction data. Test quality may
belong downstream unless the schema defines it as extracted evidence.

### Certification Tooling

Source:

- `/Users/murphy/source/julie/xtask/src/tree_sitter_certification.rs`
- `/Users/murphy/source/julie/xtask/src/tree_sitter_certification_data.rs`
- `/Users/murphy/source/julie/xtask/src/tree_sitter_certification_report.rs`
- `/Users/murphy/source/julie/xtask/src/tree_sitter_real_world.rs`
- `/Users/murphy/source/julie/xtask/src/tree_sitter_real_world_report.rs`
- `/Users/murphy/source/julie/xtask/src/cli.rs`
- `/Users/murphy/source/julie/xtask/src/process.rs`
- `/Users/murphy/source/julie/xtask/src/changed.rs`
- `/Users/murphy/source/julie/xtask/test_tiers.toml`

Product target: recreate xtask around extractor tiers. Do not copy Julie's whole
test runner. Keep certification and real-world evidence, but make them operate
through the extractor crate or `julie-extract`, not Julie daemon/workspace code.

## Do Not Move

Leave these in Julie:

- `/Users/murphy/source/julie/src/handler.rs`
- `/Users/murphy/source/julie/src/daemon/`
- `/Users/murphy/source/julie/src/adapter/`
- `/Users/murphy/source/julie/src/dashboard/`
- `/Users/murphy/source/julie/src/search/` except extraction-owned config pieces
- `/Users/murphy/source/julie/src/embeddings/`
- `/Users/murphy/source/julie/src/watcher/`
- `/Users/murphy/source/julie/src/tools/` except extraction policy references
- `/Users/murphy/source/julie/.claude/`
- Julie plugin sync/dev-link code
- Julie daemon release packaging

## Packaging

Source workflow:

- `/Users/murphy/source/julie/.github/workflows/release.yml`

Do not copy as-is. Current release ships `julie-server`, `julie-adapter`, and
`julie-daemon`. New release should ship one CLI binary plus checksums and
contract docs.

Target release assets:

- `julie-extract` for macOS arm64
- `julie-extract` for macOS x64
- `julie-extract` for Linux x64
- `julie-extract.exe` for Windows x64
- checksums
- schema docs
- release notes

## Known New Product Work

- JSONL export writer. Julie currently has JSONL input parsing in the extractor
  pipeline; it does not have a standalone artifact export writer.
- Clean SQLite schema.
- CLI report schema.
- Artifact metadata schema.
- Default-fast test runner.
- Release packaging for one binary.

## Confidence

90/100. The extraction-owned areas are clear. The redesign areas need dedicated
contract design before code migration.
