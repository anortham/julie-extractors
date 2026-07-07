# Autonomous Run Report — Workspace Reference Resolution

**Status:** COMPLETE + post-plan perf fixes + Codex review fixes — awaiting user push/PR decision (approval boundary)
**Plan:** `docs/plans/2026-07-06-workspace-reference-resolution-implementation-plan.md`
**Branch:** `feature/workspace-reference-resolution` (base `main` @ `2d76908`)
**Tip:** `85eb8d6` · tree clean
**Tasks:** 8/8 complete · **Phases/batches:** A(1,2) · B(3,4) · C(6,7) · serial 5 & 8, all done
**Execution:** razorback:subagent-driven-development (parallel implementers, lead inline review)

## Post-plan follow-ups (after the 8 tasks, per user requests)

**1. Both Task 7 perf findings fixed** (`2ae8029`, release-notes `8f13c66`) — user asked for both before finishing:
- FINDING 1 (delta O(workspace)): `resolve_workspace` scopes the identifier locator + covered-set load to `delta_scope_files` (changed files ∪ by-name-worklist-hit files); index stays whole-workspace. Delta **110 ms → 83 ms — now MEETS the 100 ms target**. Cross-file demotion/uniqueness tests still pass (caller file enters scope via by-name worklist).
- FINDING 2 (var-limit): 5 by-names/by-files worklists chunk their `IN()` binds via `chunked_by` (dedup + re-sort). Huge-delta probe resolves at all scales to 80k (was graceful error ≥16384). Perf harness huge-delta test refocused to a chunking regression guard; delta ceiling 175→150 ms.

**2. Codex review + 3 correctness fixes** (`b15d983`, release-notes `85eb8d6`) — user ran Codex on the branch; it found and fixed 3 items, each with a regression test:
- Stale resolved overlays survived a Full pass → `resolve_full` now re-checks all resolved pending/identifiers workspace-wide (`recheck_resolved_*` + `worklist_resolved_pending`/`worklist_resolved_identifiers`) and demotes stale ones (test `incremental_scan_demotes_uniqueness_regression_from_skipped_file`).
- Aliased tier-2 imports over-resolved (matched any same-named symbol) → `load_import_records` reads import `source`, resolves relative specifiers to a concrete file; unresolvable module ⇒ no resolution (test `tier2_aliased_import_requires_resolved_source_module` + `tier2_missing_module_alias` fixture).
- `resolution_report` emitted duplicate rows → `SUM(cnt) GROUP BY` (test `resolution_report_aggregates_by_language_tier_outcome`, count==2 one row).

**Post-fix gates (tip `85eb8d6`):** default 22/0 · clippy 0 (incl `--features test-perf`) · fmt clean · perf (release, `--test-threads=1`) Full 1256 ms, Delta 83 ms (meets target), chunked delta to 80k · CLI resolution+operations+store contracts green.

## What shipped

A workspace-level reference-resolution pass that fills `identifiers.target_symbol_id` and
resolves pending relationships into edges via a deterministic, tiered, confidence-stamped pass.

| Task | Commit | Summary |
|---|---|---|
| 1 Schema v4 + storage | `901c68c` | `pending_resolutions` + `identifier_resolutions` overlay tables (CHECK, FK CASCADE/SET NULL, indexes); `resolution_store` atomic primitives; metadata keys |
| 2 Pending spans + IDs | `376f211` | `PendingSpan` on pending relationships (byte-identical to identifier spans); occurrence-distinct pending IDs |
| 3 Writer hook seam | `356a3fe` | Non-escaping HRTB `FnMut` hook in all 6 mutating paths, inside the tx, SAVEPOINT-guarded non-fatal contract; row-domain handoff |
| 4 Resolver tier chain | `39b584e` | Pure `resolve_one` tiers 2–4 (same-lang, typed, exactly-one, tier-2 allowlist); 28 unit tests |
| 5 Workspace pass wiring | `6480d30` | `resolve_workspace` hook at all 4 write sites; Full/Delta + demotion; span propagation; v3 backfill; status metadata; per-lang/tier report; capability gaps; dogfood gate |
| 6 Contract fixtures | `b911dce` | 25 per-language fixtures + parity guard (proven to bite); **lead fix: `import_binding` camelCase `importedName`** — aliased TS/JS imports now resolve tier 2 |
| 7 Performance gate | `515f049` | `test-perf` harness times real `resolve_workspace` @92k ids via minimal `src/lib.rs` seam + CLI convention guard |
| 8 Contracts/notes | `5c69f41` | `sqlite-schema-v4.md` contract, `capabilities.json` reference_resolution block, hardened strict gate, `v2.9.0.md` release notes |

Lead fixes: `9670030` (xtask dogfood schema-v4 tripwire — Task 1 rollout miss caught by the gate) · `bf3467c` (clippy sweep, 0 warnings).

## Real-repo evidence (dogfood, exit 0)

- `ResolutionFailed` gate clean; scan status ok. **4,618 pending + 62,189 identifier resolutions; 38.7% resolved across 66,807 outcomes, 30+ languages.** Rust tier-1 8,479 + tier-4 17,094; JavaScript tier-2 imports firing; C# tier-3 receiver-typed. Incremental rescan (0 changes) 550 ms.

## Tests / gates

- `cargo xtask test default` GREEN at tip (22 suites, 0 failures).
- `cargo clippy --workspace --all-targets`: 0 warnings.
- `node scripts/language-data-quality-report.mjs --strict`: 36 languages, silent_cells 0, quality_bar_debts 0, exit 0 (gate now also fails on debts, bite-proven).
- Dogfood: real exit 0 on the julie-extractors repo (720,919 records).
- Perf (release, 92k ids): Full **1,212 ms** (<2s); single-file Delta **~110 ms**.

## Judgment calls / lead decisions

- **Two Task 7 perf findings — initially deferred, then fixed at user request** (see Post-plan follow-ups above): (1) single-file Delta 110 ms → 83 ms via delta-scoped locator/covered load; (2) by-names worklists now chunk their binds, so huge deltas resolve instead of hitting the SQLite var limit. The v2.9.0 Known Limitations were updated to drop these and keep only the genuinely-remaining items (full pass is O(workspace); conservative per-language tier coverage).
- **Task 8 FINDING 1 (worker):** static `reference_resolution` gap entries in capabilities.json would collide with the runtime `capability_snapshot.rs` rows on the `language_capability_gaps` PK → used a doc-only top-level block; machine honesty stays runtime rows + parity guard.
- **History note:** `4d89a4a` (mine) overclaims in its message but contains only plan.md ticks — task8's own `5c69f41` holds the Task 8 deliverables (a serial-commit race; git dedup'd, tree correct). Squash-merge collapses this; no rewrite attempted (unpushed, `rebase -i` unavailable here).

## Blockers hit

None. All gates green.

## Next steps (need user decision — approval boundary)

Push + PR / local merge / keep as-is. Not auto-performed: CLAUDE.md requires explicit approval before push/release. **Both original Task 7 perf findings are now fixed (not deferred), and the Codex review is done with all 3 findings fixed** — the branch is review-clean and gate-green; nothing outstanding blocks the finish decision.

## Files changed

65 files, +9,314 / −103 vs `2d76908` (includes the two post-plan follow-ups above).
