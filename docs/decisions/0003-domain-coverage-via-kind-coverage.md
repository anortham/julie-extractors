# 0003: Per-Domain Capability Coverage via kind_coverage, Not Schema v4

## Context

The 2026-06-09 data-quality review (docs/findings/2026-06-09-data-quality-review.md,
finding 2.5) showed the capability matrix could not express extraction depth:
the five boolean capability columns claim near-uniform coverage while actual
quality spans three tiers. The execution plan
(docs/plans/2026-06-09-extraction-data-quality.md, Task 10) proposed a new
`domain_coverage_json` column on `language_capabilities`, bumping the SQLite
schema to v4, and folding in removal of the redundant `schema_version`
metadata key.

During execution, code reality contradicted the plan's load-bearing
assumption: `kind_coverage_json` is already domain-keyed (symbols,
relationships, identifiers, body_spans, structural_facts,
complexity_metrics — each `{supported, not_applicable, open_gaps}`) and
already flows from `fixtures/extraction/capabilities.json` through the CLI
capability snapshot into SQLite and JSONL. A parallel `domain_coverage_json`
column would have duplicated that mechanism.

## Decision

1. Express per-domain coverage by extending `kind_coverage` with four new
   domain keys — `annotations`, `doc_comments`, `literals`,
   `source_regions` — alongside the six existing ones. Claims are
   fixture-evidence-backed and machine-enforced by the capability-matrix
   tier (claims without golden evidence fail; evidence without claims fails).
2. No SQLite schema bump. The change is additive content inside an existing
   opaque-JSON column; schema v3 remains current.
3. Defer removal of the redundant `schema_version` metadata key (it
   duplicates `sqlite_schema_version`) to the next genuine schema bump.
   Removing a published metadata key is a contract break for downstream
   readers and does not justify a version bump on its own.
4. Unsupported-but-possible domains carry empty
   `{supported: [], not_applicable: [], open_gaps: []}` entries, matching the
   existing convention for structural_facts/complexity_metrics. `open_gaps`
   entries require a named `planned_closure_task`; none are fabricated.

## Consequences

- Easier: downstream consumers query per-domain depth from existing
  `kind_coverage_json` / JSONL capability records with no migration.
- Easier: no v4 contract documents, no artifact-crate changes, no new
  reader code for consumers.
- Harder: `kind_coverage` is now load-bearing for ten domains; its shape is
  contract. Renaming or restructuring it requires a schema-version event.
- Deferred: the `schema_version` metadata key remains written (redundant)
  until the next schema bump; docs/contracts/cli.md notes the code constants
  as source of truth.

## Applies To

- `fixtures/extraction/capabilities.json` (kind_coverage domains)
- `crates/julie-extractors/src/capability_snapshot.rs` (`CapabilityKindCoverage`)
- `crates/julie-extractors/src/tests/capability_matrix.rs` (enforcement)
- `crates/julie-extract-cli/src/commands.rs` kind_coverage `json!` blocks
  (languages report and artifact capability snapshot)

## Future Agents

- Add new extraction domains as new `kind_coverage` keys with
  fixture-evidence enforcement; do not add parallel coverage columns.
- When the next real SQLite schema bump happens, fold in: drop the
  `schema_version` metadata key (keep `sqlite_schema_version`), and document
  the removal in the new schema contract.
- The two CLI `json!` blocks enumerate domains explicitly; a new domain must
  be added there or it will silently not reach artifacts (this bit us once).
