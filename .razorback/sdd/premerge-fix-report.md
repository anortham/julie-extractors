# Pre-merge fix report — export and retirement gaps

**Branch:** `remove/resolution-write-path`  
**Worktree:** `/home/murphy/source/julie-extractors/.worktrees/remove-resolution-write-path`  
**HEAD before:** `98ad9b41`  
**Date:** 2026-08-18

## Findings closed

### Finding 1 — first applied maintenance on a legacy store StalePlans

- Inspect no longer reads retired resolution tables or `bases/` bytes.
- Scratch capacity ignores `resolve-*` / `resolution-*.partial.db` names.
- A writer-open retirement now leaves the same inspect fingerprint.
- Test: `first_gc_apply_on_unmigrated_legacy_store_does_not_stale_plan`.

### Finding 2 — empty / all-unsupported bound views cannot export

- Export uses `EXTRACTION_IDENTITY_EPOCH` when no file version is visible.
- Those views receive current global capability rows.
- Tests: `export_succeeds_on_an_empty_import`, `export_succeeds_on_an_all_unsupported_import`.

### Finding 3 — scratch reaper swallows read_dir errors

- Scratch `read_dir` now matches file reap: `NotFound` is ok, other errors return.
- Unix test: `reap_retired_resolution_scratch_propagates_read_dir_errors`.

### Finding 4 — legacy export copies retired capability gap rows

- Export skips `reference_resolution.%` gap rows.
- Emitted rows match `current_capability_fingerprints()`.
- Test: `export_omits_legacy_reference_resolution_capability_gaps`.

## Verification

```bash
cargo test -p julie-extract-cli --test store_cli_contract
# 21 passed

cargo test -p julie-extract-artifact --test store_maintenance_contract
# 23 passed

cargo test -p julie-extract-artifact --test store_resolution_retirement_contract
# 3 passed

cargo test -p julie-extract-artifact --lib store::layout::tests
# 1 passed

cargo test -p julie-extract-cli --tests --no-run
cargo test -p julie-extract-artifact --tests --no-run
# both compiled
```

## Residual concerns

1. Store.db physical fingerprint still uses file length. Retirement did not change it in the seeded apply test. A later writer that grows `store.db` can still StalePlan.
2. Empty and all-unsupported exports write current-binary capability rows. The store never staged a snapshot for those views.
3. Scratch permission test is Unix-only.
4. Unused helpers remain in `maintenance.rs`: `parse_scoped_generation`, `checked_base_path`, `orphan_base_files`. They were already unused before this fix.
