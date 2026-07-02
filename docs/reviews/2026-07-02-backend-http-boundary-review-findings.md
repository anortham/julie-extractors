# Backend HTTP Boundary Review Findings (2026-07-02)

Multi-agent adversarial review of the 4 unpushed commits implementing
`docs/plans/2026-07-02-backend-http-boundary-coverage.md` (origin/main..4bdec36).
38 raw candidates, all verified CONFIRMED, deduplicating to ~34 unique issues.
This file is the durable worklist for the fix lane; line numbers reference the
pre-fix tree at commit 4bdec36.

## Raw findings (38, as reported by finders)

### 1. crates/julie-extractors/src/base/framework_structural_facts/python_web.rs:540

Django URL collector slices content[second_start..second_end] with start > end when a path()/re_path() call has only one argument, panicking the extractor.

**Failure scenario:** A Python file that imports django.urls path and contains a single-argument call like path("admin/") (or any obj.path("x") call, since the boundary check accepts a preceding dot): first_end == close, so second_start = close+1 while find_top_level_comma_or_end(content, close+1, close) returns close, and content[close+1..close] panics with 'slice index starts at X but ends at Y' — the whole file extraction crashes instead of producing an artifact. node.rs guards the identical pattern with 'if second_start >= close { continue; }' (node.rs:364) but this call site has no guard.

### 2. crates/julie-extractors/src/base/framework_structural_facts/rails.rs:105

Every bare 'end' line pops the Rails scope stack, but do-blocks that never pushed (resources do, member, collection, constraints, draw do) also emit 'end', so scopes are popped prematurely.

**Failure scenario:** routes.rb: namespace :api do / resources :posts do / member do / get 'activate' / end / end / get 'health', to: 'health#show' / end — the 'end' closing the member block pops the '/api' scope, so the later health route is emitted with normalized_route_template '/health' instead of '/api/health'; every route after any nested do..end block inside a namespace/scope gets the wrong effective path, breaking the endpoint/client join contract the plan defines.

### 3. crates/julie-extractors/src/base/framework_structural_facts/spring.rs:154

parse_mapping_annotation treats every string literal in the annotation arguments as a route template, so produces/consumes/params/headers values become fake routes.

**Failure scenario:** @GetMapping(value = "/users", produces = "application/json") — string_literals(args) returns ["/users", "application/json"] and the retain filter only runs when args contains "method" (and only strips verb names), so the extractor emits a second GET route fact with route_template "application/json" normalized to "/application/json"; any Spring controller using produces/consumes string literals (extremely common) yields spurious endpoint rows.

### 4. crates/julie-extractors/src/base/framework_structural_facts/go_http.rs:261

collect_route_calls has no is_identifier_boundary check on the receiver, so the needle 'r.GET' matches inside longer identifiers like 'server.GET' or 'apiRouter.GET'.

**Failure scenario:** Go file with r := gin.Default() plus another variable apiRouter (or server, or any identifier ending in 'r') calling apiRouter.GET("/x", h): the substring search finds "r.GET" at offset 8 inside "apiRouter.GET" and emits an extra route fact whose span starts mid-identifier — duplicate/false route rows with corrupt start_byte/start_column. node.rs guards the same pattern with is_identifier_boundary (node.rs:549) but this Go path omits it.

### 5. crates/julie-extractors/src/base/framework_structural_facts/node.rs:457

Express route-chain scanning window ends at the first newline (statement_end returns on '\n' at depth 0 right after app.route(...)), so multi-line chained routes produce no verb facts.

**Failure scenario:** The idiomatic multi-line form app.route('/users')\n  .get(list)\n  .post(create); — statement_end(content, close+1) hits the newline immediately after the route(...) call and returns close+2, so the .get/.post chain search window is empty and zero route facts are emitted for the chain (silent missing endpoints); the golden fixture and tests only cover the single-line form so the gap is invisible. Conversely, within a single line the scan also matches '.get('/'.post(' inside handler bodies (e.g. map.get(key)), producing duplicate route facts.

### 6. crates/julie-extractors/src/base/framework_structural_facts/node.rs:563

collect_route_method_calls does not require a handler argument, so Express's single-argument settings getter app.get('name') is emitted as a GET route.

