# Finish the HTTP Boundary Lane — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Complete the HTTP request↔handler boundary so Miller can bridge a client call to its server handler across NestJS, Kotlin+Spring, Laravel, Phoenix, axum, and actix — plus client-request facts for kotlin/php/elixir/rust and per-framework prefix-registration facts.

**Architecture:** Each framework is a plain free-function collector in `base/framework_structural_facts/` (server) and `base/framework_structural_facts/http_clients/` (client), dispatched by language in `mod.rs`, reusing the shipped `route_fact`/`normalize_route_template` builders and the one `normalized_route_template` join key. New-language collectors are AST-driven; static-vs-dynamic detection uses a tree-sitter whole-argument allowlist check (ADR-0005), never a hand-rolled mask.

**Tech Stack:** Rust, tree-sitter (kotlin-ng, elixir, php, rust grammars), the julie-extractors structural-facts framework, Node.js data-quality report script.

**Architecture Quality:** Design §4.1 Gate Mode — additive; caller-facing artifact interface unchanged (same `StructuralFact` rows, same join key, additive pattern ids). Only genuinely shared new surface is the AST static-argument helper (§4.4), reused across the 4 new languages. Architecture risk: **medium** (silence-critical AST enumeration + new prefix-registration shape + shared-file churn). Doubt Pass + Codex review folded into the spec. Full design: `docs/plans/2026-07-03-finish-http-boundary-lane-design.md`.

## Global Constraints

Bind every task. Copied from the design (`…-design.md`).

