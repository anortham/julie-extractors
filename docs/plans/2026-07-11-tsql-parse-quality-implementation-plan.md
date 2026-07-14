# T-SQL Parse-Quality Closure Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: use `razorback:subagent-driven-development` when delegation is available; otherwise use `razorback:executing-plans`. This plan is grammar-first and test-driven. Do not suppress, filter, merge, or downgrade parse diagnostics.

**Goal:** Reduce the Terraform T-SQL corpus from 283 `error` + 1 `missing` diagnostics to zero while preserving zero Razor diagnostics, then emit normalized SQL symbols/facts for the newly parsed T-SQL constructs.

**Architecture:** Start from the published T-SQL extension fork `tree-sitter-sequel-tsql` 0.4.2 (source commit `b3db1ee85908a0c0e425bc59ddf04c6ad107eecf`), because a live probe reduced this corpus from 284 diagnostics to 53 without source preprocessing. Carry the grammar in an `anortham/tree-sitter-sql` fork pinned by exact revision; close the residual syntax classes in the grammar, then adapt the existing SQL extractor to normalized bracketed names and add one useful `sql.merge_statement.v1` fact. Parse recovery remains tree-sitter's responsibility; extraction remains inside `julie-extractors`.

**Tech Stack:** Rust 2024, tree-sitter 0.26.8, tree-sitter SQL grammar (`grammar.js` + generated C parser), SQLite artifact diagnostics, golden fixtures, capability matrix.

## Architecture Quality

**Affected modules:** external SQL grammar fork; `crates/julie-extractors` parser dependency and language inventory; `src/sql/` symbol/identifier extraction; `src/base/sql_structural_facts.rs`; structural-fact registry; SQL goldens/capability row.

**Caller-facing interface:** unchanged `julie-extract` CLI and SQLite/JSONL contracts. The behavioral change is fewer persisted `parse_diagnostics`, normalized SQL names, and one additive registered structural-fact pattern.

**Depth/locality check:** SQL syntax belongs in the grammar fork. SQL semantic normalization belongs in one shared SQL helper used by existing extractor consumers. No T-SQL preprocessing layer, CLI dialect switch, or artifact schema change is introduced.

**Test surface:** grammar corpus tests prove concrete node shapes; extractor tests and registered goldens prove symbols/identifiers/facts through the canonical extraction pipeline; the CLI corpus scan proves persisted SQLite diagnostics.

**Seams/adapters:** no new framework seam. The only new helper is SQL identifier normalization, required because bracket-quoted identifiers are grammar-level `identifier` nodes whose source text retains `[...]`.

**Rejected shortcuts:** deleting or filtering diagnostics; stripping `GO` or brackets before parsing; regex recovery as primary support; treating all 284 nested diagnostics as independent bugs; changing the SQL extension to `.tsql`; switching parsers without running the existing general-SQL goldens; claiming T-SQL support from parser success alone.

**Architecture risk:** high. A parser dependency change can alter node shapes for every SQL dialect, so the dependency switch is fail-fast and precedes extractor work.

## Locked Decisions

1. **Implementation base:** use `codex/blazor-razor-support` at `1af555fb598d003fcdcbfab6711827c0e058041e` or a descendant/main commit containing it. The rebuilt 2.13.0 CLI at this commit is the reference that produces zero Razor diagnostics. Do not implement from current `main` at `dc69141`, which still produces 232 Razor errors + 3 missing on the same corpus.
2. **Grammar base:** fork `jamie8johnson/tree-sitter-sql` commit `b3db1ee85908a0c0e425bc59ddf04c6ad107eecf` (crate `tree-sitter-sequel-tsql` 0.4.2), then pin the owned fork by exact Git revision. Do not depend on an unpinned branch or wait for an upstream release.
3. **Acceptance target:** all six Terraform `.sql` files must have zero `error` and zero `missing` rows. Partial reduction is an intermediate metric, not completion.
4. **Diagnostic integrity:** malformed T-SQL must still emit diagnostics. Tests include negative controls; the fix must expand valid grammar, not make `ERROR` nodes invisible.
5. **Extraction scope:** normalize bracketed object/column names and add `sql.merge_statement.v1`. `GO`, `SET`, `IF`, `BEGIN/END`, `DECLARE`, and `THROW` are grammar-supported but intentionally emit no first-class artifact rows in this issue.
6. **Capability scope:** register two new SQL golden fixtures and add `sql.merge_statement.v1` to SQL structural-fact support. Keep the existing `sql.advanced_dml_and_procedure_structure` gap open, but remove MERGE from its reason/closure text; INSERT, DELETE, procedures/functions, windows, and other vendor DDL remain named debt.
7. **Test-tier discipline:** minimized T-SQL fixtures run in focused/golden tiers. The live Terraform scan is a release/affected-change gate only and must not enter the default suite.

