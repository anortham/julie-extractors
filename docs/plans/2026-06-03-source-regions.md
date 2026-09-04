# Source Regions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Add stable source-region rows so downstream tools can tell which byte ranges are comments, doc comments, string literals, or embedded language blocks.

**Architecture:** Keep `julie-extractors` focused on extraction facts. The extractor emits normalized spans and region kinds; upstream products such as Julie, Miller, and Eros decide how to search, rank, embed, or display those spans. SQLite remains the primary output, JSONL remains the secondary export, and reports expose row counts.

**Tech Stack:** Rust, tree-sitter node spans, `rusqlite`, SQLite artifact v2, JSONL v2, report schema v2, `julie-extract` CLI, focused extractor fixtures.

**Architecture Quality:** Medium risk because this changes public artifact contracts. The shape is approved as a new `source_regions` row domain with no search index, no embedding table, no raw AST dump, and no full source text storage.

---

## Source Documents

- `AGENTS.md`: product boundary, SQLite-primary output, JSONL-secondary output, CLI-first integration, and default-suite discipline.
- `RAZORBACK.md`: strategy-tier areas, worker eligibility, escalation triggers, and verification ownership.
- `docs/contracts/extracted-data-v1.md`: public extracted-data domains and capability evidence rules.
- `docs/contracts/sqlite-schema-v1.md`: canonical SQLite schema and required indexes.
- `docs/contracts/jsonl-v1.md`: JSONL record order and payload contracts.
- `docs/contracts/reports.md`: JSON report row-count contract.
- `docs/testing-strategy.md`: default, language, contract, changed-path, and slow-gate routing.

## Current State

- `ExtractionResults` already carries symbols, relationships, identifiers, type facts, literals, and parse diagnostics.
- `ArtifactFile` already persists child rows through `ArtifactWriter`, then exports them through JSONL.
- Literals are currently semantic facts: useful strings such as URLs, SQL, or
  configured literal carrier facts. Route remains reserved unless route carriers
  are explicitly configured. They are not a complete map of every string literal
  span.
- Parse diagnostics already prove the artifact can expose parser-derived span facts without owning search behavior.
- There is no general row domain for "this byte range is a comment" or "this byte range is a string literal."

## Product Decision

Add one new artifact row domain:

```text
source_regions
```

Each row describes a source range and its role:

- `comment`: a normal comment span.
- `doc_comment`: a comment span that is attached to a symbol as documentation.
- `string_literal`: a complete string-literal span, even when it is not a useful semantic `literal`.
- `embedded`: a range whose content is parsed as another language or sublanguage.

Do not add these in this slice:

- search index tables
- vector or embedding tables
- token-ranking data
- raw tree-sitter node dumps
- full source text copies
- one row for every generic code block

Plain code can be treated as "everything else in the file" by consumers that need it.

## Public Contract Shape

### Versioning

Treat this as a v2 public contract change.

Implementation must bump:

- `crates/julie-extract-artifact/src/schema.rs`: `SQLITE_SCHEMA_VERSION` from `1` to `2`.
- `crates/julie-extract-artifact/src/schema.rs`: `EXTRACT_CONTRACT_VERSION` from `1` to `2`.
- `crates/julie-extract-artifact/src/jsonl.rs`: `JSONL_SCHEMA_VERSION` from `1` to `2`.
- `crates/julie-extract-artifact/src/reports.rs`: `REPORT_SCHEMA_VERSION` from `1` to `2`.
- `crates/julie-extractors/src/lib.rs`: `EXTRACTION_CONTRACT_VERSION` string with a `source-regions-v1` marker or equivalent.

Documentation should create the current v2 contract docs and keep v1 docs as historical release contracts:

- Create: `docs/contracts/extracted-data-v2.md`
- Create: `docs/contracts/sqlite-schema-v2.md`
- Create: `docs/contracts/jsonl-v2.md`
- Modify: `docs/contracts/reports.md`
- Modify: `docs/contracts/cli.md`
- Modify: `README.md` contract links

### SQLite

Add `source_regions` with these columns:

```sql
CREATE TABLE source_regions (
    source_region_id TEXT PRIMARY KEY,
    file_id TEXT NOT NULL,
    path TEXT NOT NULL,
    language TEXT NOT NULL,
    kind TEXT NOT NULL,
    containing_symbol_id TEXT,
    start_line INTEGER NOT NULL,
    start_column INTEGER NOT NULL,
    end_line INTEGER NOT NULL,
    end_column INTEGER NOT NULL,
    start_byte INTEGER NOT NULL,
    end_byte INTEGER NOT NULL,
    metadata_json TEXT,
    FOREIGN KEY(file_id) REFERENCES files(file_id),
    FOREIGN KEY(containing_symbol_id) REFERENCES symbols(symbol_id)
);
```