- **M2 silence:** emit facts ONLY for static route/URL literals; a false positive (guessed/wrong route) is worse than a miss. Silence is the default.
- **Static-literal detection = AST whole-argument allowlist check (§4.4).** Run on the whole route/URL argument node; emit only when it is *itself* a plain static string literal of the language (reject `binary_expression`/concat, `format!`/`sprintf`/macro calls, identifiers, member/subscript access), then require no interpolation child. Allowlist safe node kinds; unknown wrapper nodes fail closed to silence. Never a hand-rolled per-byte mask for the new languages.
- **`normalized_route_template` (`:param` flavor) is the single server-side join key.** Do not invent a second. Use existing `ParamFlavor` variants; add one only if none fits, with exhaustive table-driven unit tests.
- **Prefix/receiver tracing:** same-file, single-assignment (data-flow) OR lexical-containment (block/decorator) only; conflicting or non-literal prefix POISONS (emit `route_template` only). Cross-file join is Miller's job.
- **Verb omission = not verb-restricted** (omit `verb`/`verb_source`; verb UPPERCASE; verb_source = `attested`|`default`).
- **`actix.scope_route.v1` is binary:** fully shipped (spec + fixture + emission) OR fully absent (no registry spec/row) + a Rust `open_gaps` entry. Never registered-but-unemitted (the registry conformance test rejects dead specs).
- **Contract marker bump:** append `.backend-http-boundary-v2` to `EXTRACTION_CONTRACT_VERSION` (`crates/julie-extractors/src/lib.rs`); update the api-surface marker test.
- **Data-quality bar:** `open_gaps` entries carry reason + required closure + planned task; `not_applicable` only when the language genuinely lacks the construct. `node scripts/language-data-quality-report.mjs --strict` must end with `silent_cells=0` AND `quality_bar_debts=0` (read the debts line; don't trust the exit code alone).
- **Registry conformance:** every emitted metadata key declared in `structural_fact_registry.rs` SPECS with correct type + presence; per-language registered pattern-id set must equal emitted set; regenerate `docs/contracts/structural-fact-patterns.json` (`UPDATE_CONTRACT_JSON=1 cargo test -p julie-extractors structural_fact_registry`) and commit it (byte-for-byte sync test is ungated in the default suite).
- **Test discipline:** default suite stays under the **90s tripwire**; per-language narrow test commands; no slow gates leaked into default.
- **Doc sync:** `AGENTS.md` and `CLAUDE.md` stay byte-equivalent — run `scripts/check-agent-doc-sync.sh` before completion.

## Verification Strategy

**Project source of truth:** `CLAUDE.md` (Test Discipline, Data Quality Bar), the design doc §5/§8, and the crate's feature-gated test names.

**Worker red/green scope:** the framework's own test module — `cargo test -p julie-extractors <fw>` (e.g. `nestjs`, `kotlin_spring`, `laravel`, `phoenix`, `axum`, `actix`, and `http_client` for client arms). This proves the new emission + negative/silent cases + the `containing_symbol_id` binding assertion.

**Worker ceiling:** the worker may run its own framework test module, the AST static-argument unit tests it added, and the three feature-gated conformance gates for its slice:
- `cargo test -p julie-extractors --features test-golden golden_fixtures_match_canonical_extraction`
- `cargo test -p julie-extractors --features test-golden structural_fact_registry`
- `cargo test -p julie-extractors --features test-capability-matrix capability_matrix`
Workers do not own the full-crate regression run.

**Worker gate invariant:**
- framework test module → the collector emits exactly the expected facts for static routes and **nothing** for the negative fixtures (concat/format/const/interpolation/comment).
- `structural_fact_registry` → every emitted metadata key is declared; registered pattern ids == emitted ids per language (no dead specs).
- `golden_fixtures_match_canonical_extraction` → the committed `expected.json` matches canonical extraction.
- `capability_matrix` → each claimed `capabilities.json` row is fixture-backed.

**Lead affected-change scope:** after each task, the lead runs the regenerated-JSON sync check and `node scripts/language-data-quality-report.mjs --strict` (must be `silent_cells=0`, `quality_bar_debts=0`).

**Branch gate:** before PR — full `cargo test -p julie-extractors` (release features as documented), the strict data-quality report, `scripts/check-agent-doc-sync.sh`, and a wall-clock check that the default suite is under the 90s tripwire.

**Replay/metric evidence:** the data-quality report `silent_cells` and `quality_bar_debts` are **hard gates** (both must be 0); the per-domain counts are report-only.

**Escalation triggers:** any change to `scan.rs` shared builders, `http_boundary.rs` normalizer, or a new `ParamFlavor` variant → run the full crate test suite (those are consumed by every family). A new grammar dependency in `Cargo.toml` → `cargo build -p julie-extractors` from clean.

**Assigned verification failure:** workers stop and report when assigned verification fails, unless a task explicitly says to update that gate (only Task 0 updates the api-surface marker test).

**Verification ledger:** record invariant, command, scope label, commit SHA, result, timestamp per task. Reuse a passing ledger entry at the same HEAD instead of rerunning an expensive gate.

## Parallel Execution Contract

Commit mode: **`serial-worker-commit`** — tasks run serially (heavy shared-file contention), each worker commits its slice after assigned verification passes. All framework tasks edit the same shared registration files (`framework_structural_facts/mod.rs`, `structural_fact_registry.rs`, `structural_facts.rs`, `capabilities.json`, the regenerated `structural-fact-patterns.json`, and `src/tests` module registration), so they cannot safely run in parallel worktrees — the regenerated contract JSON alone guarantees merge conflicts.

| Task | Parallel batch | File ownership | Serialization required | Dependency reason |
|---|---|---|---|---|
| Task 0: Foundation (AST helper + marker + ADR) | None - serial | Create `framework_structural_facts/static_arg.rs`; Modify `lib.rs` (version const), `src/tests/api_surface.rs`; Create `docs/decisions/0005-ast-static-literal-detection.md` | Yes | Contract-first + risk-first: the shared static-argument helper, the marker bump, and the ADR gate all downstream tasks. |
| Task 1: NestJS server routes | None - serial | Create `framework_structural_facts/nestjs.rs`, `src/tests/nestjs/`, `fixtures/extraction/typescript/nestjs_routes/` (+ js); Modify shared registration files | Yes | Shared registration files (mod.rs dispatch/consts, registry SPECS, capabilities.json, contract JSON, tests reg) are edited by every task. |
| Task 2: Kotlin+Spring (server + client) | None - serial | Create `framework_structural_facts/kotlin_spring.rs`, `http_clients/kotlin.rs`, `src/tests/kotlin_spring/`, `fixtures/extraction/kotlin/*`; Modify shared registration files + `spring.request_mapping.v1` languages + `static_arg.rs` (kotlin arm) + `http_clients/mod.rs` | Yes | Same shared registration files; also adds the Kotlin arm of the static-arg helper. |
| Task 3: Laravel (server + client) | None - serial | Create `framework_structural_facts/laravel.rs`, `http_clients/php.rs`, `src/tests/laravel/`, `fixtures/extraction/php/*`; Modify shared files + `static_arg.rs` (php arm) + `http_clients/mod.rs` | Yes | Same shared registration files; adds the PHP arm. |
| Task 4: Phoenix (server + client) | None - serial | Create `framework_structural_facts/phoenix.rs`, `http_clients/elixir.rs`, `src/tests/phoenix/`, `fixtures/extraction/elixir/*`; Modify shared files + `static_arg.rs` (elixir arm) + `http_clients/mod.rs` | Yes | Same shared registration files; adds the Elixir arm. |
| Task 5: axum (server + client) | None - serial | Create `framework_structural_facts/axum.rs`, `http_clients/rust.rs`, `src/tests/axum/`, `fixtures/extraction/rust/*`; Modify shared files + `static_arg.rs` (rust arm) + `http_clients/mod.rs` | Yes | Same shared registration files; adds the Rust arm (reused by Task 6). |
| Task 6: actix (server) | None - serial | Create `framework_structural_facts/actix.rs`, `src/tests/actix/`, `fixtures/extraction/rust/actix_*`; Modify shared files | Yes | Reuses the Rust static-arg arm + rust dispatch from Task 5; same shared registration files. |
| Task 7: Contract sweep + release readiness | None - serial | Modify `capabilities.json` (open_gaps), `docs/contracts/*`, `CHANGELOG`/version; Create `docs/plans/2026-07-03-miller-http-boundary-consumer-followup.md` | Yes | Final gate; depends on all prior tasks landing. |

## Task 0: Foundation — AST static-argument helper, contract marker, ADR-0005

**Files:**
- Create: `crates/julie-extractors/src/base/framework_structural_facts/static_arg.rs`
- Modify: `crates/julie-extractors/src/base/framework_structural_facts/mod.rs` (add `mod static_arg;`)
- Modify: `crates/julie-extractors/src/lib.rs:127` (`EXTRACTION_CONTRACT_VERSION`)
- Modify: `crates/julie-extractors/src/tests/api_surface.rs` (marker enumeration test)
- Create: `docs/decisions/0005-ast-static-literal-detection.md`
- Test: `crates/julie-extractors/src/base/framework_structural_facts/static_arg.rs` (inline `#[cfg(test)]` table-driven unit tests)

**Interfaces:**
- Produces: `static_route_arg(node: tree_sitter::Node, content: &str, lang: StaticArgLang) -> Option<&str>` — returns the literal's inner text ONLY when `node` is an approved static string-literal argument for `lang` (whole-argument allowlist per §4.4); `None` for concat/format/macro/identifier/interpolated/heredoc-with-interp. `enum StaticArgLang { Kotlin, Elixir, Php, Rust }` with per-language arms added by later tasks (Task 0 ships the enum + Rust arm + the shared allowlist scaffold and test harness).
- Produces: `EXTRACTION_CONTRACT_VERSION` ending in `.backend-http-boundary-v2`.

**Contract inputs:** design §4.4 (per-language node kinds), §6 (marker doctrine), `lib.rs:121` doc-comment (bump rule).

**File ownership:** Create `static_arg.rs`; Modify `lib.rs` (version const), `src/tests/api_surface.rs`; Create `docs/decisions/0005-...md`.

**Serialization required:** Yes.

**Dependency reason:** Contract-first + risk-first: the shared helper, marker, and ADR gate all downstream tasks.

**What to build:** The shared, silence-critical static-argument helper and its test harness; the release contract-marker bump; and the ADR recording the AST-over-mask decision.

**Approach:**
- `static_route_arg` takes the *argument expression node* (not a pre-plucked literal). It matches the node kind against a per-language allowlist of static string-literal kinds; anything else (binary/additive concat, call/macro like `format!`, identifier, member/subscript, array) → `None`. On a matched literal, apply the interpolation/child check from §4.4. Ship the **Rust arm** now (trivial: `string_literal`/`raw_string_literal` accepted, but the wrapper-node rejection of `format!`/concat/`const` is the real work) as the reference implementation; later tasks add Kotlin/Elixir/PHP arms in their slices.
- Table-driven unit tests enumerate, per language arm present: accepted static forms and every rejected form (concat, format/macro, const-ref, interpolation, comment-adjacent). Tests are the silence guard — be exhaustive.
- Bump `EXTRACTION_CONTRACT_VERSION` by appending `.backend-http-boundary-v2`; update the api-surface marker test to expect it.
- ADR-0005: context (mask is the silence guard; hand-rolling `$`/sigil/heredoc lexers is the highest M2 risk), decision (AST whole-argument allowlist check for new languages; mask not extended), consequences, applies-to, future-agents.

**Acceptance criteria:**
- [ ] `static_route_arg` exists with the Rust arm + allowlist scaffold; rejects concat/format/const wrappers (unit-tested).
- [ ] Exhaustive table-driven unit tests for the Rust arm pass; harness ready for Kotlin/Elixir/PHP arms.
- [ ] `EXTRACTION_CONTRACT_VERSION` ends with `.backend-http-boundary-v2`; api-surface marker test updated and green.
- [ ] `docs/decisions/0005-ast-static-literal-detection.md` written.
- [ ] Worker-scope verification passes; committed per `serial-worker-commit`.

## Task 1: NestJS server route facts (`nestjs.route.v1`)

**Files:**
- Create: `crates/julie-extractors/src/base/framework_structural_facts/nestjs.rs`
- Modify: `framework_structural_facts/mod.rs` (extend the ts/js/jsx/tsx dispatch arms; add `NESTJS_ROUTE_PATTERN_ID` const; test-gated pattern-id arm)
- Modify: `structural_fact_registry.rs` (SPECS: `nestjs.route.v1`), `structural_facts.rs` (test arm), `capabilities.json` (typescript + javascript rows), `docs/contracts/structural-fact-patterns.json` (regenerate), `docs/contracts/{jsonl-v3,sqlite-schema-v3}.md`
- Create: `src/tests/nestjs/` (+ register in `src/tests` mod), `fixtures/extraction/typescript/nestjs_routes/{source.ts,expected.json}`, `fixtures/extraction/javascript/nestjs_routes/{source.js,expected.json}`
- Test: `src/tests/nestjs/`

**Interfaces:**
- Consumes: `route_fact`/`RouteFactSpec` (`scan.rs:533`/`:516`), `normalize_route_template(_, ParamFlavor::Colon)` (`http_boundary.rs:20`), the existing TS/JS collector path.
- Produces: `nestjs.route.v1` facts with `api_style="decorator_routing"`, `route_template`, `normalized_route_template` (Colon `:id`), `verb`/`verb_source`, class-`@Controller`-joined `effective_route_template`.

**Contract inputs:** design §2a, §5 checklist; the shipped Spring class+method join model (`spring.rs:58`) as the decorator-prefix analog.

**File ownership:** per the contract table (Task 1 row).

**Serialization required:** Yes. **Dependency reason:** shared registration files.

**What to build:** A NestJS collector emitting one route fact per `@Get/@Post/@Put/@Patch/@Delete/@All(...)` method decorator, joined to its `@Controller('base')` class prefix same-file. `@All` → verb omitted.

**Approach:** Rides the existing TS/JS infrastructure (no new `static_arg` arm — TS uses the existing path, but apply the whole-argument principle: reject `@Get('/a/' + x)`). Class `@Controller` prefix (string | `{path}` | array) → `class_route_template`; method decorator subpath joined → `effective_route_template`. Fact span on the decorator so `containing_symbol_id` binds to the method. Document exclusions: `RouterModule.register` dynamic composition and `setGlobalPrefix` (cross-file) stay silent. Negative fixtures: interpolation/concat decorator args, dynamic verb. Follow the §5 definition-of-done for registry/capabilities/contract/tests.

**Acceptance criteria:**
- [ ] `nestjs.route.v1` emitted for TS and JS with correct verb + normalized template; `@Controller` prefix joined into `effective_route_template`.
- [ ] Negative fixtures (concat/interpolation decorator args) emit nothing.
- [ ] `containing_symbol_id` binds to the handler method (binding assertion).
- [ ] Registry spec + regenerated JSON + capabilities rows + contract docs updated; all four worker gates green.
- [ ] Worker-scope verification passes; committed.

## Task 2: Kotlin+Spring server routes + Kotlin client facts

**Files:**
- Create: `framework_structural_facts/kotlin_spring.rs`, `framework_structural_facts/http_clients/kotlin.rs`
- Modify: `framework_structural_facts/mod.rs` (new `kotlin` server dispatch arm + pattern-id test arm), `http_clients/mod.rs:21` (kotlin client arm), `static_arg.rs` (Kotlin arm), `structural_fact_registry.rs` (`spring.request_mapping.v1` `languages` → `[java, kotlin]`; `http.client_request.v1` add kotlin), `structural_facts.rs`, `capabilities.json` (kotlin row: `spring.request_mapping.v1` + `http.client_request.v1` + Ktor/coRouter `open_gaps`), regenerated JSON, contract docs
- Create: `src/tests/kotlin_spring/`, `fixtures/extraction/kotlin/spring_routes/{source.kt,expected.json}`, `fixtures/extraction/kotlin/http_client/{source.kt,expected.json}`
- Test: `src/tests/kotlin_spring/`, `src/tests/http_client/` (kotlin cases)

**Interfaces:**
- Consumes: `static_route_arg(_, _, StaticArgLang::Kotlin)` (adds the Kotlin arm), `route_fact`, `normalize_route_template(_, ParamFlavor::Braces)`, `http_boundary::client_request_metadata`/`classify_url`.
- Produces: `spring.request_mapping.v1` facts for kotlin (`framework="spring"`, `api_style="annotation_routing"`, metadata key set identical to the Java spec); `http.client_request.v1` for kotlin.

**Contract inputs:** design §2a/§2b, the Kotlin+Spring verdict (new collector, reuse pattern id), §4.4 Kotlin node kinds (`string_literal`/`multiline_string_literal`, `interpolation` child).

**File ownership:** per the contract table (Task 2 row).

**Serialization required:** Yes. **Dependency reason:** shared registration files + adds the Kotlin static-arg arm.

**What to build:** A Kotlin Spring collector (annotation controllers) reusing the `spring.request_mapping.v1` id with a Kotlin-correct implementation, plus the Kotlin arm of `static_route_arg`, plus a Kotlin client collector for Ktor client (or OkHttp).

**Approach:** Add the Kotlin `static_arg` arm first (emit only `string_literal`/`multiline_string_literal` with no `interpolation` child; reject `+` concat, `"$base/x"`, identifiers). Kotlin annotation arrays use `["/a","/b"]` (brackets, not Java `{...}`) — handle multi-path. Class `@RequestMapping` prefix joined per-class (reset per class/object/companion object). Verb from `@GetMapping`/… name or `@RequestMapping(method=[RequestMethod.X])`. Handle single-line `@GetMapping("/x") fun f()` binding. Emit the **exact** Java `spring.request_mapping.v1` metadata key set/types (registry conformance is strict). Client: Ktor `client.get("https://...")` / `HttpClient` calls with a static URL literal → `http.client_request.v1`. Exclusions as `open_gaps`: `RouterFunction`/`coRouter` DSL, Ktor server routing. Negative fixtures: `@GetMapping("$base/x")` and `@GetMapping("/a" + x)` emit nothing.

**Acceptance criteria:**
- [ ] Kotlin `static_route_arg` arm with exhaustive unit tests (accepts static; rejects `$`/`${}`/concat/const).
- [ ] `spring.request_mapping.v1` emitted for Kotlin annotation controllers with the Java-identical metadata contract; bracket multi-path handled; class prefix joined; binding asserted.
- [ ] `$`-interpolation and concat cases emit nothing (negative fixtures).
- [ ] `http.client_request.v1` emitted for the primary Kotlin client from static URLs.
- [ ] Ktor server routing + `RouterFunction`/`coRouter` + deferred clients recorded as `open_gaps` with reason + closure task.
- [ ] Registry (`languages=[java,kotlin]`) + regenerated JSON + capabilities + contract docs updated; four worker gates green.
- [ ] Worker-scope verification passes; committed.

## Task 3: Laravel server routes + prefix + PHP client facts

**Files:**
- Create: `framework_structural_facts/laravel.rs`, `http_clients/php.rs`
- Modify: `mod.rs` (new `php` server arm + pattern-id test arm), `http_clients/mod.rs` (php arm), `static_arg.rs` (PHP arm), `structural_fact_registry.rs` (`laravel.route.v1`, `laravel.resource_route.v1`, `laravel.route_prefix.v1`, `http.client_request.v1` + php), `structural_facts.rs`, `capabilities.json` (php row), regenerated JSON, contract docs
- Create: `src/tests/laravel/`, `fixtures/extraction/php/laravel_routes/{source.php,expected.json}`, `fixtures/extraction/php/http_client/{source.php,expected.json}`
- Test: `src/tests/laravel/`, `src/tests/http_client/` (php cases)

**Interfaces:**
- Consumes: `static_route_arg(_, _, StaticArgLang::Php)` (adds PHP arm), `route_fact`, `normalize_route_template(_, ParamFlavor::Braces)`.
- Produces: `laravel.route.v1`, `laravel.resource_route.v1`, `laravel.route_prefix.v1`, `http.client_request.v1` (php).

**Contract inputs:** design §2a/§2b/§2c, §4.4 PHP node kinds (`string` vs `encapsed_string`, allowlist of `string_content`/`escape_sequence`; nowdoc/heredoc distinct kinds).

**File ownership:** per the contract table (Task 3 row).

**Serialization required:** Yes. **Dependency reason:** shared registration files + PHP static-arg arm.

**What to build:** Laravel facade-route collector (`Route::get('/x', [Ctrl::class,'m'])`), resource routes, same-file prefix (`Route::prefix('lit')->group(...)` and `Route::group(['prefix'=>'lit'], ...)`) → `route_group_prefix`/`effective_route_template` and a `laravel.route_prefix.v1` at the prefix site; plus the PHP arm of `static_arg` and a Guzzle + `Http` facade client collector.

**Approach:** PHP `static_arg` arm uses the allowlist: `string` (single-quote) always static; `encapsed_string` only if children are exclusively `string_content`/`escape_sequence` (reject `variable_name`/`member_access_expression`/`subscript_expression`/`dynamic_variable_name`); `heredoc` same check; `nowdoc` static. `Route::resource`/`apiResource` → `laravel.resource_route.v1`. `RouteServiceProvider` `/api` prefix is cross-file → document limitation (route_template is not the absolute path), but a same-file `Route::prefix()` literal emits `laravel.route_prefix.v1`. Client: Guzzle `$client->get('url')` + `Http::get('url')`. Exclusions: `#[Route]` attributes (Symfony), interpolated/concat args, Symfony/curl clients → `open_gaps`.

**Acceptance criteria:**
- [ ] PHP `static_route_arg` arm with exhaustive unit tests (allowlist; rejects `.` concat, `"$x"`, `{$x}`, heredoc-with-interp, const refs).
- [ ] `laravel.route.v1` + `laravel.resource_route.v1` + `laravel.route_prefix.v1` emitted with correct verbs/templates; same-file prefix joined; binding asserted.
- [ ] `http.client_request.v1` emitted for Guzzle + `Http` facade static URLs.
- [ ] `#[Route]` attributes, Symfony/curl clients, cross-file `RouteServiceProvider` prefix recorded as `open_gaps`/documented limitations.
- [ ] Registry + regenerated JSON + capabilities + contract docs updated; four worker gates green.
- [ ] Worker-scope verification passes; committed.

## Task 4: Phoenix server routes + forward + Elixir client facts

**Files:**
- Create: `framework_structural_facts/phoenix.rs`, `http_clients/elixir.rs`
- Modify: `mod.rs` (new `elixir` server arm + pattern-id test arm), `http_clients/mod.rs` (elixir arm), `static_arg.rs` (Elixir arm), `structural_fact_registry.rs` (`phoenix.route.v1`, `phoenix.resource_route.v1`, `phoenix.forward.v1`, `http.client_request.v1` + elixir), `structural_facts.rs`, `capabilities.json` (elixir row), regenerated JSON, contract docs
- Create: `src/tests/phoenix/`, `fixtures/extraction/elixir/phoenix_routes/{source.ex,expected.json}`, `fixtures/extraction/elixir/http_client/{source.ex,expected.json}`
- Test: `src/tests/phoenix/`, `src/tests/http_client/` (elixir cases)

**Interfaces:**
- Consumes: `static_route_arg(_, _, StaticArgLang::Elixir)` (adds Elixir arm), `route_fact`, `normalize_route_template(_, ParamFlavor::Colon)`, the Rails `scope_stack` model shape (`rails.rs:368`) as the lexical-containment analog (AST form).
- Produces: `phoenix.route.v1`, `phoenix.resource_route.v1`, `phoenix.forward.v1`, `http.client_request.v1` (elixir).

**Contract inputs:** design §2a/§2b/§2c, §4.3 (lexical-containment analog), §4.4 Elixir node kinds (`string`/`sigil`/`charlist`, `interpolation` child, `sigil_name ∈ {s,S}`).

**File ownership:** per the contract table (Task 4 row).

**Serialization required:** Yes. **Dependency reason:** shared registration files + Elixir static-arg arm.

**What to build:** Phoenix router-macro collector (`get "/path", Ctrl, :action`) with `scope "/api" do … end` lexical prefixes, `resources` → resource routes, `forward "/lit", Plug` → `phoenix.forward.v1`; the Elixir arm of `static_arg`; and a Req client collector.

**Approach:** Elixir `static_arg` arm: `string`/`sigil`/`charlist` with no `interpolation` child; for a `sigil`, require `sigil_name ∈ {s,S}`. Verb = macro name (`get`/`post`/…, attested). `scope` prefixes via an AST lexical-containment stack (reuse the Rails shape: poison on non-literal/interpolated prefix). `resources "/x", Ctrl` → `phoenix.resource_route.v1`. `forward` → `phoenix.forward.v1` prefix fact. Client: `Req.get("url")`. Exclusions (fresh, no prior deferral text): `pipe_through`, `live`, `socket`, `channel`, options-only scope contribute no prefix; Tesla/HTTPoison/Finch/`:httpc` clients → `open_gaps`. Negative fixtures: `get "/u/" <> id`, `#{}`-interpolated paths, `~r` sigils emit nothing.

**Acceptance criteria:**
- [ ] Elixir `static_route_arg` arm with exhaustive unit tests (accepts `~s`/plain static; rejects `<>` concat, `#{}`, `~r`, heredoc-with-interp).
- [ ] `phoenix.route.v1` + `phoenix.resource_route.v1` + `phoenix.forward.v1` emitted; `scope` prefixes joined same-file; binding asserted.
- [ ] `http.client_request.v1` emitted for Req static URLs.
- [ ] `pipe_through`/`live`/`socket`/`channel` and deferred clients recorded as `open_gaps`/documented exclusions.
- [ ] Registry + regenerated JSON + capabilities + contract docs updated; four worker gates green.
- [ ] Worker-scope verification passes; committed.

## Task 5: axum server routes + nest + Rust client facts

**Files:**
- Create: `framework_structural_facts/axum.rs`, `http_clients/rust.rs`
- Modify: `mod.rs` (new `rust` server arm + pattern-id test arm), `http_clients/mod.rs` (rust arm), `static_arg.rs` (Rust arm already scaffolded in Task 0 — finalize for route args), `structural_fact_registry.rs` (`axum.route.v1`, `axum.nest.v1`, `http.client_request.v1` + rust), `structural_facts.rs`, `capabilities.json` (rust row), regenerated JSON, contract docs
- Create: `src/tests/axum/`, `fixtures/extraction/rust/axum_routes/{source.rs,expected.json}`, `fixtures/extraction/rust/http_client/{source.rs,expected.json}`
- Test: `src/tests/axum/`, `src/tests/http_client/` (rust cases)

**Interfaces:**
- Consumes: `static_route_arg(_, _, StaticArgLang::Rust)`, `route_fact`, `normalize_route_template(_, ParamFlavor::Braces)`, the Go single-assignment + poison receiver model (`go_http.rs`) as the builder-chain analog.
- Produces: `axum.route.v1`, `axum.nest.v1`, `http.client_request.v1` (rust). The `rust` server dispatch arm (shared with Task 6 — Task 5 creates it, Task 6 extends it).

**Contract inputs:** design §2a/§2c, §4.4 Rust (no interpolation; whole-argument rejects `format!`/concat/const), the 0.7-vs-0.8 param under-report decision.

**File ownership:** per the contract table (Task 5 row).

**Serialization required:** Yes. **Dependency reason:** shared registration files; creates the rust dispatch arm Task 6 extends; adds/finalizes the Rust static-arg arm.

**What to build:** axum route collector (`Router::new().route("/path", get(handler))`), `.nest("/lit", sub)` → `axum.nest.v1` mount, receiver-traced same-file `Router` builder; plus the Rust client collector (reqwest).

**Approach:** `route("/path", get(h).post(c))` → one fact per method-router verb; `any`/`any_service` → verb omitted. Receiver `Router::new()` single-assignment traced (poison on conflict/non-literal). `.nest("/lit", expr)` → `axum.nest.v1` with `mount_path`/`normalized_mount_path`; the target is a cross-file fn call → no guessed join (Miller's job). Flavor `Braces` (0.8); document the 0.7 `:id` honest under-report (no version-sniff). Disambiguate axum vs actix by import gate (`use axum::`) + arg1 shape. Client: `reqwest::get("url")` / `client.get("url")`. Negative fixtures: `.route(format!("/u/{id}").as_str(), …)`, concat, const path emit nothing.

**Acceptance criteria:**
- [ ] Rust `static_route_arg` arm finalized for route args (rejects `format!`/concat/const wrappers); unit-tested.
- [ ] `axum.route.v1` emitted (one per verb; `any` → verb omitted); receiver poison on non-literal; binding asserted.
- [ ] `axum.nest.v1` emitted for same-file `.nest(literal, …)`; no guessed cross-file join.
- [ ] `http.client_request.v1` emitted for reqwest static URLs.
- [ ] 0.7 param under-report + deferred rust clients documented (`open_gaps`/limitation).
- [ ] Registry + regenerated JSON + capabilities + contract docs updated; four worker gates green.
- [ ] Worker-scope verification passes; committed.

## Task 6: actix server routes (attribute + scope) + mount

**Files:**
- Create: `framework_structural_facts/actix.rs`
- Modify: `mod.rs` (extend the `rust` server arm to also run actix; pattern-id test arm), `structural_fact_registry.rs` (`actix.attribute_route.v1`, `actix.mount.v1`, and `actix.scope_route.v1` **iff completed**), `structural_facts.rs`, `capabilities.json` (rust row), regenerated JSON, contract docs
- Create: `src/tests/actix/`, `fixtures/extraction/rust/actix_attribute_routes/{source.rs,expected.json}`, `fixtures/extraction/rust/actix_scope_routes/{source.rs,expected.json}` (iff scope shipped)
- Test: `src/tests/actix/`

**Interfaces:**
- Consumes: `static_route_arg(_, _, StaticArgLang::Rust)` (from Task 5), the `rust` dispatch arm (from Task 5), `route_fact`, `normalize_route_template(_, ParamFlavor::Braces)`.
- Produces: `actix.attribute_route.v1` (`api_style="attribute"`, verb ALWAYS, no `route_group_prefix`/`effective_route_template` keys), `actix.mount.v1`, and — iff completed — `actix.scope_route.v1` (`api_style="call_routing"`, verb OPT + verb_source, `route_group_prefix`/`effective_route_template` OPT).

**Contract inputs:** design §2a/§4.5 (two provenance models, aspnet split precedent), the binary scope_route rule (Global Constraints).

**File ownership:** per the contract table (Task 6 row).

**Serialization required:** Yes. **Dependency reason:** reuses Task 5's rust arm/dispatch; shared registration files.

**What to build:** actix attribute-macro route collector (`#[get("/x")]`) as a complete claim; the `web::scope("/lit")` mount fact; and — if feasible within scope — the builder scope-route collector. If scope-route slips, it is fully absent + a Rust `open_gap`, never a dead spec.

**Approach:** Attribute route: `#[get("/x")]`/`#[route("/x", method="GET")]` on a fn → `actix.attribute_route.v1`, verb from macro name (ALWAYS), no in-file prefix (registration is cross-file). Mount: `web::scope("/lit")` bound to `.configure(fn)`/`.service` → `actix.mount.v1`. Scope route (attempt): `web::scope("/api").route("/x", web::post().to(h))` — the scope prefix is same-file in the same chain → `route_group_prefix` + `effective_route_template`; verb from `web::<verb>()` (OPT). Disambiguate from axum by import gate (`use actix_web::`) + arg shape. Exclusions: `.configure(fn)` cross-file scope, `web::resource().route()` guard forms → `open_gaps`. **Decision point:** attempt `actix.scope_route.v1`; if same-file scope tracing can't be made honest within the slice, drop its spec/row/fixtures entirely and file the Rust `open_gap` with a named closure task.

**Acceptance criteria:**
- [ ] `actix.attribute_route.v1` emitted for `#[get/post/route(...)]` macros with verb ALWAYS; binding asserted; complete honest claim.
- [ ] `actix.mount.v1` emitted for `web::scope(literal)`.
- [ ] `actix.scope_route.v1` EITHER fully shipped (spec + fixture + emission, same-file scope joined) OR fully absent + a Rust `open_gaps` entry — no dead registry spec.
- [ ] Negative fixtures (format/concat/const route args) emit nothing.
- [ ] Registry + regenerated JSON + capabilities + contract docs updated; four worker gates green.
- [ ] Worker-scope verification passes; committed.

## Task 7: Contract sweep + release readiness

**Files:**
- Modify: `fixtures/extraction/capabilities.json` (Ktor + all deferred idioms/libraries as `open_gaps` with reason + closure; verify every claimed row is fixture-backed)
- Modify: `docs/contracts/{jsonl-v3,sqlite-schema-v3}.md` (final consistency), `CHANGELOG.md`/version metadata (v2.8.0 prep — do NOT release; user approval gates release)
- Create: `docs/plans/2026-07-03-miller-http-boundary-consumer-followup.md` (per-family Miller consumer checklist: const + whitelist + `IsMountFactPattern` + `RouteFamilyForMount` mapping + anchor rule)
- Verify: `AGENTS.md`/`CLAUDE.md` sync

**Interfaces:**
- Consumes: all prior tasks' emitted facts, capabilities rows, and registry specs.
- Produces: a clean strict data-quality report, a synced contract, and the Miller follow-up tracker.

**Contract inputs:** design §2c (Miller consumer requirements), §8, §10 acceptance criteria; CLAUDE.md release/version rules.

**File ownership:** per the contract table (Task 7 row).

**Serialization required:** Yes. **Dependency reason:** final gate; depends on all prior tasks.

**What to build:** The release-readiness sweep — every capability claim fixture-backed, every deferral an `open_gaps` entry, the strict report at 0/0, contract docs consistent, the Miller cross-repo consumer work captured as a tracked follow-up (not edited here — separate repo).

**Approach:** Enumerate every new/deferred idiom and library from Tasks 1–6 and confirm each is either a fixture-backed claim or an `open_gaps` entry with reason + required closure + planned task (Ktor, `RouterFunction`/`coRouter`, Symfony `#[Route]`, Tesla/HTTPoison/Finch/`:httpc`, OkHttp/Retrofit/WebClient, hyper/ureq, `.configure` cross-file scope, cross-file global prefixes). Write the Miller follow-up doc as the per-family consumer table from design §2c. Run the branch gate. Version bump prep only — **do not tag or release** (needs explicit user approval).

**Acceptance criteria:**
- [ ] `node scripts/language-data-quality-report.mjs --strict`: `silent_cells=0`, `quality_bar_debts=0`.
- [ ] Every claimed `capabilities.json` row is fixture-backed; every deferral is an `open_gaps` entry with reason + closure task.
- [ ] `docs/contracts/*` consistent with emitted facts; regenerated JSON committed.
- [ ] Miller consumer follow-up doc written (per-family const + whitelist + `IsMountFactPattern` + `RouteFamilyForMount` + anchor rule).
- [ ] `scripts/check-agent-doc-sync.sh` passes; full `cargo test -p julie-extractors` green; default suite under the 90s tripwire.
- [ ] Version metadata prepped for v2.8.0 (not tagged/released).
- [ ] Worker-scope verification passes; committed.

## Notes for executors

- Read `docs/plans/2026-07-03-finish-http-boundary-lane-design.md` before starting any task — it carries the per-language node kinds, the join doctrine, and the exclusion list.
- Use `@razorback:grounding-in-current-docs` per §5 step 0 to verify each framework's *current* routing syntax before writing its collector.
- Tasks are serial. After each task: checkpoint and continue immediately to the next (slice boundaries are not stop points).
- The one intentional decision left to the implementer's judgment is `actix.scope_route.v1` (Task 6) — complete it or defer it fully; either is acceptable, a dead spec is not.
