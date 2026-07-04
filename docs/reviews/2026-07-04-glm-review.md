# Web Structural Data + SQLite Review

Four parallel deep-review passes (web, data/SQL, framework, SQLite) plus my own independent pass on the writer and merge/dedup architecture. Every finding below is grounded in code I or a reviewer read at `HEAD` (`d7167a0`), with `file:line` citations.

> **Codex validation note (2026-07-04):** this file now contains the incoming GLM review plus validation annotations from a live repo pass. Treat `Codex validation status` as authoritative where it corrects or narrows a lower section.

## Codex validation status

Validated against `HEAD` `d7167a0` with:

- `cargo build --bin julie-extract`
- Targeted temporary fixtures scanned through `./target/debug/julie-extract scan`
- SQLite queries against the emitted `structural_facts` table
- `node scripts/language-data-quality-report.mjs --strict`

### Confirmed implementation findings

- **P0/P1 silence and contract:** ASP.NET and Spring dynamic-route guessing, Ruby and Java HTTP-client dynamic or partial URL emission, Django `re_path` facts missing `normalized_route_template`, and Vue SFC script comment/string false positives are real.
- **P1/P2 framework extraction:** Express middleware mislabeled as router mounts; Spring comment and class-token prefix gaps; Rails parenthesized routes, multi-resource routes, and member/collection prefix gaps; Actix direct `App::new().route(...)`; Go `var mux`; and Razor `@page` normalized-route omission are real.
- **P1/P2 data and web extraction:** SQL `INTEGER PRIMARY KEY` nullability, YAML flow collections, React/Vue `path:` false positives, JS string escape handling, Next.js/Nuxt file-route gaps, SQL subquery/join/recursive flags, JSON/TOML path bugs, CSS comma-in-attribute selector classification, Markdown inline-code/frontmatter issues, JSX nested route parent loss, and NuxtLink `:to`/relative misses are real.
- **Capability honesty:** `fixtures/extraction/capabilities.json` currently has empty `open_gaps` for languages whose structural support is narrow. The strict data-quality report passes with `silent_cells: 0` and `quality_bar_debts: 0`, so this honesty gap is not currently caught by the gate.
- **SQLite/writer performance:** lack of WAL checkpointing, repeated spool deserialization, JSONL export sorts and metadata reparse, secondary indexes built before fresh bulk loads, delete fan-out, full existing-file scans, per-file lookups/counts, and single-row insert loops are confirmed as scale risks.

### Downgraded or not reproduced

- The duplicate-ID writer crash is **not a current CLI P0**: `extract_for_language` only sorts structural facts, but the CLI artifact mapper dedupes structural facts by ID before writing. Keep a lower-level safety task so non-CLI or future writer paths cannot regress.
- The SQL trigger-name claim did **not** reproduce. The validation fixture emitted the correct `trigger_name` and `target_table`; add a regression test rather than a speculative fix.
- The `index: true` no-boundary match did **not** reproduce.
- `markup_scan` is not fully HTML-comment-aware internally, but the tested HTML artifact output did not emit the commented htmx fact because later tree filtering suppressed it. Keep a regression test and only fix code if a current artifact path fails.
- "Zero unit tests" is overstated. SQL and data-format test modules exist. The actionable issue is thin value-semantic coverage for the reviewed edge cases.
- The Java and Ruby client examples below are imprecise as bare snippets. The Java repro goes through `HttpRequest.newBuilder(... URI.create(...))`; the Ruby repro goes through `Net::HTTP` around `URI.parse(...)`.

## Bottom line

The implementation is **architecturally strong**: single-transaction SQLite writes with correct PRAGMAs, prepared-statement reuse, a solid `static_arg.rs` silence reference, and good route-shape depth where covered. Validation confirmed the major silence-doctrine, Vue, route-normalization, data/web correctness, capability-honesty, and SQLite scale findings. It also downgraded the duplicate-ID writer issue from current CLI P0 to lower-level hardening, and rejected or narrowed a few sample claims. SQLite has no confirmed P0/P1 correctness issue; the new fact volume scales **linearly**, not quadratically, but several P2 perf items compound at scale.

---

## P0 — fix now

