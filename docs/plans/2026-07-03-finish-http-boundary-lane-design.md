# Finish the HTTP Boundary Lane — Design

- **Date:** 2026-07-03
- **Status:** Draft for review (brainstorming output; Doubt Pass + Codex review folded in; pre-implementation-plan)
- **Owner:** julie-extractors
- **Predecessor:** v2.7.0 backend HTTP boundary lane (`docs/plans/2026-07-02-backend-http-boundary-coverage.md`, `docs/decisions/0004-http-boundary-join-contract.md`)
- **Target release:** v2.8.0

## 1. Goal

Complete the HTTP request↔handler boundary so Miller can bridge a client call to
its server handler across every mainstream backend stack. v2.7.0 shipped the lane
for Express/Fastify, FastAPI/Flask/Django, Spring (Java), Go (net/http, gin, echo),
and Rails, plus `http.client_request.v1` for python/csharp/go/java/ruby/js/ts. This
release fills the remaining mainstream gaps on **both** sides of the boundary and
adds **cross-file prefix-registration facts** so Miller can reconstruct absolute
public paths.

The theme is deliberately narrow: **finish and widen the boundary/join lanes.** No
new product surface (no editor/daemon/search), no new join semantics — cross-file
joining stays Miller's job (decision 0004).

## 2. Scope (locked)

Three sub-lanes, decided with the user.

### 2a. Server route facts — 6 frameworks

| Framework | Language(s) | Server pattern id(s) | Route param flavor | Feasibility |
|---|---|---|---|---|
| NestJS | typescript, javascript | `nestjs.route.v1` | `Colon` (`:id`) | high |
| Spring (Kotlin) | kotlin | `spring.request_mapping.v1` (extend `languages`) | `Braces` (`{id}`) | high |
| Laravel | php | `laravel.route.v1`, `laravel.resource_route.v1` | `Braces` (`{id}`, `{id?}`) | high |
| Phoenix | elixir | `phoenix.route.v1`, `phoenix.resource_route.v1` | `Colon` (`:id`) | high |
| axum | rust | `axum.route.v1` | `Braces` (0.8) | high |
| actix-web | rust | `actix.attribute_route.v1`, `actix.scope_route.v1` | `Braces` | attribute: high / scope: medium |

**actix is split into two pattern ids** (see §4.5), mirroring the shipped
`aspnet.attribute_route.v1` vs `aspnet.minimal_api.route.v1` precedent. The
attribute-macro family (`#[get("/x")]`) ships as a complete honest claim.

**`actix.scope_route.v1` is binary — fully shipped or fully absent, never a
registered-but-unemitted pattern.** The registry conformance test
(`registry_pattern_ids_match_emitted_union_per_language`) requires the registered
pattern-id set to equal the emitted set per language, so a registered spec with no
emission is a dead pattern that **fails the gate**. Therefore, if `scope_route` slips:
delete its pattern-id const, registry spec, and capability row entirely and record it
as a Rust `open_gaps` entry (reason: builder-idiom same-file scope tracing pending;
named closure task). It cannot be "listed but optional." Recommended default: attempt
to complete it (it is the medium-feasibility item); decide before writing the plan.

### 2b. Client-request facts — 4 languages

Add `http.client_request.v1` for the languages that lack it and now gain a server
side: **kotlin, php, elixir, rust**. (TS/JS already have it, so NestJS needs no
client work.) Reuse `http_boundary::client_request_metadata` / `classify_url`; add
per-language client arms in `base/http_clients/`.

**v2.8.0 ships one dominant, single-call-idiom client per language** (the shape the
existing collectors — `go.rs`, `java.rs`, `python.rs` — already prove; ~80–200 lines
of independent per-library detection each, one golden fixture each):

- kotlin: **Ktor client** (`client.get("...")`) — or OkHttp if the grammar shape is cleaner.
- php: **Guzzle** (`$client->get('...')`) + **Laravel `Http` facade** (`Http::get('...')`).
- elixir: **Req** (`Req.get("...")`).
- rust: **reqwest** (`reqwest::get("...")` / `client.get("...")`).

