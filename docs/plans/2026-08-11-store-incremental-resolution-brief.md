# Store-Path Incremental Resolution — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Date:** 2026-08-11
**Status:** Approved direction; lifecycle corrections incorporated after architecture review.
**Repo state read:** `main` @ `692ef4e` (v2.31.4 plus the native capacity-probe repair). Resolution code is unchanged from `1067fb7`.

**Goal:** Make `store resolve` proportional to the changed dependency closure while preserving row-identical exact output, bounded memory, crash safety, and the public base-plus-delta read contract.

**Architecture:** Capture every manifest transition and its predecessor exact-resolution tuple at the single manifest-publication seam. Resolve only the dependency closure, carry forward unaffected rows from the durably rooted predecessor overlay, and continue to publish a full exact base-or-delta result through the existing CAS state machine. When accumulated delta drift crosses a fixed threshold, publish the exact scratch directly as a new base and atomically rotate the current view to it.

**Tech Stack:** Rust, rusqlite, SQLite STRICT tables, Cargo feature-gated contract/performance tests, Miller C# consumer contract tests.

**Architecture Quality:** High risk. The approved shape keeps manifest-transition capture in the artifact store, scope derivation in one store module, resolver behavior behind the existing `ResolutionSession` seam, and publication behind `ResolutionBindingStore`; callers retain the existing CLI and SQLite contracts.

## Global Constraints

- `julie-extract` remains the only writer of family-store catalogs and resolution files.
- Miller keeps reading the existing base-plus-cumulative-delta overlay; no new Miller tool or query-time resolver is added.
- `RESOLUTION_VERSION` remains `6`; scoped and full runs must produce equal canonical semantic row digests
  (defined under Verification). The schema-only `catalog_sha256` is never an equivalence gate.
- Peak resolver memory remains bounded by `RESOLUTION_WINDOW_SIZE`; no corpus-sized candidate index is permitted.
- The base-build call remains full forever. Only per-request exact convergence may scope.
- A missing, incomplete, corrupt, mixed-writer, or epoch-incompatible journal always falls back to the current full-resolve behavior.
- `JULIE_STORE_RESOLUTION_DELTA=off` restores the current full per-request path verbatim.
- No historical manifest retention is required for the fast path.
- Every durable mutation is fenced, crash-recoverable, and committed with the manifest or binding transition it describes.
- Public schema additions are documented in `docs/contracts/store-v1.md` and `docs/contracts/sqlite-store-schema-v2.md`.

---

## Goal

Make `store resolve` incremental. Today both call sites (`crates/julie-extract-cli/src/store/resolve.rs:313`
and `:419`) run `run_resolution_session(&mut session, true, true)` — a full-corpus exact pass on every
resolve. The legacy artifact path already ships row-level delta resolution (v2.26.0–v2.28.0; single-file
delta gate 51 ms), but the store path bypasses it entirely. Measured cost: after a 98-file delta, the next
full-corpus exact replay costs 2m24s even with the v2.31.4 bounded repair
(`docs/findings/2026-08-10-store-resolution-performance-repair.md`; 98 is the count of files that carry gap rows);
the Miller dogfood store carries ~416k identifier resolutions. Target: resolve cost proportional to the
change, not the corpus — minutes down to seconds.

## User-confirmed decisions (2026-08-11)

1. **Base rebase/compaction is IN scope** (T7). It is the only fix for the `exact_gap_json` growth
   (session measurement on the Miller store: 151.7 MB across 71 `resolution_deltas` rows, an identical
   7.4 MB payload repeated per non-empty generation; this figure is not yet recorded in any findings doc,
   so the Task 7 thresholds are provisional and Task 8 re-measures and records them).
2. **No shadow/trial period in production.** The scoped path ships directly once verified by (a) the
   row-digest equivalence test suite and (b) a real replay on the Miller dogfood store. Keep only an
   off-switch env var (`JULIE_STORE_RESOLUTION_DELTA=on|off`) as an escape hatch. Tasks 1-7 default it to
   `off`/forced-full; Task 8 alone may flip the default to `on` after every hard gate passes.
3. **The fast path must always work.** Add the additive producer-owned scope lifecycle tables,
   written atomically at the shared manifest-publication seam for import/update/delete/`from_artifact`,
   so scope derivation never depends on retained historical manifests. A full resolve remains only as
   the safety fallback for a missing/corrupt journal.

## Architecture-review corrections (2026-08-11)

1. **The predecessor exact overlay must survive manifest invalidation.** `ManifestStore::invalidate_resolution_binding`
   currently clears `resolution_base_id`, `resolution_delta_generation`, and `resolution_exact_at` before the
   view advances. A new `resolution_scope_state` row captures the last exact tuple before that clear, preserves it
   across multiple manifest changes, and roots its base/delta until a newer exact publication commits.
2. **Journal completeness must be provable across generation reuse.** Path rows alone cannot distinguish “no change”
   from a missing write, and manifest generations are content-addressed/reusable rather than monotonic transition ids.
   Every non-no-op manifest flip therefore gets one uniquely keyed `resolution_scope_batches` header containing a
   monotonic `transition_id`, `previous_transition_id`, from/to generations, row count, deterministic change hash,
   request id, and completion timestamp. The chain follows transition ids; generation is payload, never identity.
3. **Write at the private manifest core, not in each executor.** `ManifestStore::publish_transaction` owns state capture,
   batch/header creation, path rows, and binding invalidation in one transaction. This covers import, update,
   delete, `from_artifact`, the public `publish` path, and generation-reuse restores without duplicated lifecycle policy.
   Same-generation no-op reuse produces no batch.
4. **Rebase must rotate the live binding.** Merely marking the exact scratch as a ready base is insufficient because
   `bind_base` returns an existing exact binding. The rebase branch must CAS the view directly to the new base and
   an empty delta in the same publication transaction.
