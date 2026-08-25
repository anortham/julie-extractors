# Decision: `test_linkage` / `test_coverage` symbol metadata

Date: 2026-08-25

## Context

Miller has a reader for two symbol-metadata keys, `test_linkage` and
`test_coverage`. The reader turns them into graph edges and Miller's continuous
test selector treats those edges as its `explicit_linkage` evidence tier. No
`julie-extract` store has ever written either key.

The question for this repo: should the extractor start writing them, and in
what shape? This doc records the verified reader contract, what the extractor
can honestly produce from one file, and the verdict.

## Verified reader contract (primary source: Miller repo)

All citations are `/home/murphy/source/miller` at the checkout read on
2026-08-25.

### Where the keys are read

| Fact | Source |
| --- | --- |
| The two key names, in this order | `src/Miller.Indexing/TestLinkageReader.cs:12` |
| Rows scanned: `SELECT symbol_id, metadata_json FROM symbols WHERE is_test = 1 AND metadata_json IS NOT NULL` | `src/Miller.Indexing/TestLinkageReader.cs:38-44` |
| Keys must be top-level properties of the `metadata_json` object | `src/Miller.Indexing/TestLinkageReader.cs:108-114` |
| Each target becomes `GraphEdge(test_symbol_id, target_id, key, confidence, key)` | `src/Miller.Indexing/TestLinkageReader.cs:120` |
| A target equal to the test symbol id is dropped | `src/Miller.Indexing/TestLinkageReader.cs:119` |
| Edges join the graph as supplemental edges | `src/Miller.Indexing/SymbolGraphReader.cs:85`, `src/Miller.Indexing/SqliteSymbolGraphIndex.cs:780` |

### Accepted value shapes

`ReadTargetIds` (`src/Miller.Indexing/TestLinkageReader.cs:135-171`) accepts
four shapes for the value of either key:

| Shape | Example |
| --- | --- |
| String | `"test_linkage": "<id>"` |
| Array (recursive) | `"test_linkage": ["<id>", "<id>"]` |
| Object with an id property | `"test_linkage": {"symbol_id": "<id>"}` — also `target_symbol_id` and `source_symbol_id`, all three read if present |
| Object with an id list | `"test_linkage": {"symbol_ids": ["<id>"]}` |

`ReadConfidence` (`:126-133`) reads `confidence` only from the object form,
clamps it to `0..1`, and defaults to `1.0`. Miller's own tests pin both object
spellings: `{"test_coverage":{"symbol_id":"…","confidence":0.97}}`
(`tests/Miller.Tests/Indexing/SqliteSymbolGraphIndexTests.cs:630`) and
`{"test_linkage":{"symbol_ids":["…"],"confidence":0.98}}`
(`tests/Miller.Tests/Indexing/SymbolGraphReaderTests.cs:85-86`).

### The load-bearing fact: targets are symbol ids, not names

Every accepted shape carries a **`symbols.symbol_id` value**. Miller does no
name resolution for these edges. `SymbolGraphReader.Read` takes a name
resolver and discards it on the linkage path — `_ = resolveName;`
(`src/Miller.Indexing/SymbolGraphReader.cs:84`) — and the pinned test passes a
resolver that returns nothing yet still gets the edge
(`tests/Miller.Tests/Indexing/SymbolGraphReaderTests.cs:91`). A target id that
matches no row is dropped at query time with no diagnostic
(`tests/Miller.Tests/Indexing/SqliteSymbolGraphIndexTests.cs:566-590`, which
ends `Assert.Empty(actual.Nodes)`).

### What the edges buy

| Consumer | Effect |
| --- | --- |
| `ContinuousTestImpactSelector` | Edge kind `test_linkage`/`test_coverage` selects the `explicit_linkage` tier at confidence 0.65, against `graph_reference` at 0.58 for a `calls` edge (`src/Miller.Testing/Selection/ContinuousTestImpactSelector.cs:64-65`, `:955-965`, `:1021-1023`) |
| `ImpactRanker` | Same priority bucket as `calls`. Ranking does not change (`src/Miller.Core/Graph/ImpactRanker.cs:29`, `:38`) |