**Failure scenario:** Common Express code const port = app.get('port') or app.get('view engine'): the first argument parses as a string literal, first_end == close passes the sole-argument check, and a route fact with verb GET and normalized_route_template '/port' (or '/view engine') is emitted — false endpoint rows for a config read, unlike collect_express_mounts which explicitly requires a second argument.

### 7. docs/contracts/sqlite-schema-v3.md:543

The deleted contract row pinned http.client_request.v1 anchors to `call_expression`; its replacement claims a "parser-covered call span", but the new Java collector anchors client-request facts to `local_variable_declaration` nodes (see fixtures/extraction/java/backend_http_boundaries/expected.json), so neither the old guarantee nor the new stated invariant holds for Java.

**Failure scenario:** A downstream consumer (e.g. a Miller join or SQL query) that follows the documented contract and filters structural_facts on call-shaped node_kind values (previously the hard `call_expression` guarantee, now the documented "parser-covered call span") silently drops every Java http.client_request.v1 row, so Java outbound HTTP calls never join to server endpoints and the boundary report under-counts Java clients with no error.

### 8. crates/julie-extractors/src/base/framework_structural_facts/go_http.rs:304

Go 1.22 host-pattern handling from the plan is not implemented but its acceptance criterion is checked off: split_go_pattern only splits METHOD from the rest, never extracts a host segment, and no `host` metadata key exists in the registry.

**Failure scenario:** Plan Task 6 criterion (line 291, checked [x]) requires `"GET example.com/users/{id}"` to emit `host=example.com` with `normalized_route_template=/users/:id`. Actual output embeds the host in the path: route_template="example.com/users/{id}" normalizes to "/example.com/users/:id", so host-scoped routes get a wrong join key and never match client `target_path` joins in Miller; the checked checkbox makes incomplete work look complete (violates user CLAUDE.md completeness rule).

### 9. crates/julie-extractors/src/base/framework_structural_facts/go_http.rs:228

gin `r.Any(...)`, gin `r.Handle("VERB", "lit", ...)`, echo `e.Any(...)`, and nested group composition are all unimplemented, but plan Task 6 criterion line 292 is checked [x]; collect_group_framework_routes only scans GET/POST/PUT/PATCH/DELETE/HEAD/OPTIONS and collect_grouped_receivers only traces groups created directly from routers.

**Failure scenario:** A file with `r.Any("/health", h)` or `r.Handle("PUT", "/x", h)` or `users := v1.Group("/users"); users.GET("/:id", h)` emits zero facts for those routes — routes silently missing from the extraction artifact while the plan claims them done and capabilities claim gin/echo support.

### 10. crates/julie-extractors/src/base/framework_structural_facts/go_http.rs:339

route_fact hard-codes api_style="mux_routing" for all three Go families, but the plan contract specifies api_style="call_routing" for gin.route.v1 and echo.route.v1; the registry description was written as mux_routing to match the narrowed implementation.

**Failure scenario:** Downstream consumers (Miller) filtering handler facts by the documented per-family api_style values from the Cross-Family Doctrine (call_routing for call-based registration) get wrong metadata for every gin/echo route; the contract doc was adjusted to the implementation rather than the plan being explicitly revised.

### 11. crates/julie-extractors/src/base/framework_structural_facts/go_http.rs:75

Echo import gate only matches the exact path "github.com/labstack/echo/v4", but the plan contract says the gate is a labstack/echo import of any major version path.

**Failure scenario:** A Go file importing `github.com/labstack/echo` (v3 style) or `github.com/labstack/echo/v5` emits zero echo.route.v1 facts even for textbook `e.GET("/x", h)` registrations — silent capability gap that contradicts the decided contract and the capability claim.

### 12. crates/julie-extractors/src/base/framework_structural_facts/spring.rs:73

All method-level Spring facts hard-code attribute_kind="http_method", but the plan and the registry key description both specify attribute_kind="request_mapping" for a method-level @RequestMapping without a method element.

**Failure scenario:** `@RequestMapping("/x")` on a method emits attribute_kind="http_method" with no verb — a combination the contract says cannot occur; consumers relying on the documented class_route/http_method/request_mapping trichotomy misclassify these facts, and the documented "request_mapping" value is never emitted by any code path.

