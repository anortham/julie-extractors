# T-SQL Parse-Quality Closure Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: use `razorback:subagent-driven-development` when delegation is available; otherwise use `razorback:executing-plans`. This plan is grammar-first and test-driven. Do not suppress, filter, merge, or downgrade parse diagnostics.

**Goal:** Reduce the Terraform T-SQL corpus at `821e6b1a268cb392b1abb5080243a299db2a9bc9` from 283 `error` + 1 `missing` diagnostics to zero while preserving zero Razor diagnostics, then emit normalized SQL symbols/facts for the newly parsed T-SQL constructs.

**Architecture:** Start from the published T-SQL extension fork `tree-sitter-sequel-tsql` 0.4.2 (source commit `b3db1ee85908a0c0e425bc59ddf04c6ad107eecf`), because a live probe reduced this corpus from 284 diagnostics to 53 without source preprocessing. Carry the grammar in an `anortham/tree-sitter-sql` fork pinned by exact revision; close the residual syntax classes in the grammar, then adapt the existing SQL extractor to normalized bracketed names and add one useful `sql.merge_statement.v1` fact. Parse recovery remains tree-sitter's responsibility; extraction remains inside `julie-extractors`.

**Tech Stack:** Rust 2024, tree-sitter 0.26.9, tree-sitter CLI 0.26.3, tree-sitter SQL grammar (`grammar.js` + generated C parser), SQLite artifact diagnostics, golden fixtures, capability matrix.

## Global Constraints

- Base Julie Extractors work on current `main` `bfced7be9abd8754d11d5152f2ca84e57dec3f0d`; preserve its `tree-sitter-razor` pin and zero-diagnostic Terraform behavior.
- Work grammar changes in the isolated `codex/tsql-parse-quality-grammar` worktree rooted at `b3db1ee85908a0c0e425bc59ddf04c6ad107eecf`; do not modify the dirty `feat/tsql-julie-integration` checkout.
- The final Cargo dependency must resolve from `https://github.com/anortham/tree-sitter-sql` at one full commit SHA; no committed local path, unpushed revision, or floating branch is acceptable.
- The user approved the grammar fork push on 2026-07-13. `anortham/tree-sitter-sql` branch `tsql-parse-quality` now resolves to the verified final grammar commit `63ea933e464813d01cab5d7febcb0f77409c247b`; Cargo integration must pin that exact remote revision.
- Preserve all existing general-SQL node-derived extraction, goldens, capability claims, and artifact contracts except the two evidence-proven `sql:basic` corrections recorded below: `active_workers` moves from recovery metadata to clean-parser `bodySpanSource="statement_text"`, and the `jobs` table fact reports its two parser-backed table constraints instead of zero. The checked-in baseline contains no view diagnostic for that row, so `parse_diagnostics` remains unchanged; no other general-SQL artifact row may change.
- Keep the Terraform scan and full grammar certification outside the default test tier.
- Do not edit Julie, tree-sitter-razor, Miller, or Eros. Do not push, tag, publish, release, or choose a release version without explicit approval.

## Architecture Quality

**Affected modules:** external SQL grammar fork; `crates/julie-extractors` parser dependency and language inventory; `src/sql/` symbol/identifier extraction and routine-complexity compatibility; `src/base/sql_structural_facts.rs`; structural-fact registry; SQL goldens/capability row.

**Caller-facing interface:** unchanged `julie-extract` CLI and SQLite/JSONL contracts. The behavioral change is fewer persisted `parse_diagnostics`, normalized SQL names, and one additive registered structural-fact pattern.

**Depth/locality check:** SQL syntax belongs in the grammar fork. SQL semantic normalization belongs in one shared SQL helper used by existing extractor consumers. No T-SQL preprocessing layer, CLI dialect switch, or artifact schema change is introduced.

**Test surface:** grammar corpus tests prove concrete node shapes; extractor tests and registered goldens prove symbols/identifiers/facts through the canonical extraction pipeline; the CLI corpus scan proves persisted SQLite diagnostics.

**Seams/adapters:** no new framework seam. The only new helper is SQL identifier normalization, required because bracket-quoted identifiers are grammar-level `identifier` nodes whose source text retains `[...]`.