So the whole delta for a target Miller can already reach by a `calls` edge is
**+0.07 selection confidence**.

### What writing the key costs

The reader runs behind a `LIMIT 1` probe that short-circuits when no test
symbol carries either key. Miller measured the unprobed scan on its own store:
32,436 metadata blobs parsed for zero edges, 2,978 ms per graph load against
206 ms for the probe (`src/Miller.Indexing/TestLinkageReader.cs:17-24`).
Writing either key on any test symbol in a store flips that probe true and
restores the full scan for that store.

## What the extractor can honestly produce from one file

Symbol ids are location hashes: `md5(file_path:name:start_line:start_column:
end_line:end_column:start_byte:end_byte)`
(`crates/julie-extractors/src/base/types.rs:408-421`, and the older
`generate_id` at `crates/julie-extractors/src/base/extractor.rs:356-360`). The
extractor can compute an id only for a symbol it parsed, so only for symbols in
the file it is extracting. This repo does not do workspace-global resolution by
design; Miller computes that at query time.

An in-file call resolves to a target id only through
`ScopedSymbolIndex::resolve_call_target`
(`crates/julie-extractors/src/base/relationship_resolution.rs:191-217`), which
returns `Resolved` only when the terminal name matches exactly one callable
symbol in the same file and the call has no receiver, or a `this`/`base`
receiver. Anything else is `ReceiverQualified`, `Ambiguous`, or `Missing`, and
the C# extractor then writes a pending relationship instead
(`crates/julie-extractors/src/csharp/relationships.rs:476-545`).

### Measured C# behaviour

A probe run of `pipeline::extract_canonical` over a C# test class on
2026-08-25 produced this:

| Call in the test method | Result |
| --- | --- |
| `Helper()` — private helper in the same test class | Resolved `Calls` relationship, `to_symbol_id` set, confidence 0.9 |
| `_sut.PlaceOrder(1)` — the production call | Pending relationship, `callee_name = "_sut.PlaceOrder"`, no target id |
| `OrderService.StaticPlace()` — static production call | Pending relationship, `callee_name = "OrderService.StaticPlace"`, no target id |
| `Run()` where two same-file methods are named `Run` | Pending relationship — the name is ambiguous in-file |

The only in-file target a C# test method can name is a helper declared in the
same test file. That helper is test code, not a production symbol.

### The declarative case is already published

A second probe on the same day, over
`[TestOf(typeof(OrderService))]` and `nameof(OrderService.PlaceOrder)` inside a
test class, showed the extractor already emits:

- annotation `testof` on the test class, `raw_text = "TestOf(typeof(OrderService))"`;
- identifier `OrderService`, kind `TypeUsage`, contained by the test class;
- identifiers `OrderService` (`VariableRef`) and `PlaceOrder` (`MemberAccess`),
  contained by the test method.

Miller name-resolves identifiers at query time and already scores them as the
`identifier_reference` tier. The declarative subject link needs no new key.

## Decision

**Not yet.** `julie-extract` does not write `test_linkage` or `test_coverage`.
The C# pilot is not implemented.

The reader wants a target `symbols.symbol_id`. For a real C# test, the
production symbol under test lives in another file, so the extractor cannot
compute its id without the workspace-global resolution this repo does not own.
The only ids the extractor can honestly write are same-file targets, and for
those three things are true at once:

1. The edge already exists. A resolved same-file call is already a
   `relationships` row, and Miller reads relationships rows verbatim as edges
   at confidence 1.0 (`tests/Miller.Tests/Indexing/SymbolGraphReaderTests.cs:55-69`).
2. The target is not production code. In C# it is a helper in the test file, so
   labelling it `explicit_linkage` would overstate the evidence Miller has.
3. Writing it costs Miller the full metadata scan on every graph load of that
   store, for zero new edges.

A pilot here would trade a measured Miller regression for a mislabelled tier.
That fails the data-quality bar in `CLAUDE.md`: positive support means useful
rows, not syntax the parser happens to expose.