## Reproduced Baseline

### Exact reference run

Built from the clean worktree `/Users/murphy/source/julie-extractors/.worktrees/blazor-razor-support` at `1af555fb598d003fcdcbfab6711827c0e058041e`:

```bash
cargo build -p julie-extract-cli --bin julie-extract
RUN_DIR=$(mktemp -d /tmp/julie-tsql-baseline.XXXXXX)
./target/debug/julie-extract scan \
  --root /Users/murphy/source/Terraform \
  --db "$RUN_DIR/terraform.sqlite" \
  --force --json > "$RUN_DIR/scan.json"
sqlite3 -readonly "$RUN_DIR/terraform.sqlite" \
  "SELECT language, kind, COUNT(*) FROM parse_diagnostics GROUP BY language, kind ORDER BY language, kind;"
sqlite3 -readonly "$RUN_DIR/terraform.sqlite" \
  "SELECT path, kind, COUNT(*) FROM parse_diagnostics WHERE language='sql' GROUP BY path, kind ORDER BY path, kind;"
```

Observed: status `ok`; 418 paths scanned; 388 supported/extracted; 30 unsupported; 0 failed; diagnostics only for SQL (`283 error`, `1 missing`); no Razor rows.

| File | Error | Missing | Representative failing source |
|---|---:|---:|---|
| `db/baseline.sql` | 225 | 0 | `CREATE TABLE [edr].[EdrForms]`, bare `IDENTITY`, `nvarchar(max)`, computed `AS ... PERSISTED`, `GO` |
| `db/changes/0001_drop_rfa.sql` | 18 | 0 | `IF OBJECT_ID(...) IS NOT NULL`, bracketed `DROP TABLE`, `GO`, `IF EXISTS` |
| `db/changes/0002_drop_ef_migrations_history.sql` | 6 | 0 | `IF OBJECT_ID(...)`, bracketed name, `GO` |
| `db/changes/0003_access_admin.sql` | 23 | 1 | `SET`, `IF/BEGIN/END`, `MERGE ... USING (VALUES ...)`, `THROW`, computed persisted column, named inline constraints |
| `db/changes/0004_seed_admin_bootstrap.sql` | 4 | 0 | `SET NOCOUNT`, initialized `DECLARE @AdGroup`, `IF NOT EXISTS ... BEGIN/END` |
| `db/changes/0005_edr_rowversion.sql` | 7 | 0 | `IF COL_LENGTH(...)`, `BEGIN/END`, bracketed `ALTER TABLE`, `GO` |

### Taxonomy

| Class | Evidence | Classification | Ownership / disposition |
|---|---|---|---|
| Bracket-quoted identifiers | 198 exact one-character `[`/`]` diagnostic rows; upstream issue `DerekStride/tree-sitter-sql#320`; `[dbo].[T]` isolated probe has 6 diagnostics on 0.3.11 and 0 on the T-SQL fork | Grammar gap causing cascades | Grammar Task 2; extractor normalization Task 5 |
| `GO` batch separator | 17 exact `GO` rows plus larger spans containing `GO`; isolated probe 1 -> 0 on T-SQL fork | Grammar gap | Grammar Task 2; no artifact row |
| T-SQL data/DDL modifiers | `IDENTITY` (6 exact rows), `(MAX)`, computed `AS ... PERSISTED`, inline named PK/default/FK constraints | Grammar gap; then extractor compatibility | Grammar Task 3; extractor Task 5 |
| Batch/session control | `SET NOCOUNT ON`, `SET XACT_ABORT ON`, `IF [NOT] EXISTS`, `IF OBJECT_ID/COL_LENGTH`, `BEGIN/END` | Grammar gap | Grammar Task 4; intentional no first-class facts |
| Procedural statements | initialized `DECLARE @AdGroup NVARCHAR(256) = ...`; `THROW 5000x, ..., 1` | Grammar gap | Grammar Task 4; intentional no first-class facts |
| T-SQL MERGE | `MERGE ... USING (VALUES ...) ... WHEN NOT MATCHED THEN INSERT` creates multi-line recovery spans | Grammar gap plus extractor gap | Grammar Task 4; `sql.merge_statement.v1` Task 5 |
| Already-supported syntax obscured by cascades | isolated probes: `N'...'`, `rowversion`, `ALTER TABLE ... ADD`, `DROP INDEX ... ON`, `CREATE UNIQUE INDEX`, `ADD CONSTRAINT ... FOREIGN KEY`, `DROP SCHEMA` all parse cleanly | Not a gap | Add regression controls; do not rewrite these rules |
| Remaining SQL semantic breadth | INSERT/DELETE facts, procedures/functions, windows, additional vendor DDL | Existing explicit capability debt | Non-goal; keep open gap with updated wording |