**Deferred as documented `open_gaps`** (each carries reason + named closure task;
their detection shapes — annotation-based Retrofit, fluent builder chains — are
unlike any proven collector and each needs its own fixture): Retrofit,
WebClient/RestTemplate, OkHttp (if not chosen above); Symfony HttpClient, `curl_*`;
Tesla, HTTPoison, Finch, `:httpc`; hyper, ureq.

Only **static URL/path literals** produce client facts (M2 silence); static-vs-dynamic
detection uses the AST check of §4.4, not a hand-rolled parser.

### 2c. Prefix-registration fact families

Emit a fact at the **definition site** of a route prefix so Miller can join it to
the routes it governs. Follows the existing `express.router_mount.v1` /
`rails.mount.v1` shape (per-framework — see §4.5 for why per-framework is correct).

| Framework | Prefix-registration pattern id | Emitted for |
|---|---|---|
| axum | `axum.nest.v1` | `.nest("/lit", sub_router)` — same-file mount |
| actix | `actix.mount.v1` | `web::scope("/lit")` bound to a `.configure(fn)` / `.service` |
| Laravel | `laravel.route_prefix.v1` | `Route::prefix('lit')`, `RouteServiceProvider` group prefix |
| Phoenix | `phoenix.forward.v1` | `forward "/lit", Plug` |

Rules: emit **only static literal** prefixes, at their own source location, with
`mount_path` / `normalized_mount_path` (and `mount_target` where a same-file target
exists). Cross-file joining of prefix→routes is Miller's job. Same-file resolvable
prefixes still also flow into `route_group_prefix` / `effective_route_template` on
the route fact itself (existing behavior).

**Miller integration is cross-repo work, and a whitelist entry alone does NOT make a
mount joinable.** Verified against the current Miller consumer: `IsMountFactPattern`
accepts only Express/FastAPI/Flask/Django (Rails is whitelisted but treated
**evidence-only**), and `RouteFamilyForMount` (`BackendHttpBridgeProvider.cs`) is a
hard-coded `switch` mapping each mount id to the route family it prefixes
(`ExpressRouterMount → ExpressRoute`, …). So for each new prefix family to actually
**join** (not sit inert as evidence-only), Miller needs, per family:

| Requirement (Miller side) | Where |
|---|---|
| pattern-id const | `BridgeStructuralPatterns.cs` |
| bridge whitelist entry | `BridgeFactPatternIds` (absent ⇒ silent no-op) |
| `IsMountFactPattern` arm | `StructuralRouteFactAdapter.cs` |
| **route-family mapping** | `RouteFamilyForMount` switch (`BackendHttpBridgeProvider.cs`) |
| **anchor rule** (`mount_target` vs `included_module`) | mount-composition read path |

This is the release-checklist deliverable for each of `axum.nest.v1`,
`actix.mount.v1`, `laravel.route_prefix.v1`, `phoenix.forward.v1`. It is genuine
cross-repo Miller work; the extractor-emitted facts are correct and valuable
regardless, and same-file-resolvable prefixes still degrade gracefully via
`route_group_prefix`/`effective_route_template` on the route fact — so the mount
families are additive value, not a correctness dependency for the extractor release.
Landing the Miller consumer side can trail the extractor release, but each family is
**evidence-only until its route-family mapping + anchor rule exist**, and the design
must not claim a working cross-file join before then.

### Explicitly OUT / deferred (documented exclusions, not silent gaps)

- **Ktor** — deferred for **release scope** (a full 7th framework lane: collector,
  pattern id, registry spec, fixtures, contract docs, tests), **not** a doctrine
  impossibility. A safe restricted lexical gate exists (§4.6) and is recorded as the
  `open_gaps` closure plan on the kotlin row.
