# Store Concurrent Fencing Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Close the multi-worktree writer races found in the post-Ph2d review so concurrent import/resolve/maintain against one family store cannot mutate a frozen source generation or leave unfenced durable effects.

**Architecture:** Keep the existing split (`coord.db` owns leases/intents; generation-local `store.db` owns extraction and resolution). Make maintenance intent a first-class lease authority, raise the frozen source writer floor during promote/repair with a defined two-database state machine, force every durable `store.db` mutation through generation+lease fences, and close pin/lease heartbeats that currently leak under concurrent reclaim.

**Tech Stack:** Rust 2024, rusqlite/SQLite WAL, existing `GenerationFence` / `StoreCoordinator` / maintenance intent catalogs, Cargo integration tests with `test-store-crash` and `test-store-contract` features.

**Architecture Quality:** No new public product surface. Tighten the existing store kernel contracts so Ph2d design rules (intent blocks ordinary writers; temporary `min_writer_version` raise; fenced store mutations; pin roots) match production code.

**Main architecture risks (both must stay visible during implementation):**
1. Dual authority: generation build releases the writer lease while intent is live; ordinary lease acquire must honor intent.
2. Metadata integrity: temporary raised floor and intent mirror keys must not permanently land on a newly published destination generation (`copy_store_metadata` currently copies every `store_meta` key except `generation_state`).

**Approved design references:**
- [docs/architecture/versioned-index-store.md](../architecture/versioned-index-store.md)
- [docs/plans/2026-08-08-index-store-ph2d-lifecycle-design.md](2026-08-08-index-store-ph2d-lifecycle-design.md) (maintenance ownership, floor raise, intent mirror)
- Review findings from session review of `84cadd4..2206c18` (Critical 1–3, Important 4–9)
- Plan consensus revisions from Claude + Codex plan review (2026-08-10)

## Global Constraints

- Use the workspace Rust toolchain (prefer `RUSTUP_TOOLCHAIN=1.97.1` when installed; otherwise the repo `rust-toolchain`).
- Do not add MCP, daemon, search, embedding, or Miller runtime types to this crate.
- Do not claim cross-database atomicity between `coord.db` and `store.db`. Order multi-db steps and define crash recovery for each intermediate state.
- No `ATTACH`, whole-corpus `Vec` materialization, long single-transaction generation builds, or in-place full `VACUUM`.
- Ordinary import/update/delete/resolve/enqueue/claim must remain fail-closed under live foreign maintenance intent.
- Maintenance owner may write only via an explicit maintenance-owner fence (`run_id` + `owner_id` + `owner_pid` + `fencing_token`). Matching holder id/PID alone must never bypass a live foreign intent.
- Expired intents and dead-owner takeover remain allowed only after existing PID/expiry policy.
- Default store-writer lease remains short (5s). Long work must heartbeat or re-acquire. Never commit a durable store mutation past a lost fence.
- Exact publish must revalidate the writer lease against **wall clock** at commit time (not only `fence.now_ms` captured at open).
- Pin protection for base rebuild/GC must honor pin `expires_at` the same way maintenance GC does.
- Keep request `StoreReport` and maintenance report schema v1 byte-compatible unless a task explicitly amends a contract.
- Prefer typed errors (`MaintenanceInProgress`, `WriterLeaseLost`, `MaintenanceBusy`, `CasLost`) over stringly failures.
- TDD: write the failing contract first for each task; RED then GREEN; no silent test weakening.
- Do not push, tag, release, or change Miller pins without explicit approval.
- This plan is **fully serial** (`serial-worker-commit`). “Batch” labels are ordering labels only; never dispatch two tasks in parallel.

## Two-database maintenance floor state machine (normative)

Applies to Tasks 1–2 and finish/abort paths. Workers must implement this order, not invent a same-transaction cross-db write.

| Step | DB | Durable effect | On crash |
|---|---|---|---|
| M1 | `coord.db` | Insert/refresh `maintenance_intent` + hold `writer_lease` under maintenance fencing token | Successor may takeover only if owner dead/expired; otherwise `MaintenanceBusy` |
| M2 | source `store.db` | Through maintenance-fenced writer: set `min_writer_version` to `max(prior, maintenance_binary)`; write intent-mirror meta keys; record prior floor already stored on intent as `source_min_writer_version` | Intent live + floor not raised: Task 1 still blocks foreign writers by intent. Finish/abort or successor redoes M2 before release |
| M3 | `coord.db` | Optional heartbeat; then **release writer lease only** for long generation build (intent remains live) | Foreign writers must still refuse (Task 1). Maintenance owner continues build |
| M4 | build | Windowed copy/build destination; heartbeats intent | Stale owner recoverable; no foreign writers on source |
| M5 | destination `store.db` | During metadata materialization, **force** `min_writer_version = source_min_writer_version` (pre-maintenance) and **omit or clear** temporary intent-mirror keys. Never copy raised floor into destination as permanent state | Incomplete dest cleaned by existing partial-gen recovery |
| M6 | FS + source/dest | Publish `CURRENT` (existing protocol) | Existing generation crash recovery |
| M7 | source `store.db` then `coord.db` | **Order is mandatory:** (1) if source is still `serving`, open maintenance-fenced writer and restore `min_writer_version` + clear intent-mirror keys; (2) then delete `writer_lease` and `maintenance_intent` in `coord.db`. If source is already `retired`, skip (1); destination already holds pre-maintenance floor from M5 | Crash after (1) before (2): intent still live, floor restored — foreign writers still blocked by intent only (safe). Crash before (1) on serving source: intent live + raised floor — foreign writers blocked; successor completes restore then clears intent |