**Rejected shortcuts:** deleting or filtering diagnostics; stripping `GO` or brackets before parsing; regex recovery as primary support; treating all 284 nested diagnostics as independent bugs; changing the SQL extension to `.tsql`; switching parsers without running the existing general-SQL goldens; claiming T-SQL support from parser success alone.

**Architecture risk:** high. A parser dependency change can alter node shapes for every SQL dialect, so the dependency switch is fail-fast and precedes extractor work.

## Locked Decisions

1. **Implementation base:** use current `main` `bfced7be9abd8754d11d5152f2ca84e57dec3f0d`. A fresh 2.13.0 CLI build at this commit scans Terraform `821e6b1a268cb392b1abb5080243a299db2a9bc9` with 283 SQL errors, 1 SQL missing diagnostic, zero Razor diagnostics, and 418/388/30/0 scanned/extracted/unsupported/failed files.
2. **Grammar base:** fork `jamie8johnson/tree-sitter-sql` commit `b3db1ee85908a0c0e425bc59ddf04c6ad107eecf` (crate `tree-sitter-sequel-tsql` 0.4.2). The verified grammar was pushed with approval to `anortham/tree-sitter-sql` branch `tsql-parse-quality` at `63ea933e464813d01cab5d7febcb0f77409c247b`; pin that full remote revision. Do not commit a local path or wait for an upstream release.
3. **Acceptance target:** all six Terraform `.sql` files must have zero `error` and zero `missing` rows. Partial reduction is an intermediate metric, not completion.
4. **Diagnostic integrity:** malformed T-SQL must still emit diagnostics. Tests include negative controls; the fix must expand valid grammar, not make `ERROR` nodes invisible. Parameterless `THROW;` is valid only inside `CATCH`, so the malformed control is a two-argument `THROW 50001, N'message';`, not bare `THROW;`.
5. **Extraction scope:** normalize bracketed object/column names and add `sql.merge_statement.v1`. `GO`, `SET`, `IF`, `BEGIN/END`, `DECLARE`, and `THROW` are grammar-supported but intentionally emit no first-class artifact rows in this issue.
6. **Capability scope:** register two new SQL golden fixtures and add `sql.merge_statement.v1` to SQL structural-fact support. Keep the existing `sql.advanced_dml_and_procedure_structure` gap open, but remove MERGE from its reason/closure text; INSERT, DELETE, procedures/functions, windows, and other vendor DDL remain named debt.
7. **Test-tier discipline:** minimized T-SQL fixtures run in focused/golden tiers. The live Terraform scan is a release/affected-change gate only and must not enter the default suite.
8. **Grammar-base compatibility:** live SQL-tier evidence proved `tree-sitter-sequel-tsql` base `b3db1ee85908a0c0e425bc59ddf04c6ad107eecf` cleanly parses routines that 0.3.11 routed through recovery. Preserve the established artifact contract at the existing SQL extractor seam: procedures remain `Function` symbols with `CREATE PROCEDURE` signatures and parameter symbols, signatures prefer direct declared `function_argument` text while preserving the legacy `parameter_declaration`/`parameter` fallback when that list is absent, and callable complexity includes the trailing statement delimiter. Do not roll back the selected grammar or rewrite existing tests/goldens beyond the reviewed `active_workers` and `jobs.constraint_count` corrections recorded below.
9. **Constraint-wrapper compatibility:** final capability review proved the selected grammar places table-level `constraint` nodes inside one direct `constraints` wrapper. The general-SQL `jobs` fixture has two such constraints, so its prior `constraint_count=0` row was false. Count only direct constraints plus direct children of that wrapper; do not recursively count column constraints or nested statements. The reviewed general-SQL migration is `jobs.constraint_count: 0 -> 2`; `workers` remains zero and `sql:cross_file` remains byte-stable.

## Reproduced Baseline

### Exact reference run

Built fresh from `/Users/murphy/source/julie-extractors/.worktrees/blazor-review-fixes` at current main `bfced7be9abd8754d11d5152f2ca84e57dec3f0d`, against clean Terraform `821e6b1a268cb392b1abb5080243a299db2a9bc9`:

```bash
cargo build -p julie-extract-cli --bin julie-extract
RUN_DIR=$(mktemp -d /tmp/julie-tsql-baseline.XXXXXX)
./target/debug/julie-extract scan \
  --root /Users/murphy/source/Terraform \
  --db "$RUN_DIR/terraform.sqlite" \
  --force --json > "$RUN_DIR/scan.json"
sqlite3 "file:$RUN_DIR/terraform.sqlite?immutable=1" \
  "SELECT language, kind, COUNT(*) FROM parse_diagnostics GROUP BY language, kind ORDER BY language, kind;"
sqlite3 "file:$RUN_DIR/terraform.sqlite?immutable=1" \
  "SELECT path, kind, COUNT(*) FROM parse_diagnostics WHERE language='sql' GROUP BY path, kind ORDER BY path, kind;"
```

Observed on 2026-07-13: status `ok`; 418 paths scanned; 388 supported/extracted; 30 unsupported; 0 failed; diagnostics only for SQL (`283 error`, `1 missing`); no Razor rows. SQLite 3.51 could not open this WAL-mode artifact with `-readonly`; the immutable URI above is the verified non-mutating query form.

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

A fresh build of the old local-only branch `771a2152bf5f27197a43eb1f1b4f5cabcbf0449d` using `tree-sitter-sequel-tsql` 0.4.2 reduced the six files from 284 diagnostics to 53 (49 error + 4 missing), a reduction of 231 rows / 81.3%, while retaining zero Razor rows. Its focused probes were 13 passing / 10 failing and its `sql:basic` golden regressed, so the reduction validates the grammar base but not the old integration. Residuals are exactly the planned Task 3-4 classes: `IDENTITY`, `(MAX)`, computed persisted columns, named inline constraints, `SET`, `IF/BEGIN/END`, initialized `DECLARE`, `THROW`, and `MERGE ... USING (VALUES ...)`.

### Final remote grammar evidence

The final local grammar passed all 496 corpus cases, the full Rust grammar tests and doctest, all six Terraform SQL files with zero parser diagnostics, and malformed controls that remained diagnostic. After explicit approval, commit `63ea933e464813d01cab5d7febcb0f77409c247b` was pushed to `https://github.com/anortham/tree-sitter-sql` on branch `tsql-parse-quality`; `git ls-remote` resolves that branch to the exact commit. This is the only approved grammar revision for the Julie Extractors integration.

The full Julie SQL tier then exposed two compatibility regressions that were absent from the minimized goldens: `test_extract_stored_procedures_functions_and_triggers` lost the recovery-path procedure contract, and `sql_callable_symbol_complexity_uses_body_span_with_predicate_evidence` ended at the clean routine node instead of the trailing delimiter. Both failures reproduce unchanged against the unmodified grammar base `b3db1ee85908a0c0e425bc59ddf04c6ad107eecf`, proving they predate the final T-SQL grammar work. Task 4 therefore includes a minimal extractor adapter for the grammar base's clean routine nodes; existing tests remain unchanged.

The final grammar pin also cleanly parses the general-SQL `CREATE VIEW active_workers AS ...` body-span fixture that 0.3.11 routed through recovery. Preserve the truthful clean-path result: do not add `extractedFromError`, keep the trigger in that fixture on its proven recovery path, and populate the existing `bodySpanSource="statement_text"` contract when statement inference validates an already adequate AST-provided view body span without replacing it. The checked-in `sql:basic` baseline already contains only the three trigger diagnostics, so this migration changes no `parse_diagnostics` row.

## Verification Strategy

**Project source of truth:** `AGENTS.md`, this plan, `fixtures/extraction/README.md`, and the repository's `cargo xtask test` tiers.

**Worker ceiling:** focused grammar corpus cases, focused SQL parser/extractor tests, and `cargo xtask test language sql`. Workers do not own live Terraform scans, full grammar certification, or branch gates unless the lead asks for diagnostic output.

**Worker gate invariant:** focused parser tests prove valid T-SQL is diagnostic-free and malformed controls remain diagnostic; focused extractor tests prove normalized caller-facing artifact rows; the SQL language tier proves existing SQL behavior remains stable.

**Lead affected-change scope:** full grammar generation/corpus tests, SQL language tier, golden/capability/registry tests, and one fresh Terraform replay after Tasks 2, 3, and 4 when the parser changes.

**Replay/metric evidence:** SQL and Razor diagnostic counts and failed-file count are hard gates; timing and total non-diagnostic row counts are report-only unless a contract test fails.