- **`nestjs.global_prefix.v1`** (`app.setGlobalPrefix('api')`) — OUT. It has no
  `mount_target` (cannot flow through Miller's mount-read path), Miller has zero
  global-prefix consumption code and no app-boundary key, and an app-global prefix
  mis-fires in a monorepo with multiple Nest apps. A fact with no safe consumer.
  NestJS `@Controller` class prefix still flows into `effective_route_template` on
  the route fact. Reintroduce only as an explicitly app-scoped non-mount fact if a
  real consumer need is shown.
- **Kotlin `RouterFunction`/`coRouter` functional DSL** — `nest()`/`path()` nesting
  is not single-assignment same-file. Annotation routing is the Kotlin+Spring claim.
- **Interpolated / templated / heredoc / sigil / const / identifier route args**,
  and **cross-file prefixes not emitted at a literal definition site** — OUT by M2
  silence. Documented so `route_template` is not mistaken for the absolute public path.
- **PHP `#[Route]` attributes** — Symfony idiom, not Laravel. Future `symfony.route.v1`.
- **axum 0.7 `:id` param recording** — the extractor can't know the crate version;
  emit `Braces` (0.8). A 0.7 `:id` template passes through to a correct join key but
  won't populate `dynamic_segments`. Documented honest under-report; no version-sniff.

## 3. Product & doctrine constraints

- **M2 silence:** facts only for static route/URL literals; no guessed or dynamic
  routes. Silence is the default; a false positive is worse than a miss.
- **Same-file, single-assignment** receiver/prefix data-flow tracing only.
  Conflicting or non-literal assignments **poison** the trace (emit `route_template`
  only). *Lexical block/closure containment* (Rails `scope`, Phoenix `scope do`,
  Ktor `routing{}`) is a separate, already-accepted model — not governed by the
  single-assignment rule.
- **Verb omission = not verb-restricted** (omit `verb`/`verb_source` when the route
  accepts any method).
- **`normalized_route_template` (`:param` flavor) is the one server-side join key.**
  Do not invent a second join key.
- Cross-file join stays in Miller. Product boundary unchanged: facts only.
- Data-quality bar: `open_gaps` entries carry reason + required closure + planned
  task; `not_applicable` only when the language genuinely lacks the construct.

## 4. Architecture

### 4.1 Gate Mode summary

- **Affected modules:** `base/framework_structural_facts/` (6 collector modules +
  dispatch arms + pattern-id consts), `base/http_clients/` (4 client arms),
  `base/structural_fact_registry.rs` (new specs), `base/structural_facts.rs`
  (test-gated arms), `fixtures/extraction/capabilities.json` + golden fixtures,
  `docs/contracts/*`. `framework_structural_facts/scan.rs` `MaskLanguage` is **not**
  extended (see §4.4 — static-literal detection moves to the AST for new languages).
- **Caller-facing interface:** unchanged for downstream consumers — same
  `StructuralFact` rows in SQLite/JSONL, same `normalized_route_template` join key,
  additive pattern ids. `collect_framework_structural_facts` gains language arms;
  signature unchanged.
- **Depth/locality:** each framework is behavior-local to its collector + one
  dispatch arm. The only genuinely shared new surface is the AST static-literal
  helper (§4.4) reused across the 4 new languages.
- **Rejected shortcuts:** plugin-registration table to avoid dispatch churn
  (speculative — serialize merges instead); straight Java-collector reuse for Kotlin
  (bogus routes from `$`-interpolation / bracket arrays); hand-rolled mask lexers for
  the new languages' interpolation/heredoc/sigil forms (moves the silence guard into
  the most error-prone code — rejected for the AST check of §4.4).
- **Per-framework prefix families are kept (not generalized)** — see §4.5.
- **Architecture risk:** medium. Doubt Pass completed (§9); core doctrine unchanged.

### 4.2 The collector contract (reused verbatim from v2.7.0)

A collector is a **plain free function**, not a trait impl:

```rust
pub(super) fn collect_<fw>_route_facts(
    language: &str, tree: &Tree, file_path: &str, content: &str,
) -> Vec<StructuralFact>
```

