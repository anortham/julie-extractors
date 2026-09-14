# Maintainability cleanup verification

Status: complete locally.
Plan: docs/plans/2026-09-14-maintainability-cleanup.md
Branch: refactor/maintainability-cleanup
Worktree: /home/murphy/source/julie-extractors/.worktrees/maintainability-cleanup
Verified implementation: 3a9f34e68437d12dd12df5e484569b55953845be
Base: 460fd1a1f5b0ab7a266cc8596c800f9c5ff83fd1

## Changes

All six audited cuts were implemented across five parallel Terra tasks and reviewed by the lead.

- Removed the retired reference-resolution report generator and its test. Kept historical JSON and corrected active documentation/metadata.
- Derived code structural pattern IDs from existing definitions through an iterator without a new allocation.
- Shared identical data/web/SQL structural-fact construction helpers. Preserved the SQL language adapter and web identity-aware builder.
- Removed the test-only serial extraction implementation. Existing incremental spool coverage retains equivalent assertions; the force test now exercises the production spool pipeline.
- Used Serde serialization for existing capability kind-coverage structs instead of manual JSON mapping.
- Shared the artifact writer bulk-load wrapper while preserving poison, eligibility, timing, and committed-error restoration rules.

Net reduction: 828 lines across code and tests, including deleted scripts. No dependency changes.

## Compatibility evidence

- Shared fact_for_node, fact_for_span and base_metadata body hashes match the original builders.
- All per-language capability JSON is identical to the base. Only the retired global canonical_coverage metadata block was removed.
- Existing CLI contracts check emitted coverage against fixture values; fingerprint construction hashes those same values.
- Full golden/contract suite passed without modifying expected outputs.
- No changes to schema definitions, extraction-contract version, identity epoch, package manifests, or Cargo.lock. Package version remains 2.42.1 until release preparation.
- Exact tracked-text checks found no retired-script invocation in this repo automation or the available /home/murphy/source/miller, julie, and code-kb checkouts. Historical references in prior plans/release evidence remain intentional.

## Verification

| Scope | Command | Result |
| --- | --- | --- |
| Baseline | cargo check -p julie-extract-artifact -p julie-extract-cli | Passed |
| Format | cargo fmt --check | Passed |
| Metadata | cargo metadata --format-version 1 | Passed |
| Build/test tooling | cargo test -p xtask | 119 passed |
| Default | cargo xtask test default | 4,870 passed; 124 seconds |
| Contract | cargo xtask test contract | 381 passed, including goldens, capability checks, downstream smoke, SQLite/JSONL/CLI, crash recovery, and store equivalence |
| Quality ledger | node scripts/language-data-quality-report.mjs --strict | silent_cells=0; quality_bar_debts=0 |
| Windows writer | cargo test -p julie-extract-artifact --test writer_contract | 51 passed |
| Windows force scan | cargo test -p julie-extract-cli --lib force_scan_ignores_existing_hashes_and_extracts_all_supported_files | 1 passed |
| Windows incremental scan | cargo test -p julie-extract-cli --lib incremental_scan_can_spool_supported_files_without_parser_work_for_unchanged_files | 1 passed |

Additional focused worker evidence: 226 structural-fact tests, 2 writer batching tests, and writer unit coverage passed. Linux logs: /tmp/julie-maintainability-default.log, /tmp/julie-maintainability-contract.log, /tmp/julie-maintainability-xtask.log. Windows ran the verified implementation commit on NTFS through win-test; logs are ~/.local/share/win-test/logs/20260914T142604Z-julie-extractors-3896466.log, ~/.local/share/win-test/logs/20260914T142641Z-julie-extractors-3899793.log, and ~/.local/share/win-test/logs/20260914T142753Z-julie-extractors-3913858.log.

## Review and source control

Lead review caught and corrected an orphan cfg(test) attribute after the serial helper deletion. The combined CLI build and tests then passed. No unresolved findings. No external model review or separate security/dependency audit was selected; security scope was none declared.

Implementation approval authorized local commits. No push, PR, deployment or release was requested or performed. The implementation stays on the named branch/worktree. The final follow-up commit contains only memory/verification notes, so the tested source remains unchanged.

The main checkout and the existing cursor-command-cost, reader-retention-contract and wal-recurrence worktrees are clean. The existing .claude/worktrees/ct-language-audit-plan worktree still has its two pre-existing untracked audit/plan documents; they were not changed. Worker memory files created in the main checkout were moved into this task worktree and included in local commits.