5. **Shipped schema-v2 stores need an explicit writer-side upgrade path.** Keep `PRAGMA user_version=2`, add
   `store_meta.resolution_scope_journal_version=1`, and run `ensure_resolution_scope_feature()` on the first mutating
   open/publication before any manifest flip. Read-only opens remain validation-only. Generation promotion upgrades
   the source/destination in a defined order so catalog fingerprint checks compare like with like. Older writers remain
   safe: any transition without a valid batch breaks the chain and forces a full resolve. The contract must state that
   delta mode requires journal feature version 1 with complete coverage.

## Grok architecture-review disposition (2026-08-11)

The read-only review against `692ef4e` found three blockers; all are incorporated above and in the task contracts:
generation reuse now has unique transition identity, pre-existing v2 stores now have an explicit mutating-open feature
upgrade with promotion ordering, and exact carry-forward now merges per semantic row key. Its remaining concrete
findings are also adopted: scope state enters maintenance eligibility SQL, the private `publish_transaction` core is
the writer seam, Tasks 1-7 stay default-off, catalog authority/promotion tests are explicit, structural path kinds are
defined, ready exact bases are reused, and task paths are crate-qualified. No review finding was rejected.

## Claude verification-pass disposition (2026-08-11)

A second read-only verification pass at `692ef4e` re-proved the ground truth and found one gate defect plus
seam and evidence gaps; all are incorporated:

1. **The `catalog_hash` equivalence gates proved nothing.** `resolution_base_catalog_hash`
   (`julie-extract-artifact/src/store/resolution.rs:3713`) hashes only normalized `sqlite_master` DDL, so any
   two exact files from the same binary always match regardless of row content. Every equivalence gate now
   compares the canonical semantic row digest defined under Verification, matching the legacy row-dump oracle
   (`resolution_scope_equivalence.rs:166`).
2. **The manifest seam has three bypasses.** `apply_forward_rollback` (`store/generation.rs:1061`, `:1121`)
   writes `views.current_generation` directly; generation promotion copies the `views` table verbatim
   (`logical_copy_generation`, `store/generation.rs:611`); and `store/executor.rs:1157` writes an exact
   resolution binding directly, bypassing `ResolutionBindingStore`. Task 2 now owns making each preserve or
   explicitly invalidate scope state, and the executor exact-binding write moves behind the binding store.
3. **Evidence provenance.** The 2m24s figure measures a full-corpus exact replay after a 98-file delta (98 is
   the gap-file count), and the 151.7 MB gap-growth figure was a session measurement with no recorded source;
   both are now stated as such and Task 8 records the re-measured values.
4. **Precision fixes.** Four session phases are inert, not two; the base-file identifier key is
   `(version_id, identifier_id)`; `store_meta` reads must tolerate an absent journal-version key; journal
   writes are bounded on corpus-scale transitions; `has_ready_base` binds at `resolve.rs:235` and
   `publish_exact` at `store/resolution.rs:1258`.

## Verified ground truth (read at `1067fb7`, revalidated unchanged at `692ef4e`)

- `resolve.rs:313` is the **base build** call site (runs only when no ready base exists for the current
  `resolver_output_epoch`); `:419` is the **per-request exact pass**. Both hard-code full corpus.
- `StoreScratchResolutionSession::open_resolution_pass` (`store/resolution_session.rs:1358`) hard-codes
  `ResolutionWorklists { effective_full: true, .. }`, and `prior_resolution_state()` (`:1348`) returns
  `None` — these two are the actual switches; `run_resolution_session` (`resolution.rs:2056`) computes
  `requested_full = is_full_scan || prior.is_none()`.
- The published delta is a **complete base→current diff**, not an increment over the previous delta:
  `stream_resolution_diff` (`julie-extract-artifact/src/store/resolution_diff.rs:555`) diffs the fixed
  ready base against the freshly materialized exact file; `publish_exact`
  (`julie-extract-artifact/src/store/resolution.rs:1258`, body in `publish_exact_with_markers` at `:1282`)
  copies the diff into
  `resolution_identifier_deltas`/`resolution_pending_deltas` under a new `delta_generation` and CASes
  `views`. This static-base diff is why `exact_gap_json` re-serializes the same growing payload every
  resolve (`canonical_gap_payload`, `:1870`).
- Identifier rows are **total per visible version** (`enforce_identifier_totality`,
  `resolution_diff.rs:795`); pending rows carry explicit `replace`/`tombstone` operations.
- The exact file (`resolve-exact-<id>.db`, written by `finish_exact`, `resolution_session.rs:455`) **is
  already base-format** (`ResolutionBaseWriter`). Its `catalog_hash` (`resolution.rs:3713`) hashes only the
  schema DDL, so it can never prove content equivalence; the equivalence oracle is the canonical semantic
  row digest defined under Verification.
- `select_nearest_ready_base` (`store/resolution.rs:2173`) already supports multiple ready bases and
  binds the one sharing the most versions with the current manifest — rebase needs only a build trigger
  (`has_ready_base` at `resolve.rs:235` currently short-circuits on any ready base for the epoch).
- The shipped legacy delta machinery in `resolution.rs`: `resolve_delta` (`:2256`), `delta_scope_files`
  (`:3408`) with name-set expansion via `import_names_linked_to` (`:730`) and
  `receiver_names_bound_to_types` (`:755`), file-keyed `files_importing_module_candidates` (`:711`),
  crossover `DELTA_SCOPE_CROSSOVER = 0.7` (`:3081`) denominated in identifier share with a
  single-changed-file exemption, and the `DemotedCoLocation` repair (`:2321`).
- `ResolutionScopeInput` (`julie-extract-artifact/src/writer.rs:182`) carries `changed_file_ids` +
  `touched_symbol_names` (inserted + OLD names) in the legacy path; nothing equivalent reaches the store
  path today.
- `ManifestStore::publish_transaction` (`store/manifest.rs:626`) invalidates the current resolution binding before
  advancing `views.current_generation`; `invalidate_resolution_binding` (`:540`) clears the exact tuple.