**Abort path:** Same ordering as M7. Under the maintenance fence, restore source floor and clear mirrors while intent is still live; only then clear lease/intent; drop incomplete destination. Never clear intent first while a serving source still has a temporary raised floor.

### Recovery and transaction seams (normative)

These seams close remaining implementer ambiguity. They are binding for Tasks 3–8.

1. **Abort/finish restore-before-clear:** Always restore serving-source floor/mirrors under the maintenance fence **before** deleting `maintenance_intent`. Clearing intent first is a plan defect.

2. **Exact-publish heartbeat seam (mandatory):** There is no production publish API without a hook. `publish_exact` / `publish_exact_with_markers` both require a pre-transaction heartbeat callback parameter (tests may pass a recording stub that still runs; a silent no-op is allowed only in unit tests that do not assert lease liveness). The hook is invoked **exactly once immediately before** `BEGIN IMMEDIATE` on every publish attempt. There is **no** in-transaction or mid-window heartbeat: SQLite IMMEDIATE work either completes under the renewed lease or the transaction rolls back. CLI `with_writer_lease` always supplies a hook that calls `heartbeat_lease_for` + wall-clock ownership check. Artifact code must not assume a CLI-only timer thread.

3. **Cursor release:** `release_consumer_cursor` (and advance) must recheck foreign live intent **inside the same IMMEDIATE transaction** that deletes/updates the cursor row. Cursor deletion removes a GC/pruning root and is a coordinator mutation, not a best-effort side channel.

4. **Import base CAS transaction boundaries (Task 8):**
   - T8a: IMMEDIATE store txn inserts/updates catalog row `state=building` for `base_id` (or reclaims abandoned building with identity checks).
   - T8b: Materialize base file under scratch/partial path (no ready claim yet).
   - T8c: Durable publish into generation bases dir (hardlink/rename) + fsync as existing base publish does.
   - T8d: IMMEDIATE store txn CAS `building → ready` only if file identity and semantic counts match; on conflict/mismatch leave non-ready and surface typed error.
   - Retry/recovery: a later import/resolve with the same identity may reclaim `building` owned by a dead request, or ignore an existing `ready` with matching identity (`ON CONFLICT` ready-hit is success only after identity compare).

5. **Capacity re-probe coverage (Task 6):** Re-probe live free bytes immediately before **each** of: first GC delete/demotion cohort, scratch purge batch, **and** generation staging/create for promote/repair/rollback (not only `MaintenanceExecutor::apply` GC path). Promotion staging uses the same provider injection as apply.

## Exact-publish lease strategy (normative for Task 4)

**Single strategy (no alternatives):** wall-clock revalidation + mandatory pre-`BEGIN IMMEDIATE` heartbeat + fail-closed CAS.

1. Off-lease compute remains off-lease (no writer lease held during corpus session).
2. Before starting `publish_exact`, acquire/renew writer lease and set `ResolutionPublicationFence.now_ms` to wall clock at that moment.
3. Inside `publish_exact` / `validate_publication_fence`, compare `writer_lease.expires_at` against **current wall clock** (`system_now_ms()`), not only the captured `fence.now_ms`. Fail with `FenceLost` if expired.
4. Strictly **before** `BEGIN IMMEDIATE` (never inside the transaction), invoke the mandatory heartbeat hook so the IMMEDIATE work starts with a full lease TTL.
5. Perform the entire exact publish (windowed copies + view CAS) in **one** IMMEDIATE transaction. If wall-clock revalidation fails before the view CAS UPDATE, roll back — no partial exact binding. Do **not** heartbeat mid-transaction or split windowed copies across multiple writer leases for a single publish attempt.
6. If a corpus is large enough that one IMMEDIATE publish cannot fit inside `lease_duration_ms` even after a pre-BEGIN heartbeat, fail closed with a typed timeout/fence error and leave the view non-exact; do not invent multi-lease publish protocols in this plan.

Partial exact bindings are never allowed.

