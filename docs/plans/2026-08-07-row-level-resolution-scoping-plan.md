# Row-Level Resolution Scoping Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when
> subagent delegation is available. Fall back to razorback:executing-plans for single-task,
> tightly-sequential, or no-delegation runs.

**Goal:** Replace the whole-file RECHECK arms of delta resolution with keyed row selection so
a one-file save re-resolves the rows its change can affect (~1.6% median) instead of every
row in every file containing a touched name (80–87% median, 16–18 s).

**Architecture:** `resolve_delta` (`crates/julie-extract-cli/src/resolution.rs:1852-1979`)
already runs each sweep as a by-names worklist (row-level) merged with an in-files worklist
over the widened `scope_files` (the amplifier). The redesign expands the **name set** through
the two keying relations that motivated the file unions — import aliases and receiver types —
and reuses the existing by-names worklists with the expanded set. The path-keyed module arm
cannot be name-keyed and keeps whole-file recheck over its small file set. No SQL shape
changes; no schema changes.

**Tech Stack:** Rust (toolchain 1.97.1 via `RUSTUP_TOOLCHAIN=1.97.1`; repo floor is 1.95),
rusqlite, xtask test tiers.

**Architecture Quality:** Approved shape: two new pure accessors on
`WorkspaceCandidateIndex` (name-set expansion), a narrowed file set for the in-files arms,
unchanged worklist SQL, unchanged tier logic, and a savepoint-based shadow mode in the
resolution entry point. Main risk: an outcome-affecting keying relation not covered by name
expansion — mitigated by the equivalence oracle, the hazard cases, and shadow mode on real
repos. If code reality contradicts this shape, report a plan mismatch; do not redesign
locally.

## Global Constraints

- **Byte-identical output:** for any corpus state, row-scoped delta resolution produces the
  same overlay rows (`pending_resolutions`, `identifier_resolutions`,
  `identifiers.target_symbol_id`) as the current file-scoped path. `RESOLUTION_VERSION`
  (`crates/julie-extract-cli/src/resolution.rs`, currently 6) MUST NOT change.
- **Old-name collection preserved:** `ResolutionScopeInput.touched_symbol_names` keeps
  unioning inserted names + OLD names collected before deletion
  (`crates/julie-extract-artifact/src/writer.rs:167-169`). The rename case (`Foo → Bar`)
  is a first-class equivalence case, not an afterthought.
- **No artifact schema change.** No new tables, columns, or indexes.
- **The equivalence oracle is never relaxed:** a `resolution_scope_equivalence.rs` failure
  is a design defect (2026-08-05 design, T1 rule).
- **The module re-point class stays file-keyed:** `files_importing_module_candidates`
  (`resolution.rs:531-542`) selects by PATH existence, not names; its file set keeps
  whole-file recheck.
- **Performance honesty:** `load_index` (`resolution.rs:2331`) stays whole-workspace and is
  a measured floor (2026-08-05: ~2.5 s at 120k identifiers). The measured target is the
  save-shape A/B in Task 5, expected ≥3× wall win on the Miller-repo shape; do NOT promise
  sub-second. Candidate-index narrowing is a named follow-up, out of scope here.
- Build gates: `cargo fmt --check`, `cargo test -p xtask`, `cargo xtask test default`,
  `cargo xtask test contract` — all with `RUSTUP_TOOLCHAIN=1.97.1`.

## Verification Strategy

**Project source of truth:** `docs/testing-strategy.md`, `docs/release.md`, xtask tiers
(`cargo xtask test default|contract`), perf tests behind `--features test-perf`.

**Worker red/green scope:** the specific test file the task adds or extends, e.g.
`RUSTUP_TOOLCHAIN=1.97.1 cargo test -p julie-extract-cli --test resolution_scope_equivalence`.

**Worker ceiling:** `RUSTUP_TOOLCHAIN=1.97.1 cargo xtask test default`. Workers do not run
contract or perf tiers on their own.