- Import/update, delete, and `from_artifact` converge on `ManifestStore::publish_in_transaction`
  (`store/executor.rs:838`, `:871`, and `:1028`), while the public `publish` path also converges on the private
  `ManifestStore::publish_transaction`. The private core is the complete seam for content publications, but
  not for every `views` write: `apply_forward_rollback` (`store/generation.rs:1061`, `:1121`) sets
  `current_generation` directly, generation promotion copies the `views` table verbatim
  (`logical_copy_generation`, `store/generation.rs:611`), and `store/executor.rs:1157` writes an exact
  resolution binding directly, bypassing `ResolutionBindingStore`. All three must preserve or explicitly
  invalidate scope state (Task 2).
- Manifest generation is reusable: a content-hash match may flip a view back to an older generation. Scope history
  therefore cannot use `manifest_generation` as a unique transition key.
- `ResolutionBindingStore::bind_base` (`store/resolution.rs:1129`) returns `current_binding` when one exists, so a
  newly ready base does not rotate an already exact view without a dedicated CAS transition.
- Store schema validation accepts exactly schema v2 or a new empty database (`store/schema.rs:91`); existing-generation
  opens validate without running schema initialization, and generation promotion compares authoritative catalogs.
  The journal therefore needs an explicitly versioned writer-side additive v2 upgrade and upgrade-aware promotion.
- The store session's `CandidateLookup` issues **live SQL against the current manifest per edge**
  (unlike the legacy in-memory `WorkspaceCandidateIndex`), so tier-4 global-uniqueness counts are
  evaluated against the current corpus by construction, regardless of scope.
- `RESOLUTION_VERSION = 6` (`resolution.rs:1915`) — unchanged by this design.

## Core design decision

**Scope the resolution, not the diff/publish.** The scoped pass recomputes only the rows the change can
affect and writes them into the scratch. A new overlay-materialization step then fills in every
non-recomputed row from the prior overlay (bound base + bound delta), producing a full-corpus exact file
**row-identical** (equal canonical semantic row digest) to what a full pass would produce. Everything downstream — `stream_resolution_diff`,
gap facts, `publish_exact`, totality validation, the atomic CAS, and Miller's read contract — is
untouched. The O(corpus) tier resolution is replaced by O(scope); the surviving O(corpus) work is a bulk
row copy plus the streaming diff (expected seconds at 416k rows).

### Components

1. **Durable scope lifecycle (new additive store tables — producer-owned public contract change).**
   `resolution_scope_state` stores one row per unresolved view transition: predecessor manifest generation/hash,
   predecessor base id, predecessor delta generation, resolver epoch, and journal-through transition id. The first
   content-changing manifest after an exact binding captures the tuple; later changes preserve it. Foreign keys and
   the concrete base/delta protected-set and eligibility queries treat this state as a root until exact publication
   clears it.

   `resolution_scope_batches` stores one immutable header per non-no-op manifest transition:
   `(transition_id INTEGER PRIMARY KEY AUTOINCREMENT, view_id, previous_transition_id, from_generation,
   to_generation, change_count, change_hash, request_id, completed_at)`. The non-reused transition id permits
   generation reuse such as `gen1→gen2→gen1`.
   `resolution_scope_journal` stores one child row per changed path:
   `(transition_id, path, change_kind, old_version_id, new_version_id, touched_names_json)`. `change_kind` distinguishes
   `path_added`, `path_deleted`, and `content_replaced`; only add/delete feed module-repoint expansion.
   `ManifestStore::publish_transaction` derives both from the old/current manifest entries and symbol names and writes
   them before invalidating the live binding. The header and child rows commit with the manifest flip.
   When no scope state exists for the view, or the change count exceeds a fixed bound, the seam writes only a
   header marked scope-unusable and no child rows; chain validation already classifies that as full fallback.
   This bounds journal write cost on corpus-scale transitions such as a first import or `from_artifact`.

   Scope reads require a state row whose predecessor exact tuple is coherent and a header chain from its exact
   transition through the requested view state with matching counts and hashes. Missing tables, a missing or duplicate
   header, a broken transition link, count/hash mismatch, unavailable predecessor tuple, epoch mismatch, or an older
   writer's unjournaled transition forces full resolution. After exact publication, journal rows at or below the new
   exact transition are prunable; rows still named by `resolution_scope_state` are not.

2. **`store_delta_scope` (new module, mirroring `delta_scope_files`).** At resolve time, read journal
   rows across the validated transition chain from bound exact state to current, union touched names, and apply the three shipped
   expansions ported to store SQL over versions visible at the current generation:
   import aliases (`import_names_linked_to` semantics over `symbols WHERE kind='import'` +
   `resolution::import_binding`), receiver types (`type_facts.resolved_type IN touched` → receiver
   names), and module re-point files (`path_added`/`path_deleted` only × import module candidates). Apply the
   identifier-share crossover (`DELTA_SCOPE_CROSSOVER`, single-changed-file exemption) verbatim.
   Fallback-to-full triggers (each logged in the terminal payload): no exact prior state, epoch
   mismatch, journal gap/corruption, crossover promotion, env off-switch.

3. **`StorePriorOverlay` (new).** Read layer over the predecessor tuple in `resolution_scope_state`, not the
   newly bound current view. It opens the rooted ready base read-only and overlays the rooted cumulative delta:
   per-key `COALESCE(delta, base)` lookups and by-names/by-files resolved worklists. The state row remains a GC root
   for the whole off-lease computation and is cleared only by a successful exact-publication CAS.