## Verification Strategy

**Project source of truth:** `AGENTS.md` / `CLAUDE.md`; Ph2d command patterns under `docs/plans/2026-08-08-index-store-ph2d-lifecycle-plan.md`.

**Worker red/green scope:** Narrowest package integration tests named in each task, always with `--test-threads=1`.

**Worker ceiling:** Package-local store tests only. No full workspace or real-world corpora.

**Worker gate invariant:** Each task proves its fencing behavior with an explicit concurrent/failure fixture.

**Lead affected-change scope:** After each task commit:

```bash
RUSTUP_TOOLCHAIN=1.97.1 cargo test -p julie-extract-artifact \
  --test store_coordinator_contract --test store_connection_contract \
  --test store_generation_contract --test store_maintenance_contract \
  -- --test-threads=1
RUSTUP_TOOLCHAIN=1.97.1 cargo test -p julie-extract-cli \
  --features test-store-contract \
  --test store_resolution_contract --test store_operations_contract \
  -- --test-threads=1
```

**Branch gate:** Before handoff/PR:

```bash
RUSTUP_TOOLCHAIN=1.97.1 cargo test -p julie-extract-artifact \
  --features test-store-crash \
  --test store_generation_crash_contract --test store_maintenance_crash_contract \
  --test store_crash_contract -- --test-threads=1
RUSTUP_TOOLCHAIN=1.97.1 cargo test -p julie-extract-artifact \
  --test store_resolution_binding_contract --test store_resolution_base_contract \
  -- --test-threads=1
RUSTUP_TOOLCHAIN=1.97.1 cargo test -p julie-extract-cli \
  --features test-store-contract \
  --test store_equivalence --test store_mixed_version \
  --test store_maintenance_mixed_version \
  --test store_import_contract --test store_resolution_adapters \
  -- --test-threads=1
RUSTUP_TOOLCHAIN=1.97.1 cargo clippy -p julie-extract-artifact -p julie-extract-cli -- -D warnings
RUSTUP_TOOLCHAIN=1.97.1 cargo fmt --check
```

**Replay/metric evidence:** No performance hard gates.

**Escalation triggers:** Touching publish/CURRENT, lease takeover, exact publish, or floor/meta copy requires crash-feature tests. Import base identity requires import + resolution adapter contracts.

**Assigned verification failure:** Workers stop and report; do not delete or weaken contracts to go green.

**Verification ledger:** Record invariant, command, scope label, commit SHA, result, timestamp per task.

## Parallel Execution Contract

All tasks are **serial**. Labels are sequence IDs only.

| Task | Parallel batch | File ownership | Serialization required | Dependency reason |
|---|---|---|---|---|
| Task 1: Intent-aware lease authority + maintenance-owner API | Serial 1 | `coordinator.rs`, `connection.rs`, `store_coordinator_contract.rs`, `store_connection_contract.rs`, `store_generation_contract.rs` | Yes | Risk-first foundation. |
| Task 2: Floor raise, intent mirror, destination meta normalize | Serial 2 | `maintenance.rs`, `generation.rs` (`copy_store_metadata` / build path), contracts, docs/contracts if meta keys added | Yes | Depends on Task 1 intent+owner APIs. |
| Task 3: Wall-time lease validation + exact-publish fence clock | Serial 3 | `connection.rs`, `resolution.rs` (`validate_publication_fence`, publish path), `resolve.rs` (`with_writer_lease`), related contracts | Yes | Shares connection with Task 1; must land before terminal-append work relies on wall-time fence. |
| Task 4: Ban unfenced resolve terminal writes | Serial 4 | `resolve.rs` (`append_resolution_terminal`), optional small artifact helper, `store_resolution_contract.rs` | Yes | Depends on Task 3 wall-time fence helpers. |
| Task 5: Pin release-on-failure + expiry-aware protection | Serial 5 | `resolve.rs` pin lifecycle, `resolution.rs` `base_is_protected`, pin/base contracts | Yes | Shares `resolve.rs` with Task 4. |
| Task 6: Live capacity re-probe at apply | Serial 6 | `maintenance.rs` capacity provider plumbing, CLI maintenance wiring if provider is injected, contracts | Yes | Depends on Task 2 acquire/finish structure. |
| Task 7: Enqueue/claim/cursor intent recheck inside coordinator txns | Serial 7 | `coordinator.rs` enqueue/claim/cursor, coordinator contracts | Yes | Uses Task 1 intent predicate; split from import CAS for proof isolation. |
| Task 8: Import base building→ready CAS | Serial 8 | `from_artifact.rs`, `executor.rs` (~1062+), import/resolution adapter contracts | Yes | Independent product slice after core fencing. |
| Task 9: Docs + branch evidence | Serial 9 | architecture/contracts/docs/evidence only | Yes | After all code tasks green. |