- **AST-driven collector.** New-language collectors (kotlin/elixir/php/rust) walk the
  tree for the framework's call/annotation/macro nodes and read the route argument
  from the AST — they do **not** raw-scan with `SourceMask` (which covers only
  Js/Go/Java/C#/Python/Ruby and is deliberately not extended, §4.1). NestJS rides the
  existing TS/JS path.
- **Import gate first:** early-return empty unless the framework is imported
  (`content.contains("...")` or a parsed-import scan). Preserves M2 silence.
- **Detect static routes via the AST whole-argument check of §4.4** (not a hand-rolled
  mask): emit only when the route/URL argument node is itself a plain static string
  literal — reject concatenation, `format!`/`sprintf`/macro calls, and identifier/const
  references *before* extracting any value.
- **Emit via shared builders** — never inline normalization:
  - `route_fact(language, tree, file_path, content, start, end, spec: RouteFactSpec, enrich)`
    (`scan.rs:533`), or Spring's hand-rolled `mapping_fact` shape when class-prefix
    semantics don't fit `RouteFactSpec`.
  - `normalize_route_template(template, flavor) -> NormalizedTemplate`
    (`http_boundary.rs:20`) with one of the 5 existing `ParamFlavor` variants.
  - Anchor the fact span to a real parser node (`smallest_node_covering_range`);
    reject comment/string nodes (`is_comment_or_string_node`).
  - Leave `containing_symbol_id = None`; it's filled post-hoc by
    `attach_containing_symbols` (`mod.rs:180`).
- **Metadata baseline** (`base_metadata`): `pattern_version=1`, `query_family="framework"`,
  `framework`. Add `api_style`, `route_template` (raw), `normalized_route_template`
  (Always), `dynamic_segments` (omit when empty), `verb`/`verb_source` (omit both
  when not verb-restricted; verb UPPERCASE; verb_source = `attested`|`default`).

### 4.3 Prefix / mount tracing — pick the model that fits

- **Receiver-traced builder chains** (axum `Router::new()`, actix builder): Go's
  single-assignment + **Poisoned** model (`go_http.rs`) — a conflicting/non-literal
  assignment poisons and emits `route_template` only.
- **Lexical block/closure nesting** (Phoenix `scope do`, Laravel `->group(closure)`,
  Ktor `routing{}`): Rails `scope_stack` with interpolation poison guard
  (`rails.rs:368`). This is **lexical containment**, a distinct accepted model — not
  the single-assignment data-flow rule.
- **Per-scope class/decorator prefix** (Spring class `@RequestMapping`, NestJS
  `@Controller`): reset-per-class join (`spring.rs:58`).

When resolvable same-file: emit `route_group_prefix`/`class_route_template` +
`effective_route_template`. When the prefix target is not traceable in-file: emit
**only** the dedicated prefix-registration fact (§2c).

For the new AST-driven languages these are **AST analogs** of the models above (walk
the enclosing block/decorator/receiver in the tree), not the existing byte-offset
raw-scan tracers — "reuse the Rails `scope_stack` model" means reuse the *shape*
(lexical-containment stack with a poison guard on non-literal prefixes), implemented
over AST traversal.

### 4.4 Static-literal detection — AST whole-argument check (resolved: Lane B)

The silence guard — deciding static-vs-dynamic — is the single most M2-critical
piece of a collector: a false "static" promotes a computed path to a guessed route.
For the 4 new languages this decision lives in a **tree-sitter AST check**, not a
hand-rolled per-byte lexer, because the grammars have already solved the lexing.

**The check operates on the whole route/URL ARGUMENT EXPRESSION node, not on a
plucked string literal.** This is the correctness core: an interpolation-*child*
check alone would still leak a false positive when a collector extracts the first
string literal out of a larger expression. All of these must emit **nothing**:

- `@GetMapping("/u/" + id)` / `@Get('/u/' + id)` — binary concat (Kotlin/Java/TS)
- `Route::get('/u/' . $id, ...)` — PHP `.` concat
- `get "/u/" <> id` — Elixir `<>` concat
- `.route(format!("/u/{id}").as_str(), ...)` — Rust `format!`/macro-built
- `@GetMapping(PATHS.USER)` / `Route::get(self::PREFIX . '/x')` — const/identifier ref

So the rule is an **allowlist on the argument node kind**: emit a value **only when
the route argument node is *itself* a plain, single, static string literal** of the
language (reject `binary_expression`/additive/concat nodes, `call_expression`/macro
invocations like `format!`/`sprintf`, identifiers, member/subscript access, and
array/collection args unless each element is independently checked). Then, on that
lone literal node, apply the interpolation check:

- **Kotlin:** arg is a `string_literal` / `multiline_string_literal` with **no
  `interpolation` child** (covers both `$x` and `${...}`).
- **Elixir:** arg is a `string` / `sigil` / `charlist` with **no `interpolation`
  child**; for a `sigil`, additionally require `sigil_name ∈ {s, S}`. The grammar
  handles `#` comments, `"""` heredocs, and every `~s{}/[]/()/<>/||///` delimiter.
- **PHP:** arg is a `string` (single-quote, never interpolates) **or** an
  `encapsed_string` whose children are **only** `string_content`/`escape_sequence`
  (allowlist — reject any `variable_name`/`expression`/`member_access_expression`/
  `subscript_expression`/`dynamic_variable_name` child). `nowdoc` is static;
  `heredoc` uses the same allowlist check.
- **Rust:** arg is a `string_literal` / `raw_string_literal` (the grammar has no
  string interpolation, so a lone literal is always static) — but the whole-argument
  rule still rejects `format!(...)`/concat/`const` wrappers.

Use an **allowlist of safe node kinds, never a denylist** — an unknown wrapper node
must fail closed to silence. A lightweight comment/string-span mask may still drive
byte-level delimiter matching for prefix tracing, but the **static-vs-dynamic
decision must not live in it**. Captured in **ADR-0005 (AST static-literal detection
for new HTTP boundary languages)**, written during implementation.

Residual risk: the guard is now "correctly enumerate the safe argument-node
allowlist + interpolating child kinds per grammar." That enumeration must be
**grammar-verified and covered by exhaustive table-driven unit tests** per language,
with **negative fixtures for concat / format-macro / const-ref / comment / string
forms in every framework** (Elixir `sigil_name` gating and the PHP `encapsed_string`
child set are the two to nail).

### 4.5 Prefix families stay per-framework (corrected rationale)

Miller's `TryReadMountFact` consumes all mount families **framework-blind** (reads
`normalized_mount_path`/`mount_path` + `mount_target`/`included_module` uniformly),
so the earlier rationale — "a generalized family fragments Miller's per-framework
contract" — is **false and removed**. Per-framework ids are nonetheless kept because:

