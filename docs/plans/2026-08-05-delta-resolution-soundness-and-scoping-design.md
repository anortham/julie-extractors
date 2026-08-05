# Delta resolution: soundness first, then scoping — design — 2026-08-05

**Status:** design, awaiting approval. Two changes in one sequence, in this order: fix the incremental
resolution path's soundness, then let whole-repo scans use it.

**Provenance:** Miller's worktree delta-rebind program needed a cost model and got a surprise —
[`miller/docs/findings/2026-08-05-rebind-p1-cost-model.md`](../../../miller/docs/findings/2026-08-05-rebind-p1-cost-model.md).
Measuring the scan path turned up both a large avoidable cost and a live correctness defect, and they
are the same code path.

## The two findings

**1. Whole-repo scans never scope resolution.** Both whole-repo write sites construct the resolution
scope with `is_full_scan: true` hard-coded (`crates/julie-extract-artifact/src/writer.rs:1087`,
`:1390`) while already computing and passing `changed_file_ids` at those same sites (`:1085`, `:1388`).
The hook then takes the whole-workspace branch
(`crates/julie-extract-cli/src/resolution.rs:1551`). Measured on a 1,397-file / 121k-symbol artifact
with 2.25.0, `--jobs 4`:

| Operation | Total | resolution | extraction |
|---|---:|---:|---:|
| Full build from scratch (bulk path) | 16.40 s | 4.66 s | 4.32 s |
| Whole-repo scan, 1 changed file | 14.40 s | **13.97 s** | 32 ms |
| `update` verb, same 1 changed file | 2.56 s | — | — |

A one-file whole-repo scan resolves for **3× what the full rebuild spends** on the identical corpus,
because a populated artifact is also permanently ineligible for the bulk path
(`artifact_is_unwritten` requires zero files *and* zero revisions, `writer.rs:1639-1646`).

**2. The incremental path is unsound today, and the whole-repo full pass is what hides it.**
The delta worklists match on `pending_relationships.target_terminal_name` and `.target_receiver`
(`resolution_store.rs:767`, `:811`), but two resolution tiers key on names those columns never carry.

Reproduced end-to-end with the pinned 2.25.0 binary — `a.ts` holds
`import { realName as localName } from './b'; localName();`, then `b.ts` gains
`export function realName()`:

| Path after the edit | `pending_resolutions` resolved | identifier target |
|---|---:|---|
| `update --file src/b.ts` (delta) | **0** | **NULL** |
| whole-repo `scan` (full) | 1 | `5714479fab41ca12…` |

Tier 2 gates on `import.local_name == terminal_name` then looks candidates up by
`import.imported_name` (`resolution.rs:1045-1052`); the touched-name set carries only the changed
file's own symbol names (`writer.rs:1244`), so `realName` never surfaces the row keyed on `localName`.
**Miller drives steady-state indexing through `update`** (`JulieExtractRunner.cs:619-637`), so this is
live: aliased-import references stay unresolved until some whole-repo scan happens to repair them.

Three further defects of the same shape were confirmed by execution in the investigation and are
carried into the test plan below: tier-3 receivers keyed through `type_facts.resolved_type`
(`resolution.rs:1085-1112`); NULL-target identifiers never retried because both delta fill arms are
never-attempted-only (`resolution_store.rs:868`, `:892` vs the full arm's
`r.target_symbol_id IS NULL` at `:960`); and module specifiers re-pointing when a file is added or
removed (`resolve_import_module_file`, `resolution.rs:2251`).

## Why the order is forced

Scoping the whole-repo scan removes the only automatic pass that re-derives the whole overlay on a
**live** artifact — Miller's force scans always build into a fresh sibling `symbols.db.rebuild` where
`prior.is_none()` forces Full anyway (`JulieExtractRunner.cs:477-486`). So scoping first would convert
four self-healing defects into permanent ones, and the artifact would report `partial`/`ready` while
doing it. Soundness first is not tidiness; it is the precondition.

## The change

### Phase A — make the delta path sound (ships on its own merit)

1. **Widen the touched-name reach to the two name-blind keys.** Two new accessors on
   `WorkspaceCandidateIndex` (`resolution.rs:351-359`), both reading data `load_index` already loads
   whole-workspace, so no extra SQL:
   - `files_declaring_type_named(&names)` — files whose `type_facts.resolved_type` is a touched name.
   - `files_importing_names(&names)` — files importing a touched name under either
     `imported_name` or `local_name`.
   Union both into `delta_scope_files` (`resolution.rs:2604-2627`), which must take `&index` and whose
   caller at `:1566` passes the index built at `:1556`. Widening *inside* `delta_scope_files` is
   load-bearing: `scope_files` is the set the locator and covered-set are built from, and a file
   outside it makes `IdentifierLocator::locate` return `None` (`:2709`) and silently drops co-location.
