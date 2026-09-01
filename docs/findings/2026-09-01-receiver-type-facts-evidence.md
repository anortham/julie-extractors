# Receiver Type Facts: Evidence Scan (Task 9)

Date: 2026-09-01. Binary built from branch `worktree-receiver-typed-call-resolution`
at commit `8cf506ea` (all wave-1 language tasks plus the gate-2 corrupt-row fix).
Every number below comes from artifacts produced by that one commit.

## Method

Each corpus directory was scanned into its own SQLite artifact:

```
cargo run -q -p julie-extract-cli --bin julie-extract -- scan --root <dir> --db <db>
```

Queries run per artifact:

```sql
-- total symbols
SELECT COUNT(*) FROM symbols;

-- parameter symbols
SELECT COUNT(*) FROM symbols
WHERE kind='variable' AND json_extract(metadata_json,'$.role')='parameter';

-- parameter symbols with a type fact (hard gate 3)
SELECT COUNT(DISTINCT s.symbol_id) FROM symbols s
JOIN type_facts t ON t.symbol_id = s.symbol_id
WHERE s.kind='variable' AND json_extract(s.metadata_json,'$.role')='parameter';

-- variable symbols, split by presence of a type fact
SELECT COUNT(*) FROM symbols WHERE kind='variable';
SELECT COUNT(*) FROM symbols s WHERE s.kind='variable'
AND EXISTS (SELECT 1 FROM type_facts t WHERE t.symbol_id=s.symbol_id);

-- untyped new-initializer locals (hard gate 1)
SELECT COUNT(*) FROM symbols s WHERE s.kind='variable'
AND s.signature LIKE '%= new %'
AND NOT EXISTS (SELECT 1 FROM type_facts t WHERE t.symbol_id=s.symbol_id);

-- corrupt resolved_type values (hard gate 2)
SELECT COUNT(*) FROM type_facts
WHERE resolved_type LIKE '% %'
   OR instr(resolved_type, char(9)) > 0
   OR instr(resolved_type, char(10)) > 0
   OR resolved_type LIKE '%,%'
   OR resolved_type LIKE '%<'
   OR (resolved_type LIKE '%>%' AND resolved_type NOT LIKE '%<%');

-- call-site receiver_type rows
SELECT COUNT(*) FROM identifiers
WHERE json_extract(metadata_json,'$.receiver_type') IS NOT NULL;
SELECT COUNT(*) FROM pending_relationships
WHERE json_extract(metadata_json,'$.receiver_type') IS NOT NULL;
```

## Corpora

