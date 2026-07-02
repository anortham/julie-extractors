# Backend HTTP Boundary Coverage Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Extend both sides of the HTTP boundary beyond the v2.6.x JS/ASP.NET coverage — route-handler definition facts for Express/Fastify, FastAPI/Flask/Django, Spring, Go (net/http, gin, echo), and Rails, plus `http.client_request.v1` coverage for Python, C#, Go, Java, and Ruby — so Miller can bridge client requests to handlers in every major backend ecosystem.

**Architecture:** All new families ride the existing `structural_facts` row family through `collect_framework_structural_facts`. The 1,753-line `base/framework_structural_facts.rs` is split into a directory module first (mirroring the 2026-07-01 `web_structural_facts` split), then each ecosystem lands as its own focused module. A new shared `base/http_boundary.rs` helper owns the join-key semantics — URL classification, verb normalization, and route-template normalization — so `http.client_request.v1` metadata stays byte-identical across all ten languages and every handler family emits the same normalized join key.

**Tech Stack:** Rust, tree-sitter, structural-facts pipeline, golden fixtures + capability matrix, pattern registry conformance.

**Architecture Quality:** Approved shape: `base/framework_structural_facts/` directory module (mod.rs dispatch + one module per ecosystem + shared helpers), `base/http_boundary.rs` for cross-family join-key helpers. Caller-facing interface unchanged (`collect_framework_structural_facts(language, tree, file_path, content, symbols)`); the real public contract is pattern IDs + metadata keys in the registry. Risk is medium: large contract surface, heuristic source detection consistent with existing collectors, M2 silence doctrine applies to every family. If code reality contradicts a decided shape, workers report a plan mismatch rather than redesigning locally.

## Global Constraints

- Contracts are API: every new pattern id and metadata key lands in `crates/julie-extractors/src/base/structural_fact_registry.rs`, the regenerated `docs/contracts/structural-fact-patterns.json`, `docs/contracts/jsonl-v3.md`, and `docs/contracts/sqlite-schema-v3.md` in the same task that ships it. The registry conformance test (`registry_pattern_ids_match_emitted_union_per_language`, `structural_fact_registry.rs:3538`) and the checked-in-JSON sync test must stay green per task.
- Capability claims require golden fixture evidence in `fixtures/extraction/capabilities.json`; after capability or fixture changes run `node scripts/language-data-quality-report.mjs --strict` and keep `silent_cells` and `quality_bar_debts` at `0`.
- Silence doctrine (M2): dynamic or unresolvable URL/route expressions stay silent — no guessed routes, static string literals only. Template literals, f-strings, interpolation, string concatenation, and identifier arguments never emit.
- Naming rule: client-side references carry `target_path` + `url_kind`; handler definitions carry `route_template` (raw, exactly as written, including trailing slash when present) and `normalized_route_template` (the cross-family join key — see Cross-Family Doctrine). Do not rename existing ASP.NET keys.
- One `EXTRACTION_CONTRACT_VERSION` bump for this plan: append `.backend-http-boundary-v1` (`crates/julie-extractors/src/lib.rs:127`) and add the marker to `crates/julie-extractors/src/tests/api_surface.rs` (`test_public_contract_version_marks_current_fact_families`) with the first shape-changing task (Task 1); verify in the release task.
- Grounding (razorback:grounding-in-current-docs): every ecosystem task (2–7) starts with a grounding check against current framework docs for the exact registration syntax, verb sets, and param-syntax flavor decided below. Record the checked source in the task commit message. If docs contradict a decided contract, stop and report — contract adjudication is strategy-tier per RAZORBACK.md.
- Handler fact spans: place the fact span on the registration/decorator/annotation node so `containing_symbol_id` binds to the handler symbol under the v2.6.1 scope-bearing binding semantics (`docs/plans/2026-07-02-containing-symbol-binding-fix.md`). Every ecosystem task must include a binding assertion in its tests.
- Tests prove behavior through emitted `structural_facts` rows; helper unit tests may supplement, never substitute.
- Default suite stays under the 90s tripwire. Each ecosystem's tests get narrow, per-language test modules so agents can test one ecosystem without paying for all.
- Per-ecosystem branch gates: Tasks 2–7 each land on their own feature branch and pass the branch gate before merge. Tasks 2–7 are independent of each other once Task 1 is merged and may execute in any order. Collector file ownership is non-overlapping (one ecosystem module + one `http_clients/<language>.rs` file per task), but four touchpoints are shared across tasks by nature — `framework_structural_facts/mod.rs` dispatch arms, `structural_fact_registry.rs` specs + regenerated JSON, the two contract docs, and `tests/mod.rs` — so parallel sessions must serialize their merges to main and rebase before the branch gate; conflicts in those four files are expected to be additive and trivially resolvable.
- Releases require explicit user approval. One release (v2.7.0) at the end; no mid-lane publishes.
- `languages/*.toml` `[literal_carriers] url` lists: each client-side task verifies the client methods it detects are present in that language's url carrier list and adds any missing ones (URL literal tagging is complementary evidence, not a substitute for `http.client_request.v1` facts).

---

## Cross-Family Doctrine

These rules bind every new handler family. They extend the M6 naming rule and the 2026-07-01 codex-review reconciliation; they are recorded durably in `docs/decisions/0004-http-boundary-join-contract.md`.

