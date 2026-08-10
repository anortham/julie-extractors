# Autonomous run report — Store Concurrent Fencing Hardening

- **Status:** Complete (PR pending — filled in after PR creation)
- **Plan:** docs/plans/2026-08-10-store-concurrent-fencing-hardening.md
- **Branch:** fix/store-concurrent-fencing
- **Worktree:** .worktrees/fix/store-concurrent-fencing
- **Base:** main (merge-base 2bfe383)
- **HEAD:** 96f767d
- **PR URL:** pending — filled in after PR creation

## What shipped

Closed multi-worktree writer races on the versioned family store:

1. Foreign live maintenance intent blocks ordinary writer-lease acquire even with no lease row; explicit `try_acquire_for_maintenance`.
2. Temporary source `min_writer_version` raise + `maintenance_tmp_*` mirrors; destination normalize; restore-before-clear; Drop abort cleanup.
3. Wall-clock writer-lease validation; exact-publish mandatory pre-BEGIN IMMEDIATE heartbeat.
4. Fenced resolve terminal writes; pin release-on-failure; expiry-aware base protection.
5. Capacity re-probe before GC and promotion staging.
6. Enqueue/claim/cursor intent recheck inside IMMEDIATE coordinator transactions.
7. Import resolution bases prefer building→ready discipline with live-owner reclaim rules.

## External review (codex)

- **Reviewer:** codex (adversarial pre-merge)
- **Findings total:** 5
- **Fixed:** 5 (commit 96f767d)
  - Maintenance Drop restores floor/intent on unfinished apply
  - Always wall-clock lease validation; drain quanta heartbeat wall-domain leases
  - Maintenance-owner acquire requires live matching intent
  - Import does not steal live building owners
  - Orphan base file cleanup on import error paths
- **Dismissed:** 0
- **Flagged for human review:** 0
- **Cost:** not reported by codex-cli

## Tests

Branch gate (Task 9 + post-fix subset):

- Artifact crash/store/resolution contracts: PASS
- CLI import/resolution contracts: PASS
- Clippy -D warnings, fmt --check: PASS (Task 9)
- Post-fix: connection 26, coordinator 59, generation 8, maintenance 19, import 31, resolution 13: PASS

## Judgment calls

- Maintenance M1 keeps atomic intent+lease insert rather than splitting to `try_acquire_for_maintenance` for TTL atomicity.
- T8 building+ready may still share one import quantum transaction; orphan file cleanup mitigates crash mid-publish.
- Writer-lease expires_at forced to wall domain on drain/quantum heartbeats while service clocks can remain injected for tests.

## Blockers hit

None.

## Next steps

- Human review and merge of PR
- Optional follow-up: multi-txn durable T8a/T8d for import bases; cooperative resolve cancel/scratch nonces

## Files changed

See `git diff --stat main...HEAD` on branch (25+ files under store, contracts, evidence).