- The **shipped mount lane is already per-framework** (`express.router_mount.v1`,
  `rails.mount.v1`, plus `aspnet.minimal_api.route_group.v1`); a mixed
  generalized+per-framework regime is worse than either uniform one.
- Mount-prefix consumption in Miller is **heterogeneous** — include-style mounts go
  through the uniform `TryReadMountFact`, while aspnet-style uses provider-specific
  `route_group_prefix`/`group_variable` linking. Per-framework ids preserve that
  distinction cleanly and keep each registry spec's `KeyPresence` honest.

(The generalized-family objection's *facts* were correct — Miller is framework-blind
for include-style mounts — but its *conclusion* overreached given the existing
per-framework regime. Do not thrash the whole prefix lane.)

### 4.6 Ktor restricted gate (closure plan for the deferred open_gap)

Recorded so the deferral is scope-only, not doctrinal. A safe Ktor gate emits a
`ktor.route.v1` fact only when **all** hold:

1. callee is a **bare `simple_identifier`** verb in
   `{get, post, put, patch, delete, head, options}` (rejects `navigation_expression`
   callees like `map.get` / `client.get`);
2. the call has a trailing `annotated_lambda` / `lambda_literal` child;
3. arg0 is a `string_literal` with **no `interpolation` child** (flavor `Braces`);
4. the call is lexically contained in a `routing{}` / `route{}` lambda block (AST
   parent walk, same class as Rails `scope_stack`).