4. **`StoreScratchResolutionSession` delta mode (modification).**
   - `prior_resolution_state()` returns `Some(..)` when a valid prior overlay exists (state exact, same
     epoch), else `None` (forcing full, as today).
   - `open_resolution_pass` with `full=false`: build the scope; on crossover return
     `effective_full: true`; else return scoped `ResolutionWorklists`.
   - `next_phase_chunk`: scoped freezing for `Pending`/`Relationships`/`Identifiers`; four phases are
     currently inert (`ResolvedPending`, `PropagationCovered`, `ResolvedIdentifiers`, `PropagationOwned`;
     `resolution_session.rs:1488-1496`). The resolved phases hydrate from `StorePriorOverlay` by
     names/files; the propagation phases stay chunk-inert while their predicates consult scratch ∪ prior —
     this is what lets `resolve_delta` run **unmodified**.
   - `identifier_is_covered`/`propagation_is_owned` consult scratch ∪ prior overlay.
   - `CandidateLookup` stays exactly as-is (corpus-wide live SQL) — tier-4 soundness.

5. **`materialize_overlay_exact` (new step before `finish_exact`).** Merge by semantic row key, not by file/version:
   for every current semantic table primary key (identifier keys are `(version_id, identifier_id)`), scratch wins and any
   missing identifier/pending row is bulk-copied from the predecessor overlay; removed versions are never copied. This preserves untouched
   identifiers in a partially recomputed file/name arm. `finish_exact` then runs unchanged, including
   `finish_with_target_lookup` target
   validation — a hard safety net: a stale carried-forward row pointing at a removed target fails loudly
   instead of publishing.

6. **Wiring.** `resolve.rs:419` becomes `run_resolution_session(&mut exact_session, false, true)` with
   the session deciding via `prior_resolution_state` + crossover. The base build at `:313` stays
   `(true, true)` forever. Content-addressed version_ids mean unchanged files keep their version_id, so
   carried-forward rows key identically to the base/delta rows they replace — no remapping.

7. **Base rebase/compaction.** After diffing, take the rebase branch when either (a) current replacement plus
   tombstone rows exceed 25% of the bound base's semantic rows or (b) cumulative serialized `exact_gap_json` bytes
   for the view/base exceed 64 MiB. Promote the just-produced exact file through
   `ResolutionBaseCatalog::begin_build → publish_scratch → mark_ready`; if `begin_build` finds a ready base for the same
   manifest hash/epoch, reuse it rather than rebuilding. Then use a new fenced
   `ResolutionBindingStore::publish_rebased_exact` transaction to insert an empty delta for the same manifest,
   CAS from the old binding to the new base, set `resolution_exact_at`, clear scope state, append the store-log
   effect, and leave no persisted copy of the oversized old-base diff. CAS loss leaves a reusable ready base but
   changes no view binding; ordinary maintenance collects a CAS-loser unreferenced base. Gate: Miller must re-read
   the base binding after the store sequence advances and must never cache a base path across advances.

## Architecture Quality

**Affected modules:** artifact store schema/manifest/resolution/GC, CLI store scope/session/resolve/reporting, store
contracts, feature-gated equivalence/performance suites, and Miller's existing family-store binding tests.

**Caller-facing interface:** unchanged `store resolve` and base-plus-cumulative-delta SQLite view. New terminal fields
are additive. Callers never manage predecessor tuples, journal batches, scratch overlay hydration, or rebase CAS.

**Depth/locality check:** `ManifestStore` owns transition capture; a new artifact-side scope-lifecycle module owns
journal validation and GC roots; a new CLI-side `store_delta_scope` module owns dependency closure; the existing
`ResolutionSession` seam owns full-versus-delta execution; `ResolutionBindingStore` owns both delta and rebase
publication. No lifecycle rule is copied into command executors.

**Test surface:** public manifest publication, binding/maintenance contracts, public `store resolve`, and Miller's
read-session contract. Private SQL helpers receive focused tests only where corruption classification cannot be
observed through a public transition.

**Seams/adapters:** `ResolutionSession` already has two production adapters and is retained. The new scope-lifecycle
module is an internal artifact-store boundary with manifest, resolve, and GC callers; no public Rust trait is added.

**Rejected shortcuts:** reconstructing predecessor state from retained logs/deltas; executor-specific journal writes;
keeping a corpus-sized candidate index; promoting a base without rotating the view; changing `RESOLUTION_VERSION`;
or resolving on demand in Miller.

**Architecture risk:** high. Correctness depends on the journal chain and predecessor root surviving multiple writes,
crashes, GC, and CAS loss. Risk is controlled by atomic transition capture, explicit completeness proofs, full fallback,
row-identical differential oracles, and crash/GC tests at the public state-machine boundary.

## Correctness: the dependency closure

Two separable concerns:

- **Per-edge answers against the right corpus:** guaranteed structurally — the store `CandidateLookup`
  queries the live manifest for every edge it resolves, so a scoped pass can never use a stale
  uniqueness count.
- **Selecting every row whose outcome could have changed:** this is what the shipped name-set expansion
  solves. A symbol named `N` added/removed/renamed anywhere puts `N` (new and OLD spellings) in the
  touched set; the by-names arms then recheck every row named `N` corpus-wide, including tier-4
  uniqueness flips in unchanged files. The three known non-name-keyed relations are covered by the same
  shipped mechanisms (import aliases, receiver types, module path existence). Residual risk — an
  un-modeled keying relation — is caught by the equivalence suite and by `finish_with_target_lookup`
  (dangling-target subclass, deterministically).

## Verification and rollout (no trial period)

**Equivalence oracle:** the canonical semantic row digest — a deterministic digest over ordered dumps of
`resolution_base_versions`, `identifier_resolutions`, and `pending_resolutions` from an exact file. The
schema-only `catalog_sha256` proves nothing about row content and is never an equivalence gate; the legacy
suite's full row-dump comparison (`resolution_scope_equivalence.rs:166`) is the model.