### 13. crates/julie-extractors/src/base/framework_structural_facts/spring.rs:43

current_class_template is set when a class-level @RequestMapping is seen but never reset at the next class declaration, so it leaks across classes in the same file.

**Failure scenario:** A file with `@RequestMapping("/api/users") class UsersController {...}` followed by a second `class HealthController` (no class mapping) whose `@GetMapping("/health")` emits effective_route_template="/api/users/health" and a wrong normalized join key — Miller joins client requests to the wrong handler.

### 14. crates/julie-extractors/src/base/framework_structural_facts/rails.rs:105

Scope tracking pops the namespace/scope stack on any bare `end` line, including `end`s that close non-scope blocks (resources do-blocks, constraints, member/collection, the draw block itself).

**Failure scenario:** In `namespace :admin do  resources :users do ... end  get "reports" end`, the inner `end` pops "/admin", so `get "reports"` emits without scope_path and with normalized_route_template "/reports" instead of "/admin/reports" — wrong join key, broken client-to-handler bridging.

### 15. crates/julie-extractors/src/base/framework_structural_facts/rails.rs:21

The Rails gate only checks the file path or the presence of the draw string anywhere in the file; it never verifies a route line is inside the routes.draw block, yet plan Task 7's checked criterion says DSL in config/routes.rb outside a draw block stays silent, and no test covers it.

**Failure scenario:** Helper code above the draw block in config/routes.rb (e.g. a `get "…"` call in a locally defined method) emits rails.route.v1 facts the contract says must stay silent — false route facts in the artifact while the plan checkbox claims the criterion is met.

### 16. crates/julie-extractors/src/base/framework_structural_facts/http_clients/go.rs:66

collect_calls finds the needle (e.g. "http.Get") with no leading identifier-boundary check, unlike the python collector which calls is_identifier_boundary.

**Failure scenario:** A Go file that imports net/http and contains `smtphttp.Get("https://internal/x")` on an unrelated type emits a false http.client_request.v1 fact (the needle "http.Get" matches inside "smtphttp.Get") — phantom client boundaries in the artifact.

### 17. crates/julie-extractors/src/base/framework_structural_facts/http_clients/java.rs:25

The "HttpRequest.newBuilder" needle search has no identifier-boundary check, so it matches inside longer type names.

**Failure scenario:** A file importing java.net.http that also uses a wrapper type `MyHttpRequest.newBuilder()` gets a false http.client_request.v1 fact for the wrapper call, since the needle matches the suffix of "MyHttpRequest.newBuilder".

### 18. crates/julie-extractors/src/base/framework_structural_facts/node.rs:800

node.rs re-implements parse_js_string_literal, parse_js_identifier, is_js_identifier, find_matching_paren/brace/bracket, find_top_level_comma_or_end, and a JS comment/string lexer that all already exist in web_structural_facts/js_object_scan.rs — which plan Task 2 explicitly told this task to consume (along with extending js_imports.rs, which was left untouched).

**Failure scenario:** Two divergent JS scanning stacks now coexist: a fix in js_object_scan (e.g. template-literal or regex-literal handling) will not reach express/fastify route detection, so the same source line can be a client fact via one lexer and silently skipped as a route via the other — inconsistent artifacts and doubled maintenance.

### 19. crates/julie-extractors/src/base/framework_structural_facts/http_clients/python.rs:181

client_fact is copy-pasted five times (python.rs:181, csharp.rs:158, go.rs:120, java.rs:55, ruby.rs:74) with identical smallest_node_covering_range + is_comment_or_string_node + NormalizedSpan + fact_for_span bodies; it belongs once in http_clients/mod.rs or helpers.rs.

**Failure scenario:** Any change to client-fact construction (e.g. adding a confidence rule or fixing the comment/string guard) must be applied in five places; a missed copy makes http.client_request.v1 emission diverge per language, breaking the plan's byte-identical metadata guarantee.

### 20. crates/julie-extractors/src/base/framework_structural_facts/http_clients/csharp.rs:219

http_clients/csharp.rs re-implements find_matching_paren/find_matching_delimiter/find_top_level_comma_or_end even though framework_structural_facts/helpers.rs (which this file already imports from) exports C#-aware versions of the same functions.