## Contract, for the day this opens

If and when the extractor writes these keys, this is the shape it must write.
It is recorded now so no future change has to re-derive it.

- Key: `test_linkage` for a declared test-to-subject link; `test_coverage` for
  an executed-coverage link. Both are top-level properties of the symbol's
  `metadata_json`.
- Carrier: only a symbol whose `symbols.is_test` column is `1`. That column is
  written from the `is_test` metadata flag
  (`crates/julie-extract-cli/src/extraction.rs:410`), which only
  `apply_test_role` sets (`crates/julie-extractors/src/test_detection.rs:44-61`).
  A key on a non-test symbol is invisible to Miller.
- Value: the object form,
  `{"symbol_ids": ["<symbol_id>", …], "confidence": <0..1>}`. The object form is
  the only one that can carry confidence, and `symbol_ids` is the only plural
  spelling Miller reads.
- Targets: `symbols.symbol_id` values, never names. A name is silently dropped.
- Self-links are dropped by Miller; do not emit them.
- Write path: `apply_test_role`'s module in
  `crates/julie-extractors/src/test_detection.rs`, or a focused sibling beside
  it. Direct metadata inserts of these keys stay forbidden, the same rule that
  already governs `is_test`, `test_lifecycle`, and `test_container`.

## What would open it

Either side can close the gap; neither is this task.

1. **Miller side, preferred.** Derive `explicit_linkage` from facts julie
   already publishes: an `is_test` symbol whose `pending_relationships` row
   (`crates/julie-extract-artifact/src/schema.rs:272-297` — carries
   `from_symbol_id`, `caller_scope_symbol_id`, `target_terminal_name`,
   `target_receiver`, `target_namespace_json`, `target_import_context`) or whose
   `identifiers` row resolves to a changed production symbol. Miller already
   name-resolves both at query time. This needs no new extractor output and
   keeps the probe short-circuit intact.
2. **Extractor side.** Only becomes honest for a language whose tests and
   production code share a file. Rust `#[cfg(test)] mod tests` is the one
   idiomatic case in this repo's language set: the tested function is parsed in
   the same file, so its id is computable. Even there, the resulting edge
   duplicates a `calls` relationship Miller already reads, so it buys +0.07
   selection confidence for the cost of the full metadata scan. It is worth
   doing only after Miller makes the scan cheap, or gates it per store.

## Rollout order, if the extractor side is chosen

Ordered by how often a test and its subject share a file, which is the only
condition under which the extractor can compute the target id:

| Rank | Language | Reason |
| --- | --- | --- |
| 1 | Rust | `#[cfg(test)] mod tests` puts subject and test in one file by convention |
| 2 | Zig | `test` declarations sit beside the code they exercise |
| 3 | Go | `_test.go` is a separate file, but same package; only same-file targets qualify |
| 4 | Python, JavaScript, TypeScript | Same-file tests happen but are not the norm |
| — | C#, Java, Kotlin, Scala, Swift, Ruby, PHP | Tests live in a separate project or directory. Nothing to emit in-file. |

Each step needs a golden fixture where the subject and the test are in one
file, and a `capabilities.json` row. Do not open a language until its fixture
proves a resolved same-file target id.

## Guard

`crates/julie-extractors/src/tests/test_linkage_contract.rs` scans the crate's
production sources and fails if either key literal appears. It keeps Miller's
probe short-circuit intact until this decision is revisited, and it names this
doc in its failure message.

## Pointers

- Reader: Miller repo `src/Miller.Indexing/TestLinkageReader.cs`
- Selector tiers: Miller repo `src/Miller.Testing/Selection/ContinuousTestImpactSelector.cs`
- Test-role write path: [2026-08-20-test-role-contract-closure.md](2026-08-20-test-role-contract-closure.md)
- Why this repo has no workspace-global resolution: [2026-08-18-resolution-write-path-retirement.md](2026-08-18-resolution-write-path-retirement.md)