| Corpus | Files | Provenance |
|---|---|---|
| csharp-baseline | `SymbolGraph.cs`, `GraphTraversal.cs` | Real: `miller/src/Miller.Core/Graph/` (the plan's baseline pair) |
| csharp-receiver | `WorkspaceOpenPrimeService.cs` | Real: `miller/src/Miller.Server/Workspaces/`. Miller's C# style has zero `this.Method(...)` calls anywhere in `src` (grep-verified); this file's `base.StopAsync(...)` call exercises the same self-reference receiver path. |
| typescript | `doc.ts`, `registries.ts` | Real: `zod/packages/zod/src/v4/core/` (miller has no `.ts` files) |
| python | `mcp_client.py` | Real: `miller/scripts/benchlib/` |
| rust | `relationship_resolution.rs`, `creation_methods.rs` | Real: this repo, `crates/julie-extractors/src/base/` |
| go | `main.go`, `math_test.go`, `synthetic_sample.go` | The only real Go under `~/source` is two tiny fixture files of other repos (julie real-world fixture, miller test fixture); the third file is synthetic, written for this scan to get locals and a method receiver |
| java | `LongPollingTransport.java`, `NegotiateResponse.java` | Real: `aspnetcore` SignalR Java client |

## Baseline (C#, pre-change, 2026-09-01)

Sample `Miller.Core/Graph/{SymbolGraph,GraphTraversal}.cs`:

| Metric | Pre-change | At 8cf506ea |
|---|---|---|
| Parameter symbols (`role=parameter`) | 0 | 173 (144 with type facts) |
| Variable symbols | 215 | 244 (173 parameters + 71 locals) |
| Variables with type facts | 185 | 205 |
| Untyped `var x = new Foo(...)` locals | 30 | 0 |
| Truncated generic field facts (`IReadOnlyDictionary<string,`) | present | 0 — now `resolved_type='IReadOnlyDictionary'` with `metadata.declared='IReadOnlyDictionary<string, GraphNeighbour[]>'` |
| Type facts total | 222 | 242 |

The 10 untyped locals remaining are foreach and deconstruction shapes
(`var node`, `var edge`, `var neighbours`, deconstructed `key`/`neighbours`,
`var flag`) — correctly skipped, no declared or constructor-derived type in the
syntax. The 29 parameters without facts are untyped lambda parameters.

## Per-language results (all at 8cf506ea)

| Extract | Symbols | Params | Params w/ fact | Variables | Vars w/ fact | Untyped `new` locals | Type facts | Corrupt | Receiver id rows | Receiver pending rows |
|---|---|---|---|---|---|---|---|---|---|---|
| csharp-baseline | 298 | 173 | 144 | 244 | 205 | 0 | 242 | 0 | 0 | 0 |
| csharp-receiver | 41 | 8 | 8 | 16 | 16 | 0 | 27 | 0 | 1 | 0 |
| typescript | 71 | 10 | 8 | 26 | 20 | 0 | 27 | 0 | 1 | 0 |
| python | 47 | 19 | 13 | 30 | 14 | 0 | 19 | 0 | 0 | 0 |
| rust | 225 | 100 | 100 | 146 | 109 | 0 | 145 | 0 | 0 | 0 |
| go | 28 | 8 | 8 | 9 | 9 | 0 | 13 | 0 | 0 | 0 |
| java | 92 | 14 | 14 | 22 | 22 | 0 | 66 | 0 | 0 | 0 |

## Hard-gate verdicts

| Gate | Verdict | Evidence |
|---|---|---|
| 0 untyped `var x = new Foo(...)` locals in the C# sample | PASS | 0 in csharp-baseline; 0 in every extract |
| 0 corrupt `resolved_type` values across every measured extract | PASS | 0 in all seven corpus artifacts (and in the one-line TS repro) |
| ≥1 parameter symbol with a type fact per measured language | PASS | C# 144, TypeScript 8, Python 13, Rust 100, Go 8, Java 14 |

Corpus-wide `missing`-rate movement is report-only and was not measured: the
full measurement needs Miller to pin a release of this branch.

## Gate 2 history

The first measurement of these same corpora, at `aab5341f`, found 15 corrupt
rows: 2 plan-emitted TypeScript recorded facts (`const $output: unique symbol`
recorded `resolved_type='unique symbol'`) and 13 legacy `infer_types()` rows
(python locals carrying raw source text from multi-line call initializers,
python/rust/java method-return values such as `dict[str, Any]`,
`Option<&'a Symbol>`, `Override public void`). Commit `8cf506ea` fixed both
paths: the TypeScript declared-type path rejects the `unique symbol` node
shape, and `convert_types_map` drops any legacy value that can never
verbatim-match a type symbol (whitespace, comma, trailing `<`, `>` without
`<`).

Two consequences of the legacy filter, both expected:

- Multi-argument generic legacy values are dropped as unbindable, so
  type-fact totals shrink where legacy inference produced them (python 24→19,
  rust 150→145, java 69→66 on these corpora).
- Symbols the recorded path skips can still receive a clean-shaped legacy
  fallback fact. The zod `unique symbol` consts now carry legacy
  `resolved_type='any'` (`is_inferred=1`); the typescript extract has 7 such
  `any` rows. These are harmless for receiver binding (no `any` type symbol
  exists to match) and are pre-existing legacy behavior.

## receiver_type end-to-end proof

C#, real miller file (`WorkspaceOpenPrimeService.cs`), `await base.StopAsync(cancellationToken)`:

```
StopAsync|call|{"receiver":"base","receiver_type":"BackgroundService"}
```

For `base.` receivers the extractor records the first declared base-list
entry's name; for `this.` it records the enclosing type's name (doc comment on
`csharp/identifiers.rs::self_receiver_type`). Miller's `src` has no
`this.Method(...)` calls at all, so the `base.` call is the real-corpus proof;
the `this.` path is covered by the C# identifier tests and by the TypeScript
row below.

TypeScript, real zod file (`registries.ts:66`), `this.get(schema)`:

```
get|call|{"receiver":"this","receiver_type":"$ZodRegistry"}
```

In the C# sample the pending relationship for `base.StopAsync` does not exist
because the call resolved in-file (the override resolves to itself), so the
receiver_type rode the identifier row only; pending metadata carries it when a
self-receiver call stays unresolved.

## Remaining gaps (recorded rationale)

- C# foreach and deconstruction locals stay untyped: the syntax states no
  type, and the initializer is not a constructor. Correctly skipped, not debt.
- Python local parenting: parameters parent to the enclosing
  function/constructor/method (SQL-verified), but locals parent to class or
  file scope, never the enclosing callable. Policy v6's scope walk will not
  find python locals as receivers until parenting is fixed (ledger finding
  from Task 5; parameters, the common receiver shape, work today).
- TypeScript/JavaScript pending relationship rows are absent for
  receiver-qualified calls by policy; receiver_type rides identifier rows
  there.
- Call-site receiver_type metadata is emitted by the C# and
  TypeScript/JavaScript extractors in wave 1. Python, Rust, Go, and Java emit
  none (0 rows measured, no call-site emitter in source); their wave-1 scope
  was parameter/local/field type facts.
- Go call extraction did not record the synthetic sample's
  receiver-qualified method call (`c.describe(...)`) as a relationship or
  pending row — pre-existing call-extraction scope, noted for completeness.

## Static-internal caveat (consumer contract)

The spec's reproduced example (`SymbolGraph.ShortestPathWithEvidence` →
`GraphTraversal.ShortestPathWithEvidence`) has a static receiver naming an
`internal` class. Policy v6's static-type tier refuses non-public types
cross-file, so that exact chain needs a Miller-side policy change. The facts
in this plan fix variable/parameter/field receivers, which are the bulk of the
~22k cross-parent gap.
