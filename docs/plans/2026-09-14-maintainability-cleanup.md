# Maintainability cleanup

Approved by the user's 2026-09-14 instruction to implement the six audited cuts.

## Scope and constraints

Preserve SQLite schema, JSON shapes, emitted facts, IDs, spans, ordering, capability fingerprints, extraction contract version, and identity epoch. No dependencies or runtime features added. No release, tag, push, or PR. Package version remains unchanged until release preparation. Historical resolution evidence remains available. Use existing shared helpers and direct functions; no generic framework.

Architecture risk is low for table/serialization cleanup and medium for artifact bulk-load control flow. Shared fact construction must preserve specialized identity variants. Public CLI and artifact APIs remain unchanged. Security scope: none declared; no dependency/security behavior changes.

## Verification strategy

Baseline: cargo check -p julie-extract-artifact -p julie-extract-cli.
Workers: inspect existing caller-facing tests before refactoring, run the narrow relevant checks before/after, and add a meaningful regression check only where coverage is missing. Pure refactors use existing behavior tests as their baseline, not artificial failures. Any defect found requires a failing regression first.
Lead affected/branch: cargo fmt --check; cargo test -p xtask; cargo xtask test default; cargo xtask test contract; node scripts/language-data-quality-report.mjs --strict. Compare golden outputs without updating expectations. Record schema and contract constants unchanged.
Specialist: focused Windows bulk-load/writer tests through win-test after a clean local commit. No full parser certification or real-world corpus needed for unchanged language behavior.

## Parallel execution contract

All tasks below are independent, batch A, serialization not required, dependency reason: non-overlapping files. Commit mode: parallel-lead-commit; workers do not stage or commit. Lead owns final review, integration verification, plan, and memory. Use one shared task worktree and CARGO_TARGET_DIR=/home/murphy/source/julie-extractors/target; avoid broad worker test runs.

- [x] Task 1: Retire coverage script and its test. Own scripts/reference-resolution-coverage-report.mjs, scripts/reference-resolution-coverage-report.test.mjs, fixtures/extraction/capabilities.json metadata, docs/contracts/reference-resolution-coverage-v1.md and docs/site/extractors.html. Check tracked automation and available Miller/Julie/code-kb checkouts for executable usage before removal. Keep historical JSON/docs, remove or relabel stale active-gate claims. Strict quality report must retain zero silent cells and quality debts; no emitted language rows or fingerprints change.
- [x] Task 2: Derive code structural pattern IDs from existing pattern definitions and share identical structural-fact builders. Own crates/julie-extractors/src/base/code_structural_facts.rs, data_structural_facts.rs, sql_structural_facts.rs, web_structural_facts/fact_builders.rs, base/mod.rs and one minimal shared helper module if needed. Own related structural-fact tests only. Return iterator for IDs if caller can extend directly, avoiding new allocation. Preserve identity-aware builders and ordering. Existing golden facts unchanged.
- [x] Task 3: Remove test-only serial extraction path. Own crates/julie-extract-cli/src/commands.rs. Move its two tests to existing production spool pipeline, retain assertions on parser invocation/force behavior and resulting files. No production extraction semantics change.
- [x] Task 4: Derive capability kind-coverage serialization. Own crates/julie-extractors/src/capability_snapshot.rs and crates/julie-extract-cli/src/capability_snapshot.rs plus a focused serialization test if needed. Serialize only relevant existing data structs; replace manual kind_coverage_json/domain mapping. Preserve exact emitted JSON and fingerprints including empty collections and gap fields.
- [x] Task 5: Share artifact bulk-load wrapper policy. Own crates/julie-extract-artifact/src/writer.rs and focused writer tests if missing coverage. Keep poison check, eligibility consumption, begin timing, restoration on uncommitted errors only, and committed-error distinction exactly. One private helper accepting the differing operation is sufficient. Both in-memory and spool entry points remain covered.

## Completion evidence

All five tasks implemented and lead-reviewed. Default tier passed in 124 seconds. Worker structural-fact tests: 226 passed; writer contract: 51 passed. Formatting and strict quality report passed with silent_cells=0 and quality_bar_debts=0. Net code/test cut: 828 lines. Contract and Windows gates run on the local implementation commit; their final results will be recorded in memory. Schema, dependency, contract, identity and package version files are unchanged.
