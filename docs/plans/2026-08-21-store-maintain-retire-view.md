# `store maintain retire-view` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Add a `store maintain retire-view` verb that permanently removes one dead view's rows from a family store, so a removed Miller workspace stops pinning data forever.

**Architecture:** A new maintenance verb on the existing plan/apply pattern. Plan mode is read-only and reports what would be deleted. Apply mode acquires the standard maintenance fence and deletes `manifest_entries` → `manifests` → `views` for one `view_id` in one IMMEDIATE store.db transaction. Freed `file_versions` become ordinary GC candidates for later `gc --apply` runs.

**Tech Stack:** Rust (edition 2024, floor 1.95), rusqlite, clap, serde. Crates: `julie-extract-artifact` (core), `julie-extract-cli` (verb + report).

**Architecture Quality:** The verb reuses `MaintenanceExecutor`'s acquire gates and `MaintenanceAction::Gc` for the coordinator intent, so no coord.db DDL changes and the schema catalog sha256 stays valid. The report exposes a new CLI-side `StoreMaintenanceAction::RetireView`. Risk: an incomplete delete set orphans manifests and bricks every maintenance verb with `integrity_failed` — the atomic three-table delete and the post-retire inspect test are the mitigation.

## Global Constraints

- Background evidence: the planning survey recorded in Miller's `docs/findings/2026-08-21-context-latency-diagnosis.md` session; key facts repeated inline below with file:line.
- The three deletes (`manifest_entries`, `manifests`, `views`) MUST commit in ONE transaction. FKs are `ON DELETE NO ACTION DEFERRABLE INITIALLY DEFERRED` (schema.rs:946, :974, :1008) — deferred checks make the single-transaction child-first delete legal.
- MUST NOT delete: `family_allocator_marks` (generation-identity reuse guard, schema.rs:1283-1287; store-v1.md:82), `store_log` rows (receipt-then-safe-cursor pruning only), `request_receipts` (delete-trapped, schema.rs:1296-1300), `consumer_cursors` (not view-scoped).
- MUST NOT add a `maintenance_intent.action` value: the CHECK is `IN ('gc','repair','promote','rollback')` (schema.rs:1258). Reuse `MaintenanceAction::Gc` for the intent. `store_schema_contract.rs:17-21` (catalog sha256 vs doc) must stay green with zero DDL edits.
- Refusals: view id absent → `view_not_found` (`InvalidArguments` class); any queued-or-claimed request whose `payload_json` `$.view_id` equals the target → `busy`; the standard acquire gates (family-wide claimed request, live writer lease, live foreign intent → `busy`; stale binding → `stale_plan`) apply verbatim.
- Do NOT require or infer anything from the view's root path. A dead workspace's root is gone; `canonicalize()` on it fails, and root existence proves nothing (2026-08-21 decision: never infer death from a missing root). The caller names the view id; that is the whole authority.
- Retiring the current writer's own... there is no "own view" concept in maintenance; any view id is retirable when the gates pass, including the last view in the store.
- Windows: this verb deletes rows, not files. Do not add file deletes. Close no-longer-needed connections before returning (no unlink is performed).
- Clippy gate: `cargo clippy --workspace --all-targets --all-features --no-deps -- -D warnings` must stay clean.
- CLAUDE.md/AGENTS.md are not edited by this plan; `scripts/check-agent-doc-sync.sh` stays green by construction.

## Verification Strategy

**Project source of truth:** `.github/workflows/ci.yml` fast-gates job; `xtask/src/test_tiers.rs`; repo CLAUDE.md.

**Worker red/green scope:** the focused test file for the change:
`cargo test -p julie-extract-artifact --test store_maintenance_contract` (core) and
`cargo test -p julie-extract-cli --test store_maintenance_cli_contract` (CLI).

**Worker ceiling:** the six focused commands below. Workers do not run `cargo xtask test default`/`contract`.

```bash
cargo test -p julie-extract-artifact --test store_maintenance_contract
cargo test -p julie-extract-cli --test store_maintenance_cli_contract
cargo test -p julie-extract-cli --test store_cli_contract
cargo test -p julie-extract-cli --test test_tiers
cargo test -p julie-extract-artifact --test store_schema_contract
cargo clippy --workspace --all-targets --all-features --no-deps -- -D warnings
```

**Worker gate invariant:** Task 1 proves the atomic delete + post-retire plan integrity; Task 2 proves the CLI contract (one-line stdout JSON, plan read-only, apply mutates, refusal classes); Task 3 proves the help surface and doc lists include the verb.

**Lead affected-change scope:** `cargo xtask test changed <touched paths>` after the batch.

**Branch gate:** `cargo fmt --check`, clippy (above), `cargo xtask test default`, `cargo xtask test contract`.

**Security scope:** `cargo-deny check --all-features` at the branch gate (dependency audit); no secrets scan declared beyond it — matches CI.

**Replay/metric evidence:** none — all assertions are hard test gates.