**Escalation triggers:** any existing SQL golden change, Razor diagnostic, malformed control becoming clean, parser inventory drift beyond the named SQL dependency, or default-tier leakage requires root-cause analysis before continuing.

**Verification ledger:** record invariant, command, scope, commit SHA, result, and timestamp under `.razorback/sdd/verification-ledger.md`. Evidence is stale when the relevant HEAD changes.

**Worker red/green scope:**

```bash
cargo test -p julie-extractors tests::sql::parse_quality -- --nocapture
cargo test -p julie-extractors tests::sql::structural_facts -- --nocapture
cargo xtask test language sql
```

**Grammar scope (in `/Users/murphy/.config/razorback/worktrees/tree-sitter-sql/tsql-parse-quality`):**

```bash
npx --yes --package=tree-sitter-cli@0.26.3 -- tree-sitter generate
npx --yes --package=tree-sitter-cli@0.26.3 -- tree-sitter test
```

**Golden/capability scope:**

```bash
cargo xtask test golden
cargo xtask test capability
UPDATE_CONTRACT_JSON=1 cargo test -p julie-extractors structural_fact_registry
cargo test -p julie-extractors --features test-capability-matrix structural_fact_registry
cargo test -p julie-extractors test_public_contract_version_marks_current_fact_families -- --nocapture
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
cargo test -p julie-extractors tests::test_tiers::test_tier_convention_keeps_slow_gates_out_of_default_suite -- --exact --nocapture
node scripts/language-data-quality-report.mjs --strict
cargo build --locked -p julie-extract-cli --bin julie-extract
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

**Negative-control gate:** a malformed bracket identifier, unterminated `BEGIN`, invalid `IDENTITY(1,)`, malformed `MERGE`, and two-argument `THROW 50001, N'message';` must each still produce at least one diagnostic.

## Parallel Execution Contract

| Task | Parallel batch | File ownership | Serialization | Dependency |
|---|---|---|---|---|
| Task 1: Freeze probes/baseline | none | SQL parse-quality tests + minimized source fixtures only | serial | Establishes red tests and exact taxonomy |
| Task 2: Validate owned T-SQL grammar base | none | isolated grammar fork worktree and base corpus only | serial | Must preserve existing SQL node contracts before more grammar edits; final Cargo pin waits for the verified Task 4 commit |
| Task 3: DDL/type grammar closure | batch A | grammar `grammar.js`, T-SQL DDL corpus | serial within grammar repo | Depends on Task 2 base |
| Task 4: Batch/control/MERGE grammar closure and remote pin | batch A | grammar `grammar.js`, T-SQL control/MERGE corpus, Cargo/language inventory | serial within grammar repo | Depends on Tasks 2-3; one verified final grammar commit must be pushed with approval before Cargo integration |
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
4. Add the malformed negative controls from the verification strategy. Use two-argument `THROW 50001, N'message';`; Microsoft documents parameterless `THROW;` as valid inside `CATCH`.
5. Run the focused test and record the exact current failure count. Expected: valid-shape tests fail; supported controls and malformed controls pass.
6. Commit only tests/fixture sources: `test(sql): freeze Terraform T-SQL parse gaps`.

**Acceptance:** every taxonomy row has a named test; source fixtures contain no application-specific names/data beyond generic equivalents; the current 0.3.11 parser demonstrates red without changing diagnostic collection.

### Task 2: Validate the owned T-SQL grammar foundation

**Objective:** establish a clean owned-fork candidate with bracket identifiers and `GO` support while proving the grammar base preserves existing general-SQL node shapes. Do not add a local path dependency to Julie Extractors.

**Grammar files (external repo):**
- Use isolated checkout: `/Users/murphy/.config/razorback/worktrees/tree-sitter-sql/tsql-parse-quality` from `jamie8johnson/tree-sitter-sql@b3db1ee85908a0c0e425bc59ddf04c6ad107eecf`
- Create: `test/corpus/tsql_identifiers_and_batches.txt`
- Existing generated files must remain reproducible: `src/parser.c`, `src/grammar.json`, `src/node-types.json`
- Preserve `src/tree_sitter/array.h` from `45013b1f4c575bf6b4ead72730504cf7b6535ccb`; that file was manually vendored after generation, and CLI 0.26.3 rewrites it to an older bundled header. The downgrade is not a grammar output and must not be committed.

**julie-extractors files:** none. Task 4 integrates the final remotely resolvable grammar commit after grammar closure; Task 2 must leave the extractor task branch free of local-only dependency changes.

**Steps:**
1. Preserve the 0.4.2 bracket/GO rules in the fork and add grammar corpus expectations showing bracketed parts remain named `identifier` nodes and `GO` is a dedicated `go_statement` sibling, never absorbed into the prior statement. The grammar root is `program`; reject the old local corpus expectations that used `source_file`.
2. Run grammar generate/test.
3. Use a throwaway external probe or the already measured clean old branch to confirm the intermediate 53-diagnostic ceiling without committing any Julie dependency change.
4. Compare named node shapes for every existing SQL golden input against current `tree-sitter-sequel` 0.3.11. Stop if a non-T-SQL named node shape changes; fix the fork compatibly or reject the switch.
5. Commit only the clean grammar base/corpus: `test(tsql): certify identifiers and batch separators`.

**Acceptance:** bracket and GO corpus cases are green under root `program`; all existing grammar corpus tests pass; generated files are reproducible and clean after regeneration; no local path is committed in Julie Extractors; the measured intermediate posture remains <=53 SQL and 0 Razor.

### Task 3: Close T-SQL DDL/type grammar gaps

**Objective:** parse the baseline schema's DDL without recovery nodes.

**Files (grammar repo only):**
- Modify: `grammar.js`
- Create: `test/corpus/tsql_ddl.txt`
- Regenerate: `src/parser.c`, `src/grammar.json`, `src/node-types.json`

**Required grammar shapes:**
- Bracket-quoted identifiers with SQL Server's escaped closing bracket (`]]`) remain one named `identifier`, so Task 5 can normalize them without source recovery.
- `IDENTITY` and `IDENTITY(seed, increment)` as a named column modifier with integer children.
- `nvarchar(max)` and `varbinary(max)` as parameterized type nodes; numeric lengths continue to parse.
- Computed columns: `name AS expression [PERSISTED]` as `column_definition`, not an opaque/error span.
- Inline named column constraints such as `CONSTRAINT PK_Name PRIMARY KEY` and `CONSTRAINT DF_Name DEFAULT (...)`.
- Table-level named composite PK/FK constraints with bracketed multipart references.
- Preserve clean parsing of `rowversion` and ordinary ALTER/INDEX/FK statements.

**Steps:** add one failing grammar corpus case per shape; run test red; implement the minimal rule; regenerate; run full grammar corpus green after each shape. Use the standalone grammar probe against `db/baseline.sql` and require no DDL/type/constraint diagnostics. Task 4 owns the final remote pin, Task 1 green run, and full live scan after all grammar classes close.

**Acceptance:** `db/baseline.sql` has no DDL/type/constraint diagnostics; malformed IDENTITY and computed-column controls remain diagnostic; existing dialect corpus remains green.

### Task 4: Close batch/control-flow/procedural/MERGE grammar gaps

**Objective:** parse the five change scripts and the baseline's schema guard without treating procedural T-SQL as artifact semantics.

**Files (grammar repo only):**
- Modify: `grammar.js`
- Modify: `bindings/rust/lib.rs` (correct the inherited doctest to use the package's actual `tree_sitter_sequel_tsql` crate name; runtime API stays unchanged)
- Create: `test/corpus/tsql_control_flow.txt`
- Create: `test/corpus/tsql_merge.txt`
- Regenerate: `src/parser.c`, `src/grammar.json`, `src/node-types.json`

**Julie Extractors integration files after remote approval:**
- Modify: `crates/julie-extractors/Cargo.toml` to a Git dependency on `https://github.com/anortham/tree-sitter-sql` pinned by the full verified revision and aliased so Rust call sites continue to use `tree_sitter_sequel::LANGUAGE`.
- Modify: `Cargo.lock` and prove the exact remote source/revision with `cargo tree` plus lockfile inspection.
- Modify: `crates/julie-extractors/src/language_spec/specs.rs` parser inventory label to `tree-sitter-sequel-tsql`.
- Modify: `fixtures/extraction/capabilities.json` SQL `parser_crate` and `dependency_status` (`git_pinned`) only; fixture registration waits for Task 6.
- Modify: `crates/julie-extractors/src/sql/routines.rs`, `src/sql/mod.rs`, and `src/sql/complexity_metrics.rs`, plus focused coverage in `src/tests/sql/procedures.rs`, only for the proven grammar-base routine compatibility adapter described in locked decision 8.
- Update exact parser-inventory expectations found by `cargo xtask test certification`; do not weaken them.