### Parser candidate evidence

A standalone tree-sitter probe using `tree-sitter-sequel-tsql` 0.4.2 reduced the six files from 284 diagnostics to 53 (49 error + 4 missing), a reduction of 231 rows / 81.3%. Residuals are exactly the planned Task 3-4 classes: `IDENTITY`, `(MAX)`, computed persisted columns, named inline constraints, `SET`, `IF/BEGIN/END`, initialized `DECLARE`, `THROW`, and `MERGE ... USING (VALUES ...)`. This proves the fork is the smallest valuable base but not the final solution.

## Verification Strategy

**Worker red/green scope:**

```bash
cargo test -p julie-extractors tests::sql::parse_quality -- --nocapture
cargo test -p julie-extractors tests::sql::structural_facts -- --nocapture
cargo xtask test language sql
```

**Grammar scope (in `/Users/murphy/source/tree-sitter-sql`):**

```bash
npx --yes --package=tree-sitter-cli@0.26.3 -- tree-sitter generate
npx --yes --package=tree-sitter-cli@0.26.3 -- tree-sitter test
```

**Golden/capability scope:**

```bash
cargo xtask test golden
cargo xtask test capability
UPDATE_CONTRACT_JSON=1 cargo test -p julie-extractors structural_fact_registry
node scripts/language-data-quality-report.mjs --strict
```

The strict report must finish with `silent_cells=0` and `quality_bar_debts=0`.