1. **Equivalence suite** (`store_resolution_scope_equivalence`, analogous to the legacy
   `resolution_scope_equivalence`): for each hazard case — rename, import-alias flip, receiver-type
   change, module re-point, tier-4 uniqueness flip in an unrelated file, file add/delete, demoted
   co-location, partial-file/name-arm recomputation with untouched sibling identifiers, multi-transition changes before
   resolve, `from_artifact`, identical-manifest no-op, `gen1→gen2→gen1` generation reuse, journal gap/hash/count
   corruption, and mixed-writer missing coverage — run scoped and full sessions against the same store
   state and assert equal canonical semantic row digests, with row-level diff on mismatch.
   An oracle failure is a design defect; the test is never relaxed.
2. **State-machine suite:** crash before/after batch write, manifest flip, predecessor capture, exact publish, base
   rename, base-ready mark, and rebase CAS; multiple manifest changes while one resolve runs; CAS loss; GC between
   manifest change and resolve; pin-held predecessor; view removal; pre-existing v2 feature upgrade; upgrade-before-
   promotion catalog matching; rollback/promotion; and recovery after each torn state. Every case must either publish
   one coherent exact tuple or retain a coherent predecessor and fall back full.
3. **Deterministic sequence differential:** seeded small-store mutation sequences run scoped and forced-full after
   every transition and compare canonical semantic row digests. Seeds and minimal mismatch replay are printed on failure.
4. **Dogfood replay gate:** replay the Miller-store 98-file delta A/B (scoped vs full): zero
   canonical row-digest mismatches, and the resolve phase drops from minutes to seconds. This is the ship
   gate.
5. **Escape hatch:** `JULIE_STORE_RESOLUTION_DELTA=off` restores today's behavior verbatim. Default is
   on once all hard gates pass. Terminal payload gains additive fields: `resolution_mode`, scope sizes,
   fallback reason, timings.

## Risks (ranked)

1. Worklist-semantics divergence from the legacy shapes — mitigate by porting the legacy SQL shapes
   rather than re-deriving them; equivalence oracle.
2. Un-modeled keying relation (dependency closure) — oracle + hazard suite + target validation; same
   exposure the shipped legacy delta already carries.
3. Journal write-path gaps leaving stale scope — one manifest-publication seam, immutable batch headers,
   deterministic count/hash validation, and full fallback on any broken chain.
4. O(corpus) carry-forward copy cost eating the win on huge stores — measure in Task 5; bulk
   `INSERT…SELECT` over ATTACHed SQLite, expected seconds at 416k rows.
5. `base_id` churn on consumers from rebase — atomic live rotation plus the Miller read-contract check.
6. Additive schema change on shipped schema v2 — run an explicit mutating-open feature upgrade, update catalog
   authority/promotion ordering, prove idempotence, prove an older writer causes a typed full fallback rather than
   partial scope, and document it before release.

## Verification Strategy

**Project source of truth:** `AGENTS.md`, `.github/workflows/ci.yml`, `xtask/src/test_tiers.rs`,
`docs/contracts/store-v1.md`, and `docs/contracts/sqlite-store-schema-v2.md`.

**Worker red/green scope:** the narrow artifact or CLI feature-gated test target owned by the task, with
`--test-threads=1` for store state-machine tests.

**Worker ceiling:** owned focused test binaries plus `cargo fmt --all -- --check` and the directly affected crate's
Clippy target. Workers do not run the Miller replay or full branch gate.

**Worker gate invariant:** each task proves its public transition, corruption fallback, or exact row-digest
equivalence before handing off.

**Lead affected-change scope:** after each coherent batch, run the artifact store schema/manifest/resolution contracts,
CLI `store_resolution_contract`, `store_resolution_mechanism`, `resolution_session_contract`, and the new scope
equivalence target under `test-store-resolution-contract`.

**Branch gate:** `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets --all-features --no-deps -- -D warnings`;
`cargo test -p xtask`; `cargo xtask test default`; `cargo xtask test contract`; `cargo deny check --all-features`;
the feature-gated store-resolution performance harness; and the real Miller replay.

**Security scope:** `security-secrets`: `gitleaks detect`; `security-deps`: `cargo deny check --all-features` with
critical/high findings as hard failures and lower severities report-only.

**Replay/metric evidence:** canonical row-digest equality, coherent binding/journal invariants, zero state-machine
partial writes, and a Miller 98-file scoped resolve measured in seconds are hard gates. Wall time, CPU time, peak RSS,
scope rows/files/names, carry-forward time, diff time, publish time, and rebase frequency are recorded report-only.

**Escalation triggers:** schema/catalog hash changes require all artifact store contracts; resolver worklist changes
require legacy resolution equivalence; binding/GC changes require crash and maintenance suites; any consumer binding
change requires Miller fast + Scale store tests; dependency changes require Cargo deny and exact lockfile review.

**Assigned verification failure:** workers stop and report when an assigned invariant fails; they do not weaken the
oracle, crossover, memory bound, corruption fallback, or timing gate.

**Verification ledger:** record invariant, command, scope, commit SHA, result, and UTC timestamp. Replay rows also
record corpus identity, manifest generations, base/delta ids, exact hashes, hard-gate metrics, and report-only metrics.

## Parallel Execution Contract