This reuses the shipped `kotlin/test_calls.rs` curried-call + trailing-lambda + vocab
machinery, and rides the Kotlin collector built for Kotlin+Spring. Deferred only
because it is a full 7th framework lane.

## 5. Per-framework "definition of done" (the reusable checklist)

Each framework family is one branch. Steps touching shared files (dispatch, registry
SPECS + regenerated JSON, contract docs, `tests/mod.rs`) are the known conflict
points — **serialize merges**.

0. **Grounding check** (`razorback:grounding-in-current-docs`) against the
   framework's current routing docs for exact registration syntax, verb set, param
   flavor. Record checked source in the commit.
1. **Collector module** `framework_structural_facts/<fw>.rs` (free fn, import gate,
   AST static-literal detection §4.4, shared builders).
2. **Register (wiring points):** `mod <fw>;` + `use`; `pub(super) const <FW>_PATTERN_ID`;
   dispatch arm in `collect_framework_structural_facts` (new `match` arm for
   kotlin/elixir/php/rust; extend the existing ts/js arms for NestJS); test-gated
   `framework_structural_fact_pattern_ids_for_language` arm.
3. **Registry + JSON contract (same commit):** add `StructuralFactPatternSpec` to
   `SPECS` declaring every emitted metadata key with type + presence; the
   `languages` set must equal the emission set; regenerate the checked-in JSON
   (`UPDATE_CONTRACT_JSON=1 cargo test -p julie-extractors structural_fact_registry`)
   and commit `docs/contracts/structural-fact-patterns.json` (byte-for-byte sync
   test is ungated).
4. **Capabilities ledger + golden fixture:** add pattern id(s) to the language row's
   `kind_coverage.structural_facts.supported` and register a `fixtures[]` entry;
   author `fixtures/extraction/<lang>/<family>/source.<ext>` with static routes +
   negative/silent cases; generate `expected.json` (`UPDATE_GOLDEN=1`). Any
   unimplemented construct stays `open_gaps` with reason + closure, never a silent zero.
5. **Contract docs:** add pattern rows + metadata keys to `docs/contracts/jsonl-v3.md`
   and `docs/contracts/sqlite-schema-v3.md` (framework rows are NOT test-enforced —
   do it by hand or it drifts).
6. **Miller integration (cross-repo, for prefix-registration ids only):** add the
   const + whitelist entry + `IsMountFactPattern` arm in Miller, or the fact is a
   silent no-op there. Track as a checklist item, not a schema change.
7. **Tests (narrow, ungated):** per-framework module registered in `tests/mod.rs`;
   assert positive emission (exact verb/route_template/normalized_route_template/
   effective_route_template/dynamic_segments), negative/silent cases (interpolation,
   non-literal prefix, dynamic verb), and a **binding assertion** that
   `containing_symbol_id` resolves to the enclosing handler.
