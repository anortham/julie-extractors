# Task 7 Report: Immutable Resolution Base Publication and Recovery

## Status and state

- Status: complete; continuing directly to Task 8.
- Base: `8f6794a42608c64fce1d0c065fd3aec6d6d17fb9`.
- Branch: `codex/index-store-ph2c`.
- Worktree: `/Users/murphy/source/julie-extractors/.claude/worktrees/index-store-ph2c`.
- Final commit: this report's containing commit.
- Push/release: not performed.

## Delivered behavior

- Added `ResolutionBaseCatalog::{begin_build,recover,publish_scratch,mark_ready,find_ready}` and typed lifecycle outcomes/errors.
- Registration transactionally inserts one deterministic building identity plus every manifest-visible L2 source-version root before returning an off-lease scratch path.
- Scratch completion, target validation, no-replace atomic publication, and ready CAS remain separate durable boundaries.
- Ready lookup revalidates file identity, catalog hash, row counts, bytes, SHA-256, roots, integrity, and manifest-visible targets.
- Recovery handles absent/building/ready catalog state crossed with missing, complete, incomplete, or corrupt scratch/final files, while live owners and pins/deltas prevent destructive cleanup.
- Concurrent stale/successor builders converge on one immutable final path; the stale ready CAS is fenced and both scratch files are removed safely.

## RED/GREEN ledger

1. Lifecycle RED: `ResolutionBaseCatalog` and typed begin outcome were absent. GREEN: registration roots versions, off-lease build publishes, ready CAS succeeds, and identical reuse returns the ready row.
2. Recovery RED: no catalog/file state classifier existed. GREEN: valid final files win, corrupt files require dead-owner proof, and pinned missing-ready files remain untouched.
3. Target-integrity RED: a caller could certify a nonexistent target while building the standalone file. GREEN: publication and every ready lookup stream distinct targets through manifest-visible Store symbol checks.
4. Concurrency RED: a second caller could only observe a building row. GREEN: registration creates one row, successor recovery reassigns dead ownership, no-replace publication admits one final identity, and the stale CAS is rejected.
5. Crash RED: the lifecycle had no real process boundaries. GREEN: self-reexec aborts after row insert, root insert, scratch close, final publication, pre-ready commit, and post-ready commit all reopen and converge to one ready base with one root and no scratch orphan.
6. Tier RED: the new lifecycle harness would otherwise enter the default suite. GREEN: it is gated by `test-store-resolution`; crash boundaries remain under `test-store-crash` and normal builds do not read the hook environment.

## Verification ledger

- `store_resolution_base_contract`: 8/8 passed.
- Resolution-base crash boundary contract: 1/1 parent matrix passed across six abort points.
- Full artifact all-feature suite: passed, including crash 13/13 and all Store regressions.
- Artifact all-target/all-feature Clippy with `-D warnings`: passed.
- `cargo fmt --all -- --check`: passed.
- `git diff --check`: passed.
- Cold `cargo +1.97.1 xtask test default`: every test passed, then the 90-second tier tripwire fired.
- Warm rerun of the exact default command: exit 0.

## Miller evidence

- The target worktree remains unregistered; Miller onboarding returned `workspace_onboarding_empty`.
- Per the approved fallback, inspection used targeted `rg`, bounded reads, exact tests, crash subprocesses, and direct diff review. No Miller result was invented.

## Scope judgment

- Recovery accepts the coordinator's already-adjudicated live/dead owner fact; Task 9 owns the dedicated resolve claimant/heartbeat integration.
- Publication uses an atomic no-replace hard link followed by source unlink. This preserves immutable rename visibility while preventing a concurrent builder from overwriting an already published final file; recovery covers the intermediate two-link state.
