# Web / HTTP Structural Facts — Deep Review (2026-07-03)

Adversarial multi-agent review of the current tree (HEAD `2f445fa`, releases
v2.5.10 → v2.7.0) covering both structural-fact surfaces:

- **Frontend** `web_structural_facts/` (css, html, http_client, js_imports,
  js_object_scan, jsx_scan, nextjs_nuxt, react, vue, fact_builders)
- **Backend** `framework_structural_facts/` (aspnet, go_http, node, python_web,
  rails, spring, markup, razor, scan, helpers) + `http_clients/`
- **Shared** `http_boundary.rs` (join normalizer), `containing_symbol.rs`,
  `structural_fact_registry.rs`, the contract docs, and the capability matrix.

Method: 12 review dimensions → per-finding adversarial verification against the
actual code (several reproduced by compiling `build_flags`/`SourceMask` or via
`extract_canonical`) → dedupe/synthesis. 35 raw candidates, 25 confirmed, 10
refuted. This review targets the **post-fix** tree and does **not** re-report the
38 findings closed by the 2026-07-02 fix lane (see
`2026-07-02-backend-http-boundary-review-findings.md`); where a prior fix is
incomplete it is called out explicitly.

## Overall assessment

Architecture and the happy path are solid. The `normalized_route_template` join
contract, the M2 static-literal silence doctrine, and the registry/conformance
infrastructure are coherent, and golden-covered inputs extract correctly across
every framework. No panics and no memory-safety issues were found, and **nothing
here corrupts symbol extraction** — damage is confined to the `structural_facts`
family.

The dominant risk is a class of **silent, whole-file false-negatives** triggered
by common real-world code shapes that no golden fixture exercises:

1. The byte-level `SourceMask` lexer models only a subset of string forms. A
   stray quote inside an unmodeled form (regex literal, char/rune literal, some
   C# verbatim orderings) flips string-mask parity for the **rest of the file**,
   so the triggering literal *and every later route/client fact* are dropped.
2. Textual import-detection gates recognize only one canonical import shape.
   Idiomatic-but-uncovered forms (black-wrapped Python imports, Express ESM
   `{ Router }`) zero out an entire file's route facts.

There is also one **availability bug**: a non-terminating fixpoint in Go
group-receiver resolution that hangs the whole extraction run on valid Go.

Counts (deduped): **0 critical · 5 high · 9 medium · 9 low**.

Because these triggers are common and the loss is silent and unflagged, the
join graph is less complete than the golden suite suggests.

> **Status (2026-07-03): all 25 findings RESOLVED.** Commit `dfc4bd2` fixed 24
> (each with a regression test); the Go `{$}` anchor was fixed for the root case
> there and completed for scoped anchors (`/items/{$}`) in a follow-up. Verified
> green: `2642` tests pass, `cargo fmt --check` clean, `cargo clippy` clean. See
> the **Resolution** section at the end for the per-finding map.

---

## Theme 1 — `SourceMask` literal-modeling gaps (whole-file parity corruption)

`scan.rs::build_flags` models line/block comments, plain strings, Go raw
backticks, Python triple-quotes, and C# `@"` verbatim — but not regex literals,
char/rune literals, C# `@$"` ordering, or C# `"""` raw strings. Every backend
route/client collector gates needles on this single whole-file mask, so a parity
flip cascades to end-of-file. The secondary tree-sitter guard in `route_fact`
(`is_comment_or_string_node`) does **not** save the under-emission case, because
the mask-based needle skip (`node.rs:607-610`, `csharp.rs:64`, etc.) happens
*before* `route_fact` is ever called.

| Sev | Finding | Location |
|-----|---------|----------|
| **High** | **Regex literals with an odd count of a quote char flip parity.** `const q = /['"]/;` (idiomatic quote-stripping class) before `app.get('/health', …)` masks `app.get` → both routes silently dropped; parity stays flipped for the file tail. Same class in Ruby (`RE = /["']/` before a later `get "/x"`). Confirmed with the real `MaskLanguage::Js`. | `scan.rs:142` |
| Med | **No char/rune-literal modeling for Go/Java/C#** (`single_quotes` is `Js\|Python\|Ruby` only). `char q = '"';` / Go `case '"':` before an HTTP call masks the call and misparses the following literal. Confirmed by compiling `build_flags`. | `scan.rs:161` |
| Low | **C# `@$"` verbatim-interpolated ordering not recognized** (only `@"` and, by accident, `$@"`). `@$"{root}\bin\"` → the trailing `\` escapes the closing quote and runs the string away. Same gap in `helpers.rs::find_matching_delimiter`. Confirmed via a scan.rs unit test. | `scan.rs:154`, `helpers.rs:321` |

**Fix:** model these forms in `build_flags` (mask the interior of single-line
`/.../` regexes incl. `[...]` classes; consume a single char/rune literal for
Go/Java/C#; treat any `@`-flagged C# string as verbatim regardless of `@`/`$`
order and add `"""`). Add goldens with a quote-bearing regex/char literal
*before* a route so the regression is guarded.

## Theme 2 — Import-detection gates drop an entire file's facts

Emission is gated on textual import detection that recognizes only one shape;
uncovered idioms early-return and drop **every** route/client fact for the file.

| Sev | Finding | Location |
|-----|---------|----------|
| **High** | **Python import gate misses multiline `from x import (…)` and module `import x` forms.** `collect_imports` matches per physical line; `from fastapi import (` strips to `"("` → zero items → no class binds → `collect_python_web_facts` returns empty. black/ruff/isort emit the multiline form for any long import list, so every FastAPI/Flask/Django route in the file drops. `import fastapi` + `fastapi.FastAPI()` isn't handled at all. Confirmed via `extract_canonical`. | `python_web.rs:131` |
| **High** | **Express ESM `import { Router } from 'express'` never traced.** `collect_es_imports` resolves only default/namespace imports; a named `{ Router }` (or `{ Router as R }`) records no binding, so router modules — where most Express routes live — emit zero facts. Confirmed via `extract_canonical`. | `node.rs:178` |
| Med | **Inline `const router = require('express').Router()` not traced.** The `.Router()` chain is discarded; only `router()`/`router.Router()` needles are built. Also `const app = require('express')();`. Common CJS router-module idiom → all routes drop. | `node.rs:251` |
| Low | **Python client import uses whole-line equality.** `import requests  # comment` or `import requests, httpx` fail `trimmed == "import requests"`, silencing every requests/httpx call in the file. | `http_clients/python.rs:53` |

**Fix:** join Python logical-continuation import lines (or drive off tree-sitter
`import_from_statement`) and add a module-import path; call the existing
`parse_named_imports` for Express and register the `Router` binding; detect
`require('express').Router()` directly; tokenize the Python client import line.

## Theme 3 — M2 silence violations (dynamic input emits fabricated facts)

Over-emitting on a dynamic input is worse than a miss: Miller joins to a
route/verb that does not exist.

| Sev | Finding | Location |
|-----|---------|----------|
| **High** | **Spring method mapping with a non-literal path over-emits an empty route bound to the class base.** `@GetMapping(PATH_USERS)` → `parse_mapping_annotation` returns empty, but the method loop coerces `empty -> vec![""]`, emitting `route_template=""`, `effective=/api/`, `verb=GET` — a fabricated endpoint at `/api/`, with the real constant path dropped. The class-level branch already stays silent; the method-level coercion is the asymmetric bug. Path constants are idiomatic enterprise Java. | `spring.rs:49` |
| Med | **Rails `#{}` interpolation emitted as a literal route/prefix.** `parse_ruby_string_literal` never detects double-quote interpolation (Python f-strings *are* guarded in `scan.rs:356`). `get "/#{locale}/users"` → `normalized_route_template="/#{locale}/users"` (unjoinable); `scope "#{tenant}" do …` poisons every nested route. | `rails.rs:355` |
| Med | **fetch/axios ES6 `{ method }` shorthand degrades to GET.** `find_top_level_method_property_value` requires a colon, so `{ method }` (dynamic var) falls through to `VerbResolution::Get`. The explicit `{ method: verb }` form is correctly silent (test `fetch_non_static_method_emits_nothing`); the shorthand is functionally identical but mislabels the verb. | `http_client.rs:340` |

**Fix:** in Spring, only fall back to `[""]` when *no* value/path element is
present (bare `@GetMapping`); emit nothing when an element is present but
non-literal. In Ruby, treat a double-quoted literal containing unescaped `#{` as
dynamic (drop verb routes; mark scope poisoned). In `http_client.rs`, return
`Silent` for the `{ method }` shorthand.

## Theme 4 — Normalization / keyword-argument defects on static routes

Valid static input that emits a fact with a corrupted or wrong join key.

| Sev | Finding | Location |
|-----|---------|----------|
| Med | **Python kwargs with whitespace around `=` are ignored.** `keyword_value_start` uses the needle `"{key}="`, so `prefix = "/api"`, `methods = ["POST"]`, `url_prefix =`, `name =`, `namespace =` never match → dropped prefix / `verb=GET`. Legal (non-PEP8) spacing → wrong join keys. | `python_web.rs:840` |
| Med | **Rails `scope path: "/x"` keyword form drops the prefix.** `rails_scope_path` handles only positional `scope "literal"`; `scope path: "/admin"` yields no `scope_path`/`effective_route_template`. `scope path:` + `module:`/`as:` is the standard prefix-without-namespace idiom. | `rails.rs:328` |
| Med · ✅ **Resolved** | **Go 1.22 `{$}` exact-match anchor mis-normalized to `:$`.** `normalize_braces_with_dots_template` treated `{$}` as a param named `$` → `/{$}` became `/:$`. `dfc4bd2` fixed the root `/{$}` case only; the follow-up strips `{$}` at any depth so `/items/{$}` → `/items/` (no bogus `$` segment). Guarded by `strips_go_end_of_path_anchor_at_any_depth` + `go_net_http_scoped_exact_anchor_strips_dollar_segment`. | `http_boundary.rs:164` |
| Low | **Multi-parameter path segments emit one malformed `dynamic_segments` entry.** `normalize_colon_template` splits only on `/`, so `/flights/:from-:to` → `dynamic_segments=["from-:to"]` (join key is correct; only param names garbage). Braces flavor handles the equivalent correctly. | `http_boundary.rs:106` |
| Low | **Flask `verb_source` wrongly `attested` for any path containing `"methods"`.** Substring test `!args.contains("methods")`; `@app.route("/payment-methods")` → `verb_source=attested` (contract: `default`). Provenance-only. | `python_web.rs:403` |

**Fix:** tolerate whitespace around `=` in the Python kwarg scanner; parse the
Rails `scope path:` keyword form; strip `{$}` to yield `/` with no param; split
colon/gin segments on non-identifier delimiters; derive Flask `verb_source` from
the parsed methods list, not a substring test.

## Theme 5 — Backend coverage gaps (under-emission of valid static routes)

Pure under-emission (missing rows, no wrong data) except the Spring interface
case, which also exposes an **incomplete prior fix**.

| Sev | Finding | Location |
|-----|---------|----------|
| Med | **Spring class-level `@RequestMapping` on an `interface` not detected.** `is_java_class_declaration` matches only `class `, so `interface UserApi` is treated as a method; its class route is lost, and — worse — an interface following a concrete controller **inherits the prior controller's template**, so the already-"fixed" class-template-leak still fabricates wrong routes here. Interface controllers (springdoc API interfaces, `@FeignClient`) are established. | `spring.rs:373` |
| Low | **C# raw-string URL client path is dead code.** `parse_csharp_url_literal` tries `parse_csharp_string_literal` first, which matches the bare `"` of a `"""` opener and returns `Some(("", …))`, so `parse_csharp_raw_string_literal` never runs. `client.GetAsync("""/api/raw""")` → 0 facts, despite plan Task 4 claiming raw strings emit. | `http_clients/csharp.rs:177` |
| Low | **Spring class-level array `@RequestMapping({"/api","/v2"})` joins only the first template.** Two class_route facts emitted, but `current_class_template = first()`, so `/v2/users` is dropped. | `spring.rs:94` |
| Low | **Rails `match … via: :get` (single symbol) drops the route.** `rails_match_route` requires the array form via `symbol_array_keyword`; a single symbol returns `None`. `via: :get` is idiomatic. A `symbol_keyword` helper already exists unused. | `rails.rs:374` |
| Low | **Express `app.route("/x").all(handler)` in a chain is dropped.** `collect_express_route_chains` filters `JS_VERB_METHODS` by `verb.is_some()`, excluding the `("all", None)` entry — unlike the non-chain `app.all(...)`. | `node.rs:516` |

**Fix:** recognize `interface`/`enum` in `is_java_class_declaration` and the
class-reset path (this also completes the class-template-leak fix); try the C#
raw-string parser first; track all class templates (cross product); accept
single-symbol `via:`; keep the `all` hop in chains (verb omitted).

## Theme 6 — Non-termination / availability

| Sev | Finding | Location |
|-----|---------|----------|
| **High** | **Go group-prefix fixpoint never converges (infinite hang).** `collect_grouped_receivers` keys the receiver map by variable name only and runs an uncapped fixpoint; the compare-and-insert sets `changed=true` on any value change. A name bound to two different literal prefixes in different scopes (e.g. two funcs each `r := gin.Default()` then `v := r.Group("/v1")` / `v := r.Group("/v2")`) oscillates every pass → `if !changed break` never fires → hangs the whole scan with no output. Confirmed by tracing and reproduced. | `go_http.rs:215` |

**Fix:** give the fixpoint a monotonic guarantee — first-write-wins, or mark a
name `Poisoned` on conflicting bindings — plus a defensive iteration cap.

## Theme 7 — Frontend markup misclassification

| Sev | Finding | Location |
|-----|---------|----------|
| Med | **Blazor `@onclick`/`@bind`/`@onchange` misclassified as Alpine directives on the razor surface.** `is_alpine_attribute_name` treats any `@`/`:`-prefixed attribute as Alpine and maps `@` → `x-on`; on `.razor` files this emits false `alpine.directive.v1` facts for every interactive Blazor component (`@bind` → `x-on` is doubly wrong). The razor fixture only exercises real Alpine, so the collision is untested. | `markup.rs:245` |

**Fix:** gate Alpine `@`/`:` shorthand off for `language==razor`, or exclude the
known Blazor directive names (`@on*`, `@bind*`, `@ref`, `@key`, `@rendermode`,
…); add a Blazor golden asserting zero alpine facts.

## Theme 8 — Contract documentation drift

| Sev | Finding | Location |
|-----|---------|----------|
| Low | **13 registered/emitting patterns are absent from the contract structural-fact tables** in both `jsonl-v3.md` and `sqlite-schema-v3.md`: `razor.*` (3), `css.*` (4), `html.*` (4), `vue.sfc_section`/`vue.template_directive`. Their `capture_name`/`node_kind` live only in prose, and siblings (htmx, alpine) *are* documented, so a consumer reading the contract for these finds nothing. No conformance test guards doc-table sync. | `jsonl-v3.md:509`, `sqlite-schema-v3.md` |

**Fix:** add the 13 rows with their real `capture_name`/`node_kind`, and add a
conformance test that fails when a registered emitted pattern is missing from
the doc tables.

---

## Prioritized worklist

1. **Harden `SourceMask` (`scan.rs::build_flags`)** — regex literals (JS/Ruby),
   char/rune literals (Go/Java/C#), C# `@$"` ordering + `"""`. Highest leverage:
   removes the whole-file parity cascade across five languages and unblocks the
   C# raw-string client path. (Theme 1 + the C# dead-code item in Theme 5.)
2. **Make the Go group fixpoint terminating** (`go_http.rs`) — first-wins/poison
   + iteration cap. Removes the only availability bug. (Theme 6.)
3. **Fix the import gates that zero out a file** — Python multiline/module
   imports (`python_web.rs`); Express `{ Router }` and inline
   `require('express').Router()` (`node.rs`). Add a golden per shape. (Theme 2.)
4. **Close the M2 over-emission holes** — Spring non-literal method path
   (`spring.rs`), Rails `#{}` interpolation (`rails.rs`), fetch `{ method }`
   shorthand (`http_client.rs`). Fabricated join keys corrupt Miller's graph.
   (Theme 3.)
5. **Fix keyword-arg / normalization parsing** — Python `key = value` spacing,
   Rails `scope path:`, Go `{$}` anchor. (Theme 4.)
6. **Recognize Spring `interface`/`enum`** (`spring.rs`) — closes the coverage
   gap *and* completes the prior class-template-leak fix. (Theme 5.)
7. **Blazor gate on the razor markup surface** (`markup.rs`) — stops false
   alpine facts on common `.razor` input. (Theme 7.)
8. **Low-severity cleanup** — Flask `verb_source`, colon multi-param
   `dynamic_segments`, Rails single-symbol `via:`, Express chained `.all()`,
   Python client import tokenization; document the 13 missing patterns and add a
   doc-drift conformance test. (Themes 4/5/8.)

## Notes on the fixed-38 lane

Two prior fixes are **incomplete** and re-surface above:

- The Spring class-template-leak fix (findings 13/36 in the 2026-07-02 lane) does
  not cover `interface`-declared controllers — Theme 5, `spring.rs:373`.
- The duplication-consolidation left **two JS masking mechanisms**: backend
  `node.rs` masks with the byte-based `SourceMask` (no regex modeling) while the
  frontend uses the tree-based `is_ignored_syntax_range`. Prior finding #18's
  "two JS lexers" concern is only partly resolved (parsers shared, masking
  divergent) — this is the root of the Theme-1 regex gap on the backend surface.

## Resolution (2026-07-03)

All 25 findings addressed — 24 in commit `dfc4bd2` ("fix: address web structural
fact review findings", ~740 lines of new regression tests), and the Go `{$}`
residual completed in a follow-up. Grouped by theme:

- **Theme 1 (SourceMask):** `build_flags` now models JS/Ruby regex literals
  (`is_regex_literal_context` + `regex_literal_end`, char classes and flags
  masked), char/rune literals for Go/Java/C# (`single_quotes` extended), C#
  `@$"` verbatim ordering (in both `scan.rs` and `helpers.rs`), and C# `"""`
  raw strings. Known limitation: the regex heuristic keys off the preceding
  non-whitespace byte, so keyword-preceded regexes (`return /…/`, `typeof`,
  `case …:`) are not detected — a quote-bearing regex in that narrow position
  could still flip parity. Low priority; recorded for follow-up.
- **Theme 2 (import gates):** Python parenthesised-continuation imports joined
  (`python_logical_lines`) and module `import x` traced via dotted constructors
  (`parse_module_import_items`); Express ESM `{ Router }` (`parse_named_imports`)
  and inline `require('express').Router()`/`()` receivers; Python client import
  tokenised (comment/comma tolerant).
- **Theme 3 (M2 silence):** Spring non-literal method path stays silent
  (`has_route_argument` distinguishes "no element" from "non-literal element");
  Rails `#{}` interpolation poisons routes/scopes (`static_ruby_value`);
  fetch/axios `{ method }` shorthand → `Silent`.
- **Theme 4 (normalization/kwargs):** Python `key = value` spacing tolerated;
  Rails `scope path:` keyword form parsed (`string_keyword_value`); Go `{$}`
  anchor stripped at any depth (see the ✅ row above); colon multi-param
  segments split (`collect_colon_dynamic_segments`); Flask `verb_source` derived
  from the parsed methods list.
- **Theme 5 (coverage):** `is_java_class_declaration` recognises
  `interface`/`enum`/`record` (also completes the class-template-leak fix); C#
  raw-string client parser tried first; Spring array class templates
  cross-product; Rails single-symbol `via:`; Express chained `.all()`.
- **Theme 6 (availability):** Go group fixpoint made monotonic
  (None→Literal→Poisoned, first-wins-then-poison) — converges in ≤2 changes per
  name, so the infinite hang is gone.
- **Theme 7 (markup):** `is_alpine_attribute_name` is language-aware; Blazor
  `@`/`:` directives no longer emit false `alpine.directive.v1` on `.razor`.
- **Theme 8 (doc drift):** all 13 patterns added to both contract tables, plus
  a new conformance test `markdown_contract_pattern_tables_list_web_markup_pattern_rows`
  that fails if a registered emitted pattern is missing from the doc tables.

**Verification:** `cargo test -p julie-extractors` → 2642 passed / 0 failed;
`cargo fmt --all -- --check` clean; `cargo clippy -p julie-extractors
--all-targets` clean.