**Failure scenario:** The local copy lacks the `$@"` interpolated-verbatim handling that helpers.rs find_matching_delimiter has, so the same C# text is bracket-matched differently in the client collector vs the aspnet collector — divergent, harder-to-fix parsing with an existing helper one import away.

### 21. crates/julie-extractors/src/base/framework_structural_facts/http_clients/go.rs:246

is_in_go_string_or_comment, parse_go_string_literal, find_matching_paren, and find_top_level_comma_or_end are duplicated between http_clients/go.rs and go_http.rs (both added in the same commit), and the client copies silently dropped backtick raw-string support that the go_http copies have.

**Failure scenario:** http.Get(`/api/x`) with a raw-string URL is extracted by neither collector consistently: the route scanner's literal parser accepts backticks while the client scanner's rejects them, and a needle occurring inside a backtick string is treated as code by the client-side lexer — inconsistent emission between two files that should share one Go scanner.

### 22. crates/julie-extractors/src/base/framework_structural_facts/http_clients/python.rs:337

is_in_python_string_or_comment, find_matching_paren, find_top_level_comma_or_end, parse_python_string_literal, and is_python_identifier are duplicated between http_clients/python.rs and python_web.rs, both new in the same commit.

**Failure scenario:** Two Python pseudo-lexers must now be bug-fixed in lockstep; the Python-specific triple-quote/f-string rules are edge-case heavy, so an inevitable one-sided fix makes route facts and client facts disagree about which spans are inside strings — wrong or missing facts in one of the two families.

### 23. crates/julie-extractors/src/base/framework_structural_facts/spring.rs:299

parse_java_string_literal and find_matching_paren are duplicated between spring.rs and http_clients/java.rs; is_in_java_string_or_comment (java.rs) is byte-identical to is_in_csharp_string_or_comment (csharp.rs) and the Go variant — one shared C-style scanner would serve all.

**Failure scenario:** Four byte-identical or near-identical scanners across sibling files added in one commit; the next contributor fixing escape handling in one copy leaves the others wrong, producing per-language divergence in what counts as a string/comment and thus which facts emit.

### 24. crates/julie-extractors/src/base/framework_structural_facts/rails.rs:404

parse_ruby_string_literal is duplicated verbatim between rails.rs and http_clients/ruby.rs (both new in this commit).

**Failure scenario:** Ruby string parsing (e.g. adding %q/%Q or escape-mapping support) must be fixed twice; a one-sided fix makes route templates and client target paths parse the same literal differently.

### 25. crates/julie-extractors/src/base/framework_structural_facts/node.rs:724

insert_string_array is defined five times (node.rs:724, go_http.rs:377, python_web.rs:920, rails.rs:429, spring.rs:292) even though web_structural_facts/fact_builders.rs:73 already has the identical function; it should live once in framework_structural_facts/helpers.rs beside insert_string.

**Failure scenario:** Five identical private copies of a three-line metadata helper across sibling modules that already share helpers.rs — pure copy-paste maintenance overhead and a template for future contributors to keep duplicating.

### 26. crates/julie-extractors/src/base/framework_structural_facts/go_http.rs:319

route_fact is near-identically re-implemented in go_http.rs:319, node.rs:663, and python_web.rs:644 (same base_metadata + api_style + route_template + prefix join + normalize + dynamic_segments + verb assembly); a single shared handler-fact builder in helpers.rs or http_boundary.rs would replace all three.

**Failure scenario:** The Cross-Family Doctrine's baseline metadata (rule 6) is enforced only by parallel copy-paste; the copies have already drifted (go_http omits verb_source parameterization and hard-codes api_style, node hard-codes verb_source=attested), so doctrine changes require N-way edits and will inevitably diverge per family.

### 27. crates/julie-extractors/src/base/framework_structural_facts/node.rs:15

EXPRESS_VERB_METHODS and FASTIFY_VERB_METHODS are byte-identical constant tables; one shared const (or reuse of the CLIENT_METHODS-style table) does the same job.

**Failure scenario:** Redundant derivable state: adding a verb (e.g. TRACE) requires editing two identical tables in the same file; a missed edit makes Express and Fastify verb coverage silently diverge.