**Silence-doctrine breaks (emit guessed routes/URLs for dynamic expressions):**
- `framework_structural_facts/aspnet.rs:560` + `helpers.rs:113` — C# `[Route("/api/" + name)]` / `MapGet("/api/" + name, …)` emits `/api/`. No whole-argument literal check.
- `framework_structural_facts/spring.rs:299` (and `:288` array arm) — Java `@GetMapping("/a" + x)` emits `/a`; `@GetMapping({"/a" + x, "/b"})` emits `/a` and drops `/b`. Kotlin Spring is clean (uses `static_arg.rs`); Java is not.
- `framework_structural_facts/http_clients/ruby.rs:92` — Ruby `URI.parse("https://#{ENV['HOST']}/x")` emits the interpolated string verbatim; no `#{` guard, no whole-arg check. (Rails has the guard; the client collector doesn't.)
- `framework_structural_facts/http_clients/java.rs:65` — Java `URI.create("https://api.com" + path)` emits `https://api.com`. C# (`:88`) and Python (`:110`) clients apply the whole-arg filter; Java doesn't.
- **Root cause for all four:** these collectors bypass `static_arg.rs`, which Kotlin Spring / Laravel / Actix / Axum / Elixir / PHP / Rust clients already use correctly. Fix = add `Java`/`CSharp`/`Ruby` arms to `static_arg.rs` and route these collectors through it.

**Validation correction:** the Ruby and Java examples reproduce through the actual HTTP-client collectors, not as standalone expression facts. Use `Net::HTTP` plus `URI.parse(...)` for Ruby and `HttpRequest.newBuilder(... URI.create(...))` for Java regression fixtures.

**Contract break:**
- `framework_structural_facts/python_web.rs:768` — Django `re_path(r"^users/(?P<id>\d+)/$")` emits a fact with **no `normalized_route_template`** (the universal join key). Pattern claimed `supported`, no `open_gaps` entry → Miller can't join regex routes.

**Vue silence break:**
- `web_structural_facts/http_client.rs:187` + `vue.rs:262` — Vue is parsed with the **HTML grammar**, so `<script>` bodies are one `raw_text` node. `is_ignored_syntax_range` only matches node kinds containing `"comment"`/`"string"`, so it returns `false` for **every** position in a Vue script. Commented-out or string-embedded `fetch()`/`axios.get()`/`path:` in a Vue SFC emits bogus facts. The collector's own doc promises comment/string rejection — broken for Vue.

**Latent writer crash (my own finding, cross-validated):**
- `registry.rs:594-641` merges six structural-fact collectors then **sorts only — no dedup-by-id**. `structural_fact_id = md5(file_path:pattern:capture:span)` (`types.rs:397`) is the SQLite PRIMARY KEY, and the insert is a plain `INSERT` (`writer/rows.rs:205`). Any `(pattern_id, capture_name, span)` collision → constraint violation → the whole file write fails. The `2dbdcad` fix (verified sound by me and the framework reviewer — it only makes ids *more* specific) proves collisions were already a live concern, but it only covers *framework* facts; the **web** `fact_for_span` (`web_structural_facts/fact_builders.rs:38`) does not encode route_template/verb. No safety net as collector count grows. Fix = `dedup_by id` after the merge **and/or** `INSERT OR IGNORE`.

**Validation correction:** this is not a current CLI crash because the CLI artifact mapping path dedupes structural facts by ID before insert. It remains a valid lower-level hardening item for extractor/API and writer safety.

---

## P1 — meaningful wrong/missing facts

**Data/SQL:**
- `sql_structural_facts.rs:618-622` — `INTEGER PRIMARY KEY` reports `nullable: true` (blessed by `sql/basic/expected.json:748`). `column_is_not_null` never checks `keyword_primary`.
- `data_structural_facts.rs:770-887` — YAML **flow collections** (`{a: 1}`, `[1,2,3]`) emit zero mapping/sequence/key_value facts. Pervasive in K8s/CI YAML.
- `sql_structural_facts.rs:241-262` — SQL trigger `trigger_name` == target table (both read the first `object_reference`). Latent — only the ERROR fallback is fixture-tested.

**Validation correction:** the SQL trigger-name claim did not reproduce in a direct fixture. Keep a regression test for trigger name and target table extraction, but do not treat this as a confirmed code defect unless that test fails.

**Framework:**
- `framework_structural_facts/node.rs:445` — Express `app.use('/api', middleware)` is mislabeled `express.router_mount.v1` (mount fact pushed regardless of whether the target is a router).
- `spring.rs:377` — Spring route dropped when a `//` comment sits between `@GetMapping` and the method.
- `spring.rs:102,392` — Spring class-level `@RequestMapping` prefix lost when a method body contains the token `class` (e.g. `log.info("…class…")`) — `is_java_class_declaration` tokenizes lines and the `SourceMask` only checks line start.
- `rails.rs:395` — Rails parenthesized `get('/x')` / `post('/x')` missed (requires literal space after verb).
- `actix.rs` — Direct `App::new().route("/health", web::get().to(h))` not emitted (scope collector requires a `web::scope` anchor); a top-3 actix idiom.

---

## Cross-cutting: `open_gaps` honesty (P1/P2 contract theme)

Found by three reviewers independently. `capabilities.json` lists `"open_gaps": []` for languages with real missing constructs, violating the data-quality bar. Confirmed real via `scripts/language-data-quality-report.mjs:273` (`open_gaps.length > 0` is the honest signal).

- **SQL:** INSERT/DELETE DML, CREATE PROCEDURE/FUNCTION, window functions, ALTER/DROP, parameter bindings.
- **Markdown:** footnotes, reference-style links, task lists, definition lists, autolinks.
- **YAML:** flow collections, block scalars (`|`/`>`) classified `other`, tags, multi-doc path uniqueness.
- **JSON:** array indices in paths, `$ref`/`$schema` link facts, JSON5 unquoted keys.
- **TOML:** dotted-key nested tables, array-of-inline-tables paths, multi-line strings.
- **Regex:** backreferences (`\1`/`\k<name>`), possessive quantifiers (`*+`), `\Q..\E`, conditional patterns.
- **CSS:** `@supports`/`@container`/`@font-face`/`@layer`/`@charset`/`@namespace`.
- **HTML:** `<link>`/`<area>` href, `data-*`, semantic landmarks, `<img src>`.
- **Vue:** `<style>` embedded CSS not scanned; `#` v-slot shorthand not parsed (fixture-confirmed: `vue/basic/source.vue:6` `<template #actions>` emits nothing).

Fix = implement, or record an `open_gaps` entry (reason/required-closure/planned-closure) and re-run `node scripts/language-data-quality-report.mjs --strict`.

---

## P2 — correctness gaps

- `web_structural_facts/react.rs:162-182` + `vue.rs:253-274` — route-object scan treats **any** `path:` as a route: `redirect: { path: "/login" }` and `meta: { path: "/x" }` emit bogus route definitions.
- `web_structural_facts/js_object_scan.rs:64-77` — `parse_js_string_literal` mis-unescapes `\n`→`n`, `\t`→`t`, `\u0041`→`u0041`. Shared by route/URL/import parsing across vue/react/next/http. (Note: `parse_vue_string_literal` instead rejects escapes — inconsistent.)
- `web_structural_facts/nextjs_nuxt.rs:351-366` — `pages/app/page.tsx` misclassified as App-Router route `/` (uses `rev()`, so the deeper `app` wins).
- `nextjs_nuxt.rs:101-103` — Pages-Router pages with no `next/*` import / `getStaticProps` / `NextPage` emit no `file_route` (plain `export default function Home()` dropped).
- `sql_structural_facts.rs:461-488,801-812` — subquery `has_where`/`has_group_by`/`has_order_by`/`source_count` reflect the **outer** query.
- `sql_structural_facts.rs:519-525,814-822` — chained joins (`FROM a JOIN b JOIN c`) report `left_table = a` for every join.
- `data_structural_facts.rs:457-548` — JSON `depth = 2*nesting - 1`, but the registry documents it as "nesting depth."
- `data_structural_facts.rs:534-547` — JSON array indices not in paths → `[{"a":1},{"a":2}]` both get `$.x.a` (collision).
- `data_structural_facts.rs:650-678` — TOML array-of-inline-tables `a = [{b=1}]` loses parent key (`key_path: b` not `a.b`).
- `sql_structural_facts.rs:503-505` — `RECURSIVE` detection is case-sensitive (`with recursive` → `recursive: false`).
- `framework_structural_facts/go_http.rs:256` — Go `var mux = http.NewServeMux()` receivers untracked (only `:=` handled).
- `rails.rs:434` — `resources :users, :posts` drops `:posts`.
- `rails.rs` — `member`/`collection` blocks inside `resources` lose the resource prefix.
- `http_clients/{csharp,kotlin,php}.rs` — lack the Rust-style receiver proof (inconsistent M2 recall boundary).
- `razor.rs` — `@page "/users/{id}"` omits `normalized_route_template`.

---

## SQLite performance gotchas (your specific ask)

**No P0/P1.** Writer is sound: single txn per scan, WAL + `synchronous=NORMAL` + `temp_store=MEMORY` + 128 MiB cache, prepared statements reused per table, inline FK resolution, spooled streaming, **zero `.unwrap()` in non-test code**, and the eacd858 cleanup indexes fully cover every delete path (verified). New fact volume scales **linearly**. P2 items that compound at scale:

1. **No `wal_checkpoint(TRUNCATE)` at close** (`writer.rs:269-288`) — WAL grows with the new write volume; memory note records `symbols.db 1810MB → 1900MB` after checkpoint. Downstream consumers that copy just the `.sqlite` file miss WAL-resident writes. Highest leverage.
2. **Spooled scan deserializes every file 3×** (`writer.rs:716,781,827`) — plan, file/symbol insert, child rows. Passes 2+3 can merge (symbol lookup is pre-built; `defer_foreign_keys=ON` already defers FK validation) → 3×→2×, a direct multiplier on scan wall-time as per-file JSON grows.
3. **JSONL export re-parses `metadata_json` per row + un-indexed export sorts** (`jsonl.rs:1321,1373` etc.) — `export_structural_facts`/`source_regions`/`complexity_metrics` ORDER BYs match no index → full-table sort at the new volume; plus a JSON parse+serialize per row. Emit metadata raw + add covering indexes (or change export order, contract-gated).
4. **Secondary indexes built before bulk insert on fresh/`--force`** (`schema.rs:354-393`) — contract explicitly permits create-after; 2-4× cheaper to build B-trees by sorted pass at end.
5. **`delete_file_rows` issues 14 non-prepared DELETEs per rewritten file** (`writer.rs:996-1041`) — every child table has `ON DELETE CASCADE`, so a single `DELETE FROM files` would suffice; the FK-OFF safety rationale is gone (FKs now ON/deferred).
6. **`load_existing_files` full-table scan + Vec materialization every scan** (`writer.rs:890-901`, called `:560,:749`) — O(N) even on no-op incremental scans.
7. **Per-file `load_existing_file` + `ensure_data_loss_guard` COUNT** (`writer.rs:451,566,723,971`) — 2N indexed round-trips; the guard COUNT runs unconditionally even for `Indexed` files where it can never fire.
8. **No multi-row `VALUES` batching** for high-cardinality child inserts (`rows.rs:677-697` etc.); **symbol-lookup temp table loaded one `execute` per id** (`rows.rs:877-884`) and the requested set now grows with fact volume.

**Extractor-side perf (P2):** `js_object_scan.rs:185-233` `parent_route_path_for_object` is ~O(N³) and `is_ignored_syntax_range` re-walks the tree per token (should use `descendant_for_byte_range`); `nextjs_nuxt.rs:892-969` page-signal scans re-scan the whole file ~10× instead of reusing the `JsImportIndex`; `sql_structural_facts.rs` `has_child_kind` repeated subtree walks; `yaml has_directives` recursive scan.

---

## P3 — nits (sample)

Markdown inline-link regex FP inside inline code / nested brackets; setext headings report `level: 1`; frontmatter `key_count` counts lines; JSON `import { type Route }` registered as value; JSX nested `<Route>` doesn't compute `effective_route_template`; `index: true` matched without token boundary; `NuxtLink :to` v-bind / relative paths missed; `css_selector_kind` misclassifies commas in attribute selectors; `html_element_attributes` builds a HashMap for every element; `markup_scan` not HTML-comment-aware; `idx_files_path` redundant with the `path UNIQUE` auto-index; no `mmap_size` for readers; two `.expect()` in non-test writer paths (infallible today); duplicated `find_matching_brace` / `find_matching_paren` helpers across modules; **thin unit coverage** in `data_structural_facts.rs` and `sql_structural_facts.rs`.

**Validation correction:** `index: true` without a token boundary did not reproduce, and HTML artifact output did not emit the tested commented htmx fact. SQL/data-format tests do exist; the real gap is missing focused tests for these value-semantic edge cases.

---

## Strengths

- `static_arg.rs` is a strong whole-argument silence reference — collectors that use it (Kotlin Spring, Laravel, Actix, Axum, Elixir/Kotlin/PHP/Rust clients) are clean.
- Axum/Rust receiver tracing with fixpoint + poisoned/suppressed states is a correct M2 recall boundary; the `cd71847`/`351a7af`/`2dbdcad` fixes all verified to hold.
- M2 silence is well-enforced for JS/TS fetch/axios (static-string-only, `method:`→silent, dot-prefix rejection, comment/string guards).
- Route-shape depth is good where covered: `[slug]`/`[[...slug]]`/`(group)`/`@parallel`/intercepting routes all decode into `normalized_route_template` + segment arrays.
- SQLite writer: single txn, correct PRAGMAs, prepared reuse, inline parent resolution (no second UPDATE pass), panic-free hot path, complete cleanup indexes.
- Byte-offset consistency is sound across all extractors; `NormalizedSpan` aligns with tree-sitter byte columns.
- Registry conformance test enforces metadata-key contracts on the golden corpus (though it can't catch value-semantic bugs like the PK nullability or JSON depth inflation).

---

## Top recommendations (highest leverage first)

1. **Close the concatenation/interpolation silence gap universally** — add `Java`/`CSharp`/`Ruby` arms to `static_arg.rs` and convert `spring.rs`, `aspnet.rs`, `http_clients/ruby.rs`+`java.rs` to whole-arg AST checks. Removes 4 P0 silence breaks at once and brings Java/C#/Ruby up to the bar Kotlin/Elixir/PHP already meet.
2. **Fix the Vue `<script>` silence gap** — parse each Vue script section with the JS/TS grammar (or run `is_in_js_comment_or_string` over the section text) before the fetch/axios/route scans; add regression tests.
3. **Add a dedup safety net below the CLI artifact mapper** — `dedup_by id` after the extractor merge and/or an explicit writer policy. This is hardening, not a confirmed current CLI crash.
4. **Record/closing the `open_gaps` debt** — either implement the listed constructs or add honest `open_gaps` entries across SQL/Markdown/YAML/JSON/TOML/Regex/CSS/HTML/Vue; re-run the strict data-quality report.
5. **SQLite: checkpoint the WAL after commit + merge the spooled insert passes (2+3)** — the two highest-leverage perf wins for the new volume; bounds the WAL sidecar and cuts scan deserialize cost ~33%.
6. **Replace line-based Spring/Rails scanners with AST-driven collectors** — eliminates the Spring comment gap, class-prefix poisoning, and Rails parenthesized-call gap (P1×3), and makes them consistent with `kotlin_spring.rs`/`nestjs.rs`/`laravel.rs`.
7. **Add per-language unit tests for the data/SQL extractors** — existing tests did not cover these value-semantic edge cases.

Full per-finding detail with code snippets lives in the four reviewer reports: [web](7a13e8df-8c6b-472b-9db5-3bb7c3ccf9dd), [data+sql](d924276e-e954-4260-8dbb-c47d80a14b60), [framework](c9a4d43e-97f8-47bb-9169-ef69a0cde1c8), [SQLite](b3e55852-0ab1-461b-9223-e2bee1faa634). The validation notes above supersede narrowed claims. Remediation plan: `docs/plans/2026-07-04-glm-review-findings-remediation.md`.
