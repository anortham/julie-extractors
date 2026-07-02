# 0004: HTTP Boundary Join Contract — Normalized Route Templates and Mount Facts

## Context

v2.6.0 shipped both sides of the HTTP boundary for the JS/ASP.NET slice:
`http.client_request.v1` references and handler-definition families
(`nextjs.route_handler.v1`, `nuxt.server_route.v1`,
`aspnet.attribute_route.v1`, `aspnet.minimal_api.route.v1`). Those families
grew three different server-side join keys (`route_path`,
`normalized_route_template`, `effective_route_template`) and no rule for
route prefixes declared in a different file than the route.

The backend coverage lane (`docs/plans/2026-07-02-backend-http-boundary-coverage.md`)
adds sixteen handler/mount families across five more languages. Without a
single join contract, Miller would need per-family join logic and the
extractor would be tempted to guess cross-file prefixes.

## Decision

1. **`normalized_route_template` is the universal server-side join key.**
   Every handler-definition fact emits it, computed from the most-resolved
   template available (`effective_route_template` when a same-file prefix was
   resolved, else `route_template`), normalized to a leading `/` and the
   `:param` flavor (`{id}`→`:id`, `<int:id>`→`:id`, `{id:int}`→`:id`,
   `*filepath`→`:filepath`, `{path...}`→`:path`), with converter/constraint
   annotations stripped. Normalization preserves trailing slashes exactly as
   written after parameter conversion, so `/users/:id` and `/users/:id/` remain
   distinct join keys. The normalizer is a single shared implementation in
   `base/http_boundary.rs` with table-driven per-framework flavor rules.
   Regex route syntaxes (Django `re_path`) cannot be honestly normalized:
   those facts omit the key and set `route_syntax="regex"`.
2. **Raw templates are never rewritten.** `route_template` always carries the
   template exactly as written; normalization only ever adds a key.
3. **Same-file prefixes resolve; cross-file prefixes become mount facts.**
   When a route prefix is declared in the same file (ASP.NET `MapGroup`,
   gin/echo `Group`, `APIRouter(prefix=)`, Rails `namespace`/`scope`
   nesting), the extractor resolves it into `effective_route_template` on the
   handler fact. Mount-site calls still emit their mount fact when they match
   the mount contract, even if the mounted receiver is also traceable in the
   same file; the resolved handler fact is same-file convenience, and the mount
   fact is durable source evidence for Miller. When the prefix target is not
   traceable in the same file, the extractor emits only the dedicated mount-fact
   family at the mount site and never guesses the joined route. Cross-file
   joining is Miller's job.
4. **Verb omission means "not verb-restricted".** Registrations that accept
   any method omit both `verb` and `verb_source` (the
   `nuxt.server_route.v1` precedent). Multi-verb registrations emit one fact
   per attested verb.
5. **Existing ASP.NET families join the contract** by gaining an optional
   `normalized_route_template` key (compatible v1 addition, no pattern-id
   bump).

## Consequences

Easier: Miller joins client `target_path` against one key
(`normalized_route_template`) across every server family, old and new; mount
facts give it explicit cross-file prefix-join inputs instead of heuristics.
Fact families stay honest under the M2 silence doctrine — nothing emitted is
inferred across files.

Harder: consumers of pre-v2.7.0 artifacts must keep the
`effective_route_template` fallback; `rails.resource_route.v1` is join-input
only (RESTful expansion is Rails semantics and stays consumer-side); the
shared normalizer becomes contract-critical code where a flavor bug corrupts
every family at once — its unit-test table is the guard.

## Applies To

`crates/julie-extractors/src/base/http_boundary.rs`,
`crates/julie-extractors/src/base/framework_structural_facts/`,
`crates/julie-extractors/src/base/web_structural_facts/http_client.rs`, the
structural-fact pattern registry, and every current and future HTTP
handler/mount/client fact family.

## Future Agents

When adding a route-bearing framework: reuse the shared normalizer with a
flavor-table entry (never inline normalization), emit `route_template` raw,
resolve prefixes only same-file, add a mount family for cross-file joins,
and follow the verb-omission rule. Do not add a new server-side join key.