Commit mode: `serial-worker-commit`.

---

### Task 1: Intent-aware lease authority and explicit maintenance-owner acquisition

**Files:**
- Modify: `crates/julie-extract-artifact/src/store/coordinator.rs` (`try_acquire_or_takeover` ~773, enqueue later in Task 7, shared intent helper)
- Modify: `crates/julie-extract-artifact/src/store/connection.rs` (`open_writer` ~183, `validate_generation_write_fence` ~304, `GenerationFence`)
- Test: `crates/julie-extract-artifact/tests/store_coordinator_contract.rs`
- Test: `crates/julie-extract-artifact/tests/store_connection_contract.rs`
- Test: `crates/julie-extract-artifact/tests/store_generation_contract.rs`

**Interfaces:**
- Consumes: `maintenance_intent` row; `GenerationFence`; `LeaseDisposition`
- Produces:
  - Shared pure/read helper: `foreign_live_maintenance_intent(conn, now) -> Option<IntentIdentity>`
  - Ordinary `try_acquire_or_takeover(holder, now)` refuses when foreign live intent exists (returns non-Acquired / typed busy)
  - **Fixed API name:** `StoreCoordinator::try_acquire_for_maintenance(holder: LeaseHolder, owner: MaintenanceOwnerFence, now: i64) -> Result<LeaseDisposition, CoordinatorError>` where `MaintenanceOwnerFence { run_id, owner_id, owner_pid, fencing_token }` must match the live intent row on **all** fields. Ordinary `try_acquire_or_takeover` never accepts a maintenance bypass.
  - `open_writer` with a maintenance `GenerationFence` (`run_id.is_some()`) uses the maintenance path; ordinary path never treats holder_id/PID alone as intent ownership
  - After ordinary lease acquire inside `open_writer`, re-run `validate_generation_write_fence`; on failure release lease and error

**Contract inputs:** Ph2d design: intent blocks ordinary writers even when lease row is absent during generation build.

**File ownership:** coordinator + connection + contracts listed

**Serialization required:** Yes

**Dependency reason:** Plan start.

**What to build:** Close the promote race where intent is live, writer lease is deleted, and ordinary open_writer succeeds. Introduce an explicit maintenance-owner acquisition interface so implementers cannot “match holder id” as a bypass.

**Approach:**
- In the IMMEDIATE lease transaction, SELECT intent. If live and not exactly matching the **maintenance fence argument** (absent on ordinary path), refuse acquire.
- Do **not** add run_id onto `LeaseHolder`. Keep ordinary holders free of maintenance identity; pass maintenance identity only on the maintenance API.
- `open_writer`: if `generation_fence.run_id.is_some()`, call maintenance acquire/validate path; else ordinary path + post-acquire fence recheck.
- Tests: (1) live intent, no lease ⇒ ordinary acquire fails; (2) maintenance fence with full identity succeeds; (3) same holder_id/pid without run_id/token fails; (4) expired intent allows ordinary acquire; (5) promote-style lease release still blocks foreign open_writer.

**Acceptance criteria:**
- [x] Foreign live intent blocks ordinary lease acquire without a live writer lease row
- [x] Maintenance owner requires full intent identity, not holder_id/PID alone
- [x] `open_writer` rechecks fence after ordinary lease acquire and releases on failure
- [x] Existing dead/expired takeover tests still pass
- [x] Worker-scope verification passes and the worker commits

**Worker verification:**

```bash
RUSTUP_TOOLCHAIN=1.97.1 cargo test -p julie-extract-artifact \
  --test store_coordinator_contract --test store_connection_contract \
  --test store_generation_contract -- --test-threads=1
```

**Commit:** `fix(store): refuse leases under foreign maintenance intent`

---

### Task 2: Raise source floor, mirror intent, normalize destination metadata

**Files:**
- Modify: `crates/julie-extract-artifact/src/store/maintenance.rs` (acquire ~978–1117, finish/abort ~1247+, `release_writer_for_generation_build` ~1131)
- Modify: `crates/julie-extract-artifact/src/store/generation.rs` — **required:** `copy_store_metadata` ~706 and/or call sites in `logical_copy_generation` / `build_and_publish`
- Test: `crates/julie-extract-artifact/tests/store_maintenance_contract.rs`
- Test: `crates/julie-extract-artifact/tests/store_generation_contract.rs`
- Test: `crates/julie-extract-artifact/tests/store_generation_crash_contract.rs` (floor mid-step crash if hooks exist / add marker)
- Modify if new meta keys: `docs/contracts/store-v1.md`, schema contract tests

**Interfaces:**
- Consumes: Task 1 maintenance-owner fence; intent column `source_min_writer_version`
- Produces: Normative M1–M7 state machine implemented; destination never permanently inherits temporary raised floor or live intent mirrors