### 28. crates/julie-extractors/src/base/framework_structural_facts/mod.rs:152

The "typescript" dispatch arm calls collect_backend_http_client_requests, but http_clients/mod.rs matches only python/csharp/go/java/ruby, so the call always returns an empty Vec — dead code (TS client facts come from web_structural_facts/http_client.rs per the plan).

**Failure scenario:** Readers and future maintainers believe TypeScript backend client collection is routed through http_clients (and may add TS handling there, double-emitting against the web collector); today it is a guaranteed no-op allocation on every TypeScript file — the arm should match the javascript/jsx/tsx arms.

### 29. crates/julie-extractors/src/base/framework_structural_facts/http_clients/python.rs:78

Every needle hit re-runs is_in_*_string_or_comment(content, hit) from byte 0 (same pattern in python, go, java, csharp, ruby clients and node/python_web/go_http route scanners), giving O(n^2) scanning on match-dense files — and the check is largely redundant with the tree-based is_comment_or_string_node guard already applied in client_fact/route_fact.

**Failure scenario:** A large generated Python/Go file with hundreds of `requests.get`/`http.Get` occurrences pays a full-file lexer pass per occurrence, degrading extraction throughput quadratically; the parse tree already in hand (js_object_scan::is_ignored_syntax_range precedent) answers the same question in O(log n) per hit.

### 30. crates/julie-extractors/src/base/framework_structural_facts/http_clients/go.rs:38

net_http_alias re-parses the file's import lines even though collect_go_imports in go_http.rs does the same net/http alias scan, and mod.rs runs both collectors on every Go file (same double-scan for Python imports between python_web.rs and http_clients/python.rs).

**Failure scenario:** Duplicated per-file work plus two independent import parsers that can disagree (go_http recognizes only echo/v4 paths, net_http_alias has its own trim logic); an alias-parsing fix applied to one leaves the other emitting or gating differently on the identical import line.

### 31. crates/julie-extractors/src/base/framework_structural_facts/http_clients/python.rs:328

is_python_identifier, is_go_identifier (go_http.rs:484), is_js_identifier (node.rs:873), and helpers.rs is_csharp_identifier are four copies of the same ASCII identifier predicate.

**Failure scenario:** Pure duplication of a shared predicate that already exists in helpers.rs; future Unicode-identifier or edge-case fixes must be replicated four times or the languages drift.

### 32. crates/julie-extractors/src/base/framework_structural_facts/spring.rs:284

is_java_class_declaration's last three arms (public/private/protected "class " prefixes) are unreachable because `line.contains(" class ")` already matches them.

**Failure scenario:** Dead conditions that suggest the modifier list is load-bearing; a maintainer extending it (e.g. for `final class`) edits dead code while the real behavior lives in the contains() check — confusing and error-prone.

### 33. crates/julie-extractors/src/base/framework_structural_facts/http_clients/mod.rs:17

Altitude: all five new client collectors and all five route ecosystems are built as per-language string-searching lexers layered beside the tree-sitter parse instead of generalizing one shared scanning core (needle -> boundary check -> paren match -> literal parse -> fact), yielding ~9 find_matching_paren, ~8 find_top_level_comma_or_end, and ~8 string/comment lexers in one commit.

**Failure scenario:** The shared mechanism (framework_structural_facts/helpers.rs + js_object_scan.rs precedent) was bypassed rather than extended, so every future ecosystem task copies the pattern again; the codebase now has a dozen slightly-different bracket matchers whose behavioral differences (quote kinds, comments, raw strings) are undocumented and untested individually — each is a latent source of divergent emission.

### 34. crates/julie-extractors/src/base/framework_structural_facts/python_web.rs:540

collect_django_calls slices content[second_start..second_end] with second_start=close+1 > second_end=close when a django path()/re_path() call has only one argument, panicking the extractor.

**Failure scenario:** Confirmed by running `julie-extract scan` on a Python file containing `from django.urls import path` and `path("healthz")` (single argument, e.g. mid-edit urls.py or any one-arg call of a local named `path`): thread panics with 'byte range starts at 65 but ends at 64' at python_web.rs:540, the file is reported parse_failed and the entire file's extraction output (all symbols, identifiers, facts) is lost; scan report status becomes 'partial'.

