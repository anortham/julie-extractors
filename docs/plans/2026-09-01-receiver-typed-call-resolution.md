# Receiver-typed call resolution (cross-parent same-name calls)

Status: wave 1 landed, wave 2 planned. Owner repo: julie-extractors. Consumer: Miller
query-time resolution (policy v6).

Delivery state (2026-09-01): wave 1 landed on branch
`worktree-receiver-typed-call-resolution` for csharp, typescript, javascript, python,
rust, go, and java — declared-type facts for locals, parameters, and fields; parameter
symbols (kind `variable`, metadata role `parameter`); and `receiver_type` call metadata
for self-style receivers in csharp, typescript, and javascript. Evidence:
`docs/findings/2026-09-01-receiver-type-facts-evidence.md`. Wave 2 is planned in
`docs/plans/2026-09-08-receiver-type-facts-wave-2.md` and tracked as `open_gaps`
entries in `fixtures/extraction/capabilities.json` for the remaining general-purpose
languages. Wave-2 refinements for wave-1 languages: python local re-parenting to the
enclosing callable, go `:=` type facts beyond gated initializers, java bindings
(try-with-resources, enhanced-for, catch, lambda, instanceof, record components),
csharp indexer return types, and `receiver_type` emission for python, rust, go, and
java. The reproduced `SymbolGraph.ShortestPathWithEvidence` example names an `internal`
receiver type, which policy v6's static tier refuses cross-file, so that exact chain
also needs a Miller-side policy change (see the findings doc).

## Problem

A large share of in-repo call sites never get a reference edge in Miller. On the miller
repo extract (2026-09-01): 115,492 call identifiers, 83,382 resolve as `missing`. After
removing external (BCL) names, about 22,000 in-repo call sites point at names with 2 or
more definitions and get no graph edge. This breaks `trace path`, callers/callees, and
impact ranking for those sites.

Miller closed part of the gap on 2026-09-01 with a query-time fallback: a call into an
overload set (all same-name symbols share one declaring parent) now gets a low-confidence
edge per member (policy v6, reason `identifier_name`). The remaining gap is the
cross-parent case, which needs type information only the extractor can provide.

Reproduced failing example (miller repo, verified 1-hop direct call):

- `SymbolGraph.ShortestPathWithEvidence` calls `GraphTraversal.ShortestPathWithEvidence`
  through a typed receiver. Two symbols share the method name in different classes, so
  Miller cannot pick one, and the call site resolves `missing`.

## What Miller already has

- `pending_relationships` rows carry `target_receiver`, `target_display_name`,
  `target_terminal_name`, and confidence.
- Policy v6 resolves a receiver as a type when the receiver name maps to a unique type
  and binds the member through it. The step fails when the receiver is a local variable,
  parameter, or field whose type the extract does not record.

## What to build here

Emit the type facts that let the consumer bind a receiver to its declared type:

1. Local variable declared types (`var x = new Foo()` and explicit declarations).
2. Parameter declared types.
3. Field and property declared types.
4. Where the grammar gives it cheaply, the receiver's resolved type name on the call
   site fact itself.

Language parity rule applies: add each fact shape across all supported languages, not
one language at a time. General-purpose languages first-class; document per-language
gaps in `fixtures/extraction/capabilities.json` with `open_gaps` entries.

## Acceptance

- The `SymbolGraph.ShortestPathWithEvidence` chain above resolves to an edge on a fresh
  miller extract.
- The `missing` outcome rate for in-repo call identifiers drops measurably on the miller
  corpus (baseline above).
- `node scripts/language-data-quality-report.mjs --strict` stays clean.
- Extraction identity epoch bumps with the new facts so re-extraction happens.

## Non-goals

- No workspace-global reference resolution in this repo (Miller owns query-time
  resolution).
- No full type inference. Declared types only; dynamic/duck-typed languages record what
  the syntax states.
