# Execution report — producer reader-retention contract

**Status:** Complete within the approved local implementation scope.
**Plan:** `docs/plans/2026-09-04-producer-retention-contract.md`
**Branch:** `feature/reader-retention-contract`
**Verified implementation/test commit:** `4ca16853ecb054f6989aafa1410381f41273adde`
**Base:** `main`, merge base `a87121c61b9c98ca3301da614de1a7fe23eb88e1`
**PR:** Not created; push/PR publication was not authorized. No push was attempted.
**Duration:** Not measured end-to-end; checkpoint and verification timestamps are retained.
**Phases:** Producer implementation and qualification complete.
**Tasks:** 6/6 complete.
**External-model policy:** Built-in Sol workers were explicitly requested. No external model CLI was invoked.

## What was implemented locally

- One immutable manifest-root registration in `coord.db`, with atomic admission, authenticated renewal/release, and bounded snapshot reads.
- Safe legacy catalog activation and a permanent writer floor of `2.40.0`, without changing store schema version 2.
- Linux and Windows process-instance qualification; heartbeat expiry alone never permits deletion.
- Reader-root protection across GC, generation promotion, rollback, and view retirement, including coordinator exclusion held through physical deletion.
- `store reader acquire|renew|release` report v1, sanitized failures, and bounded maintenance diagnostics.
- Cursor independence, committed-log retention boundaries, mixed-version refusal, crash recovery, and actual CLI/SQL qualification evidence.

## Judgment calls

- Used the existing permanent writer-version floor rather than a fresh-store-only schema bump. Actual 2.39.0 binaries refuse all four maintenance mutations before changing registered families.
- Kept process birth identity producer-internal. Only authenticated acquire/renew JSON returns the caller's nonce; human and failure output do not.
- Defined served sequence as the maximum retained committed log row, or zero after legitimate pruning. It is not an allocator reservation, monotonic revision, or complete-history certificate.
- Kept unknown identities protected and made reader log floors inclusive. Consumer cursor acknowledgment remains independently inclusive-prunable.
- Required exact crash markers after a Windows test exposed arbitrary nonzero exits being mistaken for checkpoint proof. Corrected undersized fixture leases without relaxing production fencing or manually clearing ownership.
- Moved catalog recovery timing into a whole-file crash target, strengthened the default-suite exemption rule, and registered the target in the normal contract tier.
- Preserved the public snapshot constructor and fingerprint encoding during lint cleanup; the private helper consumes the existing snapshot rather than nine loose values.
- Corrected two invalid full commit-ID expansions in draft evidence. Every full reference was checked against Git; captured JSON fingerprints were independently recomputed.

## External review

None requested or run. The lead reviewed worker changes inline for plan compliance and code quality.

## Review campaign

- State/evidence: not run; lead inline review and built-in worker execution only.
- Round/external invocations: 0/0.
- No unresolved in-scope defect remains from the performed review and verification. This is not a security-audit or external-review certification.

## Tests

- Final changed-path gate passed on `4ca16853`: all three default packages, xtask tests, and parser certification. The earlier two failures and their corrections remain in the evidence ledger.
- Full contract gate passed. It started at `f3f433e3`; only an existing expected-command test vector and memory metadata changed during that run, not production or runner commands.
- Standalone crash gate passed: 11 tests with default threading, separate from the serial contract invocation.
- Formatting passed; workspace Clippy and documentation build passed with zero warnings.
- Required Windows admission, liveness, retention, cursor, generation crash, held-reader rollback, CLI, mixed-version, catalog crash, fingerprint, and dead-reader checks passed.
- Security scope: none declared by the plan. Nonce privacy and conservative cleanup were tested as correctness invariants.
- Exact commands, source identities, SQL facts, platform limits, and captured reports: `docs/evidence/2026-09-producer-retention-contract.md`.

## Blockers and limits

- No remaining J1 technical blocker.
- Push, PR creation, merge, release, and Miller pin changes remain outside this run's authority. The branch and worktree are retained.
- Unsupported process-identity platforms have fail-closed policy coverage, not real-platform qualification.
- Retention protects registered readers, not older already-running Miller instances or arbitrary direct SQLite readers. M1 documents the rollout and restart requirement.
- S1 CPU qualification does not establish M5 agent-task efficacy; no outcome campaign or model-default change was performed.

## Files changed

The implementation/test delta from the base through `4ca16853` is 63 files, 12,498 insertions, and 154 deletions, including checkpoints. It covers artifact store models/coordinator/schema/liveness/maintenance/generation, the reader CLI, contract tests and test-tier registration, contract documentation, and small Audit Plan 4 closure corrections. This completion commit adds the final evidence, plan status, and report metadata.

## Source control

- J1: `/home/murphy/source/julie-extractors/.worktrees/reader-retention-contract`, `feature/reader-retention-contract`. All implementation/test commits ride on this branch; it is deliberately unmerged and unpushed.
- Julie main: `/home/murphy/source/julie-extractors`, `a87121c6`, clean before handoff. The J1 plan/preparation commit is local on main; implementation stays in the task branch.
- S1 correction: `/home/murphy/source/julie-semantic-sidecar/.worktrees/s1-evidence-alignment`, `fix/s1-evidence-alignment`, `5c41f1d`. Clean, verified, unmerged, and unpushed; it belongs to the sidecar repository, not the J1 branch.
- Sidecar main: `/home/murphy/source/julie-semantic-sidecar`, `9ed082b`, clean and unchanged by the correction branch.
- Miller program/M1 clarifications: `/home/murphy/source/miller`, main. Documentation-only updates are committed separately there; M1–M5 code was not executed.
- User-owned Miller worktrees retained unchanged: `ct-providers-jvm-ruby-php-gdscript` at `700c42cb` (clean), `v1.27-postrelease-audit` at `973c0c3f` (clean), and `v1.26.0-mcp-dogfood` at `7cfa8ad1` (existing untracked `.tools`). All are under `/home/murphy/source/miller/.worktrees/`.
- User-owned Julie worktree retained unchanged: `/home/murphy/source/julie-extractors/.claude/worktrees/ct-language-audit-plan`, `2ea9b0da`, with its two existing untracked CT audit/plan documents.
- User-owned sidecar worktree retained unchanged: `/home/murphy/source/julie-semantic-sidecar/.worktrees/user-relief-2026-08-11`, `35c8f13`, clean with unrelated unmerged work.
- Miller registry prune was previewed only: 33 candidates and six unconfirmed linked-worktree retirements, unrelated to this run. No registry rows or worktrees were removed.

## Next steps requiring coordination

- Review/integrate the local J1 branch and the separate S1 evidence correction under the normal approval rules.
- M1 is the next implementation plan. Use the qualified producer contract/build and coordinate its pin; do not infer a published 2.40.0 release from the development version.
- Preserve the existing M1 → M3 dependency and the remaining M2–M5 program scope. No new MCP tool or fleet semantic service is authorized.
