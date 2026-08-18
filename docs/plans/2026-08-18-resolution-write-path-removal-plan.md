# Resolution Write-Path Removal Implementation Plan

> **Retirement plan (2026-08-18).** This plan removes the resolution write
> path. Live contracts are the current CLI/store/schema docs and
> [2026-08-18-resolution-write-path-retirement.md](../decisions/2026-08-18-resolution-write-path-retirement.md).

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** julie-extract stops producing reference resolution entirely — the `store resolve` verb, the resolution sessions, bases/deltas/pins/scope journal, and the artifact-level auto-resolve hooks are deleted — and the manifest language-classification bug that breaks cold import on C++-flavored `.h` files is fixed.

**Architecture:** Miller (the sole consumer) now computes resolution at query time from the fact tables (Miller repo, sibling "Plan A": `docs/plans/2026-08-18-query-time-resolution-phase1-plan.md` there; policy vendored as Miller's `docs/contracts/resolution-policy-v6.md`; spike evidence: 100.000% parity at Miller scale, 99.9997% at aspnetcore scale where all divergences were this repo's bounded session under-resolving). julie-extract returns to its intended boundary: a file-local extractor. Extraction, the store/coordinator, manifests, and generations are untouched; everything whose only job was materializing or maintaining workspace-global resolution is removed (~28,200 production lines, ~23,700 test lines). Schema objects are dropped **in place, without a schema-version bump**, using the repo's established idempotent-drop precedent, so existing family stores keep working and shed gigabytes instead of being refused and rebuilt.

**Tech Stack:** Rust workspace (julie-extract-cli, julie-extract-artifact, julie-extractors, xtask). No new dependencies.