**Worker gate invariant:** each task's tests state the invariant in the test name and
assertion message; the worker report names what each gate proves.

**Lead affected-change scope:** `cargo xtask test default` after each merged task.

**Branch gate:** `cargo fmt --check` + `cargo test -p xtask` + `cargo xtask test default` +
`cargo xtask test contract`, plus the Task 5 perf evidence, before release prep.

**Replay/metric evidence:** the shadow-mode dogfood (Task 4) and save-shape A/B (Task 5) are
hard gates: zero shadow mismatches, and a measured wall-clock win on the save shape. The
resolution_perf 150 ms single-file release ceiling (`resolution_perf.rs`) is a hard gate;
sweep-derived crossover numbers are report-only.

**Escalation triggers:** any equivalence-oracle failure (design defect — stop and report);
any shadow mismatch on real repos; the save-shape A/B showing no win.

**Assigned verification failure:** workers stop and report; no gate is updated to pass
without a plan revision.

**Verification ledger:** record scope, invariant, command, commit, result, time in
`.razorback/sdd/progress.md` per razorback convention.

## Parallel Execution Contract

| Task | Parallel batch | File ownership | Serialization required | Dependency reason |
|---|---|---|---|---|
| Task 1: Equivalence + hazard harness | Batch A | Test: `crates/julie-extract-cli/tests/resolution_scope_equivalence.rs` | No | None - safe parallel batch. |
| Task 2: Name-set expansion accessors | Batch A | Modify: `crates/julie-extract-cli/src/resolution.rs` (WorkspaceCandidateIndex impl block only, `:488-543` region); Test: new `#[cfg(test)]` cases in the same file | No | None - safe parallel batch. |
| Task 3: The swap in resolve_delta | None - serial | Modify: `crates/julie-extract-cli/src/resolution.rs` (`delta_scope_files`, `resolve_delta`, `run_resolution`, `delta_scope_crosses_over`); Test: `resolution_scope_equivalence.rs`, `resolution_report_scope.rs` | Yes | Consumes Task 1's harness and Task 2's accessors; same-file conflict with Task 2. |
| Task 4: Shadow mode + dogfood | None - serial | Modify: `crates/julie-extract-cli/src/resolution.rs` (entry point), `crates/julie-extract-cli/src/main.rs` (env plumbing if needed); Test: new `crates/julie-extract-cli/tests/resolution_shadow.rs` | Yes | Shadows the Task 3 path against the legacy path; needs both in-tree. |
| Task 5: Perf proof + release prep | None - serial | Modify: `crates/*/Cargo.toml`, `Cargo.lock`, `docs/release-notes/`, `resolution_perf.rs`; probe scripts under Miller's `spike/` (separate repo, lead-run) | Yes | Needs Task 4's zero-mismatch evidence; release is user-approval-gated. |

## Tasks

### Task 1: Equivalence + hazard harness (tests first, green on main)

**Files:**
- Test: `crates/julie-extract-cli/tests/resolution_scope_equivalence.rs` (extend)

**Interfaces:**
- Consumes: the public `resolve_workspace` path and the oracle helper already in the file
  (copy artifact → clear overlay → full re-resolve → compare `pending_resolutions`,
  `identifier_resolutions`, `identifiers.target_symbol_id`, ignoring
  `resolved_at_revision`).
- Produces: named hazard cases the Task 3 swap must keep green. Case names (exact):
  `rename_rederives_old_name_rows_across_files`,
  `rename_captures_new_name_rows_across_files`,
  `aliased_import_recheck_reaches_local_name_rows`,
  `receiver_type_touch_rechecks_member_rows_in_unchanged_files`,
  `module_shadowing_repoint_survives_row_scoping`.