Required indexes:

- `idx_source_regions_file_span` on `(file_id, start_byte, end_byte)`
- `idx_source_regions_kind_file` on `(kind, file_id, start_byte)`
- `idx_source_regions_symbol` on `(containing_symbol_id)`

### JSONL

Add a `source_region` record kind after `literal` and before `parse_diagnostic`.

Payload keys:

- `source_region_id`
- `file_id`
- `path`
- `language`
- `kind`
- `containing_symbol_id`
- `span`
- `metadata`

### Reports

Add `source_regions` to row-domain counts:

- `rows_written.source_regions`
- `rows_deleted.source_regions`
- `totals.source_regions`

### Capability Matrix

Do not add a language capability flag in the first slice.

Reason: source regions are an artifact row domain, not a language-support promise yet. After fixtures prove stable coverage across enough languages, add a separate coverage claim if it is useful.

## File Structure

- Modify: `crates/julie-extractors/src/base/types.rs:321` - add `SourceRegion`, `SourceRegionKind`, and `ExtractionResults.source_regions`.
- Modify: `crates/julie-extractors/src/base/extractor.rs:46` - add base storage and helper methods for source regions.
- Modify: `crates/julie-extractors/src/base/results_normalization.rs:50` - merge, offset, normalize, and refresh source-region IDs.
- Create: `crates/julie-extractors/src/base/source_regions.rs` - shared tree walk and kind normalization helpers.
- Modify: `crates/julie-extractors/src/base/mod.rs` - export the new helper module.
- Modify: selected language modules that need wrappers or embedded-region wiring, starting with Rust, JavaScript/TypeScript, HTML, and Vue.
- Modify: `crates/julie-extract-cli/src/extraction.rs:216` - map extractor source regions into artifact rows.
- Modify: `crates/julie-extract-artifact/src/model.rs:82` and `:204` - add row counts, `ArtifactFile.source_regions`, and `ArtifactSourceRegion`.
- Modify: `crates/julie-extract-artifact/src/schema.rs:10` - add table and indexes.
- Modify: `crates/julie-extract-artifact/src/writer.rs:278` - write, delete, count, and replace source-region rows.
- Modify: `crates/julie-extract-artifact/src/jsonl.rs:14` and `:108` - add record kind and exporter.
- Modify: `crates/julie-extract-artifact/src/reports.rs:5` - add report row-domain counts.
- Create: `docs/contracts/extracted-data-v2.md`, `docs/contracts/sqlite-schema-v2.md`, and `docs/contracts/jsonl-v2.md`.
- Modify: `docs/contracts/reports.md`, `docs/contracts/cli.md`, and `README.md`.
- Modify tests under `crates/julie-extractors/src/tests/`, `crates/julie-extract-artifact/tests/`, and `crates/julie-extract-cli/tests/`.

## Implementation Tasks

### Task 1: Contract Red Tests

**Files:**
- Modify: `crates/julie-extract-artifact/tests/schema_contract.rs`
- Modify: `crates/julie-extract-artifact/tests/jsonl_contract.rs`
- Modify: `crates/julie-extract-artifact/tests/report_contract.rs`

**What to build:** Add failing contract expectations for the new row domain before implementation.

**Acceptance criteria:**
- Schema contract expects the `source_regions` table and required indexes.
- JSONL contract expects `source_region` in the exact record-kind list.
- JSONL payload-key test expects the approved payload keys.
- Report contract expects `source_regions` in row-domain counts.

### Task 2: Artifact Model, Schema, And Writer

**Files:**
- Modify: `crates/julie-extract-artifact/src/model.rs`
- Modify: `crates/julie-extract-artifact/src/schema.rs`
- Modify: `crates/julie-extract-artifact/src/writer.rs`
- Modify: `crates/julie-extract-artifact/tests/writer_contract.rs`
- Modify: `crates/julie-extract-artifact/tests/writer_batching_contract.rs`

**What to build:** Persist source-region rows transactionally with the rest of a file.

**Approach:**
- Add `ArtifactSourceRegion`.
- Add `source_regions` to `ArtifactFile`, `RowCounts`, and row-domain report conversion.
- Delete stale source regions during file replacement and delete operations.
- Insert rows through cached statements beside literals and parse diagnostics.
- Validate optional `containing_symbol_id` the same way other child rows validate symbol links.

**Acceptance criteria:**
- Writer inserts source regions for a file.
- Writer deletes source regions when a file is updated or deleted.
- Row counts include source regions.
- The writer data-loss guard still preserves known-good rows on parser failure.

