# Receiver Type Facts

Date: 2026-09-01
Status: accepted

## Decision

Extractors emit declared-type facts for locals, parameters, and fields as
`TypeInfo` rows on `BaseExtractor.type_info`. Miller consumes the rows at
query time to bind a call receiver to its declared type. Two shared helpers
carry the whole contract:

- `strip_type_decorations(declared, rules)` in
  `crates/julie-extractors/src/base/types.rs` reduces declared type text to
  the base type name.
- `BaseExtractor::record_declared_type_fact(symbol_id, declared_text, rules,
  is_inferred)` in `crates/julie-extractors/src/base/creation_methods.rs`
  records one row per symbol and never overwrites an existing row.

Per-language extractors pass a `TypeNameRules` constant. No new fact table,
no new `SymbolKind`, no registry restructuring.

## Consumer contract (Miller resolution policy v6, Tier 3 Receiver)

Source: `/home/murphy/source/miller/docs/contracts/resolution-policy-v6.md`.

- Miller finds the receiver symbol with a scope walk over symbols, matched by
  name and language.
- A `type_facts` row binds only when `resolved_type` verbatim-matches the
  name of exactly one type-like symbol in the same language. Miller does no
  namespace stripping and no generic stripping.
- Confidence: 0.75 when `is_inferred=false`, 0.65 when `is_inferred=true`.

Because the match is verbatim and bare-name, `resolved_type` must be the base
type name. Dotted names (`Foo.Bar`) are recorded as-is; they stay unmatched
until a type symbol carries that exact name.

## Normalization rules

`resolved_type` = base type name:

- Strip generic argument lists: `List<int>` -> `List`.
- Strip nullable suffixes: `GraphTraversal?` -> `GraphTraversal`.
- Strip by-ref, pointer, and borrow markers: `ref`, `out`, `in`, `&`, `*`,
  `mut`. `ref Foo` -> `Foo`, `&mut Foo` -> `Foo`, `*Store` -> `Store`.
- Never strip array suffixes: `string[]` stays `string[]`.
- Never strip namespace qualifiers: `Foo.Bar` stays `Foo.Bar`.

When the base name differs from the declared text, the full declared text
goes to `TypeInfo.metadata["declared"]`.

`is_inferred=false` only for a type the syntax states. Initializer-derived
types record `is_inferred=true`.

## Metadata key contract

Plain strings at call sites:

- `"role"` = `"parameter"` on parameter symbols. Parameter symbols are kind
  `variable`; there is no parameter `SymbolKind`.
- `"receiver_type"` on call identifiers and on structured pending metadata.
- `"declared"` in `TypeInfo.metadata` for the full declared text.

## Precedence

Recorded rows win over legacy `infer_types()` map rows. The existing
precedent is `types_with_base_info` in
`crates/julie-extractors/src/registry.rs`: it extends the inferred map with
`base.type_info`, so a recorded row replaces an inferred row for the same
symbol. `record_declared_type_fact` keeps the first recorded row for a
symbol and ignores later calls for the same `symbol_id`.

## Contract markers

- `EXTRACTION_CONTRACT_VERSION` gains the suffix `.receiver-type-facts-v1`.
- `EXTRACTION_IDENTITY_EPOCH` stays 9.

## Known caveat: static receivers on internal types

The spec's reproduced example (`SymbolGraph.ShortestPathWithEvidence` ->
`GraphTraversal.ShortestPathWithEvidence` in the miller repo) has a static
receiver that names an `internal` class. Policy v6's static-type tier
refuses non-public types cross-file, so that exact chain needs a Miller-side
policy change, not extractor facts. The facts in this plan fix variable,
parameter, and field receivers.