8. **Gates:** `cargo test -p julie-extractors tests::<fw>`;
   `--features test-golden golden_fixtures_match_canonical_extraction`;
   `--features test-golden structural_fact_registry`;
   `--features test-capability-matrix capability_matrix`;
   `node scripts/language-data-quality-report.mjs --strict` with `silent_cells=0`
   AND `quality_bar_debts=0` (read the debts line; don't trust exit code alone).

## 6. Contract additions

- **New server pattern ids:** `nestjs.route.v1`, `laravel.route.v1`,
  `laravel.resource_route.v1`, `phoenix.route.v1`, `phoenix.resource_route.v1`,
  `axum.route.v1`, `actix.attribute_route.v1`, and `actix.scope_route.v1` **iff
  completed** (else fully absent per §2a — no dead registry spec).
- **Extended pattern id:** `spring.request_mapping.v1` `languages` → `[java, kotlin]`.
- **New prefix-registration pattern ids:** `axum.nest.v1`, `actix.mount.v1`,
  `laravel.route_prefix.v1`, `phoenix.forward.v1`. (`nestjs.global_prefix.v1` is OUT.)
- **Client family:** `http.client_request.v1` gains kotlin/php/elixir/rust rows
  (same existing spec; extend `languages`), scoped to the primary client per §2b.
- **Miller consumer edits (cross-repo)** are required for each new prefix-registration
  id to actually join — const + whitelist + `IsMountFactPattern` arm + route-family
  mapping + anchor rule (§2c). Evidence-only until all five exist.
- **Contract marker: BUMP required.** `lib.rs` doctrine says bump
  `EXTRACTION_CONTRACT_VERSION` "when the canonical extraction shape changes in a way
  downstream consumers must observe," and every prior lane appended a marker
  (`…http-boundary-facts-v1.…backend-http-boundary-v1`). This release adds 8+ pattern
  ids and 4 client languages — a shape change v2.7 vs v2.8 consumers must distinguish.
  **Append `.backend-http-boundary-v2`** to the version string and update the
  api-surface marker test (`crates/julie-extractors/src/tests/api_surface.rs`). (The
  earlier "additive families need no new marker" reasoning was wrong and is removed.)

## 7. Sequencing & dependencies

Ordered by value/effort and by shared-prerequisite. **Note:** the earlier "client
work is marginal because the mask is shared" framing was wrong — the shared piece is
one line; each client library is ~80–200 lines of independent detection plus a
fixture. Client scope is intentionally trimmed (§2b) to keep the lane coherent.

1. **NestJS** — no new static-literal helper (TS AST already handled), no client
   work. Fastest; do first.
2. **Kotlin+Spring** — builds the Kotlin AST static-literal check (§4.4); new Kotlin
   collector reusing `spring.request_mapping.v1`; kotlin client facts (Ktor client).
3. **Laravel** — PHP AST check; `laravel.route.v1` + resource + `route_prefix`; php
   client facts (Guzzle + Http facade).
4. **Phoenix** — Elixir AST check; reuses Rails scope-stack; elixir client facts (Req).
5. **axum** — Rust AST check (trivial — no interpolation); `axum.route.v1` +
   `axum.nest.v1`; rust client facts (reqwest).
6. **actix** — reuses the Rust check; `actix.attribute_route.v1` (ship complete) +
   `actix.scope_route.v1` (may land as `open_gaps` if scope tracing slips) +
   `actix.mount.v1`.

Client-request facts for a language ship with that language's first server framework.

## 8. Testing & quality

- Default suite stays under the 90s tripwire; per-framework narrow test commands.
- AST static-literal silence-guard unit tests are exhaustive, table-driven, and
  grammar-verified per language (they gate M2).
- Golden fixtures are the sole capability evidence; every claimed row (each client
  library included) is fixture-backed.
- `--strict` data-quality report ends at `silent_cells=0`, `quality_bar_debts=0`.
- Ktor deferral and every deferred client library are recorded as real `open_gaps`
  entries with reason + named closure task, not hollow claims.

## 9. Doubt Pass — completed

Adversarial challenge of the 5 riskiest choices was run; adjudication folded in.
Outcomes:

1. **Static-literal detection** → **revised to Lane B (AST check)**, §4.4. Silence
   guard moves out of a hand-rolled lexer. ADR-0005 to be written.
2. **Prefix families per-framework vs generalized** → **per-framework upheld on
   corrected grounds**, §4.5. False "fragments contract" rationale removed.
3. **Ktor deferral** → **upheld for release scope**, doctrine-impossibility rationale
   **deleted**; restricted gate recorded as the closure plan, §4.6.
4. **Client-side scope** → **kept two-sided (user-locked) but trimmed** to one
   primary client per language; remaining libraries deferred as `open_gaps`, §2b.
5. **actix idioms** → **split into two pattern ids**, §2a/§4.5, mirroring aspnet.

Core doctrine (single join key, M2 silence, same-file single-assignment poison,
facts-only) was not challenged and stands. Residual risks are §4.4 grammar-enumeration
correctness, `actix.scope_route.v1` feasibility, and the trimmed client fixture
burden — all implementation-plan line items, no blockers.

### 9b. External review (Codex) — folded in

A read-only Codex pass verified the doc against the code and found four material
issues, all folded in:

1. **Whole-argument silence rule** — the interpolation-*child* check alone would leak
   `"/u/" + id`, `format!("/u/{id}")`, and const refs. §4.4 now requires the AST check
   to run on the whole route argument node (allowlist of safe node kinds) before
   extracting any value, with negative fixtures per framework.
2. **Contract marker must bump** — §6 now appends `.backend-http-boundary-v2` (the
   crate doctrine and every prior lane require it).
3. **`actix.scope_route.v1` dead-pattern** — §2a now makes it binary (fully shipped or
   fully absent), since the registry test rejects registered-but-unemitted specs.
4. **Miller consumer is not framework-blind for joining** — §2c now lists the full
   per-family requirement (route-family mapping + anchor rule), not just the whitelist;
   families are evidence-only until those land.

## 10. Acceptance criteria

- [ ] `nestjs.route.v1` emitted for TS/JS, class `@Controller` prefix joined into
      `effective_route_template`, fixture-backed, registry-conformant, binding-asserted.
- [ ] `spring.request_mapping.v1` emitted for Kotlin (annotation controllers), reusing
      the Java pattern id with `languages=[java,kotlin]`; `$`-interpolation and
      bracket-array cases emit nothing (no bogus routes), proven by negative fixtures.
- [ ] `laravel.route.v1`/`laravel.resource_route.v1`/`laravel.route_prefix.v1` for PHP.
- [ ] `phoenix.route.v1`/`phoenix.resource_route.v1`/`phoenix.forward.v1` for Elixir.
- [ ] `axum.route.v1`/`axum.nest.v1` for Rust.
- [ ] `actix.attribute_route.v1` (complete) + `actix.mount.v1` for Rust;
      `actix.scope_route.v1` **either** fully complete (spec + fixture + emission)
      **or** fully absent + a Rust `open_gaps` entry — no dead registry spec.
- [ ] `http.client_request.v1` emitted for kotlin/php/elixir/rust from static URL
      literals for the primary client per language; deferred libraries recorded as
      `open_gaps`.
- [ ] AST whole-argument silence guard (§4.4) has exhaustive grammar-verified unit
      tests for all 4 languages, **including negative fixtures for concat,
      format/sprintf/macro, const-ref, comment, and interpolation forms per framework**.
- [ ] `EXTRACTION_CONTRACT_VERSION` bumped with `.backend-http-boundary-v2`; api-surface
      marker test updated.
- [ ] Each new prefix-registration id has its Miller consumer path (const + whitelist +
      `IsMountFactPattern` + `RouteFamilyForMount` mapping + anchor rule) landed, or is
      explicitly tracked as evidence-only follow-up.
- [ ] All new/extended pattern ids declared in the registry, JSON contract regenerated
      and committed, contract docs updated; ADR-0005 written.
- [ ] Miller cross-repo edits landed for each new prefix-registration id (or tracked
      as follow-up with the silent-no-op caveat noted).
- [ ] `capabilities.json` rows updated with fixture evidence; Ktor + all excluded
      idioms/libraries recorded as documented `open_gaps`/exceptions.
- [ ] `node scripts/language-data-quality-report.mjs --strict`: `silent_cells=0`,
      `quality_bar_debts=0`.
- [ ] Default test suite under the 90s tripwire; `AGENTS.md`/`CLAUDE.md` sync check passes.

## 11. Out of scope (YAGNI)

- No new product surface (editor/daemon/search/watcher).
- No cross-file join logic (Miller's job — decision 0004).
- No non-HTTP boundary lanes (GraphQL/gRPC/MQ/ORM) — separate future releases.
- No new mainstream language extractors — all 6 frameworks' languages already exist.
- No version-sniffing of framework crates (axum 0.7 vs 0.8) — honest under-report instead.
- No `nestjs.global_prefix.v1` / app-global prefix facts (no safe consumer).
- No full client-library coverage — one primary client per language; rest are `open_gaps`.
