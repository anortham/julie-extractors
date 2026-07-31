# Task 5 — Oversized-transition policy (Batch A)

- Status: COMPLETE (no commit — parallel-lead-commit)
- Worktree: `/Users/murphy/source/julie-extractors/.worktrees/erlang-xml-language-support`
- Branch: `erlang-xml-language-support`, HEAD `964f2ba` (unchanged; edits left in working tree)
- Toolchain: `RUSTUP_TOOLCHAIN=1.97.1`

## Policy change

A tracked file that grows past `MAX_SOURCE_FILE_BYTES` (1 MiB, unchanged) now has its artifact rows
removed instead of preserved, on BOTH paths, and keeps the existing `slow_file_skipped` disposition.
No new report fields, no new status values, no message churn — `slow_file_skip_message()` is
byte-identical for `scan` and `update` as before.

| Path | Before | After |
|---|---|---|
| `update` on tracked file that became oversized | `status: no_change`, rows preserved, no writer opened | `status: unsupported`, rows removed, `files_deleted: 1`, `files_unsupported: 1`, warning `slow_file_skipped` |
| `scan` encountering a tracked-but-now-oversized file | rows preserved (path was in `preserved_missing_paths`) | rows removed via the existing missing-file deletion mechanic, `files_deleted: 1`, warning `slow_file_skipped` |

Unchanged: files at exactly 1 MiB index normally; 1 MiB + 1 is skipped; the oversized skip still
blocks a reference-resolution upgrade (`schema_migration_required`, exit 3); untracked oversized
files still produce no rows.

## Implementation

`crates/julie-extract-cli/src/commands.rs`

1. `scan` (:161) — dropped `.chain(discovered.slow_file_skips.iter())` from `preserved_missing_paths`.
   Oversized paths now fall through the writer's existing missing-file deletion, exactly like a file
   that disappeared from disk. `slow_file_skips` is still used for the warnings and for
   `has_upgrade_source_gaps`, so the upgrade-blocking behavior is untouched.
2. `update` — deleted `skip_oversized_update` (the no-writer path) and routed the
   `UnsupportedReason::Oversized` branch through the existing unsupported-cleanup path, now named
   `cleanup_skipped_update` and parameterized by a new `UpdateSkipReason { IgnoredOrUnsupported,
   Oversized }`. The only difference between the variants is the diagnostic built by
   `update_skip_diagnostic`: `unsupported_file` (with its rows-removed / no-rows message pair) vs
   `slow_file_skipped`. Row removal reuses `delete_artifact_rows(..., WriteOperation::Update,
   RevisionChangeKind::Unsupported)` — the same call the ignored/unsupported path already made.
3. Import: added `ReportDiagnostic` to the `julie_extract_artifact::reports` use list.

No new seams: no new module, no new writer API, no new report field. The change reads as
"oversized transition = the existing unsupported cleanup with a `slow_file_skipped` diagnostic".

### Decisions taken (plan-consistent, noted for the lead)