2. **Add the two missing by-files worklists** in `resolution_store.rs`, each via the existing
   `chunked_by` helper (`:727`): `worklist_resolved_pending_in_files` and
   `worklist_resolved_identifiers_in_files` — the latter deliberately *without* the
   `target_symbol_id IS NOT NULL` filter, matching the by-names recheck arm.

   *Revised during implementation.* The plan called for a third worklist,
   `worklist_unattempted_identifiers_in_files` (`r.identifier_id IS NULL OR r.target_symbol_id IS
   NULL`), to close the frozen-NULL-target class. It was not needed: dropping the `IS NOT NULL` filter
   from the RECHECK arm already readmits every ambiguous/missing/no_context row in a scoped file, which
   is the same set. `restored_receiver_type_uniqueness_matches_a_full_rederivation` is the case that
   pins it — it fails on unmodified `main` and passes here. A third worklist would have been a second
   path to the same rows.
3. **Use them in `resolve_delta`** (`:1699-1806`), merging by-names and by-files results
   de-duplicated and re-sorted on the primary key, the same discipline `chunked_by` already uses.
4. **Widen by module-candidate PATH, not only by name.** A third accessor,
   `files_importing_module_candidates(&changed_paths)`, unions in every file whose relative specifiers
   could bind to a path this write created or deleted. This is not a name-keyed union and cannot be
   folded into one: module selection turns on which candidate path EXISTS
   (`import_module_candidates` / `select_module_file`), so adding `src/util.ts` over `src/util/index.ts`
   re-points `./util` for every importer while changing no symbol name any importer references. The
   defect is only invisible when the shadowing file happens to export the same binding — which is
   exactly what the first `module_shadowing_applied_by_a_delta` fixture did, and why the gate passed
   while the class was still open.

   Changed PATHS (not ids) are the key, and a deleted file's `files` row is already gone when the hook
   runs, so `changed_file_paths` reads `files` for survivors and this revision's `revision_file_changes`
   rows — written by the writer ahead of the hook, in the same transaction — for the removed ones.

   This is **not** subsumed by Phase B's `structure_changed` condition below. That condition promotes
   whole-repo *scans* to Full; the `update` and `delete` verbs are the delta path Miller drives in
   steady state (`JulieExtractRunner.cs:619-637`) and never reach it.

### Phase B — let whole-repo scans scope (the speed win)

4. **Split the overloaded boolean.** `ResolutionScopeInput` (`writer.rs:173-178`) gains
   `whole_corpus: bool`. `is_full_scan` comes to mean only "resolve the whole workspace" (the
   resolver's dispatch switch); `whole_corpus` means "this write hash-checked every file in the
   workspace". Today one flag drives four unrelated decisions (`resolution.rs:1551`, `:1561`, `:1576`,
   `:1601`), which is why a naive flip would also freeze `reference_resolution_last_full_revision` and
   pin `status` to `partial` forever.
5. **Scope conditionally at the one production site** (`write_scan_spooled_snapshot_in_mode`, before
   `:1387`):
   ```
   structure_changed = !deleted.is_empty() || planned_files.values().any(|e| e.is_none())
   is_full_scan = structure_changed || revision.mode == Some(WriteMode::Force) || crossover
   whole_corpus = true
   ```
   Leave `write_scan_snapshot_in_mode` (`:1085`) alone — its only non-test caller is
   `xtask/src/performance.rs:408` with a hookless writer.

**Why each condition:**
- *No file added or deleted.* `file_id = stable_id("file", [&path])` (`extraction.rs:295`), so a pure
  rewrite cannot change any `module_file_id` — which closes the module-re-point class structurally
  rather than by argument.
- *Not `WriteMode::Force`.* Force skips nothing (`writer.rs:1198`), so the real delta scope already
  *is* the whole workspace; scoping there funnels workspace-sized work through chunked `IN` clauses,
  strictly worse.
- *Crossover.* If the widened `scope_files` covers a large fraction of the workspace, promote to Full.
  Decided inside `run_resolution` rather than at the writer, because `scope_files` is not known until
  `delta_scope_files` has run — the writer cannot see the number the decision turns on. Threshold is
  0.6, measured by T6 rather than guessed; it only ever converts a scoped pass into a Full one, so an
  over-conservative value costs time, never correctness.

**T6 measured (release, 2,000-file / 92k-identifier seed, `FULL baseline 648 ms`):**

| Changed files | Share of corpus | Scoped | vs Full |
|---:|---:|---:|---:|
| 1 | 0.1% | 175 ms | 0.27× |
| 50 | 2.5% | 203 ms | 0.31× |
| 500 | 25% | 372 ms | 0.57× |
| 1000 | 50% | 555 ms | 0.86× |
| 1200 | 60% | 630 ms | **0.97×** |
| 1400 | 70% | 760 ms | 1.17× |
| 2000 | 100% | 1006 ms | 1.55× |

The crossing sits between 60% and 70%, so the threshold is 0.6 — the last measured point where scoping
still wins.

**The sweep has to disable promotion to measure this at all.** With the crossover live, every point
past the shipped threshold times a Full pass and reports it as the scoped cost, so the sweep can only
rediscover the number it was given — an earlier reading "confirmed" 0.5 that way, and the honest curve
shows 50% is still 0.86×. `resolve_workspace_with_crossover` exists for that, and is not a tuning knob.