**Required named nodes:**
- `set_statement` variants for `NOCOUNT ON` and `XACT_ABORT ON`.
- `if_statement` with expression predicates and either one statement or a `begin_end_block`.
- Predicates must admit `OBJECT_ID(...) IS [NOT] NULL`, `SCHEMA_ID(...) IS NULL`, `COL_LENGTH(...) IS NULL`, and `[NOT] EXISTS (SELECT ...)`.
- `declare_statement` with T-SQL `@parameter`, type, and optional initializer.
- `throw_statement` with error number, message, and state.
- `merge_statement` supporting `USING (VALUES ...) AS alias(columns)`, ON expression, and WHEN NOT MATCHED THEN INSERT ... VALUES ... for the corpus shape. Route only this new T-SQL form through the named node; preserve the existing standard `MERGE INTO ...` alternative and its corpus S-expression byte-for-byte.

**Steps:** red grammar test per statement family; minimal grammar implementation; malformed negative control; regenerate/test; run the complete grammar corpus and full grammar-repository Rust test, including the corrected binding doctest; commit the final grammar. Approval was granted and the verified commit was pushed to `anortham/tree-sitter-sql` branch `tsql-parse-quality`. Pin its remotely resolvable full SHA in Julie Extractors, run Task 1, existing SQL tests/goldens/certification, and the live scan. Do not commit or retain a local path.