### 35. crates/julie-extractors/src/base/framework_structural_facts/rails.rs:105

collect_rails_routes pops scope_stack on every bare `end` line, but only namespace/scope lines push, so any other do...end block (resources, member, collection, constraints) pops the enclosing namespace early.

**Failure scenario:** Confirmed via CLI: in `namespace :admin do resources :posts do member do get 'preview' end end; get 'stats' end`, the `end` closing `member` pops '/admin', so `get 'stats'` emits normalized_route_template '/stats' with no scope_path instead of '/admin/stats'. Downstream Miller HTTP-boundary joins (decision 0004) match clients to the wrong endpoint or miss the handler entirely.

### 36. crates/julie-extractors/src/base/framework_structural_facts/spring.rs:43

current_class_template is set when a class-level @RequestMapping is seen but never cleared when a subsequent class declaration has no class-level mapping, so the previous controller's prefix leaks into the next controller's routes.

**Failure scenario:** Confirmed via CLI: a Java file with `@RequestMapping("/api/users") class UserController` followed by `class HealthController { @GetMapping("/healthz") ... }` emits the health route with class_route_template='/api/users' and normalized_route_template='/api/users/healthz' instead of '/healthz'. Consumers see a non-existent endpoint path and boundary joins against clients calling /healthz fail.

### 37. crates/julie-extractors/src/base/framework_structural_facts/node.rs:253

collect_fastify_receivers adds fastify_plugin_parameters(content) receivers unconditionally, ignoring whether the file imports fastify at all, so any `module.exports = function (app)`/`export default function (app)` file is treated as a Fastify plugin.

**Failure scenario:** Confirmed via CLI: a plain JavaScript file with no fastify (or express) import containing `module.exports = function (app) { app.get('/health', h); }` — a common Express route-registration pattern — emits fastify.route.v1 with framework='fastify'. Codebases that never use Fastify get routes misattributed to Fastify, and files that are both express-received and match the plugin parameter emit duplicate/conflicting route facts.

### 38. crates/julie-extractors/src/base/framework_structural_facts/python_web.rs:1046

python_web's is_in_python_string_or_comment has no triple-quoted-string handling (unlike the corrected copy in http_clients/python.rs:337), so an apostrophe inside a '''...''' docstring flips quote parity for the rest of the file.

**Failure scenario:** Confirmed via CLI: a Flask file starting with the docstring '''Routes for Bob's service.''' followed by `@app.route("/health")` emits zero flask.route.v1 facts (control file without the apostrophe emits the route). All Flask/FastAPI/Django route and client facts after such a docstring are silently dropped, so those endpoints are invisible to boundary joins.

## Resolution map (fix lane, same day)

All 38 findings were fixed in the follow-up fix lane. Grouped by resolution:

- **1, 34 — Django single-argument `path()` panic**: fixed in `python_web.rs`
  (`collect_django_calls` skips calls with no second argument); regression test
  `django_single_argument_path_calls_stay_silent`.
- **38 — Python lexer missed triple-quoted strings**: replaced by the shared
  `SourceMask` (`scan.rs`); regression test
  `flask_routes_survive_module_docstrings_with_apostrophes`.
- **2, 14, 35 — Rails scope popped by non-scope `end`s**: block-kind stack in
  `rails.rs` (`BlockKind::{Draw,Scope,Other}`); test
  `rails_nested_non_scope_blocks_do_not_pop_namespace_scopes`.
- **15 — Rails draw-block gating missing**: `draw_depth` gating; split files
  under `config/routes/` allow top-level DSL; tests
  `rails_dsl_outside_the_draw_block_stays_silent`,
  `rails_split_route_files_emit_top_level_dsl`, `rails_controller_files_stay_silent`.
- **3 — Spring `produces`/`consumes` literals became routes**: element-aware
  annotation parsing (`parse_annotation_elements`; templates only from the
  positional value or `value =`/`path =`); test
  `spring_produces_and_consumes_literals_are_not_route_templates`.
- **13, 36 — class template leaked across controllers**: class-level template is
  reset at every class declaration; test
  `spring_class_prefix_does_not_leak_into_unmapped_controllers`.
