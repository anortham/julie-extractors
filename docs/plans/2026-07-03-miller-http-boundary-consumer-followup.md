# Miller consumer follow-up — finish-the-HTTP-boundary lane (v2.8.0)

> **Repo boundary:** this is **cross-repo work in `/Users/murphy/source/miller`**,
> not in julie-extractors. The extractor-emitted facts are correct and valuable
> without it. This file tracks what Miller must add to *join* (not just store)
> the new v2.8.0 facts. Landing it can trail the extractor release.

## Status snapshot (verified against the current Miller consumer)

Verified in `src/Miller.Core/Graph/` at the time of the v2.8.0 extractor release:

- `BridgeStructuralPatterns.BridgeFactPatternIds` — the accept whitelist.
- `StructuralRouteFactAdapter.TryReadClientRequest` — gates on
  `PatternId == "http.client_request.v1"` **only, no language check**
  (`StructuralRouteFactAdapter.cs:101`).
- `StructuralRouteFactAdapter.IsMountFactPattern` — accepts exactly
  Express / FastAPI-include-router / Flask-blueprint / Django-url-include
  (`StructuralRouteFactAdapter.cs:286-290`). Rails mount is **evidence-only**
  (not in this arm).
- `BackendHttpBridgeProvider.RouteFamilyForMount` — hard-coded switch mapping
  each mount id to the route family it prefixes.

## A. Joins TODAY with ZERO Miller change ✅

These v2.8.0 additions reuse pattern ids Miller already consumes:

| v2.8.0 addition | Why it already joins |
|---|---|
| **Kotlin + Spring** server routes | Reuse the existing `spring.request_mapping.v1` id (already in `BridgeFactPatternIds`). Extending its `languages` to `[java, kotlin]` is extractor-side only. |
| **Kotlin / PHP / Elixir / Rust** client requests | All emit `http.client_request.v1`, already whitelisted; `TryReadClientRequest` is **not** language-gated (verified `:101`). New-language client→route joins work immediately. |

Reusing `spring.request_mapping.v1` for Kotlin (instead of minting
`kotlin.spring.route.v1`) was the design choice precisely to get this for free.

## B. New SERVER-ROUTE pattern ids — need Miller route-side wiring ⏳

Emitted and correct extractor-side; they join client→handler on
`normalized_route_template` (`:param` flavor) **once Miller accepts them**. Not
yet in `BridgeFactPatternIds`.

New route ids: `nestjs.route.v1`, `laravel.route.v1`, `laravel.resource_route.v1`,
`phoenix.route.v1`, `phoenix.resource_route.v1`, `axum.route.v1`,
`actix.attribute_route.v1`, `actix.scope_route.v1`.

Per id, Miller needs:

| Requirement | Where |
|---|---|
| pattern-id const | `BridgeStructuralPatterns.cs` |
| whitelist entry | `BridgeFactPatternIds` (absent ⇒ silent no-op) |
| route-family classification (server route, joins on `normalized_route_template`) | route-read path / `BackendHttpBridgeProvider.cs` |

`actix.attribute_route.v1` mirrors the shipped `aspnet.attribute_route.v1`
two-provenance split (attribute vs call/scope routing) — reuse that precedent.

## C. New PREFIX / MOUNT families — need full mount-join wiring ⏳

Design §2c. A whitelist entry **alone does not join** — each needs the
route-family mapping + anchor rule, or it sits inert as evidence-only. All emit
`mount_path` / `normalized_mount_path` (and `mount_target` where a same-file
target exists).

New mount ids: `axum.nest.v1`, `actix.mount.v1`, `laravel.route_prefix.v1`,
`phoenix.forward.v1`.

Per family, Miller needs (all five — the last two are what actually make it join):

| Requirement | Where |
|---|---|
| pattern-id const | `BridgeStructuralPatterns.cs` |
| whitelist entry | `BridgeFactPatternIds` |
| `IsMountFactPattern` arm | `StructuralRouteFactAdapter.cs:286` |
| **route-family mapping** | `RouteFamilyForMount` switch (`BackendHttpBridgeProvider.cs`) |
| **anchor rule** (`mount_target` vs `included_module`) | mount-composition read path |

Family-specific anchor notes:

- **`axum.nest.v1`** — `mount_target` is a cross-file fn/expr; no same-file
  guessed join. Anchor via `mount_target` when a same-file target resolves,
  else cross-file resolution.
- **`actix.mount.v1`** — `web::scope("/lit").configure(fn)` / `.service(sub)`;
  target is the configured fn / service.
- **`laravel.route_prefix.v1`** — closure `Route::prefix('x')->group(...)` groups
  have **no same-file named `mount_target`** (emits `mount_path` /
  `normalized_mount_path` only). Miller's anchor rule must tolerate a missing
  `mount_target` (evidence-only until a target-less anchor path exists).
- **`phoenix.forward.v1`** — `forward "/lit", Plug`; target is the plug module.

Same-file-resolvable prefixes already degrade gracefully: they also populate
`route_group_prefix` / `effective_route_template` on the route fact itself, so
those routes join at their absolute path **without** the mount family. The mount
families are additive cross-file value, not a correctness dependency.

## D. Out of scope (documented exclusions, no Miller work)

Per design §2c "Explicitly OUT": `nestjs.global_prefix` (no safe consumer /
`mount_target`), Ktor server (deferred lane), `RouterFunction`/`coRouter`,
Symfony `#[Route]`, axum 0.7 `:id` recording, and all interpolated / concat /
heredoc / sigil / const / cross-file-non-literal route args (M2 silence).

## E. Consumer-side correctness note (from Task 3)

`Route::match([...])` (Laravel) and `via: [...]` (Rails) emit one fact per verb
sharing the same source span → the **same stable fact id**. Confirm the SQLite
write path does not treat fact id as a sole PK that silently drops one row on
collision. This mirrors shipped `rails.route.v1` behavior (pre-existing, not new
in v2.8.0), but it is worth an explicit check on the Miller/artifact read side.