**Contract inputs:** the four confirmed hazard shapes from
`docs/plans/2026-08-05-delta-resolution-soundness-and-scoping-design.md` (T2) and the rename
constraint from `docs/plans/2026-08-07-row-level-resolution-scoping-brief.md`. Fixtures must
carry the extractor's REAL emitted metadata shapes (contract-faithful fixtures rule).

**File ownership:** Test: `crates/julie-extract-cli/tests/resolution_scope_equivalence.rs`

**Serialization required:** No

**Dependency reason:** None - safe parallel batch.

**What to build:** Five new equivalence cases exercising delta resolution through the public
path on multi-file fixtures, each asserting the oracle property (delta result == fresh full
re-derivation) for one hazard: a cross-file rename in both directions, an aliased import
whose row carries only the local name, a member reference whose recheck is reachable only
through the receiver's type, and a module re-point behind a new shadowing file. All five
MUST PASS on unmodified main (the file-scoped path is the reference behavior) — they pin
the bar the swap must clear, and they are the red/green harness for Task 3.

**Approach:** follow the file's existing fixture-building helpers; each case is a small
multi-file corpus (2–4 files) written through the real writer path, then one `update`-shaped
delta. Do not touch existing cases.

**Acceptance criteria:**
- [x] All five new cases pass on unmodified main (run before starting Task 3).
- [x] Each case fails if its hazard's recheck arm is deleted (verify by temporarily
      commenting the corresponding in-files merge in a scratch build — evidence in the
      worker report, not committed). (Lead-run: in-files arms fed empty slices → the three
      keyed-relation cases + 7 pre-existing arm-gating cases went red; renames stayed green
      via the by-names arms, as designed.)
- [x] Worker-scope verification passes and the change is committed per commit mode.

### Task 2: Name-set expansion accessors

**Files:**
- Modify: `crates/julie-extract-cli/src/resolution.rs` (WorkspaceCandidateIndex impl block,
  after `files_importing_module_candidates` at `:531-542`)

**Interfaces:**
- Consumes: `self.imports_by_file` (elements expose `local_name: String`,
  `imported_name: Option<String>` — see `:507-521`), `self.type_facts_by_symbol` +
  `self.symbol_by_id` (see `:488-499`).
- Produces (exact signatures Task 3 consumes):
  - `fn import_names_linked_to(&self, names: &HashSet<&str>) -> BTreeSet<String>` — for
    every import where EITHER side matches `names`, both `local_name` and `imported_name`
    (when present) enter the result.
  - `fn receiver_names_bound_to_types(&self, names: &HashSet<&str>) -> BTreeSet<String>` —
    for every `(symbol_id, facts)` where any `fact.resolved_type` matches `names`, the
    symbol's NAME (via `symbol_by_id`) enters the result.

**Contract inputs:** the two existing file-keyed accessors these mirror (`:488-521`); the
superset-is-safe rule (an over-wide name set costs time, never correctness).

**File ownership:** Modify: `crates/julie-extract-cli/src/resolution.rs`
(WorkspaceCandidateIndex impl block only, `:488-543` region); Test: new `#[cfg(test)]` cases
in the same file

**Serialization required:** No

**Dependency reason:** None - safe parallel batch.

**What to build:** Two pure accessors mirroring the existing file-keyed ones but returning
NAMES instead of file IDs, with unit tests beside the existing index tests covering: alias
both directions, absent `imported_name`, receiver facts keyed to a touched type, and empty
inputs returning empty sets.

**Acceptance criteria:**
- [x] Both accessors return exactly the keyed names on a hand-built index fixture.
- [x] No behavior change anywhere (accessors are dead code until Task 3).
- [x] Worker-scope verification passes and the change is committed per commit mode.

### Task 3: The swap — keyed rows replace whole-file recheck

**Files:**
- Modify: `crates/julie-extract-cli/src/resolution.rs` — `run_resolution` (`:1633`),
  `resolve_delta` (`:1852-1979`), `delta_scope_files` (`:2972`),
  `delta_scope_crosses_over` (`:2712`)