1. **`normalized_route_template` is the universal server-side join key.** Every handler fact emits it: compute `normalize(effective_route_template if present else route_template)` where `normalize` (a) ensures a leading `/`, (b) converts each framework's param syntax to the `:param` flavor (`{id}`→`:id`, `<int:id>`→`:id`, `{id:int}`→`:id`, `*filepath`→`:filepath`, `{path...}`→`:path`), and (c) strips converter/constraint annotations. Normalization preserves trailing slashes exactly as written after parameter conversion; `/users/:id` and `/users/:id/` stay distinct join keys. The normalizer lives in `base/http_boundary.rs` with table-driven per-framework flavor rules and exhaustive unit tests. Regex route syntaxes (Django `re_path`) cannot be honestly normalized — those facts omit `normalized_route_template` and set `route_syntax="regex"`.
2. **Existing ASP.NET families join the doctrine.** `aspnet.minimal_api.route.v1` and `aspnet.attribute_route.v1` gain an optional `normalized_route_template` key (computed from `effective_route_template` when present, else `route_template`). Adding an optional key is a compatible v1 addition; pattern ids do not bump. Miller's server-side join key becomes `normalized_route_template` everywhere (falling back to `effective_route_template` for older artifacts).
3. **Same-file prefix resolution, cross-file mount facts.** When a route prefix is declared in the same file (ASP.NET `MapGroup` precedent), resolve it: emit `route_group_prefix` (or the family's named equivalent) + `effective_route_template` on the handler fact. Mount-site calls still emit their mount fact when they match the mount contract, even if the mounted receiver is also traceable in the same file; the resolved handler fact is the same-file convenience, and the mount fact is the durable source evidence Miller can use for cross-file joins. When the prefix target is not traceable in the same file, emit only the dedicated mount-fact family at the mount site (`express.router_mount.v1`, `fastapi.include_router.v1`, `flask.blueprint_registration.v1`, `django.url_include.v1`, `rails.mount.v1`) so Miller owns the cross-file join. Extractors never guess cross-file prefixes.
4. **Verb rules.** `verb` is uppercase; `verb_source` is `"attested"` (explicit method name, decorator, annotation, methods-list, or pattern prefix) or `"default"` (framework-documented default, e.g. bare Flask `@app.route` → GET, bare Java `HttpRequest` builder → GET). Registrations that accept any method (Express `app.all`/`app.use`, Django `path()`, Go patterns without a method prefix, verb-suffix-less handlers) omit both `verb` and `verb_source` — omission means "not verb-restricted", per the `nuxt.server_route.v1` precedent. Registrations naming multiple verbs (Flask `methods=[...]`, Fastify `method: [...]`, Spring `method = {GET, POST}`) emit one fact per verb.
5. **Receiver attestation.** Call-based registration (Express, Fastify, gin, echo, ServeMux) only emits when the receiver is traceable in-file to the framework's constructor (`express()`, `express.Router()`, `fastify()`, `gin.Default()`, `gin.New()`, `echo.New()`, `http.NewServeMux()`, or a `Group(...)` derived from one), with the framework import present (alias-aware). Receivers passed in as parameters are not traceable and stay silent, with the two documented exceptions in Task 2 (Fastify plugin params) — every family documents its exclusions in the contract docs.
6. **Metadata baseline.** Every handler-definition fact carries `pattern_version` (1), `query_family="framework"`, `framework`, `api_style` (fixed per family: `"call_routing"`, `"decorator_routing"`, `"annotation_routing"`, `"dsl_routing"`, `"mux_routing"`), `route_template`, `normalized_route_template` (except regex syntax), and `dynamic_segments` (param names, omitted when empty). The baseline applies to route-template-bearing families only: mount-point families carry `mount_path`/`normalized_mount_path`/`mount_target` instead, and `rails.resource_route.v1` is a resource-declaration family with no template to record (see its contract). Client facts keep the existing `http.client_request.v1` key set exactly.

## Decided Fact Contracts

### `http.client_request.v1` — language extension (references, client side)

Existing keys unchanged: `pattern_version`, `query_family="web.http_client"`, `framework` (the HTTP-client label — same value as `client`, matching current emission), `client`, `target_path`, `url_kind` (`"path"` | `"relative"` | `"absolute"`), `verb`, `verb_source`, `import_source` (optional). All ten languages emit this exact key set. Current JS/TS/Vue client facts continue to be collected from `web_structural_facts/http_client.rs`; the five new backend-language client collectors live under `framework_structural_facts/http_clients/` and reuse the same `http_boundary::client_request_metadata(...)` builder. This split is collector ownership only: `http.client_request.v1` remains one public pattern id with one metadata contract. New languages and `client` values:

| Language | `client` | Detected forms | Gate |
|---|---|---|---|
| python | `"requests"`, `"httpx"` | `requests.get/post/put/patch/delete/head/options("lit")`, `requests.request("VERB", "lit")`; same for `httpx.*` | module import (`import requests` / `import httpx`), alias-aware; qualified attribute calls on the imported module binding only |
| csharp | `"httpclient"` | `<recv>.GetAsync/GetStringAsync/GetByteArrayAsync/GetStreamAsync/GetFromJsonAsync/PostAsync/PostAsJsonAsync/PutAsync/PutAsJsonAsync/PatchAsync/PatchAsJsonAsync/DeleteAsync/DeleteFromJsonAsync("lit")`; `new HttpRequestMessage(HttpMethod.X, "lit")` | method-name allowlist + URL-shaped literal required (`url_kind` must be `path` or `absolute`); receiver type is not statically known, so bare-word literals stay silent |
| go | `"net/http"` | `http.Get/Post/PostForm/Head("lit")`, `http.NewRequest("VERB", "lit", …)`, `http.NewRequestWithContext(ctx, "VERB", "lit", …)` | `net/http` import, alias-aware package qualifier |
| java | `"java.net.http"` | `HttpRequest.newBuilder(URI.create("lit"))` and `.uri(URI.create("lit"))` chains; verb from same-statement `.GET()/.POST(…)/.PUT(…)/.DELETE()/.method("VERB", …)`; no verb call → `verb=GET`, `verb_source="default"` (JDK default) | `java.net.http` import present; chain analysis is same-statement only |
| ruby | `"net::http"` | `Net::HTTP.get/get_response/post/post_form(URI("lit")|URI.parse("lit"), …)` | fully qualified `Net::HTTP.` scope resolution (stdlib; no require gate) — unwrap the `URI(…)`/`URI.parse(…)` literal |

Verb attestation: verb-named methods and explicit `"VERB"` string literals are `"attested"`. Instance/session clients (`session.get`, `client.get`, `httpx.Client()` instances) stay silent in v1 — receiver typing is not static; document the exclusion per language.

### `express.route.v1` (definitions)

Languages: `javascript`, `jsx`, `typescript`, `tsx`. Gate: express import (CJS `require('express')` or ESM, alias-aware) + receiver traceable in-file to `express()` or `<express>.Router()`. Forms: `app.get/post/put/patch/delete/head/options("lit", …)`; `app.all("lit", …)` (verb omitted); `app.route("lit").get(h).post(h)` chains (one fact per chained verb). Metadata: `framework="express"`, `api_style="call_routing"`, baseline keys; Express params are already `:id` flavor, so `normalized_route_template` mostly equals the leading-slash-normalized template. Same-file mounts (`app.use("/prefix", router)` where `router` is defined in-file) resolve into `route_group_prefix` + `effective_route_template` on the router's facts and also emit `express.router_mount.v1` at the `app.use` site.

### `express.router_mount.v1` (mount points)

Languages: `javascript`, `jsx`, `typescript`, `tsx`. `app.use("lit", X)` with a static string first argument and at least one non-literal second argument. Metadata: `framework="express"`, `mount_path` (raw), `normalized_mount_path`, `mount_target` (source text of the mounted expression, e.g. `usersRouter`). Middleware and routers are not distinguishable statically — the contract documents that `mount_target` may name middleware; Miller filters at join time. `app.use(fn)` without a path literal stays silent.

### `fastify.route.v1` (definitions)

Languages: `javascript`, `jsx`, `typescript`, `tsx`. Gates (two receiver classes, both documented): (1) receiver traceable in-file to `fastify()`/`Fastify()` with the fastify import present; (2) the first parameter of an exported plugin function when that parameter is named `fastify` or `app` and the call matches a verb-method + static-literal-path shape. Forms: `f.get/post/put/patch/delete/head/options("lit", …)`; `f.all("lit", …)` (verb omitted); `f.route({ method: "GET" | ["GET","POST"], url: "lit", … })` (one fact per verb; both `method` and `url` must be static literals). Params are `:id` flavor. Metadata: `framework="fastify"`, `api_style="call_routing"`, baseline keys.

### `fastapi.route.v1` (definitions)

Language: `python`. Gate: fastapi import; decorator receiver traceable in-file to `FastAPI()` or `APIRouter()` (alias-aware through the import binding). Forms: `@app.get/post/put/patch/delete/head/options/trace("lit")`; `@app.api_route("lit", methods=["GET", …])` (one fact per literal verb). `APIRouter(prefix="/lit")` in the same file contributes `router_prefix` + `effective_route_template`. Param flavor `{id}`/`{id:path}` → `:id`. Metadata: `framework="fastapi"`, `api_style="decorator_routing"`, baseline keys + optional `router_prefix`, `effective_route_template`. Span on the decorator so the fact binds to the decorated function symbol.

### `fastapi.include_router.v1` (mount points)

Language: `python`. `app.include_router(x, prefix="lit", …)` — `mount_target` (source text of the router argument), optional `mount_path` (the `prefix` literal) + `normalized_mount_path`. `framework="fastapi"`.

### `flask.route.v1` (definitions)

Language: `python`. Gate: flask import; decorator receiver traceable in-file to `Flask(...)` or `Blueprint(...)`. Forms: `@app.route("lit")` (no `methods` kwarg → `verb=GET`, `verb_source="default"` per Flask's documented default); `@app.route("lit", methods=["GET","POST"])` (one fact per literal verb, attested); `@app.get/post/put/patch/delete("lit")` (Flask ≥2.0 shortcuts, attested). `Blueprint("name", __name__, url_prefix="/lit")` in the same file contributes `blueprint` (name literal), `url_prefix`, and `effective_route_template`. Param flavor `<id>`/`<int:id>` → `:id`. Metadata: `framework="flask"`, `api_style="decorator_routing"`, baseline keys + optional `blueprint`, `url_prefix`, `effective_route_template`. `add_url_rule` calls stay silent (documented exclusion).

### `flask.blueprint_registration.v1` (mount points)

Language: `python`. `app.register_blueprint(bp, url_prefix="lit")` — `mount_target` (source text), optional `mount_path` + `normalized_mount_path` from a literal `url_prefix`. `framework="flask"`.

### `django.url_pattern.v1` (definitions)

Language: `python`. Gate: `django.urls` import providing `path`/`re_path` (alias-aware). Forms: `path("lit", view, name="lit")`, `re_path(r"lit", view, name="lit")` at any position in the file (urls.py convention is not required — the import gate is the evidence). Metadata: `framework="django"`, `api_style="dsl_routing"`, `route_template` (raw), `route_syntax` (`"path"` | `"regex"`), `normalized_route_template` + `dynamic_segments` for `path` syntax (`<int:pk>` → `:pk`), omitted for regex; optional `route_name` from a literal `name=` kwarg (Django's `reverse()` join key); `view_target` (source text of the view argument, e.g. `views.detail`). No `verb` (Django dispatches methods in views) — documented.

### `django.url_include.v1` (mount points)

Language: `python`. `path("lit", include("app.urls"))` and `include((…), namespace="lit")` forms — `mount_path` (raw) + `normalized_mount_path`, `included_module` (string literal or source text), optional `namespace` literal. `framework="django"`.

### `spring.request_mapping.v1` (definitions)

Language: `java`. Gate: an import of `org.springframework.web.bind.annotation` (wildcard `.*` or specific mapping-annotation members) — a broader `org.springframework` gate would over-emit on non-MVC Spring code. Class-level `@RequestMapping("lit")` (or `value=`/`path=` element, including array values) emits an `attribute_kind="class_route"` fact. Method-level `@GetMapping/@PostMapping/@PutMapping/@PatchMapping/@DeleteMapping("lit")` emit `attribute_kind="http_method"` facts (verb from the annotation name, attested); `@RequestMapping(method = RequestMethod.GET, path = "lit")` emits per named verb; `@RequestMapping("lit")` on a method without a `method` element emits `attribute_kind="request_mapping"` with `verb` omitted. Array template values (`@GetMapping({"/a","/b"})`) emit one fact per template. Method facts carry `class_route_template` (nearest class-level literal) and `effective_route_template` (class + method joined via the existing `join_route_templates` semantics). Param flavor `{id}` → `:id`. Metadata: `framework="spring"`, `api_style="annotation_routing"`, baseline keys + `attribute_kind`, optional `class_route_template`, `effective_route_template`. Non-literal templates stay silent.

### `go.net_http.route.v1` (definitions)

Language: `go`. Gate: `net/http` import (alias-aware). Forms: `http.Handle("lit", h)`, `http.HandleFunc("lit", h)`, and `mux.Handle/HandleFunc("lit", h)` where `mux` is traceable in-file to `http.NewServeMux()`. Go 1.22+ pattern grammar is `[METHOD ][HOST]/[PATH]` — parse the three parts separately: a leading method token sets `verb` (attested; the contract doc notes Go's GET-also-matches-HEAD rule is framework semantics left to consumers, the fact records what is written); a host segment before the first `/` is recorded in an optional `host` key and excluded from `normalized_route_template` (which is computed from the path part only, so host-scoped and host-less routes normalize to the same join key — Miller can use `host` to disambiguate). `route_template` carries the path part of the pattern (trailing-slash subtree markers kept as written) with `verb`/`host` split into their own keys — adjudicated during the 2026-07-02 review fix lane: shipped goldens, tests, and the join semantics all treat `route_template` as a path, and the raw pattern is recoverable from the fact span; `{id}` → `:id`, `{path...}` → `:path`. Metadata: `framework="net/http"`, `api_style="mux_routing"`, baseline keys + optional `host`.

### `gin.route.v1` (definitions)

Language: `go`. Gate: `github.com/gin-gonic/gin` import; receiver traceable in-file to `gin.Default()`/`gin.New()` or a `Group("lit")` derived from one (nested groups compose their literal prefixes). Forms: `r.GET/POST/PUT/PATCH/DELETE/HEAD/OPTIONS("lit", …)`, `r.Any("lit", …)` (verb omitted), `r.Handle("VERB", "lit", …)` (literal verb attested). Group prefixes resolve same-file into `route_group_prefix` + `effective_route_template` (MapGroup precedent). Param flavor `:id` kept; `*filepath` → `:filepath` (recorded in `dynamic_segments`). Metadata: `framework="gin"`, `api_style="call_routing"`, baseline keys + optional `route_group_prefix`, `effective_route_template`.

### `echo.route.v1` (definitions)

Language: `go`. Gate: `github.com/labstack/echo` import (any major version path); receiver traceable in-file to `echo.New()` or a derived `Group("lit")`. Forms: `e.GET/POST/PUT/PATCH/DELETE/HEAD/OPTIONS("lit", …)`, `e.Any("lit", …)` (verb omitted). Group prefixes resolve same-file. Param flavor `:id` kept; a bare `*` wildcard stays `*` in the normalized template with no dynamic-segment name. Metadata: `framework="echo"`, `api_style="call_routing"`, baseline keys + optional `route_group_prefix`, `effective_route_template`.

### `rails.route.v1` (definitions)

Language: `ruby`. Gate: for `config/routes.rb`, routes must be inside a `routes.draw` block; for `config/routes/**/*.rb` (Rails 6.1+ split route files loaded via `draw :name`), the DSL appears at top level with no wrapping draw block — the path convention alone is the gate there. Forms: `get/post/put/patch/delete "lit"` with optional `to: "controller#action"` (recorded as `controller_action` when a literal), `as: :name` (recorded as `route_name`); `root "controller#action"` / `root to: "controller#action"` (emits `route_template="/"`, verb GET attested — Rails defines root as GET `/`); `match "lit", via: [:get, :post]` (one fact per literal via verb; `via: :all` → verb omitted; `match` without `via:` stays silent — Rails raises on it anyway). Enclosing `namespace :x do … end` and `scope "/lit" do … end` blocks contribute a joined `scope_path`, and `effective_route_template` = scope path + template (same-file by construction — routes.rb nesting). Rails templates already use `:id` flavor. Metadata: `framework="rails"`, `api_style="dsl_routing"`, baseline keys + optional `controller_action`, `route_name`, `scope_path`, `effective_route_template`. Non-DSL helpers (`concern`, `constraints`, `direct`, custom mapper extensions) stay silent — documented exclusions.

### `rails.resource_route.v1` (definitions)

Language: `ruby`. Same gate as `rails.route.v1`. Forms: `resources :users` and `resource :profile`, with `only:`/`except:` literal symbol arrays recorded as string arrays. One fact per `resources`/`resource` call — the seven RESTful expansions are Rails semantics and belong to Miller, not the extractor (M2: extract what is written). **This is a resource-declaration family, not a handler-definition family** — the Cross-Family Doctrine's handler baseline (rule 6) explicitly does not apply: there is no verbatim route template to record, so `verb`, `route_template`, and `normalized_route_template` are all absent by design, and the contract doc says so. Metadata: `framework="rails"`, `api_style="dsl_routing"`, `resource_name`, `resource_kind` (`"collection"` | `"singular"`), optional `only`/`except` (string arrays), optional `scope_path` from enclosing namespace/scope/nested-resources blocks (nested `resources` contribute their parent resource name to `scope_path` as written, e.g. `/users` — no `:user_id` synthesis).

### `rails.mount.v1` (mount points)

Language: `ruby`. Same gate as `rails.route.v1`. Forms: `mount X => "/lit"` and `mount X, at: "/lit"` (Rack apps and Rails engines). Metadata: `framework="rails"`, `mount_path` (raw literal) + `normalized_mount_path`, `mount_target` (source text of the mounted expression, e.g. `API::Base` or `Sidekiq::Web`), optional `scope_path` from enclosing blocks. Non-literal mount paths stay silent.

### ASP.NET normalized-key addition

`aspnet.minimal_api.route.v1` and `aspnet.attribute_route.v1` gain optional `normalized_route_template` (from `effective_route_template` when present, else `route_template`; `{id}`/`{id:int}` → `:id`, `{**slug}`/`{*slug}` → `:slug`). Emitted only when the source template is fully literal (which is already the emission gate). Pattern versions stay 1; registry, JSON contract, and docs updated; existing goldens regenerate with the new key.

## File Structure

- Split: `crates/julie-extractors/src/base/framework_structural_facts.rs` → `crates/julie-extractors/src/base/framework_structural_facts/{mod.rs, aspnet.rs, markup.rs, razor.rs, helpers.rs}` (Task 1; exact seam placement follows the file's existing sections — aspnet collectors :372–:806, markup/htmx :1008–:1122, razor :101–:111, shared string/template helpers :546–:616, :1286)
- Create: `crates/julie-extractors/src/base/http_boundary.rs` — normalizer + shared client-request metadata builder + `url_kind` classification (hoisted from `web_structural_facts/http_client.rs`)
- Create: `crates/julie-extractors/src/base/framework_structural_facts/{node.rs, python_web.rs, spring.rs, go_http.rs, rails.rs}` — one ecosystem per module
- Create: `crates/julie-extractors/src/base/framework_structural_facts/http_clients/{mod.rs, python.rs, csharp.rs, go.rs, java.rs, ruby.rs}` — Task 1 scaffolds the directory, dispatch wiring, and five empty per-language modules so Tasks 3–7 each own exactly one file here and never touch each other's (parallel-session safety)
- Modify: `crates/julie-extractors/src/base/framework_structural_facts/mod.rs` — language dispatch arms (python, java, go, ruby; extended csharp/js arms) + `framework_structural_fact_pattern_ids_for_language` table
- Modify: `crates/julie-extractors/src/base/web_structural_facts/http_client.rs` — consume the hoisted `http_boundary` helpers (behavior unchanged)
- Collector ownership note: JS/TS/Vue client facts stay in `web_structural_facts/http_client.rs`; new backend-language client facts live under `framework_structural_facts/http_clients/`. Both paths emit the same `http.client_request.v1` pattern id and must use the shared metadata builder so the key set cannot drift.
- Modify: `crates/julie-extractors/src/base/structural_fact_registry.rs` — 16 new pattern specs + ASP.NET key additions; regenerate `docs/contracts/structural-fact-patterns.json`
- Modify: `crates/julie-extractors/src/lib.rs:127`, `crates/julie-extractors/src/tests/api_surface.rs` — contract marker
- Modify: `docs/contracts/jsonl-v3.md`, `docs/contracts/sqlite-schema-v3.md` — pattern tables + metadata keys per task
- Modify: `fixtures/extraction/**`, `fixtures/extraction/capabilities.json`, `languages/{python,csharp,go,java,ruby}.toml` — golden/capability evidence + url literal carriers per task
- Already committed with this plan: `docs/decisions/0004-http-boundary-join-contract.md` — the Cross-Family Doctrine as a durable decision (Task 1 amends it only if grounding checks change a decided rule)
- Test: new modules under `crates/julie-extractors/src/tests/` per ecosystem (registered in `tests/mod.rs` — an unregistered module silently never runs): `express/`, `fastify/`, `python_web/`, `spring/`, `go_http/`, `rails_routes/`, plus extensions to `tests/http_client/` and `tests/structural_facts.rs`

## Task 1: Module Split + Shared Join-Key Helpers + ASP.NET Normalized Key

**Files:**
- Split: `crates/julie-extractors/src/base/framework_structural_facts.rs` → directory module per File Structure
- Create: `crates/julie-extractors/src/base/http_boundary.rs`
- Modify: `crates/julie-extractors/src/base/web_structural_facts/http_client.rs` (consume hoisted helpers)
- Modify: `crates/julie-extractors/src/base/structural_fact_registry.rs` (ASP.NET `normalized_route_template` key specs), regenerate `docs/contracts/structural-fact-patterns.json`
- Modify: `crates/julie-extractors/src/lib.rs:127` + `crates/julie-extractors/src/tests/api_surface.rs` (marker `.backend-http-boundary-v1`)
- Modify: `docs/contracts/jsonl-v3.md`, `docs/contracts/sqlite-schema-v3.md` (normalized-key addition on both ASP.NET families)
- Test: normalizer unit tests in `http_boundary.rs`; ASP.NET golden updates; `include_str!`-based convention tests keeping `framework_structural_facts/mod.rs` dispatch-only, matching the guardrail family from the web_structural_facts split (its Task 5 pattern)

**Interfaces:**
- Consumes: existing collectors unchanged; `web_structural_facts/http_client.rs` `url_kind`/metadata assembly moves, not copies.
- Produces: `http_boundary::normalize_route_template(template: &str, flavor: ParamFlavor) -> NormalizedTemplate` (returns normalized string + dynamic segment names), `http_boundary::classify_url(literal: &str) -> UrlKind`, `http_boundary::client_request_metadata(...)` — every later task consumes these; `ParamFlavor` variants: `Colon`, `Braces`, `AngleBrackets`, `BracesWithDots`, `GinWildcard` (exact enum shape may follow code reality; the flavor table in the doctrine is the contract).

**What to build:** Pure-refactor split of the framework facts module (goldens byte-identical), then the shared normalizer/classifier with exhaustive table-driven unit tests, then wire the ASP.NET families' optional `normalized_route_template` (the only behavior change — goldens regenerate once, diff reviewed to show only added keys). Also scaffold `framework_structural_facts/http_clients/` — `mod.rs` dispatch plus five empty per-language modules (`python.rs`, `csharp.rs`, `go.rs`, `java.rs`, `ruby.rs`) wired but emitting nothing — so Tasks 3–7 own one file each with no cross-task file conflicts. The empty modules are wiring scaffolding sanctioned by this plan, not stub behavior: they emit no facts until their ecosystem task lands.

**Approach:** Split first, in its own commit, proven byte-identical before any behavior change. The normalizer is the highest-leverage correctness point in the plan — get the flavor table and edge cases (optional params, catch-alls, constraint annotations, empty templates, missing leading slash) locked here so Tasks 2–7 never reimplement normalization.

**Acceptance criteria:**
- [x] Split commit: `cargo test -p julie-extractors structural_facts` green with goldens byte-identical; `mod.rs` holds only dispatch, pattern-id constants, and re-exports; convention tests enforce it.
- [x] `http_boundary` normalizer handles every flavor in the doctrine table with unit tests per flavor, including catch-all, constraint-annotation, and trailing-slash preservation cases.
- [x] ASP.NET families emit `normalized_route_template`; registry + JSON contract + docs updated; golden diff shows only added keys.
- [x] `http_clients/` scaffolding in place: five per-language modules wired into dispatch, emitting nothing, workspace green.
- [x] Marker bump + api_surface list updated; `docs/decisions/0004-http-boundary-join-contract.md` verified accurate against the implemented normalizer (amend if grounding changed a rule).
- [x] Worker-scope verification passes, committed.

## Task 2: Node — Express + Fastify

**Files:**
- Create: `crates/julie-extractors/src/base/framework_structural_facts/node.rs`
- Modify: `crates/julie-extractors/src/base/framework_structural_facts/mod.rs` (js-family arms gain node collectors; pattern-id table)
- Modify: `crates/julie-extractors/src/base/structural_fact_registry.rs` + regenerate JSON (3 new specs)
- Modify: `docs/contracts/jsonl-v3.md`, `docs/contracts/sqlite-schema-v3.md`
- Modify: `fixtures/extraction/{javascript,jsx,typescript,tsx}/{express_routes,express_mounts,fastify_routes}/` + `capabilities.json`
- Test: `crates/julie-extractors/src/tests/express/mod.rs`, `crates/julie-extractors/src/tests/fastify/mod.rs` (+ `tests/mod.rs` registration)

**Interfaces:**
- Consumes: `http_boundary` helpers (Task 1); `web_structural_facts/js_imports.rs` import index (extend with express/fastify entries); the JS string-literal and paren-matching helpers from `web_structural_facts/js_object_scan.rs`.
- Produces: `express.route.v1`, `express.router_mount.v1`, `fastify.route.v1` per the decided contracts.

**What to build:** Grounding check (Express 5 + Fastify 4/5 registration API surface, `app.route()` chain validity, Fastify shorthand set), then the in-file receiver tracer (assignments from `express()`/`express.Router()`/`fastify()`), the verb-method scanners, the `app.route(...)` chain walker, `app.use` mount facts, and Fastify's `route({...})` object form via the existing object-property parser.

**Approach:** Receiver tracing is the risk: keep it to single-assignment tracking (`const app = express()`), no dataflow. Negative cases that must stay silent: verb calls on untraceable receivers, template-literal paths, `app.use(fn)` without a path, `router.get` where `router` is a function parameter (except the two documented Fastify plugin-param exceptions). Vue is explicitly out (server frameworks don't live in SFCs); claim javascript/jsx/typescript/tsx parity.

**Acceptance criteria:**
- [x] `const app = express(); app.get("/users/:id", h)` emits `verb=GET`, `route_template=/users/:id`, `normalized_route_template=/users/:id`, `dynamic_segments=["id"]` in all four JS-family languages.
- [x] `app.route("/x").get(h).post(h)` emits two facts; `app.all` omits verb; `app.use("/api", usersRouter)` emits a mount fact with `mount_target=usersRouter`; when `usersRouter` is defined in the same file its route facts also carry `route_group_prefix=/api` and joined `effective_route_template`, and when it is not traceable only the mount fact emits.
- [x] Fastify instance and plugin-param forms emit; `f.route({method:["GET","POST"], url:"/x"})` emits two facts; non-literal `url` stays silent.
- [x] Binding assertion: route facts bind to the enclosing function/module symbol per v2.6.1 semantics.
- [x] Registry conformance + JSON sync green; contract docs updated; capability rows + goldens for four languages; strict report clean.
- [x] Worker-scope verification passes, committed; branch gate green before merge.

## Task 3: Python — FastAPI, Flask, Django + requests/httpx Clients

**Files:**
- Create: `crates/julie-extractors/src/base/framework_structural_facts/python_web.rs`
- Modify: `crates/julie-extractors/src/base/framework_structural_facts/http_clients/python.rs` (scaffolded empty in Task 1)
- Modify: `crates/julie-extractors/src/base/framework_structural_facts/mod.rs` (new `python` arm; pattern-id table)
- Modify: `crates/julie-extractors/src/base/structural_fact_registry.rs` + regenerate JSON (6 new specs + `http.client_request.v1` language extension)
- Modify: `docs/contracts/jsonl-v3.md`, `docs/contracts/sqlite-schema-v3.md`, `languages/python.toml` (url carriers: verify/extend `requests.*`/`httpx.*` method list)
- Modify: `fixtures/extraction/python/{fastapi_routes,flask_routes,django_urls,http_client}/` + `capabilities.json`
- Test: `crates/julie-extractors/src/tests/python_web/mod.rs` + extend `tests/http_client/` (+ `tests/mod.rs` registration)

**Interfaces:**
- Consumes: `http_boundary` helpers; python tree-sitter decorator/call nodes (the extractor already walks `decorated_definition` — see `python/decorators.rs`); python import bindings from `python/imports.rs` for alias-aware gating.
- Produces: `fastapi.route.v1`, `fastapi.include_router.v1`, `flask.route.v1`, `flask.blueprint_registration.v1`, `django.url_pattern.v1`, `django.url_include.v1`, python arm of `http.client_request.v1`.

**What to build:** Grounding check (FastAPI decorator verb set incl. `api_route`, Flask 2.x method shortcuts + default-methods semantics, Django 5 `path`/`re_path` import surface), then decorator scanners gated on in-file `FastAPI()`/`APIRouter()`/`Flask()`/`Blueprint()` constructor tracing, the `path()`/`re_path()` call scanner gated on `django.urls` imports, mount-fact emission per the doctrine, and the qualified-attribute-call client scanner for `requests`/`httpx`.

**Approach:** Python string subtleties: only plain string literals emit — f-strings, concatenation, and `str.format` stay silent; raw strings (`r"..."`) are literals and DO emit (Django re_path uses them). Flask default-verb rule: bare `@app.route` emits exactly one `GET`/`default` fact (not HEAD/OPTIONS — those are framework-implicit, not written). Decorator facts span the decorator node; assert binding to the decorated function symbol.

**Acceptance criteria:**
- [x] `@app.get("/users/{user_id}")` (FastAPI) emits `verb=GET` attested, `normalized_route_template=/users/:user_id`; `APIRouter(prefix="/api")` same-file yields `effective_route_template=/api/users/:user_id`.
- [x] `@app.route("/x")` (Flask) emits GET/default; `methods=["GET","POST"]` emits two attested facts; Blueprint `url_prefix` joins.
- [x] `path("users/<int:pk>/", views.detail, name="user-detail")` emits `route_syntax=path`, `normalized_route_template=/users/:pk/` (trailing slash preserved), `route_name=user-detail`, no verb; `re_path` emits `route_syntax=regex` with no normalized template; `path("api/", include("app.urls"))` emits `django.url_include.v1` rather than a handler-pattern fact.
- [x] `requests.get("https://api.example.com/users")` and `httpx.post("/x")` emit client facts with correct `client`/`import_source`; `session.get(...)` stays silent; un-imported `requests.get` stays silent.
- [x] Binding assertions; registry conformance + JSON sync; contract docs; `languages/python.toml` url carriers cover the detected client methods; capability rows + goldens; strict report clean.
- [x] Worker-scope verification passes, committed; branch gate green before merge.

## Task 4: C# — HttpClient Client Requests

**Files:**
- Modify: `crates/julie-extractors/src/base/framework_structural_facts/http_clients/csharp.rs` (scaffolded empty in Task 1)
- Modify: `crates/julie-extractors/src/base/framework_structural_facts/mod.rs` (csharp arm gains the client collector)
- Modify: `crates/julie-extractors/src/base/structural_fact_registry.rs` + regenerate JSON (`http.client_request.v1` + csharp)
- Modify: `docs/contracts/jsonl-v3.md`, `docs/contracts/sqlite-schema-v3.md`, `languages/csharp.toml` (url carriers)
- Modify: `fixtures/extraction/csharp/http_client/` + `capabilities.json`
- Test: extend `crates/julie-extractors/src/tests/http_client/`

**Interfaces:**
- Consumes: `http_boundary` helpers; `framework_structural_facts/helpers.rs` `parse_csharp_string_literal` (post-split home of :1286).
- Produces: csharp arm of `http.client_request.v1` per the decided contract.

**What to build:** Grounding check (`System.Net.Http.Json` extension-method names current), then the method-name-allowlist scanner with the URL-shape gate (`url_kind` must be `path` or `absolute`), plus `new HttpRequestMessage(HttpMethod.X, "lit")` constructor detection.

**Approach:** The URL-shape gate is the false-positive defense — `cache.GetAsync("user-key")` must stay silent. Verbatim strings (`@"/api/x"`) and raw string literals are literals and emit; interpolated strings (`$"..."`) stay silent. Verb derivation: method-name prefix before `Async`/`AsJsonAsync`/`FromJsonAsync`/`StringAsync`/etc. maps to GET/POST/PUT/PATCH/DELETE, always attested.

**Acceptance criteria:**
- [x] `await client.GetFromJsonAsync<User>("/api/users/1")` emits `client=httpclient`, `verb=GET` attested, `url_kind=path`; `cache.GetAsync("user-key")` stays silent (bare-word literal).
- [x] `new HttpRequestMessage(HttpMethod.Post, "https://api.example.com/x")` emits `verb=POST`.
- [x] Interpolated-string URLs stay silent; registry/JSON/docs/carriers/capabilities/goldens updated; strict report clean.
- [x] Worker-scope verification passes, committed; branch gate green before merge.

## Task 5: Java — Spring Handlers + java.net.http Client

**Files:**
- Create: `crates/julie-extractors/src/base/framework_structural_facts/spring.rs`
- Modify: `crates/julie-extractors/src/base/framework_structural_facts/http_clients/java.rs` (scaffolded empty in Task 1)
- Modify: `crates/julie-extractors/src/base/framework_structural_facts/mod.rs` (new `java` arm; pattern-id table)
- Modify: `crates/julie-extractors/src/base/structural_fact_registry.rs` + regenerate JSON (1 new spec + client extension)
- Modify: `docs/contracts/jsonl-v3.md`, `docs/contracts/sqlite-schema-v3.md`, `languages/java.toml` (url carriers)
- Modify: `fixtures/extraction/java/{spring_routes,http_client}/` + `capabilities.json`
- Test: `crates/julie-extractors/src/tests/spring/mod.rs` (+ `tests/mod.rs` registration) + extend `tests/http_client/`

**Interfaces:**
- Consumes: `http_boundary` helpers; java annotation nodes (see `java/annotations.rs` for the existing annotation walk); java import list from `java/imports_packages.rs`.
- Produces: `spring.request_mapping.v1`, java arm of `http.client_request.v1`.

**What to build:** Grounding check (Spring 6 annotation set + `RequestMethod` enum values, JDK HttpClient builder default-GET), then the class/method annotation scanner with class→method template joining (mirror `collect_attribute_routes_for_class`/`collect_attribute_routes_for_method` structure from the ASP.NET module), and the `HttpRequest.newBuilder`/`.uri` chain scanner.

**Approach:** Annotation-element parsing must handle: single string, `value=`/`path=` named elements, string arrays, and `method = RequestMethod.X` / `method = {RequestMethod.GET, RequestMethod.POST}`. Constants (`static final String PATH`) stay silent — literal-only doctrine. Kotlin Spring is a documented deferral (matrix cut line), recorded in capabilities notes, not `open_gaps` (the Kotlin language lacks no construct; the framework claim is simply not made).

**Acceptance criteria:**
- [x] `@RestController @RequestMapping("/api/users")` class + `@GetMapping("/{id}")` method emits `effective_route_template=/api/users/{id}`, `normalized_route_template=/api/users/:id`, `verb=GET` attested, `attribute_kind=http_method`.
- [x] `@RequestMapping(method = RequestMethod.POST, path = "/x")` emits POST; method-less `@RequestMapping` on a method omits verb; array templates emit one fact per template.
- [x] `HttpRequest.newBuilder(URI.create("https://api.example.com/users")).GET()` emits attested GET; builder without a verb call emits GET/default; non-literal URIs stay silent.
- [x] Binding assertions (class facts bind to class symbol, method facts to method symbol); registry/JSON/docs/carriers/capabilities/goldens; strict report clean.
- [x] Worker-scope verification passes, committed; branch gate green before merge.

## Task 6: Go — net/http, gin, echo Handlers + net/http Client

**Files:**
- Create: `crates/julie-extractors/src/base/framework_structural_facts/go_http.rs`
- Modify: `crates/julie-extractors/src/base/framework_structural_facts/http_clients/go.rs` (scaffolded empty in Task 1)
- Modify: `crates/julie-extractors/src/base/framework_structural_facts/mod.rs` (new `go` arm; pattern-id table)
- Modify: `crates/julie-extractors/src/base/structural_fact_registry.rs` + regenerate JSON (3 new specs + client extension)
- Modify: `docs/contracts/jsonl-v3.md`, `docs/contracts/sqlite-schema-v3.md`, `languages/go.toml` (url carriers — `http.get` etc. already listed; verify)
- Modify: `fixtures/extraction/go/{net_http_routes,gin_routes,echo_routes,http_client}/` + `capabilities.json`
- Test: `crates/julie-extractors/src/tests/go_http/mod.rs` (+ `tests/mod.rs` registration) + extend `tests/http_client/`

**Interfaces:**
- Consumes: `http_boundary` helpers; go import specs (`go/specs.rs` `extract_import_symbols` shows the import shapes; the collector needs its own lightweight import scan over the tree, alias-aware).
- Produces: `go.net_http.route.v1`, `gin.route.v1`, `echo.route.v1`, go arm of `http.client_request.v1`.

**What to build:** Grounding check (Go 1.22+ ServeMux pattern grammar — method prefix, host patterns, `{path...}`; gin/echo current verb-method sets and Group semantics), then the package-qualified call scanners with in-file receiver tracing (`http.NewServeMux()`, `gin.Default()/New()`, `echo.New()`, `Group("lit")` chains), the pattern-string parser for the 1.22 verb prefix, and the client-call scanner.

**Approach:** Import alias handling is mandatory in Go (`import g "github.com/gin-gonic/gin"`). Group tracing composes: `v1 := r.Group("/v1"); users := v1.Group("/users"); users.GET("/:id", h)` → `effective_route_template=/v1/users/:id`. Non-literal group prefixes poison the chain — routes registered on them emit with `route_template` only (no effective/prefix keys), never a guessed prefix.

**Acceptance criteria:**
- [x] `mux.HandleFunc("GET /users/{id}", h)` emits `verb=GET` attested, `normalized_route_template=/users/:id`; pattern without method prefix omits verb; `"GET example.com/users/{id}"` emits `host=example.com` with `normalized_route_template=/users/:id` (path part only).
- [x] Nested gin groups compose into `effective_route_template`; `r.Any` omits verb; `r.Handle("PUT", "/x", h)` emits PUT.
- [x] echo routes emit; `*` wildcard keeps `*` in normalized with no dynamic segment.
- [x] `http.NewRequest("POST", "https://api.example.com/x", body)` emits attested POST; aliased imports work; un-imported `http.Get` stays silent.
- [x] Binding assertions; registry/JSON/docs/carriers/capabilities/goldens; strict report clean.
- [x] Worker-scope verification passes, committed; branch gate green before merge.

## Task 7: Ruby — Rails Routes + Net::HTTP Client

**Files:**
- Create: `crates/julie-extractors/src/base/framework_structural_facts/rails.rs`
- Modify: `crates/julie-extractors/src/base/framework_structural_facts/http_clients/ruby.rs` (scaffolded empty in Task 1)
- Modify: `crates/julie-extractors/src/base/framework_structural_facts/mod.rs` (new `ruby` arm; pattern-id table)
- Modify: `crates/julie-extractors/src/base/structural_fact_registry.rs` + regenerate JSON (3 new specs + client extension)
- Modify: `docs/contracts/jsonl-v3.md`, `docs/contracts/sqlite-schema-v3.md`, `languages/ruby.toml` (url carriers)
- Modify: `fixtures/extraction/ruby/{rails_routes,http_client}/` + `capabilities.json`
- Test: `crates/julie-extractors/src/tests/rails_routes/mod.rs` (+ `tests/mod.rs` registration) + extend `tests/http_client/`

**Interfaces:**
- Consumes: `http_boundary` helpers; ruby call/block nodes (see `ruby/calls.rs` for the existing call walk and `ruby/helpers.rs` for literal parsing).
- Produces: `rails.route.v1`, `rails.resource_route.v1`, `rails.mount.v1`, ruby arm of `http.client_request.v1`.

**What to build:** Grounding check (Rails 7/8 routes DSL surface for the decided forms, including `root`, `match via:`, `mount`, and the Rails 6.1+ `draw :name` split-file convention), then the two-mode gate (draw block required for `config/routes.rb`; path convention alone for `config/routes/**/*.rb`), the verb-call/`root`/`match`/`resources`/`resource`/`mount` scanners with enclosing `namespace`/`scope`/`resources` block-nesting prefix composition, and the `Net::HTTP.` client scanner with `URI(...)`/`URI.parse(...)` unwrapping.

**Approach:** Block nesting is the core mechanism: walk tree-sitter `do_block`/`block` ancestry from each route call, collecting literal `namespace :x` (→ `/x`) and `scope "/y"` prefixes in order. Symbols (`:api`) and strings are both literal; interpolation or variables poison the prefix chain (emit with `route_template` only). `to:` with non-literal values → omit `controller_action`, still emit the route fact.

**Acceptance criteria:**
- [x] `namespace :api do get "users/:id", to: "users#show" end` emits `verb=GET`, `scope_path=/api`, `effective_route_template=/api/users/:id`, `controller_action=users#show`.
- [x] `resources :users, only: [:index, :show]` emits one resource fact with `resource_name=users`, `only=["index","show"]`; nested resources record parent in `scope_path`.
- [x] `root "home#index"` emits GET `/`; `match "legacy", via: [:get, :post]` emits two facts; `mount Sidekiq::Web => "/sidekiq"` emits a mount fact with `mount_target=Sidekiq::Web`.
- [x] A `config/routes/admin.rb` draw file with top-level DSL (no draw block) emits; routes outside `config/routes*` paths stay silent; a controller file with a `get` method call stays silent; DSL in `config/routes.rb` outside a draw block stays silent.
- [x] `Net::HTTP.get(URI("https://api.example.com/users"))` emits GET; `Net::HTTP.post_form(URI.parse("/x"), …)` emits POST; non-literal URIs stay silent.
- [x] Binding assertions; registry/JSON/docs/carriers/capabilities/goldens; strict report clean.
- [x] Worker-scope verification passes, committed; branch gate green before merge.

## Task 8: Contract Sweep + Emission-Agreement Tests

**Files:**
- Modify: `docs/contracts/jsonl-v3.md`, `docs/contracts/sqlite-schema-v3.md` (final key-by-key audit: the 16 new families, the five-language client extension, and the ASP.NET key additions, all against actual emission)
- Modify: `crates/julie-extractors/src/tests/structural_facts.rs` (extend the pinned-metadata-key-set tests — the pattern from `d0d188b` "pin metadata key sets for the four http boundary fact families" — to every new family)
- Modify: `fixtures/extraction/capabilities.json` (cross-ecosystem audit)

**Interfaces:**
- Consumes: Tasks 1–7 merged and green.
- Produces: emission-agreement coverage for every family this plan ships; the audited contract docs the release task publishes.

**What to build:** One pinned key-set test per new family (asserting exactly the documented always/optional keys appear); a doctrine test asserting every handler family that emits `normalized_route_template` produces `:param`-flavor output (property-style over the golden corpus); a cross-language client test asserting `http.client_request.v1` emits the identical key set in all ten languages.

**Acceptance criteria:**
- [x] Pinned key-set tests exist and pass for all 16 families + the client extension.
- [x] Doctrine test proves `:param` flavor holds corpus-wide; registry JSON matches registry code (sync test).
- [x] Contract docs audited key-by-key against emission; discrepancies fixed on the emission side unless the doc is wrong (adjudicate, record).
- [x] Worker-scope verification passes, committed.


### Implementation Progress Note - 2026-07-02

Tasks 1-8 are implemented in branch `codex/backend-http-boundary-v27`. Task 9 remains open because version bump, release notes, publishing, pushing, and Miller handoff require an explicit release/approval step.

## Task 9: Release v2.7.0 + Miller Handoff

**Files:**
- Modify: version metadata + release notes per `docs/release.md`

**Interfaces:**
- Consumes: Tasks 1–8 merged and green on main.
- Produces: a tagged v2.7.0 release consumable by Miller's `scripts/julie-pins.json` bump, and a Miller-side companion-plan handoff.

**What to build:** Branch/main gate, release notes calling out: the 16 new pattern ids, the `http.client_request.v1` five-language extension, the ASP.NET `normalized_route_template` addition, the `.backend-http-boundary-v1` marker, the Cross-Family Doctrine (normalized join key + mount-fact rule), and per-family documented exclusions. Do not publish without explicit user approval.

**Acceptance criteria:**
- [ ] `EXTRACTION_CONTRACT_VERSION` ends with `.backend-http-boundary-v1`; api_surface marker test green.
- [ ] Branch gate green; release notes list every consumer-visible change.
- [ ] User approval obtained before publishing.
- [ ] Miller handoff noted: companion plan to extend the fetch↔handler bridge to the new families — client `target_path` joins against `normalized_route_template` across all server families; mount facts (`express.router_mount.v1`, `fastapi.include_router.v1`, `flask.blueprint_registration.v1`, `django.url_include.v1`) are Miller's cross-file prefix-join inputs; `rails.resource_route.v1` is join-input requiring Rails-semantics expansion on Miller's side if desired.

## Verification Strategy

**Project source of truth:** `AGENTS.md` / `CLAUDE.md`, `xtask` test tiers.

**Worker red/green scope:** focused tests by exact name with fully qualified paths, e.g. `cargo test -p julie-extractors tests::express::<test_name> -- --nocapture`. Workers must confirm the filter matched at least one test ("0 tests run" is a FAIL — guards against unregistered test modules).

**Worker ceiling:** `cargo test -p julie-extractors structural_facts -- --nocapture` plus `cargo test -p julie-extractors test_public_contract_version_marks_current_fact_families -- --nocapture`.

**Worker gate invariant:** New facts emit with the decided metadata, negative cases stay silent, and no existing structural-fact family regresses (existing goldens byte-identical except files a task explicitly changes — Task 1's ASP.NET key addition is the one sanctioned golden change outside a task's own fixtures).

**Lead affected-change scope (after each task):**

```bash
cargo test -p julie-extractors structural_facts -- --nocapture
UPDATE_GOLDEN=1 cargo test -p julie-extractors --features test-golden golden_fixtures_match_canonical_extraction -- --nocapture
cargo test -p julie-extractors --features test-golden golden_fixtures_match_canonical_extraction -- --nocapture
cargo test -p julie-extractors --features test-capability-matrix capability_matrix -- --nocapture
node scripts/language-data-quality-report.mjs --strict
```

Lead review owns strategy-tier interpretation before merge: registry specs, checked-in contract JSON, `docs/contracts/*.md`, `fixtures/extraction/capabilities.json`, capability-matrix meaning, and `EXTRACTION_CONTRACT_VERSION` marker changes. Workers may prepare those deltas inside their slice, but they do not decide contract or capability semantics when evidence is ambiguous.

**Branch gate (per ecosystem branch, and before Task 9):**

```bash
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --no-deps -- -D warnings
git diff --check
node scripts/language-data-quality-report.mjs --strict
```

**Replay/metric evidence:** a real-repo CLI smoke scan per ecosystem (three-file fixture project proving rows persist to SQLite, matching the 2026-06-09 pattern) — hard gate per ecosystem task before it closes; row counts are report-only.

**Escalation triggers:** SQLite schema changes (none expected — metadata-only), report shape changes, new parser dependencies, language-detection changes, default-suite runtime growth past the tripwire, or grounding checks contradicting decided contracts.

**Assigned verification failure:** Workers stop and report when assigned verification fails, unless this plan explicitly says to update that gate.

**Verification ledger:** Record invariant, command, scope label, commit SHA, result, and timestamp per task. Reuse passing evidence for the same HEAD.

## Model Routing

**Project source of truth:** repo `RAZORBACK.md`.

**Strategy tier:** contract shapes (decided in this plan), grounding-check adjudication for every ecosystem task, emission-gate semantics disputes, capability-claim changes. Harness mapping: inherit.

**Implementation tier:** Tasks 1–8 implementation once contracts are locked (narrow file ownership, explicit ceilings, no parser-dependency changes). Workers may prepare registry/docs/capability edits required by their slice, but lead review owns contract/capability interpretation before merge. Harness mapping: inherit.

**Mechanical tier:** fixture file authoring only, and only when the fixture does not own the task's red/green gate. Harness mapping: inherit.

**Gate-interpretation reviewer:** lead reads plan + failing test + diff on any red/green dispute. Harness mapping: inherit.

**Escalation tier:** per RAZORBACK.md — capability-claim changes, contract doc changes, and anything touching `EXTRACTION_CONTRACT_VERSION` get lead review before commit. Harness mapping: inherit.

**Worker eligibility:** met once this plan's contracts stand; every ecosystem task's worker runs its grounding check first and stops if docs contradict the decided contract.

**Escalation triggers:** grounding contradictions; golden diffs outside a task's sanctioned scope; capability-matrix failures; receiver-tracing designs that exceed single-assignment tracking.

**Mechanical exclusion:** Mechanical workers cannot own failing tests, replay evidence, or acceptance gates.

**Unsupported harness behavior:** If the harness cannot choose models per agent, use `inherit` and continue.