**Branch gate:**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --no-deps -- -D warnings
cargo xtask test default
cargo xtask test contract
cargo xtask test certification
node scripts/language-data-quality-report.mjs --strict
cargo build -p julie-extract-cli --bin julie-extract
```

Then run the live Terraform scan from the implementation branch and require this query to return no rows:

```sql
SELECT language, kind, COUNT(*)
FROM parse_diagnostics
WHERE language IN ('sql', 'razor')
GROUP BY language, kind
ORDER BY language, kind;
```

Also require the scan JSON to retain `files_scanned=418`, `files_unsupported=30`, and `files_failed=0` unless the Terraform corpus itself changed; if it changed, record the new corpus commit and explain every count delta.

**Negative-control gate:** a malformed bracket identifier, unterminated `BEGIN`, invalid `IDENTITY(1,)`, malformed `MERGE`, and `THROW` without required arguments must each still produce at least one diagnostic.

## Parallel Execution Contract

| Task | Parallel batch | File ownership | Serialization | Dependency |
|---|---|---|---|---|
| Task 1: Freeze probes/baseline | none | SQL parse-quality tests + minimized source fixtures only | serial | Establishes red tests and exact taxonomy |
| Task 2: Adopt T-SQL grammar base | none | grammar fork base + Cargo/language inventory | serial | Must preserve existing SQL node contracts before more grammar edits |
| Task 3: DDL/type grammar closure | batch A | grammar `grammar.js`, T-SQL DDL corpus | serial within grammar repo | Depends on Task 2 base |
| Task 4: Batch/control/MERGE grammar closure | batch A | grammar `grammar.js`, T-SQL control/MERGE corpus | serial within grammar repo | Depends on Task 2; may be developed alongside Task 3 only with separate commits/rebase |
| Task 5: Extractor semantics | none | `src/sql/`, SQL structural collector/registry/tests | serial | Requires final node shapes from Tasks 3-4 |
| Task 6: Goldens, capability, corpus gate | none | fixture expected JSON, capability row, final evidence | serial | Integrates all previous tasks |

Tasks 3 and 4 are conceptually independent but both own `grammar.js`; use separate commits and serialize merges. Do not run two workers against the same grammar checkout.

---

### Task 1: Freeze minimized T-SQL probes and the corpus baseline

**Objective:** make every primary syntax class reproducible without the private live corpus and prove the current parser is red.

**Files:**
- Create: `crates/julie-extractors/src/tests/sql/parse_quality.rs`
- Modify: `crates/julie-extractors/src/tests/sql/mod.rs`
- Create: `fixtures/extraction/sql/tsql_ddl/source.sql`
- Create: `fixtures/extraction/sql/tsql_batch_control/source.sql`
- Do not create expected JSON or register fixtures yet; Task 6 does that after extraction semantics stabilize.

**Steps:**
1. Add a helper that parses SQL with the registered parser and returns `pipeline::parse_diagnostics_for_tree`.
2. Add one focused test per taxonomy class: bracketed multipart names; `GO`; `IDENTITY` bare and seeded; `nvarchar(max)`/`varbinary(max)`; computed persisted column; named inline constraints; `SET`; IF predicates with `OBJECT_ID`, `SCHEMA_ID`, `COL_LENGTH`, and EXISTS; BEGIN/END; initialized DECLARE; THROW; MERGE USING VALUES.
3. Add supported-control tests for Unicode `N'...'`, `rowversion`, ALTER TABLE ADD, DROP/CREATE INDEX, ADD FK, and DROP SCHEMA.
4. Add the malformed negative controls from the verification strategy.
5. Run the focused test and record the exact current failure count. Expected: valid-shape tests fail; supported controls and malformed controls pass.
6. Commit only tests/fixture sources: `test(sql): freeze Terraform T-SQL parse gaps`.

**Acceptance:** every taxonomy row has a named test; source fixtures contain no application-specific names/data beyond generic equivalents; the current 0.3.11 parser demonstrates red without changing diagnostic collection.

### Task 2: Adopt and pin the T-SQL grammar foundation

**Objective:** land bracket identifiers and `GO` support while proving the fork preserves existing general-SQL extraction.

**Grammar files (external repo):**
- Create/fork checkout: `/Users/murphy/source/tree-sitter-sql` from `jamie8johnson/tree-sitter-sql@b3db1ee85908a0c0e425bc59ddf04c6ad107eecf`
- Create: `test/corpus/tsql_identifiers_and_batches.txt`
- Existing generated files must remain reproducible: `src/parser.c`, `src/grammar.json`, `src/node-types.json`

**julie-extractors files:**
- Modify: `crates/julie-extractors/Cargo.toml:59` to a Git dependency on `anortham/tree-sitter-sql` pinned by full revision, aliased so Rust call sites continue to use `tree_sitter_sequel::LANGUAGE`
- Modify: `Cargo.lock`
- Modify: `crates/julie-extractors/src/language_spec/specs.rs:247` parser inventory label to `tree-sitter-sequel-tsql`
- Modify: `fixtures/extraction/capabilities.json` SQL `parser_crate` only; fixture registration waits for Task 6
- Update exact parser-inventory expectations found by `cargo xtask test certification`; do not weaken them.

**Steps:**
1. Preserve the 0.4.2 bracket/GO rules in the fork and add grammar corpus expectations showing bracketed parts remain named `identifier` nodes and `GO` is a dedicated `go_statement` sibling, never absorbed into the prior statement.
2. Run grammar generate/test.
3. Pin the resulting fork revision in julie-extractors and run existing SQL unit tests, `cargo xtask test golden`, and certification before modifying any extractor code.
4. Run Task 1. Expected intermediate state: bracket/GO tests green; residual valid tests red.
5. Replay the live Terraform scan. Expected intermediate metric: no more than 53 SQL diagnostics and zero Razor diagnostics.
6. Stop if an existing non-T-SQL SQL golden changes node-derived output. Fix the fork compatibly or reject the switch; do not rewrite expected JSON to accept regression.
7. Commit grammar and julie pins separately: `feat(sql-grammar): adopt pinned T-SQL grammar base`.

**Acceptance:** bracket and GO probes are green; existing SQL tests/goldens are byte-stable; live diagnostics are <=53 SQL and 0 Razor.

### Task 3: Close T-SQL DDL/type grammar gaps

**Objective:** parse the baseline schema's DDL without recovery nodes.

**Files (grammar repo only):**
- Modify: `grammar.js`
- Create: `test/corpus/tsql_ddl.txt`
- Regenerate: `src/parser.c`, `src/grammar.json`, `src/node-types.json`

**Required grammar shapes:**
- `IDENTITY` and `IDENTITY(seed, increment)` as a named column modifier with integer children.
- `nvarchar(max)` and `varbinary(max)` as parameterized type nodes; numeric lengths continue to parse.
- Computed columns: `name AS expression [PERSISTED]` as `column_definition`, not an opaque/error span.
- Inline named column constraints such as `CONSTRAINT PK_Name PRIMARY KEY` and `CONSTRAINT DF_Name DEFAULT (...)`.
- Table-level named composite PK/FK constraints with bracketed multipart references.
- Preserve clean parsing of `rowversion` and ordinary ALTER/INDEX/FK statements.

**Steps:** add one failing grammar corpus case per shape; run test red; implement the minimal rule; regenerate; run full grammar corpus green after each shape. After pin bump, run Task 1 and the live scan.

**Acceptance:** `db/baseline.sql` has no DDL/type/constraint diagnostics; malformed IDENTITY and computed-column controls remain diagnostic; existing dialect corpus remains green.

### Task 4: Close batch/control-flow/procedural/MERGE grammar gaps

**Objective:** parse the five change scripts and the baseline's schema guard without treating procedural T-SQL as artifact semantics.

**Files (grammar repo only):**
- Modify: `grammar.js`
- Create: `test/corpus/tsql_control_flow.txt`
- Create: `test/corpus/tsql_merge.txt`
- Regenerate: `src/parser.c`, `src/grammar.json`, `src/node-types.json`

**Required named nodes:**
- `set_statement` variants for `NOCOUNT ON` and `XACT_ABORT ON`.
- `if_statement` with expression predicates and either one statement or a `begin_end_block`.
- Predicates must admit `OBJECT_ID(...) IS [NOT] NULL`, `SCHEMA_ID(...) IS NULL`, `COL_LENGTH(...) IS NULL`, and `[NOT] EXISTS (SELECT ...)`.
- `declare_statement` with T-SQL `@parameter`, type, and optional initializer.
- `throw_statement` with error number, message, and state.
- `merge_statement` supporting `USING (VALUES ...) AS alias(columns)`, ON expression, and WHEN NOT MATCHED THEN INSERT ... VALUES ... for the corpus shape.

**Steps:** red grammar test per statement family; minimal grammar implementation; malformed negative control; regenerate/test; bump pinned revision once all families are green; run Task 1 and live scan.

**Acceptance:** all six live files report zero error/missing nodes at the parser level; grammar node names are stable enough for Task 5; malformed controls remain diagnostic.

### Task 5: Normalize extracted T-SQL names and add `sql.merge_statement.v1`

**Objective:** turn clean parse trees into useful, capability-backed artifact rows.

**Files:**
- Modify: `crates/julie-extractors/src/sql/helpers.rs` (add one shared bracket/double-quote/backtick identifier normalizer; unescape `]]`)
- Modify: `crates/julie-extractors/src/sql/schemas.rs`
- Modify: `crates/julie-extractors/src/sql/constraints.rs`
- Modify: `crates/julie-extractors/src/sql/relationships.rs`
- Modify: `crates/julie-extractors/src/sql/identifiers.rs`
- Modify: `crates/julie-extractors/src/base/sql_structural_facts.rs`
- Modify: `crates/julie-extractors/src/base/structural_fact_registry.rs`
- Modify: `crates/julie-extractors/src/tests/sql/structural_facts.rs`
- Modify/add focused symbol/relationship/identifier tests under `crates/julie-extractors/src/tests/sql/`
- Regenerate: `docs/contracts/structural-fact-patterns.json`

**Locked semantic behavior:**
- `[edr].[EdrForms]` yields normalized object name `EdrForms` and retains schema `edr` in metadata where that fact already exposes object qualification; it must never name the table `edr` or `[edr]`.
- Bracketed columns/constraints normalize similarly; source spans still cover the original bracketed text.
- `sql.merge_statement.v1`: `query_family="mutation_structure"`, `capture_name="merge"`, node kind `merge_statement`; required metadata `target_table` (normalized string), `source_kind` (`values|query|table`), `has_when_matched` (bool), `has_when_not_matched` (bool); optional `source_table` only for a static table source.
- No facts for GO/SET/IF/BEGIN/DECLARE/THROW.

**Steps:** write failing extractor tests first; add shared normalization and update every listed SQL consumer; add MERGE collector/registry spec; regenerate contract JSON; run focused tests, registry sync, existing SQL golden, and negative controls.

**Acceptance:** T-SQL fixtures produce correct normalized symbols, identifiers, relationships, DDL facts, and one MERGE fact; existing unquoted SQL output is byte-stable; registry JSON is synchronized.

### Task 6: Register goldens, update capability evidence, and run final gates

**Objective:** convert the minimized T-SQL sources into durable capability evidence and prove the live corpus plus Razor gate.

**Files:**
- Create: `fixtures/extraction/sql/tsql_ddl/expected.json`
- Create: `fixtures/extraction/sql/tsql_batch_control/expected.json`
- Modify: `fixtures/extraction/capabilities.json`
- Optional evidence doc if review requests retained scan output: `docs/findings/2026-07-11-tsql-parse-quality-results.md`

**Steps:**
1. Generate canonical expected JSON through the repository's golden workflow; review every row rather than accepting bulk churn.
2. Register both SQL fixtures.
3. Add `sql.merge_statement.v1` to supported SQL structural facts.
4. Edit the existing `sql.advanced_dml_and_procedure_structure` open gap so MERGE is no longer listed; keep INSERT/DELETE, routines, windows, and remaining vendor-specific DDL as named closure work with the same planned follow-up.
5. Run golden, capability, registry, strict quality, default/contract/certification, fmt, and clippy gates.
6. Build the CLI and replay the Terraform scan. Preserve the SQLite artifact + JSON report until review; report the exact implementation commit and counts.
7. Verify SQL=0 and Razor=0 using the query above and verify malformed focused fixtures still emit diagnostics.
8. Commit: `test(sql): certify T-SQL parse-quality closure`.

**Acceptance:** two registered goldens; capability row accurately reflects MERGE support and remaining debt; `silent_cells=0`; `quality_bar_debts=0`; live corpus SQL/Razor query returns no rows; 418/388/30/0 corpus posture preserved unless source changed with documented delta.

## Non-Goals

- No changes to Razor/Blazor grammar or extractor behavior; Razor is a regression gate only.
- No diagnostic filtering, severity downgrade, span coalescing, or artifact schema change.
- No SQL dialect-selection CLI, `.tsql` extension, preprocessor, or source rewrite.
- No complete T-SQL language implementation beyond constructs proven by the six files plus malformed controls.
- No first-class facts for GO, SET, IF, blocks, DECLARE, or THROW in this issue.
- No closure claim for INSERT/DELETE/routines/windows/general vendor DDL.
- No MCP, daemon, search, watcher, dashboard, or editing behavior.
- No release, push, or upstream publication unless Murphy separately requests it; local fork commits/pins are implementation prerequisites, not a release authorization.

## Implementation Handoff Body

Use this body for the child implementation card:

> **Assignee:** `cursor-agent`
>
> Implement `docs/plans/2026-07-11-tsql-parse-quality-implementation-plan.md` exactly. Base from `codex/blazor-razor-support@1af555fb598d003fcdcbfab6711827c0e058041e` or a descendant containing it. Work grammar-first in an owned fork derived from `jamie8johnson/tree-sitter-sql@b3db1ee85908a0c0e425bc59ddf04c6ad107eecf`, pin exact revisions, and follow Tasks 1-6 in order with TDD. Do not suppress diagnostics or edit Razor code. Required final evidence: focused SQL tests, grammar corpus, goldens/capability/registry, strict quality report at 0/0, default/contract/certification gates, and a fresh Terraform scan whose `parse_diagnostics` query returns no SQL or Razor rows while file counts remain 418 scanned / 388 extracted / 30 unsupported / 0 failed unless the corpus changed and the delta is documented. Coding output requires review; comment structured changed-files/tests/evidence metadata and block `review-required` rather than auto-completing.