### Task 3: JSONL Export

**Files:**
- Modify: `crates/julie-extract-artifact/src/jsonl.rs`
- Modify: `crates/julie-extract-artifact/tests/jsonl_contract.rs`

**What to build:** Export source-region rows as deterministic JSONL.

**Approach:**
- Add `source_region` to `JSONL_RECORD_KINDS`.
- Add `export_source_regions`.
- Order rows by `path`, `start_byte`, `end_byte`, `kind`, and `source_region_id`.
- Emit `metadata: null` when `metadata_json` is absent.

**Acceptance criteria:**
- Full JSONL export emits at least one `source_region` record in the contract fixture.
- Record order is stable.
- Existing bounded-write behavior remains covered.

### Task 4: Extractor Model And Shared Collector

**Files:**
- Modify: `crates/julie-extractors/src/base/types.rs`
- Modify: `crates/julie-extractors/src/base/extractor.rs`
- Modify: `crates/julie-extractors/src/base/results_normalization.rs`
- Create: `crates/julie-extractors/src/base/source_regions.rs`
- Modify: `crates/julie-extractors/src/base/mod.rs`
- Modify focused extractor tests under `crates/julie-extractors/src/tests/`

**What to build:** Add a reusable AST walk that captures source-region spans.

**Approach:**
- Define stable region kinds as Rust enum values with lowercase artifact strings.
- Capture comment nodes as `comment`.
- Mark comments already attached as symbol docs as `doc_comment`.
- Capture all complete string-literal nodes as `string_literal`, separate from existing semantic `literals`.
- Generate stable IDs from file identity, kind, start byte, and end byte.
- Keep metadata small and optional. For embedded rows, include fields such as embedded language and host node kind.

**Acceptance criteria:**
- A focused Rust or JavaScript test proves normal comments, doc comments, and string literals produce source regions.
- Existing semantic literal tests still pass and do not become complete string-span tests.
- Result normalization offsets source regions correctly for embedded extraction paths.

### Task 5: Embedded Regions

**Files:**
- Modify: `crates/julie-extractors/src/base/embedded_span.rs`
- Modify: `crates/julie-extractors/src/vue/mod.rs`
- Modify: HTML-related extractor modules if they already identify script/style blocks.
- Modify focused Vue or HTML tests.

**What to build:** Emit `embedded` rows for host files that contain parsed sublanguage ranges.

**Approach:**
- Reuse existing embedded-span offsets instead of inventing a second offset model.
- For the first slice, cover Vue script/style blocks and any existing HTML script/style path that already has clear node spans.
- Do not claim universal embedded-language coverage until fixtures prove it.

**Acceptance criteria:**
- A Vue or HTML fixture emits at least one `embedded` source region.
- The embedded row start/end bytes match the host file bytes, not the embedded parser's temporary buffer.

### Task 6: CLI Mapping

**Files:**
- Modify: `crates/julie-extract-cli/src/extraction.rs`
- Modify: `crates/julie-extract-cli/tests/operations_contract.rs`

**What to build:** Carry extractor source regions into `ArtifactFile`.

**Approach:**
- Add `map_source_regions`.
- Dedupe by `source_region_id`.
- Preserve file path, language, span, containing symbol, and metadata.
- Ensure failed and unchanged file paths initialize `source_regions: Vec::new()`.

**Acceptance criteria:**
- A CLI scan fixture persists source regions into SQLite.
- JSON report counts include source regions.
- Existing failed-parse behavior still records parse diagnostics without pretending source-region extraction succeeded.

### Task 7: Contract Docs

**Files:**
- Create: `docs/contracts/extracted-data-v2.md`
- Create: `docs/contracts/sqlite-schema-v2.md`
- Create: `docs/contracts/jsonl-v2.md`
- Modify: `docs/contracts/reports.md`
- Modify: `docs/contracts/cli.md`
- Modify: `README.md`

**What to build:** Document the new row domain as current product behavior after implementation.

**Acceptance criteria:**
- Docs say source regions are span facts, not search, embeddings, raw AST, or source-text storage.
- SQLite and JSONL docs match contract tests.
- Report examples include `source_regions`.
- CLI and report docs show schema/report version `2`.
- README links to v2 SQLite, JSONL, and extracted-data contracts after implementation and tests pass.

## Verification Strategy

**Project source of truth:** `AGENTS.md`, `RAZORBACK.md`, `docs/testing-strategy.md`, and the contracts under `docs/contracts/`.

**Worker red/green scope:** Use the narrowest command that proves the task:

- `cargo test -p julie-extract-artifact --test schema_contract`
- `cargo test -p julie-extract-artifact --test jsonl_contract`
- `cargo test -p julie-extract-artifact --test report_contract`
- `cargo test -p julie-extract-artifact --test writer_contract source_regions`
- `cargo test -p julie-extractors source_regions`
- `cargo test -p julie-extract-cli --test operations_contract source_regions`

**Worker ceiling:** Workers may run one crate's focused tests, one integration test target, `cargo xtask test language <language>`, or `cargo xtask test changed <paths...>`. Workers do not own broad contract acceptance or capability-claim interpretation.

**Worker gate invariant:** The assigned test must prove the public behavior touched by that task: schema shape, JSONL shape, report counts, writer persistence, extractor spans, embedded offset correctness, or CLI mapping.

**Lead affected-change scope:** Run `cargo xtask test changed <changed paths...>` after a coherent batch. For this plan, the changed-path set will likely include `crates/julie-extractors`, `crates/julie-extract-artifact`, `crates/julie-extract-cli`, and `docs/contracts`.

**Branch gate:** Before handoff, merge, push, or PR, run:

- `cargo fmt --all -- --check`
- `cargo xtask test default`
- `cargo xtask test contract`

**Replay/metric evidence:** Hard gates are schema contract, JSONL contract, report contract, writer contract, extractor focused tests, and CLI scan evidence. Runtime and artifact-size changes are report-only unless the default suite grows unexpectedly.

**Escalation triggers:** Public schema changes beyond this plan, JSONL payload changes beyond this plan, CLI status or exit-code changes, language capability claims, parser dependency changes, default-suite runtime growth, or embedded-region coverage that needs a broader support claim.

**Assigned verification failure:** Workers stop and report when assigned verification fails unless this plan explicitly says to update that gate.

**Verification ledger:** Record invariant, command, scope label, commit SHA, result, and timestamp. For report-only metrics, record artifact size and row-count deltas when available. If the same HEAD already has a passing ledger entry for the required scope, reuse that evidence instead of rerunning the same expensive gate.

## Model Routing

**Project source of truth:** `RAZORBACK.md`.

**Strategy tier:** Planning, architecture, decomposition, public contract interpretation, and review finding triage.
- Harness mapping: inherit in this Codex session.

**Implementation tier:** Bounded tasks after the public `source_regions` shape is fixed by this plan.
- Harness mapping: inherit.

**Mechanical tier:** Formatting, docs-only wording after contracts are decided, and rote fixture updates that do not interpret failing tests.
- Harness mapping: inherit.

**Gate-interpretation reviewer:** Lead interprets failing contract tests, changed-path routing, report-count evidence, and embedded-offset evidence.
- Harness mapping: inherit.

**Escalation tier:** Public artifact contract changes outside this plan, weak tests, repeated verification failures, language capability claims, parser dependency changes, or default-suite runtime growth.
- Harness mapping: inherit.

**Worker eligibility:** Workers are eligible only when file ownership is narrow, the public interface is already decided, and the verification ceiling is explicit.

**Escalation triggers:** Any change to public artifact schema beyond this plan, CLI status, exit code, error code, language capability claim, parser dependency version, or default-suite runtime.

**Mechanical exclusion:** Mechanical workers cannot own failing tests, replay evidence, metrics, or acceptance gates.

**Unsupported harness behavior:** If the harness cannot choose models per agent, use `inherit` and continue.

## Open Decisions

- **Doc-comment detection:** Start by marking comments that are already attached to `Symbol.doc_comment` or equivalent extracted docs. Broader language-specific doc-comment syntax can be added after this contract lands.
- **String-literal node names:** The shared collector should use a small per-language config for string node kinds. Do not infer every quoted token through ad hoc text matching.
- **Embedded coverage:** First slice proves Vue and any already-clear HTML path. Broader embedded regions should be added with fixtures, not assumed from parser support.
- **Metadata shape:** Keep metadata optional and object-shaped. Use it for embedded language and host node kind only at first.
- **Release notes:** The implementation should add a release note that calls out the artifact contract bump from v1 to v2 and names `source_regions` as the reason.

## Done Criteria

- [x] `source_regions` exists in SQLite with contract-tested indexes.
- [x] JSONL exports `source_region` records with contract-tested keys.
- [x] CLI scan writes source-region rows for focused fixtures.
- [x] Reports include source-region row counts.
- [x] Extractor tests prove comments, doc comments, string literals, and at least one embedded region.
- [x] Docs explain what source regions are and what they deliberately are not.
- [x] `cargo fmt --all -- --check` passes.
- [x] `cargo xtask test default` passes.
- [x] `cargo xtask test contract` passes.
