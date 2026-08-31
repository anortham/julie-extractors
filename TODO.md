# TODO

Open work lives in Linear on the
[julie-extractors](https://linear.app/breakingdevelopment/project/julie-extractors-2eb24a56c3df)
project. This file is a pointer, not a second backlog.

Language capability gaps stay in `fixtures/extraction/capabilities.json`. Do not
copy them here.

---

## Session: bump extraction identity epoch (Miller dogfood 2026-08-31)

julie-extract v2.38.2 is published at epoch 8. Miller F11/F19 still waits
on a miller pin of that extract release.

**Goal:** Family stores re-extract C# after BRE-16. `EXTRACTION_IDENTITY_EPOCH`
is 8 in published julie-extract 2.38.2. Remaining work is pin miller.

**Why:** BRE-16 already maps explicit C# `internal` to `Visibility::Internal`
(`csharp-visibility-v2`, julie-extract 2.38.0 / miller pin 2.38.1). It did not
bump the identity epoch. Store import reuses a completed
`(path, content_hash, extraction_epoch)` identity. Miller `workspace full` in
store mode therefore cannot rewrite L1.

**Live evidence (miller family `a271f2bd-7368-4da6-b5aa-24ffad69fb1f`, gen-001):**

- Fresh `julie-extract` 2.38.1 scan of `FullRebuildPromotion.cs` writes
  `visibility=internal` for `FileOperationRetryOptions`
  (`internal readonly record struct`).
- The same file at store epoch 7 stayed `private` after miller
  `workspace full` (rev 62324 → 62617, `swapped: no`).
- Every C# `internal…` signature at epoch 7 was stored as `private`
  (classes 723, structs 142, methods 2681, plus enums/interfaces). Local miller
  repair: `UPDATE symbols SET visibility='internal' WHERE language='csharp' AND
  visibility='private' AND signature LIKE 'internal%'` (13020 rows). That is
  dogfood-only, not the product fix.
- Manifest entries pin `file_versions` with `ON DELETE RESTRICT`. Do not
  delete live versions to force a rewrite.

**Do not:**

- Add `record_struct_declaration` to C# `extract_symbol` for this.
  `extract_record` already handles `record struct` via `record_declaration`.
  The isolated 2.38.1 scan proves extraction is already correct.
- Treat miller's leftover `.miller/symbols.db` as source of truth. Miller
  reads the family store.
- Pin miller from this repo. Miller 1.26.0 stays on 2.38.1 / epoch 7 until
  miller pins 2.38.2.

**Done:**

- Bump `EXTRACTION_IDENTITY_EPOCH` (`crates/julie-extractors/src/lib.rs`) 7 → 8.
- Update current-epoch tests, store-v1, architecture, and the 2.38.2 ledger.
- Keep the capability snapshot keyed to epoch 8.
- Ship julie-extract 2.38.2.

**Still open:**

- Pin miller to julie-extract 2.38.2. Miller then force-rescans on extractor
  upgrade and rewrites C# visibility for real.

**Acceptance:**

- A new family-store import of an unchanged C# file at epoch 8 allocates a
  new `file_versions` row and writes `visibility=internal` for explicit
  `internal` types and members.
- Epoch 7 rows stay immutable. Do not rewrite them in place.
- Golden / capability / contract tests that name the current epoch pass at 8.

Miller notes: `docs/findings/2026-08-31-v1.26.0-mcp-dogfood.md` F11 / F19 on
miller branch `fix/v1.26.0-mcp-dogfood`. Reachability follow-up stays
[BRE-17](https://linear.app/breakingdevelopment/issue/BRE-17).

---

## Open

- **Pin miller to julie-extract 2.38.2.** Epoch 8 is published. Miller 1.26.0
  still pins 2.38.1 / epoch 7, so family-store C# `internal` stays stale until
  that pin.
- [BRE-16](https://linear.app/breakingdevelopment/issue/BRE-16/record-c-internal-as-visibilityinternal)
  Extract mapping shipped in 2.38.0. Identity epoch 8 shipped in 2.38.2.
  Miller pin remains. Miller follow-up: [BRE-17](https://linear.app/breakingdevelopment/issue/BRE-17).
- [BRE-51](https://linear.app/breakingdevelopment/issue/BRE-51/emit-go-trun-subtest-names-as-test-cases)
  Emit Go `t.Run` subtest names. Miller selection is parked as [BRE-54](https://linear.app/breakingdevelopment/issue/BRE-54).
- [BRE-52](https://linear.app/breakingdevelopment/issue/BRE-52/add-an-f-source-extractor)
  F# source extractor. Parked. Miller CT is [BRE-55](https://linear.app/breakingdevelopment/issue/BRE-55).
- [BRE-53](https://linear.app/breakingdevelopment/issue/BRE-53/emit-rust-doc-test-facts-for-cargo-doctest-cases)
  Rust doc-test facts. Miller already runs package-level `cargo test --doc`.

## Cleared 2026-08-30

Verified against current `main` and filed or dropped:

| Old item | Verdict |
|---|---|
| 1–15 | Done (supply-chain, clippy, hardening, TS literals, writer perf, structural facts, complexity, body hash, ASP.NET/htmx/Alpine, file-row attribution, traversal budget, CLI/writer splits, parser-reuse decision). |
| 16 | Resolver write path retired 2026-08-18. C# locals/params and `infer_variable_type` shipped. Remaining fact is BRE-16. Miller-only leftovers: BRE-17 (blocked on BRE-16) and [BRE-18](https://linear.app/breakingdevelopment/issue/BRE-18/refuse-public-framework-homonym-static-receivers-without-import) (public framework homonym). |
| 17–18 | Obsolete with the resolution write path. Miller `RequireCommitted` no longer demands `reference_resolution_status`. |
| 19–23 | Shipped in v2.37.0. Miller already mirrors `discovery_limits` and fails the pin-bump gate on drift. |