**Contract inputs:** Two-database state machine section above; Ph2d design floor raise + intent mirror.

**File ownership:** maintenance + generation copy path + contracts

**Serialization required:** Yes

**Dependency reason:** Needs Task 1 maintenance-owner API and intent blocking.

**What to build:** Implement M1→M7. Two restore mechanisms (do not conflate):
1. **Repair/in-place / source still serving:** finish/abort UPDATEs source `min_writer_version` back to `source_min_writer_version` and clears mirrors.
2. **Promote/rebuild:** copy-time override forces destination `min_writer_version` to pre-maintenance value and strips temporary mirror keys **before** destination validation/publish. Source may become `retired` with or without cleanup; writers never open retired generations.

**Approach:**
- After M1 (intent+lease in coord), open maintenance-fenced store writer and perform M2; only then call `release_writer_for_generation_build` (M3).
- If M2 fails after M1 committed: do **not** clear intent first. Run the abort path with restore-before-clear: verify source floor is still the pre-maintenance value (or restore it if a partial meta write occurred), clear any partial `maintenance_tmp_*` mirrors under the maintenance fence, **then** delete lease+intent. Record the failure. Never leave serving source with raised floor and no intent; never clear intent while raised floor still needs restore.
- `copy_store_metadata`: when copying, set `min_writer_version` from recorded pre-maintenance value (pass it into copy). Skip keys with reserved prefix `maintenance_tmp_` (intent-mirror keys must use this prefix). If destination already has any `maintenance_tmp_*` keys, delete them before validation.
- Tests: older binary cannot open writer after M2; promote destination floor equals pre-maintenance value while source was raised during build; finish/abort restore for non-promote path; crash between M1 and M2 leaves fail-closed foreign writers via intent alone.

**Acceptance criteria:**
- [x] Acquire order is M1 then M2 then M3 (lease release only after source floor/mirror write)
- [x] Promoted destination `min_writer_version` equals pre-maintenance value (not the raised temporary floor)
- [x] Temporary intent-mirror keys do not remain on the serving destination
- [x] Serving source after abort/finish is not left with raised floor and no intent
- [x] Worker-scope verification passes and the worker commits

**Worker verification:**

```bash
RUSTUP_TOOLCHAIN=1.97.1 cargo test -p julie-extract-artifact \
  --test store_maintenance_contract --test store_generation_contract \
  -- --test-threads=1
RUSTUP_TOOLCHAIN=1.97.1 cargo test -p julie-extract-artifact --features test-store-crash \
  --test store_generation_crash_contract -- --test-threads=1
```

**Commit:** `fix(store): raise source writer floor and normalize destination meta`

---

### Task 3: Wall-time lease validation and exact-publish fence clock

**Files:**
- Modify: `crates/julie-extract-artifact/src/store/connection.rs` (`validate_writer_lease` ~269)
- Modify: `crates/julie-extract-artifact/src/store/resolution.rs` (`validate_publication_fence` ~1469, `publish_exact_with_markers` ~1255)
- Modify: `crates/julie-extract-cli/src/store/resolve.rs` (`with_writer_lease` ~468)
- Test: `crates/julie-extract-artifact/tests/store_connection_contract.rs`
- Test: `crates/julie-extract-artifact/tests/store_resolution_binding_contract.rs`
- Test: `crates/julie-extract-cli/tests/store_resolution_contract.rs`

**Interfaces:**
- Consumes: `heartbeat_lease_for`, `ResolutionPublicationFence`, normative exact-publish strategy above
- Produces: Live ownership always checked against wall clock; exact publish fails closed if lease expired before CAS

**Contract inputs:** Exact-publish lease strategy section; 5s default lease.

**File ownership:** connection + resolution publish fence + resolve lease wrapper

**Serialization required:** Yes

**Dependency reason:** Lands before Task 4 so fenced terminal append inherits wall-time validation.

**What to build:** Fix stale `checked_at` / `fence.now_ms` comparisons. Implement the single exact-publish strategy (wall-clock revalidation + mandatory pre-`BEGIN IMMEDIATE` heartbeat + fail-closed rollback). No strategy alternatives.

**Approach:**
- `validate_writer_lease`: always use `system_now_ms()` for `expires_at > ?`.
- `validate_publication_fence`: use wall clock in the SQL bind for `expires_at > ?`.
- `with_writer_lease`: heartbeat immediately after acquire; on operation success/failure still release.
- Change `publish_exact` so production callers cannot omit the heartbeat hook; invoke it once immediately before `BEGIN IMMEDIATE`; revalidate fence with wall clock before view CAS UPDATE; roll back on failure (see Recovery seams §2 and Exact-publish strategy).
- Tests: expired lease fails even if fence.now_ms is old; publish with artificially short lease fails without leaving `resolution_state=exact`; drain quanta still pass.