**Escalation triggers:** any DDL edit (forbidden by this plan — stop and report); any change to exit codes or failure-class strings beyond adding the verb (RAZORBACK.md strategy-tier rule).

**Assigned verification failure:** Workers stop and report when assigned verification fails.

**Verification ledger:** record command, scope, SHA, result, timestamp per task in the run report.

## Parallel Execution Contract

| Task | Parallel batch | File ownership | Serialization required | Dependency reason |
|---|---|---|---|---|
| Task 1: Core retire operation | None - serial | Modify: `crates/julie-extract-artifact/src/store/maintenance.rs`; Test: `crates/julie-extract-artifact/tests/store_maintenance_contract.rs` | Yes | Task 2 consumes the core entry point's signature. |
| Task 2: CLI verb + report | None - serial | Modify: `crates/julie-extract-cli/src/store/args.rs`, `crates/julie-extract-cli/src/store/maintenance.rs`, `crates/julie-extract-cli/src/store/maintenance_report.rs`; Test: `crates/julie-extract-cli/tests/store_maintenance_cli_contract.rs`, `crates/julie-extract-cli/tests/store_cli_contract.rs` | Yes | Consumes Task 1's core entry point. |
| Task 3: Contract docs | None - serial | Modify: `docs/contracts/cli.md`, `docs/contracts/store-v1.md` | Yes | Documents the verb Task 2 ships; help-surface test lands in Task 2. |

Commit mode: `serial-worker-commit` — one lane, one worker, commit per task on the feature branch.

## Task 1: Core retire operation in `julie-extract-artifact`

**Files:**
- Modify: `crates/julie-extract-artifact/src/store/maintenance.rs`
- Test: `crates/julie-extract-artifact/tests/store_maintenance_contract.rs`

**Interfaces:**
- Consumes: `MaintenanceExecutor::acquire_for_action` (maintenance.rs:1111-1286) with `MaintenanceAction::Gc`; `MaintenanceInspector::inspect()`; `MaintenanceActionHeartbeat` (:1466-1472); `validate_ownership`/`validate_plan_binding` (:1473-1481) and the pre-commit re-validation (:1623-1624).
- Produces: a public core entry point (suggested `MaintenanceExecutor::retire_view(&self, plan: &MaintenancePlan, view_id: &str) -> Result<RetireViewApplied, MaintenanceError>`) plus a read-only plan helper (suggested `plan_view_retirement(snapshot, view_id) -> Result<RetireViewPlan, MaintenanceError>`) returning counts: manifests, manifest_entries, and whether the view exists. Exact names are the worker's choice; record them for Task 2.

**Contract inputs:** Global Constraints above; the delete order `manifest_entries` → `manifests` → `views`; `TransactionBehavior::Immediate` on store.db.

**File ownership:** Modify: `crates/julie-extract-artifact/src/store/maintenance.rs`; Test: `crates/julie-extract-artifact/tests/store_maintenance_contract.rs`

**Serialization required:** Yes

**Dependency reason:** Task 2 consumes the core entry point's signature.

**What to build:** The read-only plan (counts what one view's retirement removes; errors `view_not_found` when absent) and the fenced apply (acquire gates → heartbeat → atomic three-table delete → commit → applied counts). Also the targeted per-view request refusal: any `requests` row with `state IN ('queued','claimed')` and `json_extract(payload_json,'$.view_id') = ?1` → `MaintenanceBusy`.

**Approach:** Follow the `apply_gc` execution shape. TDD in `store_maintenance_contract.rs` using the real-store style (`StoreLayout::create` in a temp dir with the `NEXT_TEMP_ID` counter). The decisive test: create two views, retire one, then run `MaintenanceInspector::inspect()` and `plan_maintenance` — both must succeed and the surviving view's manifests must still be roots. A retire that deletes only the `views` row must be impossible by construction (single transaction).

**Acceptance criteria:**
- [ ] Plan mode returns counts with zero writes (assert store.db bytes/rows unchanged).
- [ ] Apply deletes exactly the three tables' rows for the target view in one transaction.
- [ ] Post-retire `inspect` and GC planning succeed (no `UnknownRoot`).
- [ ] `family_allocator_marks`, `store_log`, `request_receipts`, `consumer_cursors` rows survive, asserted directly.
- [ ] Absent view → `view_not_found`-shaped error; per-view queued/claimed request → busy-shaped error; a live writer lease → busy.
- [ ] Worker-scope verification passes and the change is committed per commit mode.

## Task 2: CLI verb, args, and report

**Files:**
- Modify: `crates/julie-extract-cli/src/store/args.rs` (new `RetireView(StoreMaintenanceRetireViewArgs)` variant on `StoreMaintenanceCommand` :47-53; new args struct: `--store`, optional `--family`, required `--view` via `parse_store_identifier`, `--apply`, `--json`)
- Modify: `crates/julie-extract-cli/src/store/maintenance.rs` (dispatch in `run` :24-36; an `apply_retire_view` beside `apply_gc` :175-203; plan path through `inspect_context`)
- Modify: `crates/julie-extract-cli/src/store/maintenance_report.rs` (`StoreMaintenanceAction::RetireView` :13-22 → serde `retire_view`; counts fields `retired_views`, `retired_manifests`, `retired_manifest_entries` on `StoreMaintenanceCounts` :67-100; builder for the retire outcome)
- Test: `crates/julie-extract-cli/tests/store_maintenance_cli_contract.rs` (extend `maintenance_namespace_exposes_the_approved_nested_commands` :22-36 with `"retire-view"`; new contract tests), `crates/julie-extract-cli/tests/store_cli_contract.rs` (help surface :541-561 if it enumerates verbs)

**Interfaces:**
- Consumes: Task 1's core entry points (names recorded in Task 1's report).
- Produces: the CLI contract — `julie-extract store maintain retire-view --store <dir> --view <id> [--family <uuid>] [--apply] [--json]`; exit 0 on success, existing failure-class exit mapping; one-line JSON stdout; human failure on stderr.

