> **Superseded 2026-08-18.** The resolution write path is retired. See [2026-08-18-resolution-write-path-retirement.md](../decisions/2026-08-18-resolution-write-path-retirement.md).

# Workspace Reference Resolution — Design

Date: 2026-07-06
Status: design for user review (rev 4 — Codex doubt pass complete, 3 rounds, all surviving
findings folded; see "Doubt pass record" at the end)
Driver: Miller P3 dead-code candidates (see
`miller/docs/plans/2026-07-06-miller-standalone-bolstering-assessment.md`) — dead-code was demoted
there because the artifact ships 92,035 `identifiers` rows with **0 resolved `target_symbol_id`**.
Resolution also upgrades Miller `trace`/`impact`/`references export` from name-guesses to real edges.

## Verified current state (live Miller-repo artifact, julie-extract 2.8.1, 2026-07-06)

The extractor already produces the *inputs* to resolution; the workspace-level pass that consumes
them was designed for but never built:

- `pending_relationships`: 41,794 rows (35,759 `calls`, 5,140 `instantiates`, 714 `uses`, 162
  `implements`, 15 `extends`) carrying rich unresolved-target context per edge:
  `target_display_name`, `target_terminal_name`, `target_receiver`, `target_namespace_json`,
  `target_import_context`, `caller_scope_symbol_id`, `confidence`. Indexed on
  `target_terminal_name`, `file_id`, `from_symbol_id`, `caller_scope_symbol_id`.
  **Span columns exist but are never populated** (`extraction.rs` maps `start_column: None` etc.;
  0 rows have spans in the live artifact); pending IDs are deduped per
  `(display_name, kind, line)`.
- `relationships`: 7,400 resolved `calls` edges — produced by the existing **per-file** local
  resolution (`ScopedSymbolIndex::resolve_call_target` in
  `crates/julie-extractors/src/base/relationship_resolution.rs`), which resolves same-file targets
  and explicitly defers `ReceiverQualified` / `Ambiguous` / `Missing` cases into pending rows.
  `relationships.to_symbol_id` is **ON DELETE CASCADE**; `identifiers.target_symbol_id` is
  **ON DELETE SET NULL**; `pending_relationships.from_symbol_id` is **ON DELETE CASCADE**.
  `delete_file_rows` runs at the *start* of the writer transaction for updated files.
- `type_facts`: 7,246 rows of `symbol_id → resolved_type` with an `is_inferred` flag. By symbol
  kind: method 5,343, property 485, constant 477, field 474, variable 195, function 132. By
  language: csharp 6,719, razor 225, python 198, html 104, javascript 0. Variable symbols exist in
  bulk (22,971) but rarely carry type facts — receiver typing is field/property-strong,
  local-variable-weak today.
- `identifiers`: 92,035 rows (call 51,040 / member_access 25,391 / type_usage 15,604);
  `containing_symbol_id` 98% populated; `metadata_json` populated on **0 rows**;
  `target_symbol_id` all NULL.
- **Positional overlap:** 41,878 identifier rows join `pending_relationships` on
  `(path, start_line, name=target_terminal_name)` — pending rows are effectively the
  call/instantiate subset of identifiers with rich context attached.
- Symbol-name ambiguity is real: only ~21% of symbols in that artifact have a workspace-unique raw
  name (8,002 unique names vs 30,534 symbols sharing 1,486 names). Bare-name global matching is
  not a viable tier on its own.
- Import symbols exist (1,904 `kind='import'` rows) and `SymbolKind::Import` is already special-
  cased by local resolution. `target_import_context` is a free string and per-language
  inconsistent (TypeScript sometimes stores the local binding name, not a module path) — it is
  corroborating evidence, not a normalized contract.
- Symbol IDs are location-derived (path + name + span), so any edit that moves a symbol produces a
  new symbol_id; per-file rewrites delete and re-insert all of a file's rows.
- Crate layering: `julie-extract-artifact` depends only on rusqlite/serde (storage);
  `julie-extractors` owns `SymbolKind` and language semantics; `julie-extract-cli` depends on both
  and owns scan orchestration.

