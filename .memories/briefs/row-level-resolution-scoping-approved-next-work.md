---
id: row-level-resolution-scoping-approved-next-work
title: Row-level resolution scoping — approved next work
status: completed
created: 2026-08-07T14:41:41.115Z
updated: 2026-08-30T22:45:25.359Z
tags:
  - resolution
  - performance
  - row-level-scoping
  - approved
---

## What

Redesign delta resolution scoping from file-level to row-level: re-resolve only identifier rows bearing a touched name (plus all rows of changed files), not every row in every file containing a touched name. User-approved direction 2026-08-07. Full design brief: `docs/plans/2026-08-07-row-level-resolution-scoping-brief.md` — read it before planning.

## Why

A one-file save re-derives 80–87% of identifier rows (16–18 s on a 381k-identifier workspace). The touched names bear only ~1.6% of rows — the file arm is the amplifier. Crossover promotion (v2.28.0 A/B) and kind filtering (1.1×) were measured and eliminated; row-level scoping is the only path to delta-sized save cost.

## Hard constraints

- Preserve old-name collection (`ResolutionScopeInput.touched_symbol_names` unions inserted names + OLD names collected before deletion, writer.rs:167-169) — the rename case is a first-class equivalence-gate case.
- Byte-identical output to the file-scoped path for any corpus state; `RESOLUTION_VERSION` must NOT bump if equivalence holds.
- Proof bar: shadow mode running both scopings and diffing row-for-row on real repos (zero mismatches gates release) + existing gates (`resolution_scope_equivalence.rs`, four delta-hazard cases, writer_contract scope tests) + save-shape A/B latency proof.
- Deferred by decision: integer keys / name interning (v4 store schema is the home); rename identity continuity (rejected — index reports source truth).

## Sequencing

Its own julie release BEFORE/alongside Miller's index-store Ph2 (pulled forward from the Ph2 bundle by user approval). Release + Miller pin bump need user approval as always.
