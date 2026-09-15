# Autonomous run report: Remove the versioned family store and JSONL export (3.0.0)

Status: Awaiting publication approval
Plan: docs/plans/2026-09-15-remove-store-and-jsonl.md
Branch: feat/remove-store-and-jsonl (worktree .worktrees/remove-store-and-jsonl), base main d75b9f8f
Tasks: 7 of 8 complete (task 8, the release, needs user approval)
PR: none. This repo releases by fast-forward merge to main from the primary checkout per docs/release.md.

## What shipped

- ADR docs/decisions/2026-09-15-store-and-jsonl-retirement.md.
- `store` command, CLI store executor, 13 CLI store tests, `windows-capacity-store` CI job, store docs, `fs4` dependency removed.
- Artifact-crate `store` modules, 22 store tests, `MILLER_*` env vars, `getrandom`/`rustix`/`winsafe`/`machine-uid` removed.
- `export` command, `jsonl.rs`, JSONL contract docs and tests, export report fields, xtask dogfood/performance JSONL metrics removed.
- xtask release manifest and release/testing docs cleaned.
- Miller renamed to code-kb or "the consumer" in 183 files; 154 "(Miller bridge ...)" labels deleted.
- Crates bumped to 3.0.0; docs/release-notes/v3.0.0.md written.
- About 73,000 lines deleted.

## Publication authority

- local_commit_authority: authorized by the approved plan (each step is one commit).
- push_authority: missing. User CLAUDE.md: never push without approval.
- pr_authority: not applicable; repo releases via main.
- release_authority: missing. Plan step 8 is the release.

## Judgment calls

- JSONL source-language tests (`jsonl_pipeline.rs`, `jsonl_invariants.rs`, the real-world jsonl fixture) stay: they test the JSON extractor on `.jsonl` inputs, not the export. Plan listed them wrongly.
- xtask dogfood/performance JSONL metrics moved into step 4 because deleting `jsonl.rs` breaks the xtask build otherwise.
- Step 2 deleted the CLI watchdog `process_status` wrappers (only caller was the store), so step 3 relocated nothing.
- Step 4 also removed `ReportInput.format`/`output_path` and the `ReportStream` enum: export-only. Report schema stays 3.
- Step 4 left `structural_fact_registry.rs` reading the deleted `jsonl-v3.md`; the extractor lib tests were red at c87d4479 and fixed in step 6.
- Release note uses `classification: breaking` (plan wording) plus `artifact output: compatible`.
- Two site mentions keep Miller as the tool that measured the 2.2x benchmark; renaming it would be a false claim.
- Steps 5 and 6 ran in parallel (disjoint files); step 6 committed by the lead.

## Tests

- Linux at 60111576: fmt, clippy all-features -D warnings, `cargo xtask test default` (85 s under contention), `cargo xtask test contract` (86 s), doc sync, quality report: all pass.
- Windows NTFS via win-test at 60111576: `cargo xtask test default` pass; `cargo xtask test contract` see ledger.
- code-kb suite (223 tests) passes against the 3.0.0 source build with only the pin changed, in a scratch worktree that was removed afterwards.
- Timing: default tier warm 66 s before, 14 s after.
- Security scope: none declared in the plan.

## Blockers hit

None. Approval boundary reached for merge, push, and release.

## Source control

- Worktree .worktrees/remove-store-and-jsonl on feat/remove-store-and-jsonl: 8 commits ahead of main, clean.
- Primary checkout main at d75b9f8f: clean except the untracked plan file, which is now committed on the branch.
- Other worktrees (user's): ct-language-audit-plan (two untracked docs), cursor-command-cost, maintainability-cleanup, reader-retention-contract, wal-recurrence: all merged, untouched.
- code-kb repo on fix/extractor-seam, clean; scratch worktree and branch removed.

## Next steps

1. Approve: fast-forward main to the branch in the primary checkout, push main, wait for source CI, dispatch Release Binaries 3.0.0, verify assets, write docs/release-evidence/2026-09-15-v3-0-0-release.md, advance the docs/release.md pointer.
2. Bump code-kb's pin: scripts/julie-pins.json (version and four sha256), PINNED_JULIE_VERSION in crates/code-kb-core/src/sync.rs, CLAUDE.md/AGENTS.md text.
