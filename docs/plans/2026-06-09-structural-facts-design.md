# Structural Facts Slice Design

## Goal

Add a small, versioned structural fact contract that lets downstream tools
consume parser-backed facts without owning tree-sitter policy or language
coverage.

This slice proves the contract with one useful Rust pattern:
`rust.unsafe_block.v1`.

## Architecture Quality

**Affected modules:** extractor base types and registry, CLI extraction mapping,
artifact model/schema/writer/JSONL/report surfaces, current-schema performance
workload, and contract docs.

**Caller-facing interface:** `ExtractionResults.structural_facts`,
`ArtifactFile.structural_facts`, the SQLite `structural_facts` table, the JSONL
`structural_fact` record kind, row counts, and pattern coverage metadata.

**Depth/locality check:** parser-specific matching stays inside
`crates/julie-extractors/src/base/structural_facts.rs`; artifact code only
persists and exports normalized rows. Miller and Eros get facts, not a search
engine or query DSL.

**Test surface:** tests exercise the public extraction pipeline, CLI scan output,
artifact schema, writer behavior, JSONL export, reports, and the synthetic
current-schema workload.

**Seams/adapters:** the CLI mapping is the adapter from extractor facts to
artifact rows. No downstream product-specific adapter is added here.

**Rejected shortcuts:** no raw AST dump, no generic query language, no source
text storage, no ranking/search table, and no broad language promise before
fixture evidence exists.

**Architecture risk:** medium. This changes public artifact contracts, but the
implementation is patterned after the existing `source_regions` row family.

## Contract Shape

Add a new extraction row domain:

```text
structural_facts
```

Each row stores:

- stable `structural_fact_id`
- `file_id`, `path`, and `language`
- versioned `pattern_id`
- `capture_name`
- matched `node_kind`
- optional `containing_symbol_id`
- normalized line, column, and byte span
- `confidence`
- optional `metadata_json`

Add indexes for common downstream access:

- by `(file_id, start_byte, end_byte)`
- by `(pattern_id, language, path)`
- by `containing_symbol_id`

JSONL exports a `structural_fact` record after `source_region` and before
`parse_diagnostic`.

Reports include `structural_facts` in row-domain counts.

## Pattern Metadata

This slice records coverage in contract docs and in each emitted row's metadata:

```json
{
  "pattern_version": 1,
  "query_family": "safety"
}
```

The first supported pattern is:

| Pattern ID | Language | Capture | Node Kind | Meaning |
| --- | --- | --- | --- | --- |
| `rust.unsafe_block.v1` | `rust` | `unsafe_block` | `unsafe_block` | A Rust `unsafe { ... }` block. |

Language-wide capability-matrix coverage remains out of this slice. That should
be added after more patterns and languages exist, so the capability contract can
describe a meaningful matrix instead of one starter row.

## Extraction Flow

1. Language extractors return symbols and other existing facts.
2. `registry::extract_for_language` invokes `collect_structural_facts(...)`
   beside `collect_source_regions(...)`.
3. The collector walks the syntax tree for configured node kinds.
4. Matching nodes become `StructuralFact` rows with stable IDs and containing
   symbols attached by smallest containing span.
5. The CLI maps facts to `ArtifactStructuralFact`.
6. The writer persists rows, JSONL exports them, and reports count them.

## Acceptance Criteria

- [x] SQLite creates `structural_facts` with required columns and indexes.
- [x] `ArtifactWriter` inserts, replaces, deletes, and counts structural facts.
- [x] JSONL includes `structural_fact` in the exact record-kind list.
- [x] Reports include `structural_facts` in all row-domain counts.
- [x] Rust extraction emits `rust.unsafe_block.v1` for an unsafe block.
- [x] CLI scan creates non-empty `structural_facts` rows for a Rust fixture.
- [x] Current-schema writer performance workload includes structural facts.
- [x] Contract docs describe the row shape, JSONL shape, and initial pattern.
- [x] Focused default and contract-surface tests remain fast.

## Out Of Scope

- Query execution or ranking.
- Miller/Eros-specific workflows.
- Raw tree-sitter query source storage.
- Raw AST serialization.
- Broad language coverage claims.
- A public pattern registry table before multiple patterns justify it.
