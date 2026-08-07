# Row-level resolution scoping — design brief — 2026-08-07

**Status:** APPROVED direction (user, 2026-08-07). This brief is the input to a razorback
brainstorm + plan session; it is not the plan.
**Predecessors:** [`2026-08-05-delta-resolution-soundness-and-scoping-design.md`](2026-08-05-delta-resolution-soundness-and-scoping-design.md)
(the current file-scoped delta machinery), the v2.28.0 crossover release
(`docs/release-notes/v2.28.0.md`), and the Miller-side path audit
(miller repo: `spike/index-store-ph1/julie-path-audit/results.md`, probes committed under
`probes/`).

## The problem (measured)

A single-file save (`update --file`) re-resolves a widened scope: the changed file plus
**every file containing any touched symbol name**. On a real 1,420-file C#-dense workspace
(381k identifiers), the median save touches ~47 names that appear in 27–35% of files holding
**80–87% of all identifier rows** — so a one-file save re-derives ~350k rows at ~20k rows/s:
**16–18 s per save** (committed named-file evidence: 92.7%/18.1 s, 90.3%/16.0 s). The cost
lands on Miller's watcher converge (search freshness lag), `miller edit apply=true` on the
leader (agent-visible inline latency), and every direct `update --file` caller.

Two cheaper fixes were measured and eliminated:

- Crossover promotion (v2.28.0 A/B): Full ≈ or slower than the widened delta on the save
  shape — promotion sheds only per-changed-file worklist overhead; one file has none.
- Kind-based name filtering: 1.1× on typical files.

## The approach (audit §2.1 option 3)

Re-resolve only the **identifier rows that bear a touched name**, plus all rows of the
changed files — not every row in every file that contains a touched name. The file arm is
the amplifier: the same 47 median names bear only **~1.6% of identifier rows**. Expected
save cost: sub-second resolution at the measured bulk rate, ~50× scope cut. This is a
redesign of the delta resolution machinery (per-row delete/replace keyed by name, lazy
per-file context), not a parameter change.

## Constraints and facts verified in code (2026-08-07)

- `ResolutionScopeInput.touched_symbol_names` is already **the union of names inserted by
  the write and the OLD names of every symbol in the files the write deleted or rewrote**,
  collected from the DB before deletion (`writer.rs:167-169`). Row-level scoping MUST
  preserve this: a rename `Foo → Bar` re-resolves both the now-dangling `Foo` rows and the
  possibly-captured `Bar` rows. The rename case is a first-class equivalence-gate case, not
  an afterthought.
- Resolution input is names by nature — resolution IS the name→ID binding. Output
  (`identifiers.target_symbol_id`) is already ID-keyed and indexed
  (`idx_identifiers_target`); downstream joins never touch the name string.
- The scope lookup is already indexed: `idx_identifiers_name_kind` serves
  `WHERE name IN (<touched names>)`.
- Row-level scoping is a **scope optimization, not an output change**: for any corpus state
  it must produce byte-identical resolution output to the file-scoped path. If equivalence
  holds, `RESOLUTION_VERSION` does NOT bump. The proof bar below is what earns that.

## Proof bar (user-discussed, 2026-08-07)

A predeclared, Ph1-style measured proof — not unit fixtures alone:

1. **Shadow mode**: an opt-in flag runs BOTH scopings on the same write and diffs the
   resolution output row-for-row (natural-key diff; the Miller binding-proof diff tooling
   is reusable). Zero mismatches on real repos (this repo + the Miller repo at minimum,
   multi-language fixture included) gates the release.
2. The existing gates extend, never weaken: `resolution_scope_equivalence.rs`, the four
   delta-hazard cases (disappearing symbols / old-name collection among them),
   `writer_contract.rs` scope tests.
3. A/B latency measurement on the save shape proving the predicted win (the v2.28.0
   probe3/probe4 instruments in the Miller repo are reusable).

## Deferred, with reasons

- **Integer keys / name interning** (user idea, 2026-08-07): legitimate constant-factor
  win, wrong lever for this problem (scope is the 50×; joins already run on IDs). Natural
  home: the v4 store schema (Ph2) — a new schema where interning can be designed in and
  measured. Do not retrofit the v3 artifact for it.
- **Rename identity continuity** (IDs surviving renames): deliberately rejected. Symbol IDs
  derive from `file:name:line:column` (`base/extractor.rs:356`); after a definition rename,
  other files' source still says the old name, and the index must report that truth
  (dangling/re-bound), not the intent. Continuity would need rename-detection heuristics,
  which trade determinism for guesswork.

## Sequencing

Before or alongside Ph2 of the Miller index-store program (user-approved pull-forward: the
Miller program doc slated "the symbol-name scope-widening fix" for Ph2; this brief makes it
its own julie release ahead of the store work). Possible compounding benefit, to be
measured and not promised: the store's background time-to-exact is dominated by a corpus
resolution pass; small view divergences may converge row-scoped instead.

## Non-goals

- No crossover threshold changes (v2.28.0 settled that lane).
- No artifact schema change in this slice.
- No change to full-scan resolution (the whole-corpus path stays as is).