**Architecture Quality:** Medium risk. The deletion itself is mechanical; the risk sits in three seams that survive and must keep working without resolution: (1) `store export` currently HARD-REQUIRES `resolution_state='exact'` and copies resolution into the exported artifact — it must be retargeted, not deleted (Miller's `MILLER_INDEX_STORE=off` mode depends on it); (2) the coordinator loses `RequestKind::Resolve` but must keep accepting/reconciling every other kind, including replay of pre-existing resolve rows in old coord.db files; (3) the in-place schema drops must leave every non-resolution table byte-identical (Miller reads them directly). If code reality contradicts this shape, report a plan mismatch rather than redesigning locally.

**Schema decisions (codex-reviewed 2026-08-18):**
- **Family store / coord: stays v2, retired IN PLACE via an explicit WRITER-ONLY migration.** The store has NO migration mechanism — a bump to 3 hard-refuses every existing family store (`store/schema.rs:110-131`) and forces a full re-extract per workspace, exactly the class of pain this project is removing. Precedent: `drop_retired_secondary_indexes` (`julie-extract-artifact/src/schema.rs:22-28`). The retirement is NOT a naive `DROP TABLE` pass: `views` holds foreign keys into the resolution tables (`store/schema.rs:625/:690`) with `foreign_keys=ON` (`:99`) and triggers reference them (`:1240`) — Task 5 specifies a transactional `views` rebuild (values preserved, resolution FKs and retired triggers removed), then child-first drops, then `foreign_key_check`. The migration runs on WRITER open paths only; read-only opens never mutate.
- **Standalone artifact: BUMP to v7** (remove `identifier_resolutions`/`pending_resolutions` and the resolution metadata key). Artifacts are regenerated per export/scan, so there is no field-migration pain, and a bump makes OLD binaries refuse v7 cleanly instead of accepting a v6-labeled file and failing at runtime (old v6 code actively queries `identifier_resolutions`, e.g. `jsonl.rs:862`). New `docs/contracts/sqlite-schema-v7.md` + catalog, added to `release_package_items`.
- **JSONL: BUMP to v5** removing the resolution-shaped keys — do not silently redefine v4.
- Consequence to note in release docs: the v7 binary READS old v6 artifacts (only newer is refused on read) but refuses them for WRITE commands per existing artifact_access rules — a standalone-artifact user re-extracts on first write. Family stores migrate in place and do NOT rebuild.

## Global Constraints

- **Do not remove, rename, or alter any non-resolution schema object.** Miller reads `symbols`, `identifiers`, `type_facts`, `pending_relationships`, `relationships`, `structural_facts`, `file_versions`, `files`, `manifest_entries`, generations, and `views` directly. The `views` table KEEPS its `resolution_state`/`resolution_base_id`/`resolution_delta_generation`/`resolution_exact_at` COLUMNS AND VALUES — the Task 5 rebuild removes only the resolution foreign keys and retired triggers; every column value is preserved (asserted by test). "Byte-identical" applies to all other tables; for `views` the contract is column-value preservation.
- Coordinator behavior for non-resolve kinds is byte-identical. Old coord.db files may contain historical `resolve` request rows in any state — they must be tolerated/reaped, never a panic or a wedge.
- `store export` must succeed with zero resolution state and produce an artifact whose fact tables are unchanged in shape; `store import` (both cold and `--from-artifact`) must succeed on repos that previously failed only at resolve.
- File:line references are orientation hints from `main` @ `270e984a`; earlier tasks shift them — locate by symbol, not raw line.
- The language fix must make manifest language and file-version language agree **by construction** (single source), not by adding a second sniff call that could drift.
- Warnings/clippy clean at the branch gate; `cargo fmt --check` clean; `AGENTS.md`/`CLAUDE.md` stay synchronized (`scripts/check-agent-doc-sync.sh`).
- No tag, no push, no publish, no `Current published release:` pointer change — release execution needs explicit user approval (Miller's "Phase 3" pin bump happens in the Miller repo after this releases).
- Strict TDD: write each behavior test, observe the expected failure, implement the minimum change, then rerun the same worker scope green.

## Verification Strategy

**Project source of truth:** `AGENTS.md`, `docs/testing-strategy.md`, `.github/workflows/ci.yml`, `docs/release.md:37-42`.

**Worker red/green scope:** the focused test target(s) for the task, e.g. `cargo test -p julie-extract-cli --test store_cli_contract`, `cargo test -p julie-extract-artifact --test store_manifest_contract`, `cargo test -p julie-extract-artifact --test store_coordinator_contract`.

**Worker ceiling:** `cargo xtask test default` (~97 s). Workers do not run the contract tier on their own.

**Worker gate invariant:** Task 1 — a store import of a C++-flavored `.h` corpus publishes its manifest (the exact failure class `file_version_language_mismatch` no longer fires). Task 2 — `store resolve` is gone from the CLI; coordinator accepts every remaining kind and reaps stale historical resolve rows. Task 3 — export/import round-trip succeeds on a store with no resolution state. Task 4 — scan/update/delete write no resolution rows and no longer force resolution-version re-extracts. Task 5 — new stores contain no resolution objects; opening an old store drops them idempotently; schema contract tests pin the new catalogs. Task 6 — docs/tiers/flags contain no live references to the write path.

**Lead affected-change scope:** `cargo xtask test default` after each task lands.

**Branch gate:** `cargo fmt --check` && `cargo test -p xtask` && `cargo xtask test default` && `cargo xtask test contract` && `cargo clippy --workspace --all-targets --all-features` && `git diff --check`.

**Security scope:** `cargo-deny` (already in CI) at the branch gate; no secrets scan declared.

**Replay/metric evidence:** hard gates — the worker gate invariants above plus the branch gate. Report-only — lines deleted, store size reclaimed on a real family store after Task 5's reap, default-tier and contract-tier wall time before/after (both should DROP; the contract tier loses its three serialized resolution targets).

**Escalation triggers:** any diff under `store/schema.rs`, `store/coordinator.rs`, or `store/manifest.rs` requires the full contract tier at the branch gate (already planned). A failure in a non-resolution contract test is a stop-and-report, never a test edit.

**Assigned verification failure:** Workers stop and report when assigned verification fails, unless this plan explicitly says to update that gate (the resolution-test deletions and tier-registration edits named per task are the sanctioned gate updates).

**Verification ledger:** Record invariant, command, scope label, commit SHA, result, and timestamp per task; reuse passing evidence on an unchanged HEAD.

## Parallel Execution Contract

| Task | Parallel batch | File ownership | Serialization required | Dependency reason |
|---|---|---|---|---|
| Task 1: Manifest language fix | None - serial | `crates/julie-extract-cli/src/store/executor.rs` (language sites only), new/edited manifest agreement tests | Yes | Runs first so the fix is cleanly separable/releasable; later tasks edit the same executor file. |
| Task 2: Remove `store resolve` | None - serial | Delete `store/resolve.rs`, `store/resolution_session.rs`, `store/delta_scope.rs`, `store/prior_overlay.rs`; edit `store/mod.rs`, `store/args.rs`, `store/executor.rs:1668` region, `store/coordinator.rs` resolve surface (RetiredResolve + reaper), `xtask/src/test_tiers.rs`; delete/edit named tests. Report types + `test-store-resolution-contract` feature stay until Task 3 | Yes | Consumers of the store session must go before Task 4/5 delete its providers. |
| Task 3: Retarget export/import | None - serial | `store/export.rs`, `store/from_artifact.rs`, `store/import.rs`, `store/executor.rs` (from-artifact + epoch-guard regions), `store/report.rs` + `store/mod.rs` (report types deleted here), CLI `Cargo.toml` feature; named tests | Yes | Must precede Task 4 (these files reference `RESOLUTION_VERSION`) and Task 5 (they reference base/delta modules). |
| Task 4: Remove artifact-level resolution | None - serial | Delete `resolution.rs`, `resolution_session.rs` (CLI), `julie-extract-artifact/src/resolution_store.rs`, `xtask/src/resolution_performance.rs`, `xtask/tests/resolution_performance_contract.rs`; edit `lib.rs` (both), `main.rs`, `capability_snapshot.rs`, `commands.rs`, `artifact_access.rs`, artifact `model.rs`/`writer.rs`, `reports.rs`, `jsonl.rs` (v5 bump + doc), `xtask/src/commands.rs`, `xtask/src/dogfood.rs`, CLI `test-perf` feature; named tests | Yes | Deletes providers referenced by Tasks 2–3's files. |
| Task 5: Schema retirement migration + store module deletion + GC | None - serial | Delete `julie-extract-artifact/src/store/{resolution.rs,resolution_diff.rs,scope.rs}`; edit `store/{mod.rs,schema.rs,connection.rs,manifest.rs,generation.rs,model.rs,maintenance.rs,coordinator.rs}`, `julie-extract-artifact/src/schema.rs` (artifact v7), artifact Cargo feature, schema contract docs (+ new v7 doc) + `.catalog.sha256` + contract tests + `release_package_items` | Yes | Last code task: every referencing module is already gone. |
| Task 6: Docs, tiers, release prep | None - serial | `docs/**` retirement sweep, `docs/testing-strategy.md`, new ADR in `docs/decisions/`, draft `docs/release-notes/v<next>.md`, `README.md`, `AGENTS.md`+`CLAUDE.md` | Yes | Documents the finished tree. |

Commit mode: `serial-worker-commit` for every task.

Plan-package commit ownership: this plan file is currently UNTRACKED in the main checkout — the executing session's lead commits it (plus its worktree `.memories/` checkpoints) as the first commit on the task branch, before Task 1 dispatch.

---

### Task 1: Manifest entries carry the file-version's language

**Files:**
- Modify: `crates/julie-extract-cli/src/store/executor.rs:708-714` (primary), `:1842-1868` (`failed_preserved` branch)
- Test: `crates/julie-extract-artifact/tests/store_manifest_contract.rs` (raise coverage exists at `:1491`), new executor→manifest agreement test in the CLI store test suite

**Interfaces:**
- Consumes: the `file_versions.language` COLUMN (content-sniffed at write via `extraction.rs:183-184`), which is the authoritative value. NOTE (corrected in review): the struct returned by `lookup_version_in_transaction` is `StoredFileVersion` (`store/writer.rs:280`, fields at `:52`) and it has NO `language` field today — `model.rs:274` is `StoreReferenceSite.language`, a different type. The fix must either add `language` to `StoredFileVersion` and both lookup projections, or SELECT `file_versions.language` explicitly at the two manifest-build sites. Either is acceptable; single-source is the requirement.
- Produces: manifest entries whose `language` always equals the file-version's stored language, by construction.

**Contract inputs:** Bug anatomy: discovery/planning classify by extension (`discovery.rs:549-552`, `executor.rs:147-155` → `.h` = `"c"`), extraction sniffs content (`language_spec/mod.rs:293-314` → C++ header = `"cpp"`), and `build_manifest` raises `VersionLanguageMismatch` (`manifest.rs:898-904`, class `file_version_language_mismatch`) which fails the ENTIRE cold import at `executor.rs:990`. Real-world repro: aspnetcore cold-index failed after 111 s and wedged the store (2026-08-18, Miller's spike findings).

**File ownership:** as in the contract table.

**Serialization required:** Yes

**Dependency reason:** Runs first so the fix is cleanly separable/releasable; later tasks edit the same executor file.

**What to build:** At `executor.rs:710` the manifest entry takes `file.language()` (extension); replace it with the stored file-version language (via the extended `StoredFileVersion` projection or an explicit `file_versions.language` SELECT — see Interfaces). Same class of bug in the `failed_preserved` branch at `:1859-1868`: it passes `discovered.language()` beside a prior `version_id`; the prior-version lookup at `:1842-1848` selects ONLY `version_id` today — extend it to also read the stored language (join/select `file_versions.language`) and use that. Leave `executor.rs:613-620` alone (its guess is overridden one call later) and leave the sibling `ManifestEntry::failed` (`:1869-1876`) alone (`version_id: None` is never compared). No other independent classifier remains on the import path: `--from-artifact` validates its planned language against the source artifact's already-sniffed value.

**Approach:** TDD: a store-import test over a fixture with a C++-flavored `.h` file (reuse the sniff fixtures from `julie-extractors/src/tests/pipeline.rs:340-414`) that asserts the import PUBLISHES and the manifest entry's language is `cpp`; plus a `failed_preserved` case. Both fail today with `file_version_language_mismatch`.

**Acceptance criteria:**
- [x] Cold import of a mixed C/C++ `.h` corpus publishes; no `.julieignore` workaround needed
- [x] `failed_preserved` entries carry the prior version's stored language
- [x] Worker scope green; worker commits (serial-worker-commit)

### Task 2: Remove the `store resolve` verb and the store resolution session

**Files:**
- Delete: `crates/julie-extract-cli/src/store/resolve.rs` (2120 L), `crates/julie-extract-cli/src/store/resolution_session.rs` (8103 L), `crates/julie-extract-cli/src/store/delta_scope.rs` (875 L), `crates/julie-extract-cli/src/store/prior_overlay.rs` (1209 L)
- Modify: `store/mod.rs` (:12-13 mod decls, :25 dispatch arm, :37/:52 re-exports incl. `StoreResolveArgs`; the `StoreResolutionReport`/`StoreResolutionState` types and their re-exports are KEPT until Task 3 — `import.rs:760`, `from_artifact.rs:1146`, and `export.rs:1045` still use them), `store/args.rs` (:36 `StoreCommand::Resolve` + `StoreResolveArgs`), `store/executor.rs:1668-1673` (`unsupported_request_kind:resolve` arm), `crates/julie-extract-artifact/src/store/coordinator.rs` (see the coordinator design below — `RequestKind::Resolve` :43, `permits_renewable_quantum` :84, `claim_resolve`/heartbeat/currency family :791-1000, reconcile :1794, pending-selection skip :1772, parser :87 with request/receipt decode at :2572/:2690), `xtask/src/test_tiers.rs:281-294/:332-345/:404-417` (deregister the three serialized resolution targets). The `test-store-resolution-contract` Cargo feature is also KEPT until Task 3 (`export.rs:1131` has `cfg` branches on it)
- Test: delete `store_resolution_mechanism.rs`, `store_resolution_performance.rs`, `store_resolution_contract.rs`, `store_resolution_adapters.rs`, `store_delta_scope_contract.rs`, `store_resolution_scope_equivalence.rs`, `store_prior_overlay_contract.rs`, `store_resolution_sequence_equivalence.rs`, `resolution_session_contract.rs`; update `store_coordinator_contract.rs` (:154-157/:212-223/:295/:343/:405/:1243/:1919-1920), `store_cli_contract.rs`, `operations_contract.rs` (:657/:703/:2703), `path_policy.rs` (:10/:225/:242), `perf_gate_convention.rs`, both `test_tiers.rs` convention guards

**Interfaces:**
- Consumes: nothing new.
- Produces: a CLI with no `store resolve`; a coordinator that can never ENQUEUE a resolve but still PARSES persisted ones. Coordinator design (this is load-bearing — a naive variant deletion wedges old coord.db files because the parser at `coordinator.rs:87` would fail on persisted `"resolve"` strings, which flow through request decode `:2572` AND receipt decode `:2690`, and the current pending-selection merely SKIPS resolve rows `:1772`, leaving them permanently pending): introduce a persisted-only `RetiredResolve` representation that decodes from `"resolve"`, cannot be enqueued or claimed, and run an idempotent raw-row reaper before drain and before maintenance that moves queued/claimed retired rows to a typed `failed` state. Terminal rows and archived receipts stay parseable forever. The one-claimed-resolve unique index becomes unused (dropped in Task 5).

**Contract inputs:** Old coord.db files in the field contain `resolve` rows in every state. Miller stops submitting resolves independently (Plan A) but old Miller versions and crash leftovers exist.

**File ownership / Serialization / Dependency reason:** per the contract table.

**What to build:** Delete the four modules and every dispatch/registration/re-export that names them (except the report types and Cargo feature deferred to Task 3). Implement the `RetiredResolve` + reaper design above. Keep every other kind's behavior byte-identical — the coordinator contract tests are the guard.

**Acceptance criteria:**
- [x] `julie-extract store resolve` exits with the standard unknown-subcommand error
- [x] Coordinator state-matrix tests green for seeded historical resolve rows: queued, fresh-claimed, stale-claimed, committed, acknowledged, failed, terminal-log reconciliation, and receipts — all parse; queued/claimed get reaped to typed `failed`; nothing blocks other kinds
- [x] `cargo xtask test contract` no longer registers the three resolution targets; convention guards green
- [x] Worker scope green; worker commits (serial-worker-commit)

### Task 3: Retarget `store export`, `store import --from-artifact`, and import internals

**Files:**
- Modify: `crates/julie-extract-cli/src/store/export.rs` (:8 imports; DELETE the `resolution_state='exact'` gates :146-149/:283; DELETE `copy_resolution` :650-905 incl. `insert_identifier_resolution`/`upsert_pending_resolution`; resolution metadata writes :350-359/:405-434 — exported artifacts get no `reference_resolution_version` key; the pin-based snapshot at :130 and the `test-store-resolution-contract` `cfg` branches at :1131; base/delta fields leave `ExportIdentity`, reports, metadata, and lock cleanup), `store/from_artifact.rs` (:192 exact-view REUSE predicate, :244 `validate_resolution` preflight call, :278, :642 `materialize_resolution_base`, :1146 report projection, :1213-1231 the validator itself — resolution metadata/overlay requirements go entirely), `store/executor.rs` (:447 epoch guard, :1078-1079, :1254-1372 base materialization + `resolution_bound` view binding), `store/import.rs` (:545/:588/:631 and `populate_current_resolution` :760-810, failure classes :904-910), `store/report.rs` + `store/mod.rs` re-exports (`StoreResolutionReport`/`StoreResolutionState` deleted HERE, atomically with their last call sites), CLI `Cargo.toml` `test-store-resolution-contract` feature (:45-51, removed here with its last `cfg` site)
- Test: update `store_cli_contract.rs` export/round-trip cases; a NEW export test on a store with no resolution state asserting success and empty (or absent-content) resolution tables in the produced artifact; `store_manifest_contract.rs:1491` neighbors if they assert resolution metadata

**Interfaces:**
- Consumes: Task 2's tree (store session gone).
- Produces: `store export` succeeds on any bound view regardless of resolution state — this is what un-breaks Miller's `MILLER_INDEX_STORE=off` mode at its Phase 3 pin bump. `--from-artifact` imports bind views without a resolution base. Exported artifacts keep their FACT tables identical; the artifact resolution tables (still in the v6 DDL until Task 5) are simply empty.

**Contract inputs:** Miller's off-mode reader is being swapped (Plan A) to compute resolution from the artifact's fact tables — the export contract that matters is fact-table fidelity, not resolution rows.

**File ownership / Serialization / Dependency reason:** per the contract table.

**What to build:** Export: remove the exact-state refusals, resolution copying, and resolution metadata — AND replace the retiring pin-based snapshot safety with a SINGLE READ TRANSACTION held on one connection from manifest selection through every fact-table copy (the pin machinery was what kept a multi-query export from mixing generations or racing GC; without it, snapshot isolation must come from the transaction). From-artifact import: remove the exact-view reuse predicate, `validate_resolution` and its overlay requirements, base materialization, delta seeding, and the `resolution_bound` binding step (views bind the same way a cold import binds them). Import: remove `populate_current_resolution` and its failure classes.

**Acceptance criteria:**
- [x] Export → re-import round trip green on a store that never resolved
- [x] Export under a CONCURRENT update/GC run produces a consistent single-generation artifact (test)
- [x] `--from-artifact` succeeds on a fact-complete artifact carrying NO resolution metadata; binds without creating anything under `bases/`
- [x] `StoreResolutionReport`/`StoreResolutionState` and the `test-store-resolution-contract` feature no longer exist
- [x] Worker scope green; worker commits (serial-worker-commit)

### Task 4: Remove artifact-level resolution and the auto-resolve/upgrade hooks

**Files:**
- Delete: `crates/julie-extract-cli/src/resolution.rs` (6333 L, incl. `RESOLUTION_VERSION` :2299 and the `session` decl :33-35), `crates/julie-extract-cli/src/resolution_session.rs` (1024 L), `crates/julie-extract-artifact/src/resolution_store.rs` (1171 L), `xtask/src/resolution_performance.rs`
- Modify: CLI `lib.rs:11-21` (doc + `pub mod resolution` + re-export) AND `main.rs:1` (the binary declares its own `mod resolution`), `capability_snapshot.rs:89` (calls `crate::resolution::*` — remove the reference-resolution capability-gap emission), `commands.rs` (:87; scan/update/delete resolve hooks :486-535/:899-912/:2450-2490; cold-open `resolution_upgrade_required` full re-extract gate :255/:331/:490-494; version stamps :621/:633), `artifact_access.rs` (:13/:19/:315-330 read-side gate), `julie-extract-artifact/src/lib.rs:13`, `julie-extract-artifact/src/model.rs` (`ResolutionCounts` :194/:246), `julie-extract-artifact/src/writer.rs` (:10 imports, `*_with_resolution` hooks and the resolution timing phase, e.g. :1572), `reports.rs` (:1025 and the resolution row-domain counts/report codes), `jsonl.rs` (:13/:220 and the resolution-shaped keys — **bump `JSONL_SCHEMA_VERSION` 4 → 5**, do not silently redefine v4; update `docs/contracts/jsonl-v4.md` → new v5 doc + `release_package_items`), `xtask/src/lib.rs:6`, `xtask/src/commands.rs:37` (deregister the resolution-performance subcommand), `xtask/src/dogfood.rs` (:311/:326/:561-567 `ResolutionFailed` handling), CLI `Cargo.toml` `test-perf` feature (:19-23)
- Test: delete `resolution_contract.rs`, `resolution_scope_equivalence.rs`, `resolution_shadow.rs`, `resolution_report_scope.rs`, `resolution_perf.rs`; delete `resolution_store_contract.rs` (artifact crate); delete `xtask/tests/resolution_performance_contract.rs`; update `operations_contract.rs` remnants, `test_tiers.rs` guards, `perf_gate_convention.rs`

**Interfaces:**
- Consumes: Tasks 2–3 (no store-side references remain).
- Produces: `scan`/`update`/`delete` on a standalone artifact write facts only; no resolve pass, no resolution metadata, no forced full re-extract on resolution-version mismatch (an artifact carrying the old `reference_resolution_version` key opens normally; the key is ignored). Env flags `JULIE_RESOLUTION_SHADOW`, `JULIE_RESOLUTION_SHADOW_INJECT`, `JULIE_RESOLUTION_PROFILE`, `JULIE_STORE_RESOLUTION_DELTA`, and the `JULIE_EXTRACT_STORE_RESOLUTION_*` hooks die with their code.

**Contract inputs:** dogfood must pass without a resolution phase (`cargo xtask dogfood repo --root . --out-dir target/dogfood/julie-extractors`, report-only here, hard at release time per repo practice).

**File ownership / Serialization / Dependency reason:** per the contract table.

**What to build:** Delete the resolver and its session; unhook the three write commands and both open-time gates; keep the write paths' non-resolution metadata finalization intact.

**Acceptance criteria:**
- [x] `scan`/`update`/`delete` produce artifacts with empty resolution tables and no resolution metadata keys; opening a PRIOR artifact (with resolution rows + old metadata key) works and ignores both
- [x] No `JULIE_RESOLUTION*`/`JULIE_STORE_RESOLUTION*` reference remains in production code
- [x] Worker scope green; worker commits (serial-worker-commit)

### Task 5: Drop the schema objects, delete the artifact-crate resolution modules, reap on open

**Files:**
- Delete: `crates/julie-extract-artifact/src/store/resolution.rs` (4771 L), `store/resolution_diff.rs` (1752 L), `store/scope.rs` (910 L)
- Modify: `store/mod.rs` (:10-11 decls, :61-90 re-exports), `store/schema.rs` (remove `resolution_bases` :776, `resolution_base_versions` :830, `resolution_deltas` :838, `resolution_identifier_deltas` :880, `resolution_pending_deltas` :910, `resolution_pins` :942 from `STORE_SCHEMA_SQL`; remove `ensure_resolution_scope_feature` call :81; the `views` FK/trigger rebuild — see What to build; coordinator DDL: drop `uidx_coord_one_claimed_resolve`), `store/connection.rs` (:194 writer-open path INVOKES the retirement migration — existing opens only validate + install indexes/scope today, so without this hook old stores never migrate; read-only opens must not mutate) and the coordinator open path (`coordinator.rs:2968` region, same rule), `store/manifest.rs` (:10 scope imports, :732 scope-state writes during publication), `store/generation.rs` (:1025/:1255/:1515 — promotion/rollback stops copying resolution deltas and base files), `store/model.rs` (resolution record types :200/:215 and neighbors), `store/coordinator.rs` (reconcile allocator maxima still query `resolution_deltas` at :1913/:2910 — remove the resolution allocator kinds), `julie-extract-artifact/src/schema.rs` (remove `pending_resolutions` :303 and `identifier_resolutions` :320 from the artifact DDL; **bump `SQLITE_SCHEMA_VERSION` 6 → 7** per the header decision; new `docs/contracts/sqlite-schema-v7.md` + `.catalog.sha256`, added to `release_package_items`, `jsonl`/`extracted-data` docs cross-checked), `store/maintenance.rs` (:21-22/:760/:1547-1690/:2400-2460 — GC/repair of bases/deltas/pins/scope becomes: reap the `bases/` directory plus BOTH scratch families — `resolve-*.db` AND `scratch/resolution-<base>-<request>.partial.db` (`from_artifact.rs:679`) — with their `-wal`/`-shm` sidecars, closing handles first; Windows cleanup coverage), artifact `Cargo.toml` `test-store-resolution` feature (:18-20)
- Docs+contract tests in the same change (the catalog fences make them one atomic edit): `docs/contracts/sqlite-store-schema-v2.md` + both catalog SHA-256 fences, `docs/contracts/sqlite-schema-v6.md` + `docs/contracts/sqlite-schema-v6.catalog.sha256`, `crates/julie-extract-artifact/tests/store_schema_contract.rs`, `tests/schema_contract.rs`
- Test: delete `store_resolution_binding_contract.rs`, `store_resolution_schema_contract.rs`, `store_resolution_base_contract.rs`; new open-old-store test seeding a v2 store WITH resolution tables/bases and asserting drop+reap+served facts intact

**Interfaces:**
- Consumes: Tasks 2–4 (no code references the dropped objects).
- Produces: new stores/artifacts contain no resolution objects; the first WRITER open of an existing store migrates it (views rebuilt, tables dropped, `bases/` reaped — gigabytes reclaimed); store/coord stay schema v2, standalone artifacts bump to v7, JSONL is v5 (header decision); contract docs carry a dated retirement note and regenerated catalogs.

**Contract inputs:** Global constraint list — non-resolution DDL byte-identical; `views` resolution columns stay. `sqlite-resolution-base-schema-v1.md` and `sqlite-resolution-delta-schema-v1.md` are NOT in `release_package_items` (safe to retire without packaging edits); `sqlite-store-schema-v2.md`, `sqlite-schema-v6.md`, `cli.md`, `store-v1.md`, `versioned-index-store.md` ARE in the list (`xtask/src/release.rs:130-256`) and must be edited in place, not deleted.

**File ownership / Serialization / Dependency reason:** per the contract table.

**What to build:** The retirement migration, correctly ordered. The `views` table holds FOREIGN KEYS into `resolution_bases`/`resolution_deltas` (`store/schema.rs:625/:690`) with `foreign_keys=ON` (`:99`), and triggers reference resolution tables (`:1240`) — a naive `DROP TABLE` fails for bound views. The migration is one transaction: rebuild `views` preserving every column and value while removing the two resolution FKs and retired triggers, then drop scope/pin/delta/base objects child-first, then run `PRAGMA foreign_key_check`. It runs on WRITER opens only (store connection + coordinator), is idempotent, and read-only opens never mutate. Then the maintenance retarget (reap instead of repair) and the regenerated schema catalogs with their conformance tests.

**Acceptance criteria:**
- [x] Fresh store: `sqlite_master` has zero `resolution_*` / `*_scope_*` objects; coordinator has no resolve index; `foreign_key_check` clean
- [x] Legacy-store matrix green: read-only open mutates NOTHING; first writer open migrates (views column values preserved — asserted value-by-value; fact tables byte-identical; `bases/` + both scratch families reaped); second open is an idempotent no-op; migrated catalog equals the fresh-store catalog
- [x] Generation promotion/rollback, manifest publication, and coordinator reconcile green without resolution objects (targeted tests)
- [x] Schema contract tests green against the regenerated store catalog and the new artifact v7 catalog
- [x] Worker scope green; worker commits (serial-worker-commit)

### Task 6: Documentation retirement, test tiers, release prep

**Files:**
- Modify: `docs/contracts/cli.md` (:54/:67/:180-198/:324/:333/:401/:478/:494/:566 — remove `store resolve`, the resolution-upgrade semantics, and resolution report fields), `docs/contracts/store-v1.md`, `docs/contracts/reports.md`, `docs/contracts/extracted-data-v4.md`, `docs/contracts/jsonl-v4.md`, `docs/architecture/versioned-index-store.md` (:41-48/:58/:78/:93/:109/:145-153), `docs/architecture/schema-principles.md`, `README.md`, `docs/testing-strategy.md` (contract tier no longer lists resolution gates), `AGENTS.md` + `CLAUDE.md` (sync-checked)
- Retire wholly (retirement header, kept in tree): `docs/contracts/sqlite-resolution-base-schema-v1.md`, `docs/contracts/sqlite-resolution-delta-schema-v1.md`, `docs/contracts/reference-resolution-coverage-v1.md` (+ note on `scripts/reference-resolution-coverage-report.mjs`)
- Annotate as superseded (one-line banner, do not rewrite): the resolution plans/findings/decisions listed in the 2026-08-18 inventory — notably `docs/decisions/2026-08-02-fleet-safety-flags.md` (its `JULIE_RESOLUTION_SHADOW`/`JULIE_STORE_RESOLUTION_DELTA` flags are gone)
- Create: `docs/decisions/2026-08-18-resolution-write-path-retirement.md` (ADR: why, the Miller query-time replacement, the schema decisions, pointers to Miller's policy contract and spike findings), draft `docs/release-notes/v<next>.md` (language fix + removal + store-size reclaim; version number chosen at release time)
- Compat policy (the deliberate break must be classified, not discovered): CI's compat job compares extraction output against the PREVIOUS RELEASED binary (`.github/workflows/ci.yml:52`); `xtask/src/compat.rs:649` compares all non-excluded tables and `xtask/tests/compat_contract.rs:455` explicitly requires both resolution tables. Update the compat policy/exclusions and `docs/contracts/extraction-output-changes.md` to record the intentional removal, fix the compat tests, and run `cargo xtask compat-check` locally against the v2.33.7 release binary as part of this task's verification.

**Interfaces:**
- Consumes: the finished tree (Tasks 1–5).
- Produces: docs that describe the tool as it now is; a release-notes draft ready for the approval-gated release.

**Contract inputs:** `docs/release.md` rules: the `Current published release:` pointer moves at PUBLISH time only; release-evidence doc is written at publish. Neither happens in this plan.

**File ownership / Serialization / Dependency reason:** per the contract table.

**What to build:** The sweep above, then the branch gate.

**Acceptance criteria:**
- [x] `grep -ri "store resolve\|resolution_bases\|RESOLUTION_VERSION"` over `docs/` finds only retirement notes, superseded banners, and release-note history
- [x] Compat policy updated (`extraction-output-changes.md`, compat tests/exclusions); `cargo xtask compat-check` against the v2.33.7 release binary passes with the removal classified as intentional
- [x] `scripts/check-agent-doc-sync.sh` and `scripts/check-release-state.sh` pass
- [x] Branch gate green (fmt, xtask tests, default tier, contract tier, clippy, `git diff --check`)
- [x] Worker scope green; worker commits (serial-worker-commit)

---

## Cross-repo coordination (the full phase picture)

- **Plan A (Miller, parallel session):** query-time resolution + stop submitting resolves + dead-code feature removal. Runs against pinned julie-extract 2.33.7 the whole time; nothing here can break it because the pin is a released binary.
- **This plan (Plan B):** independent of Plan A's code. The one shared contract is the store schema — the Global Constraints' "non-resolution DDL byte-identical" rule is the guarantee Plan A relies on.
- **Phase 3 (Miller repo, after both land):** release this repo (approval-gated), bump Miller's `scripts/julie-pins.json`, re-run Miller's restore + Scale suite, verify Miller's off-mode export smoke (export works again — this plan's Task 3 removes the exact-state refusal that 2.33.7 still has), let Miller's parity Scale test auto-skip (it probes for `resolve` support), then release Miller (approval-gated).