- **12 — method-level `@RequestMapping` mislabeled `http_method`**:
  `attribute_kind` now mirrors the annotation kind (`request_mapping` for
  `@RequestMapping`, `http_method` for shortcut annotations); test
  `spring_bare_method_level_request_mapping_emits_request_mapping_kind`;
  sanctioned golden change (2 rows).
- **32 — unreachable `is_java_class_declaration` arms**: removed.
- **5 — Express chains broke on newlines / scanned handler bodies**: sequential
  chain walker in `node.rs` that jumps over each chained call's argument list;
  tests `express_multi_line_route_chains_emit_per_verb_facts`,
  `express_chain_scan_ignores_calls_inside_handler_bodies`.
- **6 — `app.get('port')` settings getter emitted a route**: verb-method calls
  now require an argument after the route literal; test
  `express_settings_getter_calls_stay_silent`.
- **37 — fastify plugin-param receivers ignored the import gate**: a parameter
  named `fastify` attests by itself; `app` requires an in-file fastify import
  (adjudicated, documented in contract docs); tests
  `module_exports_app_parameter_without_fastify_import_stays_silent`,
  `fastify_plugin_app_parameter_with_import_emits`.
- **4 — gin/echo receiver needles matched inside longer identifiers**:
  `is_identifier_boundary` on all receiver-call scans; test
  `gin_routes_on_longer_identifiers_stay_silent`.
- **8 — Go 1.22 host patterns unimplemented**: `split_go_pattern` parses
  `[METHOD ][HOST]/[PATH]`; optional `host` key added to the registry/contract;
  test `go_net_http_host_patterns_record_host_separately`. Adjudication: the
  plan's "route_template stays the full raw pattern" wording was revised —
  `route_template` carries the path part (verb/host split out), matching
  shipped goldens and join semantics.
- **9 — gin `Any`/`Handle` + nested groups, echo `Any` unimplemented**:
  implemented, including prefix composition via fixpoint and non-literal
  poisoning; tests `gin_any_handle_and_nested_groups_emit_boundary_facts`,
  `gin_non_literal_group_prefixes_poison_the_prefix_chain`,
  `echo_any_and_other_major_versions_emit`.
- **10 — gin/echo `api_style` wrongly `mux_routing`**: now `call_routing`
  (registry descriptions updated; sanctioned golden change, 2 rows); test
  `gin_and_echo_routes_carry_call_routing_api_style`.
- **11 — echo import gate pinned to `/v4`**: any major version of
  `github.com/labstack/echo` accepted (`is_echo_import_path`).
- **16 — Go client needle lacked boundary check**: fixed; test
  `go_client_calls_on_longer_identifiers_stay_silent`. Backtick raw-string URLs
  also fixed (`go_backtick_raw_string_urls_emit_client_requests`).
- **17 — Java `HttpRequest.newBuilder` matched longer type names**: boundary
  check added; test `java_builder_on_longer_type_names_stays_silent`.
- **7 — client_request node-kind contract row inaccurate for Java**: contract
  docs now state Java builder chains anchor the enclosing statement.
- **28 — dead `typescript` client dispatch arm**: removed from `mod.rs`.
- **18–27, 29–31, 33 — duplication/structure (the "altitude" findings)**: one
  shared scanning core (`framework_structural_facts/scan.rs`) now provides the
  per-byte `SourceMask` (single O(n) pass per file, fixing the O(n^2)
  re-lexing in 29), masked delimiter/statement scanners, per-language string
  literal parsers, and the shared `route_fact` builder. `http_clients/mod.rs`
  hosts the one shared `client_fact`. Go imports are scanned once
  (`collect_go_imports`, fixing 30). JS helpers are shared from
  `web_structural_facts::{js_object_scan, js_imports}` (18, 31). One
  `JS_VERB_METHODS` table (27). `insert_string_array`/`is_ascii_identifier`
  live in `helpers.rs` (25, 31). Ruby/Java string parsers deduplicated
  (23, 24).

Verification: 122 structural-facts/framework tests green (15 new), golden
fixtures regenerated with only sanctioned diffs plus new-behavior rows,
capability matrix green, strict data-quality report clean
(`silent_cells: 0`, `quality_bar_debts: 0`).