- **`status: unsupported` for the oversized `update` path** (`commands.rs:1516`). Discovery already
  classifies an oversized file as `FileSelection::Unsupported { reason: Oversized }`, and
  `docs/contracts/reports.md` already states "Unsupported or ignored files return `status:
  unsupported` and exit `0` after stale rows for the path are removed." Reusing that status keeps
  the vocabulary closed. `no_change` was no longer truthful once rows are deleted. The warning code
  stays `slow_file_skipped`, so callers can still tell an oversized skip from an ignore — the
  existing assertion that `unsupported_file` never appears on this path is preserved.
- **Uniform status when there are no rows to remove.** An oversized `update` against an artifact
  that has no rows for the path now also returns `unsupported` (previously `no_change`), matching
  the ignored/unsupported path, which returns `unsupported` in both the rows-removed and no-rows
  cases. One status per disposition rather than two.
- **Revision change kind.** `update` records `RevisionChangeKind::Unsupported` (file exists, not
  extractable); `scan` records the removal through its missing-path mechanic (`Deleted`). Both are
  existing vocabulary; no new kind was added. Mild asymmetry, inherited from the two mechanics.
- **`limits.rs` untouched.** The constant and the warning text are unchanged, so its doc comments
  are still accurate.

## Tests — `crates/julie-extract-cli/tests/operations_contract.rs`

Changed (authorized by the plan):

- `scan_preserves_existing_rows_when_source_file_becomes_oversized` →
  `scan_removes_existing_rows_when_source_file_becomes_oversized`: asserts `files_deleted: 1`, zero
  rows in `files`/`symbols`/`identifiers` for the path, the sibling file untouched, and that a
  second scan converges to `no_change` with `files_deleted: 0` (no revision thrash).
- `update_oversized_supported_file_preserves_rows_and_reports_slow_file_skipped` →
  `update_oversized_supported_file_removes_rows_and_reports_slow_file_skipped`: `status:
  unsupported`, `files_unsupported: 1`, `files_deleted: 1`, warning `slow_file_skipped` on the right
  path, no `unsupported_file` code, zero rows across the three domains, sibling intact.
- `resolution_upgrade_remains_blocked_when_a_source_file_is_oversized` (:1799 pre-change): the
  `symbols == before` assertion became `symbols == []`. This is a direct consequence of the scan-path
  policy change, not an unrelated gate weakening — the test's actual subject (exit 3,
  `schema_migration_required`, `reference_resolution_status = failed`, blocked follow-up `update`)
  is unchanged and still asserted.

Added:

- `scan_indexes_a_source_file_at_exactly_the_size_limit` — 1 MiB exactly, indexed, symbol present.
- `scan_skips_a_source_file_one_byte_over_the_size_limit` — 1 MiB + 1, `files_unsupported: 1`,
  `slow_file_skipped`, no `files` row.
- `update_indexes_a_source_file_at_exactly_the_size_limit` — boundary on the update path.
- `update_skips_a_source_file_one_byte_over_the_size_limit` — boundary on the update path.
- `update_reindexes_a_file_that_shrinks_back_under_the_size_limit` — transition out, rows gone, then
  shrink back and `update` re-indexes it (`regrown` symbol present).

New test helpers: `rows_for_path(db, table, path)` (per-path row count in any file-attributed
domain) and `rust_source_of_exact_size(symbol, size)` (a parseable Rust file of an exact byte
length, so the boundary tests derive their size from `MAX_SOURCE_FILE_BYTES` rather than hard-coding
it).

## Verification

- `RUSTUP_TOOLCHAIN=1.97.1 cargo test -p julie-extract-cli --test operations_contract -- oversized size_limit shrinks`
  → red first (5 failures on the old preserve behavior), green after implementation (9 passed).
- `RUSTUP_TOOLCHAIN=1.97.1 cargo test -p julie-extract-cli` (escalation gate, commands.rs changed)
  → **279 passed, 0 failed** across all targets (66 / 107 / 10 / 61 / 5 / 1 / 29 + 2 empty).
- `RUSTUP_TOOLCHAIN=1.97.1 cargo clippy -p julie-extract-cli --all-targets` → clean, 0 warnings.
- Extractor-side suites (`cargo xtask test language/golden/capability`) deliberately NOT run —
  Task 4 is mutating those files concurrently.

## Ownership note for the lead

Files touched:

- `crates/julie-extract-cli/src/commands.rs` (assigned)
- `crates/julie-extract-cli/tests/operations_contract.rs` (assigned; the Task 2/3 gap-count constant
  109 was not touched)
- `docs/contracts/reports.md` (assigned, conditional — it documented the old scan-only,
  rows-preserved wording)
- `docs/contracts/cli.md` (**outside the listed ownership set**) — this file documented the old
  behavior in four places (scan section, resolution-upgrade paragraph, the `update` outcomes list,
  and the data-loss-guard tradeoffs section). Leaving it would have made the CLI contract state the
  opposite of shipped behavior. It is a contract doc in the same family as `reports.md`, and Task 4
  is confined to `crates/julie-extractors/**` + fixtures, so there is no concurrent-edit risk. Flagged
  here for the lead to accept or revert.

`crates/julie-extract-cli/src/limits.rs` was not modified (limit and message unchanged).

## Concerns / plan mismatches

- None on the code path: `commands.rs:1445` matched the plan's description of the `no_change` path
  exactly, and the deleted-file removal path provided the mechanics as described.
- Consumer-facing behavior change worth calling out in release notes: `update` on an oversized file
  now returns `status: unsupported` instead of `no_change`, and both paths now emit a deletion
  revision. Any downstream consumer (Miller) that treated `no_change` as "artifact untouched" will
  now see a revision bump on this transition — which is the point of the fix.