**Acceptance criteria:**
- [x] No production path accepts a lease solely because `expires_at > fence.checked_at/now_ms` when wall clock is past expiry
- [x] Exact publish always heartbeats strictly before `BEGIN IMMEDIATE` and fails closed (rollback) if wall-clock ownership is lost before CAS
- [x] No mid-transaction heartbeat path exists in production code
- [x] Drain quantum path remains green
- [x] Worker-scope verification passes and the worker commits

**Worker verification:**

```bash
RUSTUP_TOOLCHAIN=1.97.1 cargo test -p julie-extract-artifact \
  --test store_connection_contract --test store_resolution_binding_contract \
  -- --test-threads=1
RUSTUP_TOOLCHAIN=1.97.1 cargo test -p julie-extract-cli \
  --features test-store-contract \
  --test store_resolution_contract -- --test-threads=1
```

**Commit:** `fix(store): validate writer leases against wall time`

---

### Task 4: Ban unfenced resolve terminal writes

**Files:**
- Modify: `crates/julie-extract-cli/src/store/resolve.rs` (`append_resolution_terminal` ~530, call sites ~351 and ~437)
- Test: `crates/julie-extract-cli/tests/store_resolution_contract.rs`

**Interfaces:**
- Consumes: fenced factory + wall-time lease validation from Task 3
- Produces: Terminal log rows only via fenced `open_writer`

**Contract inputs:** Architecture rule that store mutations are generation-fenced. Reconcile still tolerates store-terminal/coord-uncommitted tears.

**File ownership:** resolve terminal path + resolution contract

**Serialization required:** Yes

**Dependency reason:** Needs Task 3 wall-time fenced open.

**What to build:** **Required minimum:** replace raw `Connection::open(layout.store_db())` with fenced open using the active fencing token; revalidate lease with wall time before append. **Optional stretch (not required for task done):** fold terminal append into the same IMMEDIATE transaction as `publish_exact` — only if it stays a small API change; do not block the task on that redesign.

**Approach:**
- Change `append_resolution_terminal` signature to take fence/factory (or fenced connection).
- Grep CLI store for other raw write opens of `store_db`; fix resolve path for sure; list any remaining out-of-scope hits in the task report.

**Acceptance criteria:**
- [x] Resolve success path has zero raw unfenced `Connection::open(store_db)` writes
- [x] Contract proves terminal append fails closed under foreign live maintenance intent or CURRENT change
- [x] Worker-scope verification passes and the worker commits

**Worker verification:**

```bash
RUSTUP_TOOLCHAIN=1.97.1 cargo test -p julie-extract-cli \
  --features test-store-contract \
  --test store_resolution_contract -- --test-threads=1
```

**Commit:** `fix(store): fence resolve terminal log writes`

---

### Task 5: Resolve pin release-on-failure and expiry-aware base protection

**Files:**
- Modify: `crates/julie-extract-cli/src/store/resolve.rs` (pin open ~323, failure path ~181–208)
- Modify: `crates/julie-extract-artifact/src/store/resolution.rs` (`base_is_protected` ~705)
- Test: `crates/julie-extract-cli/tests/store_resolution_contract.rs`
- Test: `crates/julie-extract-artifact/tests/store_resolution_binding_contract.rs`
- Test: `crates/julie-extract-artifact/tests/store_resolution_base_contract.rs`

**Interfaces:**
- Consumes: `release_pin`, pin expiry SQL already used by pin APIs
- Produces: Best-effort pin release on all exits after pin open; expired pins do not protect bases

**Contract inputs:** Maintenance GC already filters expired pins.

**File ownership:** resolve pins + base protection

**Serialization required:** Yes

**Dependency reason:** Shares `resolve.rs` with Task 4.

**What to build:** RAII/defer pin guard after `begin_convergence`. On failure, best-effort `release_pin` while claim/lease ownership is still available; if ownership is already lost, pin remains until expiry and GC — acceptance must allow that bounded case. Fix `base_is_protected` to require unexpired pins.

**Approach:**
- Guard disarms after successful intentional release.
- Protection SQL: unexpired pin OR existing delta rows.
- Cooperative cancel / scratch nonce is **out of scope** unless trivial; document under follow-ups if skipped.

**Acceptance criteria:**
- [x] When release is possible (claim/lease still owned), failed resolve leaves no live pin for that pin_id
- [x] When ownership is already lost, pin is bounded by expires_at and does not protect forever after expiry (`base_is_protected` false once expired)
- [x] Successful exact path still releases pins once
- [x] Worker-scope verification passes and the worker commits

**Worker verification:**

```bash
RUSTUP_TOOLCHAIN=1.97.1 cargo test -p julie-extract-cli \
  --features test-store-contract \
  --test store_resolution_contract -- --test-threads=1
RUSTUP_TOOLCHAIN=1.97.1 cargo test -p julie-extract-artifact \
  --test store_resolution_binding_contract --test store_resolution_base_contract \
  -- --test-threads=1
```

