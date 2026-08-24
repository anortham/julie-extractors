# Task 4 report: QML/QMLDIR facts and multi-file golden recovery

## Status

Implementation and focused extractor gates are green. The shared fixture
helper and generated structural-fact contract are corrected, and the lead's
full contract run passed before these final narrow refactors.

## Prior audit history

The first worker stopped before implementation because the packet required a
multi-file golden but the owned golden harness only modeled one `source` path.
The report recorded that mismatch rather than creating misleading one-file
evidence. The recovery packet expanded ownership to include the golden and
capability harness files, and the current diff implements the missing support.

## Recovery changes

- Registered QML import, object-instantiation, and qmltypes declaration facts;
  normalized import metadata now agrees with the import-symbol contract.
- Registered the complete QMLDIR manifest fact family and added a real
  extensionless `qmldir` fixture covering module, type, singleton, internal,
  JavaScript, plugin, classname, typeinfo, dependency, import, Designer,
  prefer, and link-target declarations.
- Extended golden fixture rows with an optional ordered `sources` list. The
  harness validates `source == sources[0]`, rejects duplicates, extracts each
  source independently, and merges results deterministically without
  cross-file resolution.
- Added a multi-file QML golden containing physical QML and `.qmltypes` paths,
  normalized imports, local and unresolved instantiations, bindings, and Quick
  Test roles; added a dedicated qmltypes golden.
- Added registry conformance and QML structural-fact tests, capability evidence,
  and honest basename-only validation for `qmldir`.
- Corrected the registry invariant test so the existing `code.marker.v1`
  collector is included in its per-language emission union.
- Updated the CLI `languages --json` contract test to require empty extensions
  only for `qmldir`, with nonempty extensions for all other languages.
- Updated `write_all_language_fixture` to copy the exact `qmldir` basename when
  the language has no extension, preserving `source.*` lookup for all other
  languages; the fallback is generic for any future basename-only language.
- Regenerated `docs/contracts/structural-fact-patterns.json` through
  `UPDATE_CONTRACT_JSON=1 cargo test -p julie-extractors structural_fact_registry`.
- Removed the golden harness's hardcoded `qmldir` basename branch; it now routes
  every fixture through `detect_language_for_path`, including basename-only
  languages handled by the producer wrapper.
- Replaced duplicated QMLDIR pattern-ID arrays in the registry and capability
  tests with `crate::qmldir::STRUCTURAL_FACT_PATTERN_IDS`.

## TDD ledger

| Stage | Timestamp (America/Chicago) | Evidence |
|---|---|---|
| RED | 2026-08-24 recovery turn | Original handoff reproduced the missing multi-file harness seam; later contract run reproduced the basename-only CLI assertion and the store-equivalence source-file assumption. |
| GREEN | 2026-08-24 recovery turn | QML structural-fact tests, registry tests, golden tests, capability tests, and corrected CLI contract assertion pass. |
| Golden generation | 2026-08-24 recovery turn | Checked-in QML/QMLDIR expected artifacts match canonical extraction; `cargo xtask test golden` passes. |

## Verification ledger

- `workspace refresh` with Miller workspace `f882d90413a5`: passed; revision
  `52308` after the final refresh.
- Miller changed-file symbol listings, central modified-function inspections,
  registry/golden seam traces, and impact analysis: completed.
- `cargo test -p julie-extractors --features test-capability-matrix registry_pattern_ids_match_emitted_union_per_language -- --nocapture`: 1 passed.
- `cargo test -p julie-extractors --features test-capability-matrix structural_fact_registry -- --nocapture`: 16 passed.
- `cargo xtask test language qml`: 127 passed.
- `cargo xtask test golden`: 5 passed.
- `cargo xtask test capability`: 39 capability tests plus 1 pending-shape test passed.
- `cargo test -p julie-extract-cli --test operations_contract languages_json_emits_capability_snapshot_data`: 1 passed after the basename-only contract correction.
- `cargo test -p julie-extract-cli --features test-store-contract --test store_equivalence full_store_rows_equal_the_v3_extraction_only_writer_oracle -- --test-threads=1`: 1 passed.
- `UPDATE_CONTRACT_JSON=1 cargo test -p julie-extractors structural_fact_registry`: 15 passed and regenerated the checked-in contract snapshot.
- `cargo test -p julie-extractors structural_fact_patterns_json_matches_checked_in_contract -- --nocapture`: 1 passed.
- `node scripts/language-data-quality-report.mjs --strict`: passed with
  `silent_cells=0` and `quality_bar_debts=0`.
- `cargo fmt --all -- --check` and `git diff --check`: passed.
- Lead verification: `cargo xtask test contract` passed before these final
  narrow refactors; it was intentionally not rerun afterward per the packet.
- `cargo xtask test language qmldir`: intentionally not run; remains Task 5.

## Artifact review

- Basic/test-role expected-output churn is limited to the newly published
  normalized import metadata and the newly registered object-instantiation
  facts; generated output also reflects the existing normalizer's empty
  metadata representation.
- The QML cross-file artifact contains three physical source paths and
  deterministic merged rows; it does not resolve across files.
- The qmltypes artifact proves module/component/member declaration roles and
  typeinfo structural facts.
- The QMLDIR artifact contains all requested manifest fact families and keeps
  the intentionally unknown directive as parser diagnostics rather than a
  false positive fact.
- The generated structural-fact contract snapshot now includes the QML and
  QMLDIR registry rows.
- Capability claims are backed by these artifacts; strict quality reports no
  silent cells or quality-bar debts.

## Miller/API evidence

Miller workspace `f882d90413a5` confirmed:

- `FixtureRow` now owns `source`, optional `sources`, and deterministic
  `source_paths()` validation.
- `golden_fixtures_match_canonical_extraction` calls `extract_fixture`, which
  extracts each listed path independently and merges results.
- `structural_facts_conform_to_registry` walks every listed source path and
  validates emitted metadata against registered key names and value types.
- `structural_fact_pattern_specs` is consumed by registry invariants,
  capability claims, and the regenerated checked-in contract serializer.
- `write_all_language_fixture` has two callers (`store_equivalence` and
  `store_mixed_version`); impact analysis identified six likely tests.
- The impact graph for `code_structural_facts.rs` reaches the extractor
  registry, canonical pipeline, capability tests, and registry tests.
- `STRUCTURAL_FACT_PATTERN_IDS` is now the single producer-owned QMLDIR ID
  list consumed by both registry and capability tests.
- No QML structural fact is used as a resolver channel; the new facts remain
  descriptive evidence, while normalized import symbol metadata remains the
  import contract.

## Worktree

- Path: `/home/murphy/source/julie-extractors/.worktrees/qml-first-class-extraction`
- Branch: `feat/qml-first-class-extraction`
- HEAD: `1bc5b2bff3138cacf85b7c45edb3150883833753`
- Dirty state: task files are modified/untracked; the parent agent's
  `docs/plans/2026-08-24-qml-first-class-extraction-implementation-plan.md`
  change is present and must not be included in this packet's commit.
- Recovery checkpoints: `.memories/2026-08-24/154235_7fb6.md`,
  `.memories/2026-08-24/154621_46b2.md`, and
  `.memories/2026-08-24/154915_1a96.md`.

## Judgment and blocker

The extensionless `qmldir` contract is intentional and passes the extractor,
capability, CLI-language-report, helper, registry-snapshot, and lead-run full
contract checks. The final narrow refactors preserve that behavior and do not
touch the concurrently owned discovery/producer implementation. Commit the
explicit Task 4 file set plus Task 4 checkpoints; omit lead-owned files and
`.memories/2026-08-24/155448_c247.md`.