- Test: `crates/julie-extract-cli/tests/resolution_scope_equivalence.rs`,
  `crates/julie-extract-cli/tests/resolution_report_scope.rs`

**Interfaces:**
- Consumes: Task 1's harness (must stay green), Task 2's accessors.
- Produces: `delta_scope_files` splits its output into (a) `recheck_names: HashSet<String>`
  = `touched ∪ import_names_linked_to(touched) ∪ receiver_names_bound_to_types(touched)`,
  and (b) `recheck_files: Vec<String>` = `changed_file_ids ∪
  files_importing_module_candidates(structural_paths)` — no longer the name-driven file
  unions. The locator/covered-set inputs and `propagation_covered_identifiers` read the
  files containing selected rows (recheck_files ∪ files of by-names matches), preserving
  the ownership-read rule at `:1901-1906`.

**Contract inputs:** the six worklist call sites in `resolve_delta` (`:1884-1966`): each
`*_in_files(scoped_files)` arm switches to the narrowed `recheck_files`; each `*_by_names`
arm receives `recheck_names`. Worklist SQL in
`crates/julie-extract-artifact/src/resolution_store.rs:752-1023` is UNCHANGED.
`delta_scope_crosses_over` keeps identifier-row denomination (v2.28.0) but measures the
narrowed selection: scope rows = rows in `recheck_files` + rows matching `recheck_names`
(the existing chunked COUNT pattern extends with a name-IN count via
`idx_identifiers_name_kind`); the single-changed-file exemption RE-MEASURES under row
scoping (Task 5 sweep) rather than being assumed.

**File ownership:** Modify: `crates/julie-extract-cli/src/resolution.rs`
(`delta_scope_files`, `resolve_delta`, `run_resolution`, `delta_scope_crosses_over`); Test:
`resolution_scope_equivalence.rs`, `resolution_report_scope.rs`

**Serialization required:** Yes

**Dependency reason:** Consumes Task 1's harness and Task 2's accessors; same-file conflict
with Task 2.