**Acceptance:** all six live files report zero error/missing nodes at the parser level; grammar node names are stable enough for Task 5; malformed controls remain diagnostic; complete grammar corpus and Rust tests pass; existing SQL goldens are byte-stable except the reviewed `active_workers` recovery-to-clean and `jobs.constraint_count` corrections; Cargo resolves the exact full commit from `https://github.com/anortham/tree-sitter-sql`.

### Task 5: Normalize extracted T-SQL names and add `sql.merge_statement.v1`

**Objective:** turn clean parse trees into useful, capability-backed artifact rows.

**Files:**
- Modify: `crates/julie-extractors/src/sql/mod.rs` (expose the helper module within the crate so the base structural-fact collector can use the same normalizer; normalize parser-backed literal-carrier column names)
- Modify: `crates/julie-extractors/src/sql/helpers.rs` (add one shared bracket/double-quote/backtick identifier normalizer; unescape `]]`)
- Modify: `crates/julie-extractors/src/sql/schemas.rs`
- Modify: `crates/julie-extractors/src/sql/constraints.rs`
- Modify: `crates/julie-extractors/src/sql/relationships.rs`
- Modify: `crates/julie-extractors/src/sql/schema_relationships.rs`
- Modify: `crates/julie-extractors/src/sql/identifiers.rs`
- Modify: `crates/julie-extractors/src/sql/routines.rs`
- Modify: `crates/julie-extractors/src/sql/views.rs`
- Modify: `crates/julie-extractors/src/lib.rs` (append the downstream-visible `.sql-tsql-facts-v1` extraction-contract marker; numeric artifact contract versions remain unchanged)
- Modify: `crates/julie-extractors/src/tests/api_surface.rs` (require the new marker)
- Modify: `crates/julie-extractors/src/base/sql_structural_facts.rs`
- Modify: `crates/julie-extractors/src/base/structural_fact_registry.rs`
- Modify: `crates/julie-extractors/src/tests/sql/structural_facts.rs`
- Modify/add focused symbol/relationship/identifier tests under `crates/julie-extractors/src/tests/sql/`
- Regenerate: `docs/contracts/structural-fact-patterns.json`