| Task | Parallel batch | File ownership | Serialization required | Dependency reason |
|---|---|---|---|---|
| Task 1: Scope lifecycle schema and manifest capture | None - serial | `store/schema.rs`, `store/manifest.rs`, `store/scope.rs`, `store/mod.rs`, schema/manifest/scope contract tests, store contract docs | Yes | Establishes the durable contract every later task consumes. |
| Task 2: Predecessor rooting, GC, and crash recovery | None - serial | `store/resolution.rs`, `store/maintenance.rs`, `store/generation.rs`, CLI `store/executor.rs`, binding/maintenance/crash tests | Yes | Requires Task 1 schema and state transitions. |
| Task 3: Prior overlay reader | Batch A | `store/prior_overlay.rs`, prior-overlay fixture tests | No | Safe parallel batch after Task 2; consumes the frozen scope-state contract. |
| Task 4: Store dependency-closure scope | Batch A | `store/delta_scope.rs`, scope parity/equivalence fixtures | No | Safe parallel batch after Task 2; consumes journal read APIs without touching Task 3 files. |
| Task 5: Scoped session and exact materialization | None - serial | `store/resolution_session.rs`, `store/mod.rs`, `store_resolution_mechanism.rs`, `resolution_session_contract.rs` | Yes | Integrates Tasks 3 and 4 through the shared session. |
| Task 6: Resolve wiring, telemetry, and differential oracle | None - serial | `store/resolve.rs`, `store/report.rs`, CLI args/env parsing, `store_resolution_contract.rs`, new sequence/equivalence tests | Yes | Requires the complete scoped session and exact materializer. |
| Task 7: Atomic base rebase and live rotation | None - serial | artifact `store/resolution.rs`, CLI `store/resolve.rs`, binding/performance tests, Miller store read-contract tests | Yes | Shares publication paths with Task 6 and needs its exact scratch output. |
| Task 8: Dogfood replay, default flip, and release evidence | None - serial | performance harness, findings/release notes/contracts, verification ledger | Yes | Final acceptance after all implementation and local gates. |
| Task 9: Fresh-store recovery completeness | None - serial | Determined by systematic root-cause evidence; expected CLI store recovery path and focused producer/integration contracts | Yes | Added by user after a live Miller outage; runs after the lifecycle implementation is stable. |

Crate-qualified paths in each task's **File ownership** block are authoritative when the compact table uses a bare
`store/...` shorthand.

## Task breakdown (equivalence oracle is the standing acceptance gate)

### Task 1: Scope lifecycle schema and manifest capture

**Files:** create `crates/julie-extract-artifact/src/store/scope.rs`; modify artifact store schema, connection,
layout, manifest, generation-promotion, and module exports; modify schema/manifest/generation contracts,
`docs/contracts/store-v1.md`, and `docs/contracts/sqlite-store-schema-v2.md`.

**Contract inputs:** current schema-v2 migration rules, private `ManifestStore::publish_transaction`,
`ResolutionViewBinding`, and import/update/delete/`from_artifact` publication paths.

**File ownership:** Task 1 exclusively owns artifact `store/schema.rs`, `store/connection.rs`, `store/layout.rs`,
`store/manifest.rs`, `store/generation.rs`, new `store/scope.rs`, `store/mod.rs`, their focused schema/manifest/
generation contracts, and the two store contract docs. `store/generation.rs` is handed to Task 2 after Task 1 passes.

**Serialization required:** Yes; Task 1 freezes the durable schema and typed seam consumed by every later task.

**Dependency reason:** None; this is the foundation task.

**Interfaces:** produce typed `ResolutionScopeState`, `ResolutionScopeBatch`, `ResolutionScopeChange`,
`ensure_resolution_scope_feature()`, and public artifact-store operations to capture/validate a manifest transition.
Private `ManifestStore::publish_transaction` is the only writer. No executor receives journal-specific obligations.

**What to build:** writer-side additive v2 feature upgrade plus the three-table scope lifecycle. Capture the predecessor
exact tuple before invalidation, preserve it over subsequent transitions, compute deterministic touched-name payloads
from old/new versions, and write one uniquely keyed immutable header plus exact child count/hash in the manifest
transaction. Write header-only scope-unusable batches when no scope state exists or the change count exceeds the
journal bound. Update schema authority/promotion so old v2 stores upgrade before catalogs are compared. The
`store_meta.resolution_scope_journal_version` read must tolerate an absent key (seeding runs only at schema
creation), and generation promotion copies every `store_meta` key via `copy_store_metadata` — the upgrade must
stay correct under both.

**Acceptance criteria:**
- [x] Import/update, delete, `from_artifact`, public `publish`, and `gen1→gen2→gen1` reuse all produce the same uniquely keyed validated batch shape through the shared seam.
- [x] Multiple changes before resolve retain the first predecessor exact tuple and extend one explicit transition chain.
- [x] Missing feature metadata or an unjournaled older-writer transition is classified as full-fallback, never partial scope.
- [x] A pre-existing schema-v2 disk store upgrades on first mutation, remains readable before mutation, and promotes/copies only between catalog-compatible generations.
- [x] Focused schema/manifest/scope/promotion contracts and authoritative catalog hash pass; the public contract documents the additive v2 feature.

### Task 2: Predecessor rooting, GC, and crash recovery

**Files:** modify artifact store resolution, maintenance, generation recovery, their binding/maintenance/crash tests,
and the CLI executor exact-binding call site (`crates/julie-extract-cli/src/store/executor.rs`).

**Contract inputs:** Task 1's `ResolutionScopeState` and exact-publication CAS contract.

**File ownership:** After Task 1 handoff, Task 2 exclusively owns artifact `store/resolution.rs`,
`store/maintenance.rs`, `store/generation.rs`, CLI `store/executor.rs`, and the focused resolution-binding,
maintenance, generation, and crash contracts. `store/resolution.rs` is handed to Task 7 after Task 2 passes.

**Serialization required:** Yes; predecessor retention must be proven before any reader depends on it.

**Dependency reason:** Requires Task 1's durable state and feature-version semantics.

**Interfaces:** consume `ResolutionScopeState`; produce GC/recovery rules that treat its predecessor base/delta as a
durable root and clear it only with a successful newer exact-publication CAS.

**What to build:** add `resolution_scope_state` predecessor ids to the concrete protected-base/protected-delta sets and
exclude them from maintenance eligibility SQL, preventing delta/base/version reclamation while scope state names the predecessor. Make cleanup,
forward rollback, generation promotion, view removal, and crash recovery either preserve a coherent state/journal
chain or invalidate it into typed full fallback. This explicitly covers the three seam bypasses:
`apply_forward_rollback`'s direct `views` writes (`store/generation.rs:1061`, `:1121`), promotion's verbatim
`views` copy (`logical_copy_generation`, `store/generation.rs:611`), and the direct exact-binding write at
CLI `store/executor.rs:1157`, which moves behind a `ResolutionBindingStore` method that clears scope state on
exact publication.