## Goals

1. Fill `identifiers.target_symbol_id` and resolve `pending_relationships` into usable edges
   with a deterministic, tiered, confidence-stamped workspace-level pass.
2. Keep resolution honest: ambiguous/missing stays unresolved with a recorded reason; per-language
   resolution rates are advertised, not hidden.
3. Stay correct under incremental single-file updates (no stale edges, no silently lost
   unresolved context) — enforced structurally by FK semantics, not by imperative cleanup.
4. Additive artifact change only — existing consumers (Miller) keep working unchanged, then
   opt into the new facts.

## Non-goals

- Type inference / semantic analysis (rust-analyzer-grade resolution). The tiers are heuristic
  and stamped as such.
- Resolving reflection, DI-container wiring, serialization, or string-based dispatch. Statically
  impossible; Miller's dead-code layer handles these with named suppression rules.
- Normalized per-language import facts and broad `type_facts` emission (follow-ons F1/F2) —
  tier coverage is reported honestly in the meantime.
- Miller-side changes (pin bump, dead-code, contracts there) — separate Miller slice after release.

## Design

### Resolution state model (doubt-pass revision — the load-bearing change)

**Pending rows are durable facts; resolution is a derived overlay.** Pending rows are never
deleted by the resolver and never carry resolution state. Resolution lands in two places, both
with FK semantics that make invalidation automatic:

1. A new table:

   ```sql
   CREATE TABLE pending_resolutions (
     pending_relationship_id TEXT PRIMARY KEY
       REFERENCES pending_relationships(pending_relationship_id) ON DELETE CASCADE,
     target_symbol_id TEXT NOT NULL
       REFERENCES symbols(symbol_id) ON DELETE CASCADE,
     tier INTEGER NOT NULL,
     confidence REAL NOT NULL,
     method TEXT NOT NULL,
     resolved_at_revision INTEGER NOT NULL
   );
   ```

   A pending row is "resolved" iff it has a `pending_resolutions` row. If the target symbol dies
   (file rewrite, delete, move — all produce new symbol_ids), CASCADE removes the resolution and
   the pending row reverts to unresolved **with its full context intact**. If the source file is
   rewritten, the pending row itself cascades away and re-extraction re-emits it.

2. A sibling overlay for identifiers (round-2 finding: `identifiers.metadata_json` is not
   FK-governed, so resolution provenance must not live there):

   ```sql
   CREATE TABLE identifier_resolutions (
     identifier_id TEXT PRIMARY KEY
       REFERENCES identifiers(identifier_id) ON DELETE CASCADE,
     target_symbol_id TEXT
       REFERENCES symbols(symbol_id) ON DELETE CASCADE,
     tier INTEGER, confidence REAL, method TEXT,
     outcome TEXT NOT NULL,          -- resolved | ambiguous | missing | no_context
     candidates INTEGER,
     resolved_at_revision INTEGER NOT NULL
   );
   ```

   Resolved rows carry a target and CASCADE away when it dies (the identifier reverts to
   never-attempted and re-enters the worklist); ambiguous/missing rows have NULL target and are
   refreshed by re-resolution. A `CHECK` enforces outcome/target coherence
   (`outcome='resolved' ⇔ target_symbol_id IS NOT NULL`). `identifiers.target_symbol_id` is
   additionally written as a denormalized convenience for consumers, but no resolution state is
   ever written to `identifiers.metadata_json`. **The denormalized column is only FK-consistent
   for target death (SET NULL); demotion deletes the overlay row without touching it (round-3
   finding 1) — so the overlay and the denormalized column are maintained ONLY through artifact
   storage primitives that update both in the same statement batch (resolve writes both, demote
   clears both). No caller writes either surface directly.**

`relationships` rows are **not** used to store workspace resolutions (their CASCADE-on-target
plus provenance-in-metadata would silently destroy unresolved context on file rewrites — doubt
pass finding 1, confirmed at `schema.rs` FK and `writer.rs` `delete_file_rows` ordering).
Consumers that want a unified edge view read `relationships UNION pending_resolutions⋈pending`;
a compatibility view or Miller-side JOIN keeps `trace`/`impact` simple.