**Commit:** `fix(store): release resolve pins and honor pin expiry`

---

### Task 6: Re-probe free capacity at maintenance apply

**Files:**
- Modify: `crates/julie-extract-artifact/src/store/maintenance.rs` (`MaintenanceExecutor` acquire/apply, `RevalidationCapacity` ~1825, `apply_with_policy` ~1281)
- Modify if provider is constructed only at CLI: `crates/julie-extract-cli/src/store/maintenance.rs`
- Test: `crates/julie-extract-artifact/tests/store_maintenance_contract.rs`

**Interfaces:**
- Consumes: `CapacityProvider::free_bytes(path)`
- Produces: Live free-bytes sample injected into executor; re-probe immediately before first delete/stage/cohort mutation

**Contract inputs:** Ph2d capacity preflight; executor today only stores factory/run/plan and freezes plan-time free bytes.

**File ownership:** maintenance capacity plumbing + CLI wiring if needed

**Serialization required:** Yes

**Dependency reason:** Depends on Task 2 acquire/finish structure.

**What to build:**
1. Thread a `CapacityProvider` (or free-bytes callback) into `MaintenanceExecutor` and into generation promote/repair/rollback staging (construct from the same provider inspect uses).
2. Re-probe immediately before each first mutative step covered by Recovery seams §5: GC delete/demotion, scratch purge, and generation staging/create.
3. Keep semantic root binding recheck; do not freeze free_bytes from the plan forever.

**Approach:**
- Replace `RevalidationCapacity { free_bytes: plan... }` echo with provider-backed reads.
- Test: plan with `gc_fits=true`, then provider returns lower free_bytes before apply ⇒ no protected object deleted.

**Acceptance criteria:**
- [x] Apply refuses when live free bytes fall below required headroom
- [x] Re-probe occurs before first delete/stage, not only at process start
- [x] Root binding stale-plan checks still work
- [x] Worker-scope verification passes and the worker commits

**Worker verification:**

```bash
RUSTUP_TOOLCHAIN=1.97.1 cargo test -p julie-extract-artifact \
  --test store_maintenance_contract -- --test-threads=1
```

**Commit:** `fix(store): re-probe capacity before maintenance apply`

---

### Task 7: Enqueue, claim, and cursor intent recheck inside coordinator transactions

**Files:**
- Modify: `crates/julie-extract-artifact/src/store/coordinator.rs` (`enqueue` ~569, `claim_resolve` ~641, `advance_consumer_cursor` ~1706, cursor release if present)
- Test: `crates/julie-extract-artifact/tests/store_coordinator_contract.rs`

**Interfaces:**
- Consumes: Task 1 `foreign_live_maintenance_intent` helper
- Produces: enqueue insert, resolve claim, and cursor watermark mutations refuse under foreign live intent **inside the same IMMEDIATE transaction** that performs the write

**Contract inputs:** Design: enqueue/claim refuse or wait while intent live. Pre-txn `validate_write_fence` alone is insufficient (TOCTOU).

**File ownership:** coordinator request/cursor paths only (import is Task 8)

**Serialization required:** Yes

**Dependency reason:** Intent helper from Task 1; isolated proof from import CAS.

**What to build:** After `begin_coordinator`, re-read intent; if foreign live, abort write and return typed busy/false. Keep idempotent enqueue replay behavior for already-existing requests if that path does not insert new work (document choice: prefer still reporting busy when intent live even for pure read of existing request only if it mutates; non-mutating dedup return may remain allowed).

**Approach:**
- Shared helper used by lease acquire (Task 1) and these paths.
- Cursor release must run its intent check inside the IMMEDIATE txn that deletes the cursor (Recovery seams §3).
- Tests: intent appears after pre-check simulation—enqueue IMMEDIATE sees intent and refuses insert; claim_resolve returns false; cursor advance does not move watermark; cursor release does not delete under foreign live intent.

**Acceptance criteria:**
- [x] Enqueue cannot insert a new request row while foreign live intent exists (checked in IMMEDIATE txn)
- [x] claim_resolve cannot claim under foreign live intent
- [x] Cursor advance cannot raise watermarks under foreign live intent
- [x] Cursor release cannot delete a cursor under foreign live intent (IMMEDIATE in-txn check)
- [x] Worker-scope verification passes and the worker commits

**Worker verification:**

```bash
RUSTUP_TOOLCHAIN=1.97.1 cargo test -p julie-extract-artifact \
  --test store_coordinator_contract -- --test-threads=1
```

**Commit:** `fix(store): recheck maintenance intent inside coordinator writes`

---

### Task 8: Import resolution bases via building→ready CAS discipline