**Locked semantic behavior:**
- `[edr].[EdrForms]` yields normalized object name `EdrForms` and retains schema `edr` in metadata where that fact already exposes object qualification; it must never name the table `edr` or `[edr]`.
- Bracketed columns/constraints normalize similarly; source spans still cover the original bracketed text.
- Every parser-backed artifact-facing SQL name in the files above uses the one shared helper. Existing error-recovery regex paths remain unchanged unless a failing fixture proves they need normalization.
- Multipart consumers select the grammar's `name` field before normalization; relationship metadata that retains qualification normalizes each path segment.
- `create_trigger` keeps its existing symbol signature and selects the target table from the `object_reference` after `ON`; it must never reuse the trigger declaration object as `target_table`.
- `sql.merge_statement.v1`: `query_family="mutation_structure"`, `capture_name="merge"`, node kind `merge_statement`; required metadata `target_table` (normalized string), `source_kind` (`values|query|table`), `has_when_matched` (bool), `has_when_not_matched` (bool); optional `source_table` only for a static table source.
- No facts for GO/SET/IF/BEGIN/DECLARE/THROW.

**Steps:** write failing extractor tests first; add shared normalization and update every listed SQL consumer; add the contract marker and API-surface assertion with the first emitted-shape change; add MERGE collector/registry spec; regenerate contract JSON; run focused tests, registry sync, feature-gated registry/emission parity, existing SQL golden, and negative controls.

**Acceptance:** T-SQL fixtures produce correct normalized symbols, identifiers, relationships, DDL facts, and one MERGE fact; parameterized type text is retained in column signatures without false type-size body spans; existing unquoted SQL output is byte-stable except the reviewed `active_workers` and `jobs.constraint_count` corrections, and `sql:cross_file` is byte-stable; registry JSON is synchronized.

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
5. Run golden, capability, registry, feature-gated registry/emission parity, API-surface, strict quality, slow-tier convention, default/contract/certification, fmt, and clippy gates.
6. Build the CLI and replay the Terraform scan. Preserve the SQLite artifact + JSON report until review; report the exact implementation commit and counts.
7. Verify SQL=0 and Razor=0 using the query above and verify malformed focused fixtures still emit diagnostics.
8. Commit: `test(sql): certify T-SQL parse-quality closure`.

**Acceptance:** two registered goldens; capability row accurately reports `tree-sitter-sequel-tsql`, `git_pinned`, MERGE support, and remaining debt; the public extraction-contract marker and API-surface guard are synchronized; `silent_cells=0`; `quality_bar_debts=0`; retained `languages --json` exposes the parser/capability/registry contract; live corpus SQL/Razor query returns no rows; 418/388/30/0 corpus posture preserved unless source changed with documented delta.

## Non-Goals

- No changes to Razor/Blazor grammar or extractor behavior; Razor is a regression gate only.
- No diagnostic filtering, severity downgrade, span coalescing, or artifact schema change.
- No SQL dialect-selection CLI, `.tsql` extension, preprocessor, or source rewrite.
- No complete T-SQL language implementation beyond constructs proven by the six files plus malformed controls.
- No first-class facts for GO, SET, IF, blocks, DECLARE, or THROW in this issue.
- No closure claim for INSERT/DELETE/routines/windows/general vendor DDL.
- No MCP, daemon, search, watcher, dashboard, or editing behavior.
- The approved grammar-fork push is complete. No further push, release, or upstream publication is authorized; the remote pin is an implementation prerequisite, not release authorization.

## Implementation Handoff Body

Use this body for the child implementation card:

> **Assignee:** lead agent with fresh Razorback implementer subagents per task
>
> Implement `docs/plans/2026-07-11-tsql-parse-quality-implementation-plan.md` exactly from `main@bfced7be9abd8754d11d5152f2ca84e57dec3f0d`. Work grammar-first in the isolated owned-fork candidate derived from `jamie8johnson/tree-sitter-sql@b3db1ee85908a0c0e425bc59ddf04c6ad107eecf`, follow Tasks 1-6 in order with TDD, and do not commit a local dependency path. The approved remote grammar is `anortham/tree-sitter-sql@63ea933e464813d01cab5d7febcb0f77409c247b` on branch `tsql-parse-quality`; pin that exact revision. Do not suppress diagnostics or edit Razor code. Required final evidence: focused SQL tests, full grammar corpus, goldens/capability/registry, strict quality report at 0/0, default/contract/certification gates, and a fresh Terraform scan whose `parse_diagnostics` query returns no SQL or Razor rows while file counts remain 418 scanned / 388 extracted / 30 unsupported / 0 failed unless the corpus changed and the delta is documented.