**What to build:** The scope computation change described in Produces, threaded through
`run_resolution` and `resolve_delta`. Every existing test in
`resolution_scope_equivalence.rs` (including Task 1's five), `resolution_report_scope.rs`,
`operations_contract.rs`, `resolution_contract.rs`, and `writer_contract.rs` stays green
unchanged — any failure is a design defect to report, not a test to adjust.

**Approach:** keep the sweep order and flush points of `resolve_delta` exactly (the
buffered-write flush boundaries at `:1897`, `:1921`, `:1945`, `:1949`, `:1977` are
behavior-load-bearing). The diff should read as: compute `(recheck_names, recheck_files)`,
substitute them at the six call sites, adjust the ownership read, extend the crossover
count. Nothing else moves.

**Acceptance criteria:**
- [ ] Task 1's five hazard cases pass on the row-scoped path.
- [ ] Full default + contract tiers green with zero test modifications outside the two
      owned test files.
- [ ] `RESOLUTION_VERSION` unchanged.
- [ ] Worker-scope verification passes and the change is committed per commit mode.

### Task 4: Shadow mode + dogfood evidence

**Files:**
- Modify: `crates/julie-extract-cli/src/resolution.rs` (entry point
  `resolve_workspace`/`run_resolution`), env plumbing in
  `crates/julie-extract-cli/src/main.rs` only if an existing env-read pattern requires it
- Test: `crates/julie-extract-cli/tests/resolution_shadow.rs` (new)

**Interfaces:**
- Consumes: the Task 3 row-scoped path and the legacy file-scoped scope computation (kept
  as a private function for the shadow comparison).
- Produces: `JULIE_RESOLUTION_SHADOW=1` — on every scoped delta, run the LEGACY scope
  computation inside a rolled-back savepoint first, capture its overlay writes, then run
  the row-scoped path for real, and diff row-for-row (natural key, ignoring
  `resolved_at_revision`). A mismatch writes a structured report (JSON to stderr naming
  table, key, both values) and makes the process exit non-zero after the write completes.
  Off by default; zero cost when unset.

**Contract inputs:** the natural-key diff discipline from the equivalence oracle; savepoint
semantics on the existing write transaction (the hook already runs inside one —
nested SAVEPOINT/ROLLBACK TO is the mechanism).

**File ownership:** Modify: `crates/julie-extract-cli/src/resolution.rs` (entry point),
`crates/julie-extract-cli/src/main.rs` (env plumbing if needed); Test: new
`crates/julie-extract-cli/tests/resolution_shadow.rs`

**Serialization required:** Yes

**Dependency reason:** Shadows the Task 3 path against the legacy path; needs both in-tree.

**What to build:** The shadow comparison plus a test that (a) proves shadow-on with
agreeing paths changes nothing and exits zero, and (b) proves an injected divergence (a
test-only hook or a doctored overlay row) produces the mismatch report and non-zero exit.
Then the dogfood: lead runs shadowed `update` saves over real repos (this repo + the Miller
repo artifact, ≥20 saves each including renames) and records zero mismatches in the
verification ledger. Dogfood evidence is a release hard gate.

**Acceptance criteria:**
- [ ] Shadow mode exists, is off by default, and detects an injected divergence.
- [ ] Dogfood: zero mismatches over ≥40 real saves across two real repos, recorded with
      commands and counts.
- [ ] Worker-scope verification passes and the change is committed per commit mode.

### Task 5: Performance proof, sweep re-measurement, release prep

**Files:**
- Modify: `crates/julie-extract-cli/tests/resolution_perf.rs` (sweep re-run under row
  scoping; single-file 150 ms ceiling must hold), `crates/*/Cargo.toml` ×3 + `Cargo.lock`
  (version bump), `docs/release-notes/v<next>.md` (new)
- Lead-run, separate repo: Miller `spike/index-store-ph1/julie-path-audit/probes/probe3.py`
  pattern for the save-shape A/B

**Interfaces:**
- Consumes: Task 4's zero-mismatch evidence; the committed probe3/probe4 instruments in
  Miller's repo.
- Produces: measured save-shape A/B (old pinned binary vs new build) on the Miller-repo
  artifact: wall + resolution-phase ms + rows re-derived, for the two named files with
  committed evidence (92.7%/18.1 s and 90.3%/16.0 s baselines). Release notes in house
  style stating measured numbers and the honest floor. The release itself and the Miller
  pin bump are USER-APPROVAL-GATED — prepare, do not push or tag.

**Contract inputs:** `docs/release.md` (gates + preflight + closeout);
`scripts/check-release-state.sh`; the perf-honesty rule (report measured numbers, never the
brief's predictions).

**File ownership:** Modify: `crates/*/Cargo.toml`, `Cargo.lock`, `docs/release-notes/`,
`resolution_perf.rs`; probe scripts under Miller's `spike/` (separate repo, lead-run)

**Serialization required:** Yes

**Dependency reason:** Needs Task 4's zero-mismatch evidence; release is
user-approval-gated.

**What to build:** The sweep re-run (crossover + single-file-exemption re-measured under
row scoping — keep, adjust, or retire the exemption per the measurement, with the decision
recorded in the sweep's comments), the A/B evidence, the 150 ms ceiling check, release
notes, and version bump. Stop before tagging: report the numbers and request release
approval.

**Acceptance criteria:**
- [ ] Save-shape A/B measured and recorded; the win stated in wall-clock and
      resolution-phase terms against the committed baselines.
- [ ] `resolution_perf.rs` single-file 150 ms release ceiling holds.
- [ ] Branch gate green (fmt, xtask, default, contract) + preflight clean.
- [ ] Release notes written; version bumped; NOTHING tagged or pushed — user approval
      requested with the evidence.