**Files:**
- Modify: `crates/julie-extract-cli/src/store/from_artifact.rs` (`materialize_resolution_base` ~567+)
- Modify: `crates/julie-extract-cli/src/store/executor.rs` (~1062–1130 ready-first `INSERT ... state` path)
- Prefer reuse: `crates/julie-extract-artifact/src/store/resolution.rs` `ResolutionBaseCatalog`
- Test: `crates/julie-extract-cli/tests/store_import_contract.rs`
- Test: `crates/julie-extract-cli/tests/store_resolution_adapters.rs`

**Interfaces:**
- Consumes: base_id helper / catalog state machine
- Produces: Import never publishes a ready base without durable catalog building→ready discipline; crash leaves recoverable non-ready state

**Contract inputs:** Resolve catalog is the identity authority; import currently materializes file then `INSERT ... ON CONFLICT DO NOTHING` as ready.

**File ownership:** from_artifact + executor import resolution publish

**Serialization required:** Yes

**Dependency reason:** Separate proof obligation after core fencing; uses fenced writers already required for import quanta.

**What to build:** Replace ready-first insert with the T8a–T8d transaction boundaries in Recovery seams §4. Align base_id with resolve `base_id()` helper (single source).

**Approach:**
- Move orchestration into catalog APIs where possible instead of duplicating SQL in executor.
- Implement T8a→T8d in order; never combine ready CAS with file materialize in a way that can claim ready without the file.
- Crash test or simulated mid-point after T8b/T8c: no `state=ready` without matching file identity; retry reclaims abandoned `building`.

**Acceptance criteria:**
- [x] Executor path follows T8a–T8d (building before file publish; ready only after identity CAS)
- [x] Import and resolve base ids share one helper
- [x] Crash/mid-failure does not observe a ready base without a valid base file
- [x] Retry/recovery for abandoned building is defined and tested
- [x] Worker-scope verification passes and the worker commits

**Worker verification:**

```bash
RUSTUP_TOOLCHAIN=1.97.1 cargo test -p julie-extract-cli \
  --features test-store-contract \
  --test store_import_contract --test store_resolution_adapters \
  -- --test-threads=1
```

**Commit:** `fix(store): CAS import resolution bases`

---

### Task 9: Docs, architecture note, and branch evidence

**Files:**
- Modify: `docs/architecture/versioned-index-store.md`
- Modify: `docs/contracts/store-v1.md` if caller-visible floor/intent/cursor semantics change
- Create: `docs/evidence/2026-08-10-store-concurrent-fencing.md`

**Interfaces:**
- Consumes: completed behaviors + branch gate results
- Produces: durable evidence and explicit follow-ups

**Contract inputs:** All prior tasks green.

**File ownership:** docs/evidence only

**Serialization required:** Yes

**Dependency reason:** After all code tasks.

**What to build:** Document M1–M7, maintenance-owner API, enqueue/claim in-txn intent checks, wall-clock publish fence, pin expiry, capacity re-probe, import CAS. List intentional follow-ups (cooperative cancel of off-lease resolve compute; scratch nonces) if still deferred.

**Acceptance criteria:**
- [x] Architecture doc states foreign live intent blocks ordinary writer lease acquire even with no lease row
- [x] Architecture doc states destination promotion does not inherit temporary raised writer floor
- [x] Evidence file lists branch-gate commands, SHAs, and results
- [x] Docs and evidence committed

**Worker verification:** Full branch gate from Verification Strategy.

**Commit:** `docs(store): record concurrent fencing hardening evidence`

---

## Out of scope

- Miller Ph3 registry, admission, dashboard, or pin bumps
- Changing global default lease duration policy
- Automatic GC during ordinary writes
- Cooperative cancel of every off-lease resolve CPU loop / unique scratch nonces (follow-up unless done opportunistically in Task 5)
- Performance hard gates / real-world corpora
- Folding resolve terminal into `publish_exact` same transaction (optional only in Task 4)

## Success definition

Concurrent worktrees may enqueue and resolve against one family store, but:

1. Promote/repair freezes the source: foreign writers cannot acquire the store-writer lease or open fenced writers while intent is live, even when the lease row is absent.
2. Maintenance owner identity is explicit (`run_id` + owner + token), not inferred from holder id/PID.
3. Temporary raised `min_writer_version` cannot permanently land on a published destination; serving gens are not left raised without intent.
4. Resolve never writes `store.db` unfenced; exact publish revalidates lease on wall clock; pins do not protect forever after expiry.
5. Enqueue/claim/cursor mutations recheck intent inside their IMMEDIATE transactions.
6. Maintenance apply re-checks live free space before first delete/stage.
7. Import bases follow building→ready identity discipline.

When those hold, the family store is ready for multi-worktree writer stress under the existing crash/equivalence gates.
