# Pre-merge fix report — store concurrent fencing

**Branch:** `fix/store-concurrent-fencing`  
**Worktree:** `/home/murphy/source/julie-extractors/.worktrees/fix/store-concurrent-fencing`  
**Date:** 2026-08-10

## Findings closed

### Finding 1 HIGH — maintenance errors leave floor raised

- `MaintenanceExecutor` now tracks `finished: AtomicBool`.
- Successful `finish` / `finish_generation_action` restores floor, clears intent, then disarms.
- `Drop` runs best-effort `restore_serving_source_floor_and_clear_coord` when not finished.
- Failed M2 floor raise still restores then disarms so Drop does not double-clear.
- Test: `apply_error_after_floor_raise_restores_floor_and_clears_intent_on_drop`.

### Finding 2 HIGH — 10-minute fence heuristic bypasses wall-clock expiry

- Removed `near_wall` heuristic in `validate_writer_lease`.
- Lease validation always uses `system_now_ms()`.
- Drain acquires the store-writer lease with wall clock.
- Quantum path heartbeats the store-writer lease with wall clock before open/commit.
- Injected clocks still drive service windows, claim heartbeats, and `store_log` timestamps.
- Existing wall-expiry-despite-stale-`checked_at` contract still passes.

### Finding 3 MEDIUM — try_acquire_for_maintenance without live intent

- When `maintenance_owner` is `Some`, a live intent must match all owner fields.
- Missing/expired intent → `CoordinatorError::InvalidRequest` (no lease insert with caller token).
- Mismatched live intent still → `MaintenanceInProgress`.
- Test: `maintenance_owner_acquire_without_live_intent_is_invalid`.

### Finding 4 MEDIUM — import steals live building

- Building reclaim reassigns `request_id` only when prior owner is same request, absent, terminal (`failed`/`committed`/`acknowledged`), or receipted.
- Live queued/claimed foreign owners return `resolution_base_building_busy:{request_id}`.

### Finding 5 MEDIUM — T8 building not durable before file publish

- True multi-txn T8a/T8d is not feasible inside one quantum: outer IMMEDIATE writer blocks a nested IMMEDIATE.
- Documented on `materialize_resolution_base`.
- Strongest same-quantum controls:
  - reclaim safety (Finding 4)
  - cleanup of newly published final base files on CAS/error paths
  - `published_new_file` flag so post-materialize quantum failure cleans orphans without deleting reused files

## Verification

```bash
RUSTUP_TOOLCHAIN=1.97.1 cargo test -p julie-extract-artifact \
  --test store_coordinator_contract --test store_connection_contract \
  --test store_maintenance_contract --test store_generation_contract \
  -- --test-threads=1
# connection 26, coordinator 59, generation 8, maintenance 19 — all ok

RUSTUP_TOOLCHAIN=1.97.1 cargo test -p julie-extract-cli \
  --features test-store-resolution-contract \
  --test store_import_contract --test store_resolution_contract \
  -- --test-threads=1
# import 31, resolution 13 — all ok
```

Coordinator suite is 59/59 (prior 58 + one new InvalidRequest case).

## Residual concerns

1. Same-quantum T8: building and ready still commit together. Process kill after file publish and before quantum commit can leave an orphan base file with no durable ready/building row; reclaim path and maintenance scratch/base GC remain the recovery path.
2. Drop restore is best-effort (`let _ =`); a second failure during Drop still leaves intent/floor for successor/expiry recovery.
3. Claim/fail request rows still stamp service-clock times; only writer-lease `expires_at` is forced to wall domain on drain/quantum paths.