**Acceptance criteria:**
- [x] GC between manifest publication and resolve cannot remove the predecessor overlay.
- [x] CAS loss and a second manifest update preserve the original exact predecessor and all later journal batches.
- [x] Crash points around capture, flip, exact publish, clear, rollback, and promotion recover without a dangling root.
- [x] Forward rollback, generation promotion, and the executor exact-binding path each preserve or explicitly invalidate scope state; the executor path publishes through `ResolutionBindingStore`.
- [x] Focused binding, maintenance, generation, and crash contracts pass.

### Task 3: Prior overlay reader

**Files:** create `crates/julie-extract-cli/src/store/prior_overlay.rs` and focused fixture tests.

**Contract inputs:** Task 2's validated, GC-rooted predecessor base/delta tuple.

**File ownership:** Task 3 exclusively owns new CLI `store/prior_overlay.rs` and its focused fixture target.

**Serialization required:** No; runs in parallel Batch A with Task 4 after Task 2.

**Dependency reason:** Requires the frozen durable predecessor contract, but does not share Task 4 files.

**Interfaces:** consume one validated/rooted `ResolutionScopeState`; produce bounded per-key, by-name, and by-file
merged reads over predecessor base plus cumulative delta.

**What to build:** open the base read-only, overlay replacement/tombstone rows, preserve version-qualified identity,
and expose the resolved worklists needed by `resolve_delta` without materializing the corpus.

**Acceptance criteria:**
- [x] Replacement and tombstone precedence matches Miller's family-store reader for collision fixtures.
- [x] By-name/by-file reads are deterministic, bounded, and use composite indexes.
- [x] Missing or incoherent predecessor files/rows return typed full-fallback evidence.
- [x] Focused prior-overlay tests pass.

### Task 4: Store dependency-closure scope

**Files:** create `crates/julie-extract-cli/src/store/delta_scope.rs` and scope parity/equivalence fixtures.

**Contract inputs:** Task 1's validated batch chain, Task 2's predecessor identity, and the legacy
`ResolutionScopeInput`/`delta_scope_files` behavior as the parity oracle.

**File ownership:** Task 4 exclusively owns new CLI `store/delta_scope.rs` and its scope parity fixtures.

**Serialization required:** No; runs in parallel Batch A with Task 3 after Task 2.

**Dependency reason:** Requires durable scope inputs, but does not share Task 3 files.

**Interfaces:** consume validated scope batches and the current manifest; produce `ResolutionWorklists` or a named
full-fallback/crossover reason.

**What to build:** port the legacy touched-name, import-alias, receiver-type, module-repoint, and changed-file
expansions to manifest/version-qualified SQL. Define `path_added`/`path_deleted` as structural module-repoint inputs
and keep content-only replacement out of that arm. Apply `DELTA_SCOPE_CROSSOVER=0.7` and the single-file exemption verbatim.

**Acceptance criteria:**
- [x] Mirrored legacy/store fixtures produce identical recheck names, files, selected rows, and crossover decisions.
- [x] Journal chain/count/hash/epoch failures and env-off return named full-fallback reasons.
- [x] Tier-4 uniqueness flips in unchanged files enter scope through touched names.
- [x] Added/deleted paths reproduce legacy structural module re-point scope; content-only replacement does not over-expand it.
- [x] Focused scope parity tests pass.

### Task 5: Scoped session and exact materialization

**Files:** modify `crates/julie-extract-cli/src/store/resolution_session.rs`, store module exports,
`store_resolution_mechanism.rs`, and `resolution_session_contract.rs`.

**Contract inputs:** Task 3's `StorePriorOverlay`, Task 4's `ResolutionWorklists`, and the unchanged shared
`resolve_delta`/`finish_exact` contracts.

**File ownership:** Task 5 exclusively owns CLI `store/resolution_session.rs`, CLI store module exports,
`crates/julie-extract-cli/tests/store_resolution_mechanism.rs`, and
`crates/julie-extract-cli/tests/resolution_session_contract.rs`.

**Serialization required:** Yes; it joins both Batch A outputs and freezes exact scratch semantics.

**Dependency reason:** Requires Tasks 3 and 4 complete.

**Interfaces:** consume `StorePriorOverlay` plus scoped worklists; produce a full-corpus exact scratch file with the
same canonical semantic row digest as a forced-full session.

**What to build:** return valid prior state, freeze scoped phases, hydrate resolved phases from the prior overlay,
make coverage/propagation consult scratch union prior, and bulk-copy untouched current-manifest rows before unchanged
`finish_exact` target validation.

**Acceptance criteria:**
- [ ] `resolve_delta` runs unchanged through the store session.
- [ ] Scratch wins per `(version_id, identifier_id)`, missing sibling rows in partially recomputed files are carried forward,
      removed versions are never carried forward, and every visible identifier remains total.
- [ ] Target validation rejects stale carried-forward targets.
- [ ] Carry-forward stays bounded and focused mechanism/session tests pass.

### Task 6: Resolve wiring, telemetry, and differential oracle

**Files:** modify `crates/julie-extract-cli/src/store/resolve.rs`, store report/env parsing, and
`crates/julie-extract-cli/tests/store_resolution_contract.rs`; create
`store_resolution_scope_equivalence.rs` and `store_resolution_sequence_equivalence.rs`.

**Contract inputs:** Task 5's store-session decision/result and the existing store resolve JSON schema-v1 payload.

**File ownership:** Task 6 exclusively owns CLI resolve/report/env wiring and the three listed contract/equivalence targets.

**Serialization required:** Yes; it establishes the end-to-end scoped/full behavior used by rebase and replay.

**Dependency reason:** Requires Task 5's exact materialization and fallback semantics.