**Contract inputs:** report schema stays `report_schema_version: 1`; disposition `planned` without `--apply`, `applied` with; failure classes reuse `busy` / `stale_plan` / `integrity_failed` / `invalid_arguments` — no new class strings.

**File ownership:** Modify: `crates/julie-extract-cli/src/store/args.rs`, `crates/julie-extract-cli/src/store/maintenance.rs`, `crates/julie-extract-cli/src/store/maintenance_report.rs`; Test: `crates/julie-extract-cli/tests/store_maintenance_cli_contract.rs`, `crates/julie-extract-cli/tests/store_cli_contract.rs`

**Serialization required:** Yes

**Dependency reason:** Consumes Task 1's core entry point.

**What to build:** The verb end to end on the plan/apply pattern, spawning the real binary in tests (`CARGO_BIN_EXE_julie-extract` harness, exactly-one-newline stdout assertion, field-by-field JSON asserts).

**Acceptance criteria:**
- [ ] `retire-view` without `--apply` is read-only and reports the planned counts.
- [ ] `retire-view --apply` retires the view; a following `store maintain inspect` on the same store succeeds.
- [ ] Refusal tests: unknown view (`invalid_arguments`/`view_not_found` code), busy store (live lease), per-view claimed request.
- [ ] `maintenance_namespace_exposes_the_approved_nested_commands` includes `retire-view`.
- [ ] All new tests are default-tier (no feature gate) so `test_tiers.rs:43-58` stays untouched.
- [ ] Worker-scope verification passes and the change is committed per commit mode.

## Task 3: Contract documentation

**Files:**
- Modify: `docs/contracts/cli.md` (`### \`store maintain\`` section, lines 481-504)
- Modify: `docs/contracts/store-v1.md` (`## Views and manifests` line 61; `## Retention boundary` line 131; `## Lifecycle maintenance interface` line 139)

**Interfaces:**
- Consumes: Task 2's shipped CLI shape and report fields.
- Produces: the documented contract Miller/Eros consume.

**Contract inputs:** exact insertion points from the survey: cli.md ¶1 (:483-486) gains `retire-view` in the read-only-unless-`--apply` list; ¶2 (:488-492) gains one sentence naming what it deletes and what it refuses on. store-v1.md: insert the end-of-view-life paragraph after line 75 (opposite the "GC roots" sentence it narrows), before line 77; add `retire-view` to the verb lists at :141-143 and :155-156; amend the Ph2d reclaim sentence at :133-136 to admit retirement as the one path that removes manifest roots.

**File ownership:** Modify: `docs/contracts/cli.md`, `docs/contracts/store-v1.md`

**Serialization required:** Yes

**Dependency reason:** Documents the verb Task 2 ships; help-surface test lands in Task 2.

**What to build:** The one-paragraph contract amendment (this is the cross-repo deliverable Miller's brief names) plus the cli.md verb documentation, matching the surrounding ~95-char wrap and paragraph style. The store-v1.md paragraph must state: retirement is explicit and caller-initiated (never inferred from a missing root); it removes the view's manifests, manifest entries, and view row atomically; allocator marks, log rows, receipts, and cursors survive; freed versions become ordinary GC candidates on later runs.

**Acceptance criteria:**
- [ ] cli.md documents the verb in the existing section's style.
- [ ] store-v1.md defines end of view life in `## Views and manifests` and updates the two maintenance lists and the retention-boundary sentence.
- [ ] No schema doc (`sqlite-store-schema-v2.md`) edits — the catalog sha256 asserts stay untouched.
- [ ] Worker-scope verification passes (doc-only: the five focused test commands still green) and the change is committed per commit mode.

## Follow-ons (out of scope, recorded)

- julie-extract release 2.35.0 carrying the verb; Miller pin bump.
- Miller integration: `workspace remove`/`prune` invoking `retire-view` for the departing view (needs its own approval — new Miller behavior).
- The `_quarantine-202916` orphan sidecar copy and the 4 dead `store_families` rows in Miller's workspaces.db are Miller-side cleanups, not this plan.