**Invalidation is FK-first, name-matched second:**

- Deleted/moved/edited targets: handled entirely by CASCADE / SET NULL. No imperative demotion
  code path exists to get wrong. (Doubt-pass finding: changed-name matching misses moves and span
  shifts; location-derived symbol IDs make FK the complete signal for anything that ceased to
  exist.)
- Candidate-set changes FKs cannot catch (additions, and same-ID mutations — symbol IDs are
  path+name+span and exclude kind, so a kind/signature/type change at an unchanged span keeps
  the ID and fires no FK; round-2 finding 4): after each scan, re-run the tier chain for every
  resolved row whose **terminal name OR receiver name** matches any symbol name **inserted or
  deleted in the files touched by this scan** (not merely net-new names — a rewritten file
  re-inserts all its rows, so this set inherently covers kind changes, type-fact changes on
  receivers, and import changes). **Deleted names are collected from the old DB rows before
  `delete_file_rows` runs** (round-3 note — the incoming file set cannot supply them). Served by
  `idx_pending_terminal` /
  `idx_identifiers_name_kind`. If the tier no longer yields exactly the same single candidate,
  delete the resolution (demote).
- Re-resolution worklist for filling: unresolved pending rows and never-attempted/NULL-target
  identifiers whose terminal or receiver name matches the same touched-name set, plus all rows
  in files touched this scan.

### Resolution tiers

One candidate-matching core. Each tier is an independent filter over kind-compatible candidates;
the edge resolves at the first tier (in order) whose candidate set is exactly one. If no tier
yields exactly one, the outcome is `ambiguous` when any tier yielded ≥2 and `missing` when all
yielded 0. (Tiers 2 and 3 filter on different axes — import context vs receiver type — so a
tier-2-ambiguous edge can still legitimately resolve at tier 3.) No best-guess selection, ever —
a wrong edge is worse than a missing one: it corrupts `trace`/`impact` AND removes an unresolved
row that was shielding a same-named symbol from a false dead-code verdict.

**All tiers filter to candidates in the same language as the reference site.** (Doubt-pass
finding: polyglot workspaces share names across languages; cross-language resolution is a
distinct future capability — the bridge/structural-fact lane — not a name-match side effect.)