**Interfaces:** per-request resolve passes `full=false`; the session decides scoped/full. Terminal JSON adds
`resolution_mode`, scope file/name/row counts, fallback reason, and phase timings without changing schema v1 fields.

**What to build:** retain the full base-build call, wire the off-switch, add curated hazard equivalence and seeded
multi-generation differential sequences, and preserve the existing diff/publish path below rebase thresholds.

**Acceptance criteria:**
- [ ] Off-switch output and behavior match the pre-change full path.
- [ ] The new binary defaults to forced-full throughout Tasks 1-7; no partial implementation can activate scoped mode.
- [ ] Every curated and seeded sequence produces equal full/scoped canonical semantic row digests.
- [ ] CAS loss while a later manifest publishes never commits stale exact output.
- [ ] CLI contract, mechanism, session, and equivalence targets pass.

### Task 7: Atomic base rebase and live rotation

**Files:** modify artifact `store/resolution.rs`, CLI `store/resolve.rs`, extractor binding/performance tests,
and Miller `tests/Miller.Tests/Indexing/FamilyStoreReadSessionTests.cs` plus
`tests/Miller.Tests/Server/StoreWorkspaceIndexProviderScaleTests.cs`.

**Contract inputs:** Task 6's exact scratch and terminal outcome, existing ready-base publication, and Miller's
current `FamilyStoreReadSession` binding reopen behavior.

**File ownership:** Task 7 exclusively owns the rebase methods in artifact/CLI resolution publication, focused
extractor binding/performance tests, and the two named Miller consumer test files. It makes no Miller production change.

**Serialization required:** Yes; artifact/CLI publication files overlap earlier owners and must be handed off explicitly.

**Dependency reason:** Requires Task 6's exact output and must complete before the dogfood replay.

**Interfaces:** produce `ResolutionBindingStore::publish_rebased_exact`, a fenced CAS from the old current binding to
a ready exact base plus empty delta for the same manifest.

**What to build:** implement the fixed 25% semantic-row and 64 MiB cumulative-gap triggers, promote the exact scratch
or reuse an already-ready base for the exact manifest identity, rotate the current view atomically, and extend
cleanup/recovery for CAS-loser ready bases.

**Acceptance criteria:**
- [ ] Rebase publishes no oversized old-base diff and immediately serves the new base with an empty delta.
- [ ] A crash or CAS loss leaves either the old coherent binding or the new coherent binding, never a hybrid.
- [ ] A ready base for the exact manifest identity is reused and CAS-bound with an empty delta without a second build.
- [ ] Miller observes the sequence advance and reopens the new base path without restart.
- [ ] Binding, performance, Miller fast, and Miller Scale store tests pass.

### Task 8: Dogfood replay, default flip, and release evidence

**Files:** modify `crates/julie-extract-cli/tests/store_resolution_performance.rs`, store contract/release-note/docs-map
files, and add the dated findings/verification-ledger document.

**Contract inputs:** all Tasks 1-7 behavior, the recorded Miller 98-file transition, and the branch/security gates.

**File ownership:** Task 8 exclusively owns the performance replay harness and release/evidence documentation.

**Serialization required:** Yes; this is the final evidence and default-flip task.

**Dependency reason:** Requires every implementation and consumer contract to be complete.

**Interfaces:** produce the release evidence for default-on scoped resolution; no new runtime interface.

**What to build:** replay the recorded Miller 98-file transition in forced-full and scoped modes, compare canonical
semantic row digests and row-level output, record phase metrics/RSS/storage, re-measure and record the gap/delta
storage figures in the findings doc (replacing the provisional 151.7 MB session measurement and confirming or
revising the Task 7 thresholds), run the branch gate, then make delta mode default-on with the off-switch retained.

**Acceptance criteria:**
- [ ] Forced-full and scoped canonical row digests and semantic rows are identical.
- [ ] Scoped resolution completes in seconds rather than minutes on the recorded Miller transition.
- [ ] Rebase collapses accumulated gap/delta storage under the fixed thresholds.
- [ ] All branch, security, replay, and consumer gates pass with a complete verification ledger.
- [ ] Only this task changes the unset-env default from forced-full to scoped; explicit `off` keeps the pre-change full behavior verbatim.

### Task 9: Fresh-store recovery completeness

**Added by user (2026-08-11):** incorporate the live outage recorded in `TODO.md` where a missing Miller family-store directory causes repeated `RootRebind` recovery attempts to fail with
`resolution_input_incomplete: reference_resolution_status must be complete, found partial`.

**Execution mode:** systematic debugging. Reproduce the missing-store sequence, trace `reference_resolution_status` across extraction, store publication, resolve, and Miller's committed-input validation, identify one evidenced root cause, then add a failing regression before changing production code. The root cause determines the exact owned files; keep store-writing behavior in this repository and do not change Miller production behavior without explicit approval.

**Acceptance criteria:**
- [ ] A deterministic missing-family-store recovery regression fails for the observed partial-status reason before the fix.
- [ ] Root-cause evidence identifies where complete resolution becomes partial across the producer/consumer boundary.
- [ ] The producer fix makes refresh/`RootRebind` converge to complete reference resolution and a readable Miller workspace.
- [ ] Focused producer contracts, the recovery integration regression, and applicable Miller consumer contracts pass.
- [ ] `TODO.md` records the closure evidence or removes the resolved open item.

## Explicitly out of scope

- On-demand per-query resolution (assessed in the Miller brainstorm and parked: sound for point
  queries, unusable for graph traversal without memoization; reconsider only via Miller's
  `IndexLevelGuard.MarkDegraded` demand counter if this slice proves insufficient).
- Any Miller production behavior change. Miller keeps issuing `store resolve` and reading the base+delta overlay;
  the read contract is unchanged (Task 7 adds consumer contract tests for the existing multi-base binding model).
- Changing `RESOLUTION_VERSION` — outcomes are row-identical by construction.