The sweep asserts the shipped constant never promotes LATER than the measured crossing (verified to
fail at 0.8). It deliberately does not assert the converse: promoting early is safe, so a value that
drifts conservative should not break a build.

**Measured, Phase A vs Phase B, whole-repo scan of a 2,201-file / 124k-symbol / 120k-identifier TS
fixture, 1 file rewritten, release build:**

| Changed file | Phase A (always Full) | Phase B | |
|---|---:|---:|---|
| A leaf — no widely-shared names | 12.20 s | **2.88 s** | 4.2× faster |
| One importing a hot shared module | 12.36 s | 13.26 s | 7% slower |

The second row is the design working, not failing. Every file in that fixture imports the same 60
names, so `files_importing_names` widens the delta scope to the whole workspace; the crossover sees
that and promotes to Full. The 7% is what it costs to compute the delta scope before discovering it
was workspace-sized — the alternative, resolving a workspace-sized set through the scoped path, is
strictly worse. It also bounds how bad Phase B can get: a repo where every scan touches a hot shared
import pays ~7%, it does not regress to something pathological.

**The 13.97 s resolution figure in "The two findings" above did not reproduce.** On this machine a
whole-repo scan of Miller's own artifact with zero changed files short-circuits before the hook (the
overlay's `last_full_revision` does not advance), so it measures discovery, not resolution. The
numbers above come from a fixture where resolution provably ran. Treat the original table as
unverified until it is re-measured with a changed file.
- *`prior.is_none()` keeps forcing Full*, untouched — that is what keeps v3 backfill and Miller's
  rebuild-and-promote path on the Full branch.

## Tests, written first

- **T1 — the equivalence oracle. None exists in the repo today; it is the acceptance gate.** New
  `crates/julie-extract-cli/tests/resolution_scope_equivalence.rs`. Helper copies the artifact, clears
  the overlay (`DELETE FROM pending_resolutions; DELETE FROM identifier_resolutions;
  UPDATE identifiers SET target_symbol_id = NULL;`), re-runs resolution at full scope in-process, and
  compares both overlay tables plus `identifiers.target_symbol_id`, ignoring `resolved_at_revision`.
  Re-deriving over the *same rows* isolates the property under test from extraction-level differences a
  from-scratch comparison would confound. **A T1 failure is a design defect, never a test to relax.**
- **T2** — the four confirmed shapes, one named test each, each asserting the concrete target id equals
  the fresh-scan answer: tier-3 uniqueness regression; uniqueness restoration by rewrite *and* by file
  deletion (the latter must take the Full fallback); aliased tier-2 fill (the repro above); module
  re-point behind a new shadowing file (must take the Full fallback).
- **T3** — existing regression floor stays green unchanged: `operations_contract.rs:2647`, `:2572`,
  `:2618`, `:2435`, `:2474`, `:2676`; `resolution_contract.rs:769`.
- **T4** — scope contract in `writer_contract.rs`: pure rewrite ⇒ `!is_full_scan && whole_corpus`;
  path inserted ⇒ `is_full_scan`; path deleted ⇒ `is_full_scan`; Force ⇒ `is_full_scan`.
- **T5** — reported state, a real coverage hole today: after a whole-repo scan on a pure-TS fixture,
  `status == complete` and `last_full_revision` advanced; after a single-file `update`, it did not.
- **T6** — perf arm in `resolution_perf.rs` (behind `test-perf`) at N = 1, 50, 500 changed files, which
  also sets the crossover threshold. Must live in that file:
  `crates/julie-extract-artifact/tests/test_tiers.rs:35` fails the build on an ungated `Instant::now()`.
- **T7** — unit coverage for `delta_scope_files` widening and the three new worklists, none of which
  have direct tests today.

## Risks

- **The soundness premise is argued, not proved.** It is "an overlay outcome changes only if some name
  the widened sweep matches also changed." Two keys are closed by execution evidence and one
  structurally; T1 is the only thing standing between this design and a silently path-dependent
  artifact.
- **Phase A slows the shipped single-file path.** The extra recheck arms eat into
  `resolution_perf.rs:272`'s 150 ms release ceiling. Intended trade: it buys correctness on four live
  defects.
- **Do not promise 2.5 s.** `load_index` stays whole-workspace on both branches
  (`resolution.rs:1553-1556`) and is a hard floor; the widened `scope_files` is strictly larger than
  `changed_file_ids`. T6 measures the real ceiling before any number is quoted.
- **`structure_changed ⇒ Full` is coarse.** Provably safe and free on the edit-save loop, but the win
  never materialises in a repo that adds a file on most scans. The precise alternative — recompute
  `resolve_import_module_file` and widen by imports whose `module_file_id` actually changed — is a
  follow-up once T1 can prove it equivalent.
- **Scale is unmeasured.** Every number here is from a 1,397-file repo on macOS/APFS.

## Open question for the release decision

Phase A is a correctness fix to shipped behaviour and could go out on its own; Phase B is the
optimisation that motivated the work. They can ship together or as two releases. Either way the Miller
pin bump is a separate, explicitly approved step.