| Tier | Signal | Confidence |
|---|---|---|
| 1. Same-file scope | existing `ScopedSymbolIndex` local result (already shipping) | 0.95 |
| 2. Import-guided | candidate reachable through an import **symbol** in the source file: an import row whose name/alias matches the terminal name (or whose module path matches the candidate's defining file where the extractor recorded it). `target_import_context` is corroborating evidence only, never the sole key. **Language-gated:** import-row metadata varies per language (TS records `source`/`importedName`, Python records nothing, Dart stores only the URI — round-2 finding 5), so tier 2 is enabled per language only where a fixture-tested import contract exists; everywhere else it reports a capability gap until F4 normalizes import facts. | 0.85 |
| 3. Receiver-typed | `target_receiver` name → symbol in scope (walk `caller_scope_symbol_id` / `containing_symbol_id` parent chain, then file, then fields/properties of the enclosing type) → that symbol's `type_facts.resolved_type` → type symbol (same language, exactly one) → member with the terminal name on that type's symbol subtree | 0.75 (0.65 when the type fact `is_inferred`) |
| 3b. Static-type receiver | `target_receiver` names a type directly (`SomeEnum.Value`, `Fixture.Create()`) rather than a variable needing type inference → unique same-language type symbol → member with the terminal name. No `type_facts` row participates, so it stamps method `tier3_static_type`, not `tier3_receiver`. Refuses a type nested inside another type, and a non-public type outside its declaring file. | 0.70 |
| 4. Unique-language-global | exactly one kind-compatible candidate in the same language workspace-wide. **Enabled for `type_usage`, `instantiates`, `uses`, `extends`, `implements` and for `calls` to Function/Constructor kinds. Disabled for `member_access` and method calls** — member names collide too heavily for global uniqueness to mean anything (doubt-pass finding 7). | 0.55 |

Tier 3b was added after measurement showed tier 3 could not bind a receiver that
names a type, because `resolve_receiver_symbols` searches only the caller's scope
chain and file top-level while a referenced type usually lives elsewhere. Its two
refusals exist so that type-name uniqueness does not become finding 7's failure
keyed on a different column: a file-scoped workspace type sharing a simple name
with a framework type would otherwise hijack every same-named reference in the
workspace.

Kind compatibility: `calls` targets Function/Method/Constructor; `instantiates` targets
Class/Struct/Constructor; `uses`/type edges and identifier `type_usage` target type-like kinds;
identifier `member_access` targets Property/Field/Method (tiers 1–3 only). Method overloads
(same name, same kind) yield >1 candidate and stay ambiguous — arity/signature evidence is a
follow-on (F3), not guessed at. Partial classes (multiple same-name class symbols) likewise stay
ambiguous at tier 4 and resolve only via tiers 1–3; coverage loss, never wrong edges.

Tier 3 coverage honesty: with today's emission, receiver typing is strong for fields/properties
(C#-heavy) and weak for local variables (22,971 variable symbols, 195 type facts). The tier ships
with that measured coverage in the report; F2 broadens emission. Tier 3 assertions in tests apply
only where `type_facts` emission exists, with the gap recorded in `language_capability_gaps`.

### Data flow

1. **Emit pending spans.** Populate the existing nullable span columns
   (`start_column`/`end_line`/`end_column`/`start_byte`/`end_byte`) in the pending mapping
   (`extraction.rs`), and include occurrence identity in `pending_relationship_id` so two
   same-name calls on one line stay distinct rows.
2. **Resolve pending edges.** For each unresolved pending row, run the tier chain; write
   `pending_resolutions` on success.
3. **Propagate to co-located identifiers.** A resolved pending row updates the identifier at the
   matching span (byte-span join once spans are emitted; fall back to `(file_id, start_line,
   name)` only when exactly one identifier matches — never propagate into an ambiguous line
   join).
4. **Resolve remaining identifiers generically.** Rows with no pending counterpart get a reduced
   chain: `type_usage` → tiers 2 & 4; `call` → tiers 2 & 4 (Function/Constructor only);
   `member_access` → none today (no receiver context exists on identifiers; F1 adds it).
   Reduced-chain outcomes are recorded like all others.

### Module placement & interface

- **Resolver policy lives in `julie-extract-cli`** as a `resolution` module (or a
  `julie-extract-resolve` crate if reuse emerges — start as a CLI module, split only on need).
  The CLI already depends on both `julie-extractors` (kind semantics) and
  `julie-extract-artifact` (storage). The artifact crate gains only storage primitives: the
  `pending_resolutions` DDL, set-based upsert/demote/worklist queries, and the report row types.
  (Doubt-pass finding 8: putting policy in the artifact crate would drag language semantics into
  a pure-storage crate.)
- **The transaction seam (round-2 finding 1):** the writer owns private transactions and commits
  internally, and the CLI only calls public writer methods — so the writer API grows an explicit
  hook: each mutating method gains a `_with_resolution` variant (or an optional
  `ResolutionHook` parameter) whose signature is defined in the artifact crate as a callback over
  the open write transaction plus the scan's touched-file/name sets. The CLI supplies the policy
  closure. Resolution therefore runs **inside the same writer transaction** as the scan's row
  writes, after all file rows are in place — never as a separate post-transaction step.
  Contract details (round-3 finding 3): the callback is a non-escaping generic/HRTB closure over
  `&Transaction<'_>` — never stored on `ArtifactWriter`, no `'static` bound; it runs **before
  `update_revision_counts` and before commit in every writer path, including the spooled
  deferred-FK transaction**; and its row writes are folded into `RowCounts`/`RowDomainCounts`
  for the two new tables so revision accounting stays truthful.
- **Failure semantics (round-2 finding 2):** resolution is a derived overlay, so a resolver error
  must never fail or roll back a scan. The hook is wrapped: on error, all affected rows simply
  stay unresolved, the scan commits, and the scan report records `resolution_failed` with the
  error. This also neutralizes the force-rebuild hazard (that path deletes the old db before
  writing; a fatal resolver there would strand an empty artifact — pre-existing flow, out of
  scope to change).
- Call sites: every artifact-mutating CLI flow — full scan → `Full`; incremental scan /
  single-file update / delete → `Delta { changed_file_ids, touched_symbol_names }`.
- `ResolutionReport` (per-language, per-tier resolved counts + unresolved outcome counts) folds
  into the existing scan report machinery and the `language_capabilities` snapshot (capability
  ids like `reference_resolution.tier2_import`, `reference_resolution.tier3_receiver`).

### Performance & determinism

- Set-based SQL over temp tables — no per-row round-trips. New composite indexes to cover the
  joins: identifiers `(file_id, start_line, name)` (propagation fallback), pending
  `(target_terminal_name, kind)` if the existing single-column index proves insufficient, and
  `type_facts(symbol_id)` already exists.
- Deterministic outcomes: candidate sets are ordered by `symbol_id` before the exactly-one test
  (the test is order-insensitive, but reports and tie-breaking diagnostics must be stable);
  `pending_resolutions` rows carry `resolved_at_revision`; two identical scans produce
  byte-identical resolution tables.
- Budgets (validated by a `writer_batching_contract.rs`-pattern test before the numbers are treated as
  contract): full resolve < 2s on a 92k-identifier artifact; delta < 100ms for a typical
  single-file update. If measurement says otherwise, the budget moves, not the test.

### Contract & rollout

1. Additive artifact change: new `pending_resolutions` and `identifier_resolutions` tables,
   populated span columns, filled `identifiers.target_symbol_id`. Document resolution semantics
   (state model, tiers, confidence values, outcome recording) in the artifact contract doc.
2. **Version skew (round-2 finding 6):** bump the single-integer `SCHEMA_VERSION`. Existing
   behavior already blocks old binaries from newer artifacts (readers reject newer versions);
   a new binary opening an older artifact gets the tables via the additive `create_schema` on
   open, followed by a `Full` resolve to backfill. **Schema version alone must not signal
   resolution availability (round-3 finding 2 — a failed backfill would hide behind it):** the
   pass maintains durable `artifact_metadata` keys — `reference_resolution_status`
   (`complete | partial | failed | absent`), `reference_resolution_version`, and
   `reference_resolution_last_full_revision` — and **Miller gates on those**, never on schema
   version or table probing. Resolver failures use a stable `ResolutionFailed` report code, and
   the release/dogfood gate fails if it appears on the fixture corpus. Miller's version-aware
   leadership already forces a full rescan when a newer-extractor leader claims, which populates
   resolution on real workspaces without manual steps.
3. julie-extract minor release (2.9.0) with release-notes coverage of per-language resolution
   rates measured on the fixture corpus.
4. Miller: pin bump slice — `references export` / `trace` / `impact` consume
   `identifiers.target_symbol_id` and the `pending_resolutions` join where present (Miller
   already treats NULL as "unknown", so this is pure upside), contracts updated additively.
5. Miller P3 dead-code slice (separate design): candidates = symbols with zero resolved inbound
   edges AND zero unresolved same-name identifiers outside their own definition, minus named
   suppression rules; per-language confidence labels from the resolution report.

## Testing

- **Unit:** candidate-filter and tier-chain logic on in-memory symbol sets (pure, fast).
- **Contract:** scan per-language fixtures → assert `pending_resolutions` /
  `identifiers.target_symbol_id` via artifact SQL: same-file, cross-file-import, receiver-typed
  (where type_facts exist), unique-language-global, ambiguous-stays-unresolved, overload-stays-
  ambiguous, partial-class-stays-ambiguous, cross-language-name-collision-stays-unresolved.
- **Incremental:** full-scan-then-update sequences asserting FK-driven demotion (target file
  rewritten → resolution gone, pending context intact), uniqueness-regression demotion (add a
  second same-name symbol → previously resolved edge demoted), re-resolution (remove the
  colliding symbol → edge resolves again), file move, and no stale edges after any sequence.
- **Performance:** full and delta budgets as above, measured then enforced.
- **Determinism:** two identical scans produce byte-identical resolution outcomes.

## Acceptance criteria

- [ ] `pending_resolutions` + `identifier_resolutions` tables and storage primitives in
      `julie-extract-artifact`; resolver policy module in `julie-extract-cli`; no language
      semantics in the artifact crate; no resolution state in `identifiers.metadata_json`.
- [ ] Explicit writer transaction hook; resolution runs inside the writer transaction of every
      artifact-mutating flow with the correct Full/Delta scope; resolver errors are non-fatal
      (scan commits, rows stay unresolved, `resolution_failed` in the report).
- [ ] Pending span emission populated; occurrence-distinct pending IDs.
- [ ] Tier chain with confidence stamps, same-language constraint, kind-compatibility filters,
      and the tier-4 kind restrictions; ambiguous/missing recorded with reasons.
- [ ] Identifier propagation by span (line fallback only when unambiguous); reduced generic chain
      for identifiers without pending counterparts.
- [ ] Invalidation: FK cascade/SET NULL verified by tests; demotion worklist covers terminal AND
      receiver names against all names inserted/deleted in touched files (including same-ID kind
      changes); no imperative deletion of unresolved context anywhere.
- [ ] Tier 2 language-gated by fixture-tested import contracts; schema version bumped with the
      old-artifact backfill; `reference_resolution_status/version/last_full_revision` metadata
      maintained and documented as Miller's detection surface.
- [ ] Overlay tables and the denormalized column updated only via atomic storage primitives;
      `CHECK` constraints on outcome/target coherence; hook counts folded into revision
      accounting; `ResolutionFailed` report code wired into the release/dogfood gate.
- [ ] Per-language resolution rates in scan report + `language_capabilities`; gaps recorded.
- [ ] Contract, incremental, performance, determinism tests green; fixture corpus assertions per
      supported language.
- [ ] Artifact contract doc updated; release notes include measured resolution rates.

## Follow-ons (explicitly out of this slice)

- **F1 — identifier context enrichment:** emit receiver/import context on `member_access`/`call`
  identifiers across languages so they stop depending on co-located pending rows.
- **F2 — `type_facts` breadth:** bring JS/TS/Python/Go/Rust/Java emission up toward C# levels
  (especially local variables) to strengthen tier 3.
- **F3 — overload discrimination:** arity/signature evidence so same-kind overloads can resolve
  instead of staying ambiguous.
- **F4 — normalized import facts:** module specifier + local binding + imported name as
  first-class rows, replacing the free-string `target_import_context` as tier-2 evidence.
- **F5 — Miller P3 dead-code design** (Miller repo, after the pin bump).
- **F6 — Miller P4 history/trends design** (Miller repo; agreed direction: auto-snapshot on
  convergence, keyed by workspace_id + artifact_id + revision + extractor version).

## Doubt pass record (2026-07-06, Codex, round 1)

Verdict on rev 1: rework. Findings verified against code before acceptance:

1. **Confirmed — delete-pending-and-resurrect lifecycle unsound.** `relationships.to_symbol_id`
   CASCADE + `delete_file_rows` at transaction start would destroy the only copy of unresolved
   context on every target-file rewrite. → Replaced with durable pending rows + the
   `pending_resolutions` overlay (the rev 2 state model).
2. **Confirmed — changed-name invalidation incomplete** (moves/span shifts change location-derived
   symbol IDs without changing names). → Inverted to FK-first invalidation; name matching now
   handles only uniqueness regression on additions.
3. **Confirmed — pending spans never emitted; line-keyed dedup.** → Span emission + occurrence-
   distinct IDs pulled into this slice; span-join propagation with unambiguous-only line fallback.
4. **Confirmed — `target_import_context` not a normalized contract** (TS stores local binding
   names). → Tier 2 re-anchored on import symbols; free string demoted to corroboration; F4 adds
   normalized import facts.
5. **Partially accepted — tier 3 "not implementable".** Overstated: receiver→scoped-symbol→
   type_fact chains are implementable today (22,971 variable symbols exist, caller scope is on
   every pending row), but coverage is thin outside C# fields/properties (195 variable type
   facts). → Tier 3 kept with an explicit chain definition and measured-coverage honesty; F2
   broadens emission.
6. **Accepted — unique-global unsafe for member/method names in polyglot workspaces.** → Tier 4
   restricted to same-language + type-like/free-function targets; disabled for `member_access`
   and method calls.
7. **Confirmed — artifact-crate placement wrong boundary** (storage crate would absorb language
   semantics). → Policy moved to a `julie-extract-cli` module; artifact keeps storage primitives.
8. **Accepted — perf/determinism under-specified.** → Set-based SQL, covering indexes, stable
   ordering, measure-then-enforce budgets.

### Round 2 (Codex, on rev 2)

Verdict: overlay state model survives; six mechanics findings, all verified and folded into rev 3:

1. **Confirmed — no transaction seam existed** for CLI policy inside the writer's private
   transactions. → Explicit `ResolutionHook` callback on the writer's mutating methods.
2. **Accepted — resolver failure atomicity** (force rebuild deletes the old db before writing;
   a fatal resolver would strand a partial artifact). → Resolution errors are non-fatal; scan
   commits with rows unresolved and `resolution_failed` reported.
3. **Confirmed — identifier `metadata_json` is not FK-governed** (stale tier/method provenance
   after SET NULL). → `identifier_resolutions` overlay table; no resolution state in
   `metadata_json`; `target_symbol_id` kept only as a denormalized convenience.
4. **Confirmed — added-name demotion missed same-ID mutations** (symbol IDs exclude kind; a
   kind change at an unchanged span fires no FK). → Worklist generalized to all names
   inserted/deleted in touched files, matched against terminal AND receiver names.
5. **Confirmed — import-row metadata is per-language inconsistent** (TS `source`/`importedName`,
   Python none, Dart URI-only). → Tier 2 language-gated by fixture-tested import contracts;
   F4 widens.
6. **Accepted — version-skew story missing.** → Schema version bump, additive create + full
   backfill for old artifacts, Miller feature-detection via `artifact_metadata`.

Also re-confirmed by round 2: durable pending context survives target rewrites (pending cascades
on *source* deletion only), and tier 3 is coverage-thin but implementable — matching rev 2's
disposition of round-1 finding 5.

### Round 3 (Codex, on rev 3 — final round, bounded to deltas)

Verdict: "close"; three surviving findings, folded into this final revision:

1. **Confirmed — denormalized `identifiers.target_symbol_id` goes stale on demotion** (FK only
   maintains it on target death; deleting the overlay row does not clear it). → Both surfaces
   maintained exclusively through atomic storage primitives; `CHECK` on outcome/target coherence.
2. **Confirmed — a failed backfill could hide behind schema-version feature detection.** →
   Durable `reference_resolution_status/version/last_full_revision` metadata keys; Miller gates
   on those; stable `ResolutionFailed` report code with a release/dogfood gate.
3. **Accepted — hook placement/counting contract underspecified.** → Runs before
   `update_revision_counts`/commit in every path including spooled deferred-FK; counts folded
   into `RowCounts`/`RowDomainCounts`; non-escaping HRTB closure, never stored, no `'static`.

Verified-and-dropped by the reviewer: NULL-target CASCADE semantics in the overlay (sound),
hook expressibility (sound), worklist completeness (sound, with the deleted-names-before-
`delete_file_rows` requirement made explicit).

**Doubt pass closed at the 3-round cap.** Reviewer: Codex (read-only, local verification with
file:line evidence each round). Rounds converged monotonically: architecture rework (1) →
mechanics (2) → consistency details (3); no finding in round 3 touched the state model.
