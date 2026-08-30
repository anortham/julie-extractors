# TODO

Open work lives in Linear on the
[julie-extractors](https://linear.app/breakingdevelopment/project/julie-extractors-2eb24a56c3df)
project. This file is a pointer, not a second backlog.

Language capability gaps stay in `fixtures/extraction/capabilities.json`. Do not
copy them here.

---

## Open

- [BRE-16](https://linear.app/breakingdevelopment/issue/BRE-16/record-c-internal-as-visibilityinternal)
  Record C# `internal` as `Visibility::Internal`. `determine_visibility` still
  maps it to `private`. Miller StaticType then over-refuses those types.
  Follow-up: Miller
  [BRE-17](https://linear.app/breakingdevelopment/issue/BRE-17/accept-internal-types-in-statictype-cross-file-reachability).

## Cleared 2026-08-30

Verified against current `main` and filed or dropped:

| Old item | Verdict |
|---|---|
| 1–15 | Done (supply-chain, clippy, hardening, TS literals, writer perf, structural facts, complexity, body hash, ASP.NET/htmx/Alpine, file-row attribution, traversal budget, CLI/writer splits, parser-reuse decision). |
| 16 | Resolver write path retired 2026-08-18. C# locals/params and `infer_variable_type` shipped. Remaining fact is BRE-16. Miller-only leftovers: BRE-17 (blocked on BRE-16) and [BRE-18](https://linear.app/breakingdevelopment/issue/BRE-18/refuse-public-framework-homonym-static-receivers-without-import) (public framework homonym). |
| 17–18 | Obsolete with the resolution write path. Miller `RequireCommitted` no longer demands `reference_resolution_status`. |
| 19–23 | Shipped in v2.37.0. Miller already mirrors `discovery_limits` and fails the pin-bump gate on drift. |
