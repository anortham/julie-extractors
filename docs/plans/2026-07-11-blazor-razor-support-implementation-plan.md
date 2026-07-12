# Blazor/Razor Extraction Support Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Make the razor extractor parse real-world Blazor markup cleanly (zero diagnostics + correct semantics) and emit the fact families Miller needs for Blazor bridging.

**Architecture:** Grammar-first fix in the tree-sitter-razor fork (repo `~/source/tree-sitter-razor`, branch `fix/attribute-value-expressions` is the starting point; julie pins rev `cf7b0e5` at `crates/julie-extractors/Cargo.toml:55`). Two new additive fact families (`razor.route_reference.v1`, `blazor.component_reference.v1`) plus `http.client_request.v1` for razor. Per-file extraction emits only locally observable syntax and context; Miller owns workspace resolution, `_Imports.razor` inheritance, and external/internal component classification. Every new or expanded fact contract is registered in the structural-fact registry, exported contract JSON, and certified capability snapshot.

**Tech Stack:** Rust, tree-sitter (grammar.js + generated parser), julie golden-fixture harness.

**Architecture Quality:** Preserves the ownership boundaries from the umbrella design (`/Users/murphy/source/eros/docs/plans/2026-07-11-dotnet-blazor-stack-support-design.md`) with one correction: the extractor does not own the umbrella's proposed external-tag classification because its framework-fact interface has no workspace index. Grammar work stays in the fork; Razor symbol extraction stays in `crates/julie-extractors/src/razor/`; framework facts use the existing `collect_framework_structural_facts` dispatch and structural-fact registry. One internal `blazor_navigation` collector is justified because the same navigation contract must be emitted from both csharp and razor without duplicating scanners. Golden fixtures and registry conformance are the caller-facing test surface. Rejected: ERROR-node regex recovery as the primary fix; diagnostics-only acceptance; per-file guesses about workspace component resolution; routing structural facts through the Razor symbol extractor. Risk: high until the Task 1 grammar spike proves the direct-composition path, then medium; the fallback is nested tree-sitter-c-sharp parsing of attribute expressions, which is structured parsing, not regex recovery.

## Global Constraints

- New fact families and symbol kinds must land in the certified capability snapshot (`fixtures/extraction/capabilities.json` kind_coverage / structural_facts), not just code.
- Every new or expanded structural-fact family must update `crates/julie-extractors/src/base/structural_fact_registry.rs`, the per-language emitted-pattern arrays in `crates/julie-extractors/src/base/framework_structural_facts/mod.rs`, and `docs/contracts/structural-fact-patterns.json`.
- Route-reference metadata follows the existing route-reference vocabulary: `target_path` is the raw literal, `source_kind` identifies `navigate_to` or `href`, and `route_source` identifies the literal representation.
- Per-file extraction must not infer workspace absence. `blazor.component_reference.v1` records local namespace/import context and omits `external`; Miller derives resolution and external/internal status from workspace evidence.
- All fact additions are additive — no changes to existing fact family shapes.
- Existing razor test suite (~59 tests in `crates/julie-extractors/src/tests/razor/`) stays green throughout.
- Existing ERROR-node recovery (`razor/mod.rs:255-297`, `@inherits`/`@rendermode` regex) stays as tail safety net.
- Component identity: `_Imports.razor` and `_ViewImports.cshtml` are NOT components; `App.razor` is.
- `languages/razor.toml` carries extraction policy only (its own header rule).
- Reference corpus source: Terraform diagnostics (235 razor diagnostics in the live database on 2026-07-11; re-count at Task 1 start with `~/source/Terraform/.miller/symbols.db` table `parse_diagnostics WHERE language='razor'`) plus FluentUI documentation patterns.

## Verification Strategy

**Project source of truth:** `README.md` (contract test commands), `AGENTS.md` (source-verification rule), golden fixture harness under `fixtures/extraction/`.

**Worker red/green scope:** `cargo test -p julie-extractors razor` (razor test modules) for extractor tasks; `tree-sitter generate && tree-sitter test` in `~/source/tree-sitter-razor` for grammar tasks.

**Worker ceiling:** `cargo test -p julie-extractors`.

**Worker gate invariant:** each grammar construct proves BOTH zero parse diagnostics AND expected identifiers/calls/literals/relationships — opaque consumption of an expression is a gate failure even when diagnostics are clean (the grammar has opaque fallbacks, `grammar.js:480,497`).

**Lead affected-change scope:** `cargo test -p julie-extractors` + `cargo test -p julie-extract-artifact --test schema_contract --test jsonl_contract`.

**Branch gate:** full contract suite per README: `cargo test -p xtask`, artifact schema/jsonl contracts, `cargo test -p julie-extract-cli --test cli_contract`, `cargo test -p julie-extractors --features test-capability-matrix structural_fact_registry`, `node scripts/language-data-quality-report.mjs --strict`, plus a re-extraction of `~/source/Terraform` showing razor diagnostics ≈ 0.

**Escalation triggers:** any change to `src/base/http_boundary.rs` or fact-envelope code → run the full artifact contract suite; grammar rev bump → run the entire razor golden corpus.

**Assigned verification failure:** Workers stop and report when assigned verification fails.

**Verification ledger:** Record invariant, command, scope label, commit SHA, result, timestamp per task.

## Parallel Execution Contract

| Task | Parallel batch | File ownership | Serialization required | Dependency reason |
|---|---|---|---|---|
| Task 1: Corpus spike | None - serial | Create: `~/source/tree-sitter-razor/test/corpus/blazor-attributes/*`, `docs/plans/2026-07-11-blazor-corpus-classification.md` | Yes | Everything downstream is scoped by the classification. |
| Task 2: Attribute-value expressions (grammar) | None - serial | Modify: `~/source/tree-sitter-razor/grammar.js`, generated parser, corpus tests | Yes | Depends on Task 1 corpus; grammar edits conflict file-level with Task 3. |
| Task 3: Directive modifiers + rendermode/typeparam/render fragments (grammar) | None - serial | Modify: `~/source/tree-sitter-razor/grammar.js`, generated parser, corpus tests | Yes | Same grammar.js as Task 2. |
| Task 4: Pin bump + semantic gate | None - serial | Modify: `crates/julie-extractors/Cargo.toml:55`; Create: `crates/julie-extractors/src/tests/razor/semantic_gate.rs`, `fixtures/extraction/razor/attribute-expressions/*` | Yes | Needs the committed grammar rev from Tasks 2–3. |
| Task 5: razor.route_reference.v1 | None - serial | Create: `crates/julie-extractors/src/base/framework_structural_facts/blazor_navigation.rs`; Modify: `crates/julie-extractors/src/base/framework_structural_facts/mod.rs`, `crates/julie-extractors/src/base/structural_fact_registry.rs`, `docs/contracts/structural-fact-patterns.json`; Test: `crates/julie-extractors/src/tests/razor/structural_facts.rs` plus csharp route-reference coverage | Yes | Owns shared framework dispatch and registry before Task 6. |
| Task 6: blazor.component_reference.v1 | None - serial | Modify: `crates/julie-extractors/src/razor/mod.rs`, `crates/julie-extractors/src/base/framework_structural_facts/razor.rs`, `crates/julie-extractors/src/base/framework_structural_facts/mod.rs`, `crates/julie-extractors/src/base/structural_fact_registry.rs`, `docs/contracts/structural-fact-patterns.json`; Test: new `crates/julie-extractors/src/tests/razor/component_reference.rs` | Yes | Follows Task 5 because both update framework dispatch and registry contracts. |
| Task 7: http.client_request.v1 from @code | None - serial | Modify: `crates/julie-extractors/src/base/framework_structural_facts/http_clients/csharp.rs`, `crates/julie-extractors/src/base/framework_structural_facts/http_clients/mod.rs`, `crates/julie-extractors/src/base/framework_structural_facts/mod.rs`, `crates/julie-extractors/src/base/structural_fact_registry.rs`, `docs/contracts/structural-fact-patterns.json`; Test: new `crates/julie-extractors/src/tests/razor/client_request.rs` | Yes | Follows Task 6 because it expands the same framework dispatch and registry contracts. |
| Task 8: test_container closure | None - serial | Modify: `crates/julie-extractors/src/test_detection.rs:86-163`; Test: golden test-detection fixtures for csharp/vbnet/razor containers | Yes | Independent behavior, serialized to keep every accepted task on one verified shared checkout. |
| Task 9: Synthetic fixtures + certification + release | None - serial | Create: `fixtures/extraction/razor/{code-behind,imports,scoped-assets,typeparam,rendermode}/*`; Modify: `fixtures/extraction/capabilities.json` | Yes | Integrates Tasks 4–8; certification is last. |

Tasks 1–9 run as `serial-worker-commit`. Tasks 5–7 must serialize because each produces an independently registered, exported, and tested framework-fact contract through shared dispatch and registry files. Task 8 is behaviorally independent but stays serial so no worker lane owns overlapping checkout-wide verification state.

---

### Task 1: Corpus spike and classification

**Files:**
- Create: `~/source/tree-sitter-razor/test/corpus/blazor-attributes/` (one corpus file per construct class)
- Create: `docs/plans/2026-07-11-blazor-corpus-classification.md`

**Interfaces:**
- Produces: the classified construct list with per-class counts and the re-estimate for Tasks 2–3. Corpus files in tree-sitter test format (parse-tree expectations).

**Contract inputs:** Terraform diagnostics via `sqlite3 -readonly ~/source/Terraform/.miller/symbols.db "SELECT path, start_line, start_column FROM parse_diagnostics WHERE language='razor'"`; read the cited source spans from `~/source/Terraform`.

**What to build:** Re-count the live Razor diagnostics and record the query result in the classification document, then harvest every distinct failing construct into minimal repro snippets; classify (attribute-value implicit expression / explicit expression with lambda-ternary-collection / directive-attribute modifier / other). Add FluentUI doc patterns not present in Terraform. Record which classes the existing `fix/attribute-value-expressions` branch (07eab9c) already fixes.

**Approach:** Run each snippet through the branch parser (`tree-sitter parse`) before classifying — do not classify against the pinned rev. Anything already fixed by the branch goes in the classification as "covered by 07eab9c".

**Acceptance criteria:**
- [x] Every Razor diagnostic in the Task 1 start-of-work query result maps to a corpus snippet or a named duplicate; the recorded baseline is 235 on 2026-07-11 (`acc610f`, `8ff20ab`)
- [x] Classification doc lists per-class counts, branch coverage, FluentUI additions, and a re-estimate for Tasks 2–3 (`e1299f0`, `fc9bf57`)
- [x] Corpus files committed to the grammar repo on `fix/attribute-value-expressions`

### Task 2: Grammar — attribute-value expressions

**Files:**
- Modify: `~/source/tree-sitter-razor/grammar.js` (attribute value rules; opaque fallbacks at lines 480, 497 must lose priority to expression parses, not be deleted)
- Test: `~/source/tree-sitter-razor/test/corpus/blazor-attributes/*`

**Interfaces:**
- Consumes: Task 1 corpus and classification.
- Produces: grammar rev where implicit expressions (`@typeof(App).Assembly`, member access, method calls, indexers) and explicit expressions (`@(...)` with lambdas, ternaries, casts, collection expressions) inside attribute values produce expression nodes, not ERROR/opaque text.

**What to build:** Extend attribute-value rules to parse `@`-expressions via the embedded tree-sitter-c-sharp rules the grammar already uses. Keep the opaque pattern only for genuinely expressionless values. Close Task 1 case O1 so generic component type values such as `TValue="string"` parse as `predefined_type` rather than ERROR or opaque text.

**Approach:** Work per construct class in Task 1 frequency order. If direct composition destabilizes the grammar (conflicts, state explosion), stop and report — the recorded fallback is nested C# parsing of the attribute text, a design-level decision the lead confirms before pivoting.

**Acceptance criteria:**
- [x] All Task 1 corpus files in the implicit/explicit expression classes pass `tree-sitter test` (`0323849`)
- [x] Generic component type values in Task 1 case O1 pass as named C# type nodes
- [x] Expression content appears as parsed nodes (assert node kinds in corpus expectations, not just absence of ERROR)
- [x] Existing grammar corpus stays green (102/110 total; only the eight Task 3 cases remain RED)

### Task 3: Grammar — directive-attribute modifiers, rendermode, typeparam, render fragments

**Files:**
- Modify: `~/source/tree-sitter-razor/grammar.js` (directive attribute rules; render-mode rule at line 190 is closed to three keywords)
- Test: `~/source/tree-sitter-razor/test/corpus/blazor-attributes/*`

**Interfaces:**
- Consumes: Task 1 corpus.
- Produces: grammar rev parsing `@on{event}:{modifier}` (e.g. `@onsubmit:preventDefault`), `@bind-{Prop}:{event|format|get|set|after}`, open-set `@rendermode` arguments, constrained `@typeparam T where T : ...`, and Razor template values in embedded-C# switch arms.

**What to build:** Modifier suffix parsing on directive attributes; widen the render-mode rule from its closed keyword list; typeparam constraint clause; close Task 1 case O6 by allowing Razor template literals as expression values inside embedded-C# switch-expression arms.

**Acceptance criteria:**
- [x] Corpus files for modifiers/rendermode/typeparam pass with named nodes (`d24d075`)
- [x] Task 1 case O6 parses render-fragment literals in switch-expression arms without ERROR or opaque consumption
- [x] Existing grammar corpus stays green (110/110)
- [x] Grammar changes committed at `d24d075afe5b18eae56c4386046ed5e6e3902795` for Task 4 to pin; tag/push remains approval-gated

### Task 4: Pin bump and the semantic acceptance gate

**Files:**
- Modify: `crates/julie-extractors/Cargo.toml:55` (tree-sitter-razor rev)
- Create: `crates/julie-extractors/src/tests/razor/semantic_gate.rs`
- Create: `fixtures/extraction/razor/attribute-expressions/` (golden fixtures mirroring the corpus)

**Interfaces:**
- Consumes: grammar rev from Task 3.
- Produces: the enforced two-part gate all later razor work runs under.

**What to build:** A test module that, for every fixture in the attribute-expressions corpus, asserts (1) zero `error`/`missing` parse diagnostics and (2) the expected identifiers, calls, literals, and relationships — e.g. `@(mode => mode)` yields a lambda with a `mode` variable_ref; `@typeof(App).Assembly` yields a type_usage of `App`; `@onsubmit="LookupAsync"` yields a call/event-binding reference to `LookupAsync`.

**Approach:** Follow the existing golden-fixture pattern in `crates/julie-extractors/src/tests/razor/structural_facts.rs` and `fixtures/extraction/razor/basic/`. Expected-extraction lists live beside each fixture.

**Acceptance criteria:**
- [ ] Gate test fails if a construct parses opaquely (verified by temporarily re-pointing at the old rev — red), passes on the new rev — green
- [ ] Existing 59 razor tests green on the new rev
- [ ] Re-extraction of `~/source/Terraform` reports razor parse diagnostics ≈ 0; each remainder triaged as fixed or a named limitation in the classification doc

### Task 5: `razor.route_reference.v1`

**Files:**
- Create: `crates/julie-extractors/src/base/framework_structural_facts/blazor_navigation.rs`
- Modify: `crates/julie-extractors/src/base/framework_structural_facts/mod.rs` (dispatch from csharp and razor; emitted-pattern arrays)
- Modify: `crates/julie-extractors/src/base/structural_fact_registry.rs`
- Modify: `docs/contracts/structural-fact-patterns.json`
- Test: `crates/julie-extractors/src/tests/razor/structural_facts.rs` and csharp route-reference coverage

**Interfaces:**
- Produces: fact family `razor.route_reference.v1` — fields: `target_path` (the raw literal target), `source_kind` (`navigate_to` | `navigate_to_login` | `href`), `route_source` (`string_literal`), and `framework` (`blazor`). Emit from `.razor` and `.cs` files for exact supported `NavigationManager.NavigateTo`/`NavigateToLogin` call shapes and from internal `href="/..."` attributes; skip external `http(s)://` and fragment-only targets. This uses the route-reference vocabulary already exposed by `nextjs.route_reference.v1`.
- Consumes: nothing from other tasks (constructs parse today in `@code` and `.cs`).

**Contract inputs:** Miller will pair this with `razor.page_directive.v1` in a file-route provider; `target_path` must stay the raw literal — no normalization on the reference side.

**What to build:** Add one shared internal Blazor-navigation collector called from both the csharp and razor framework-fact dispatch arms. The csharp path emits supported navigation calls; the razor path emits those calls plus internal `href` references. Register the pattern and metadata keys, update both language pattern-ID sets, regenerate the exported contract JSON, and add a regression test that `razor.page_directive.v1` preserves the raw ASP.NET brace template and `route_parameters` metadata verbatim.

**Acceptance criteria:**
- [ ] `NavigateTo` in `@code`, `NavigateTo` in a `.cs` file, and internal `href` each emit one fact with correct `target_path`/`source_kind`/`route_source`
- [ ] External and fragment `href` values emit nothing
- [ ] Raw-template fidelity test for `razor.page_directive.v1` green (`{id?}`, `{*path}` survive verbatim in the fact payload)
- [ ] Registry conformance passes for both csharp and razor and the exported contract JSON is synchronized

### Task 6: `blazor.component_reference.v1`

**Files:**
- Modify: `crates/julie-extractors/src/razor/mod.rs:74-107` (component identity: exclude `_Imports.razor`, `_ViewImports.cshtml`)
- Modify: `crates/julie-extractors/src/base/framework_structural_facts/razor.rs` (component-reference fact emission)
- Modify: `crates/julie-extractors/src/base/framework_structural_facts/mod.rs` (pattern ID and razor dispatch)
- Modify: `crates/julie-extractors/src/base/structural_fact_registry.rs`
- Modify: `docs/contracts/structural-fact-patterns.json`
- Test: `crates/julie-extractors/src/tests/razor/component_reference.rs`

**Interfaces:**
- Produces: fact family `blazor.component_reference.v1` — fields: `tag` (PascalCase component name), `containing_component`, `namespace_context` (only locally declared `@namespace`/`@using` values), and `generic_arguments`. The extractor does not emit `external`; Miller resolves the tag against workspace components, inherited `_Imports.razor` context, and external assemblies.

**Contract inputs:** existing same-file `component-usage` relationship logic (`razor/relationships.rs:231`) stays; the fact family is the cross-file channel. PascalCase tag regex already exists (`COMPONENT_TAG_RE`).

**What to build:** Emit one fact per component tag occurrence using only syntax and symbols observable in the current file. Fix component identity so infrastructure files stop producing synthetic components. Preserve local namespace/import context as resolution input without reading sibling files or guessing that an unresolved tag is external.

**Acceptance criteria:**
- [ ] Cross-file fixture: `PageA.razor` using `<SharedWidget />` emits a fact naming the containing component and referenced tag without claiming workspace resolution
- [ ] `_Imports.razor` no longer yields a component symbol; `App.razor` still does
- [ ] FluentUI tags emit reference facts with local namespace/import context and no extractor-owned `external` classification
- [ ] Registry conformance passes for razor and the exported contract JSON is synchronized

### Task 7: `http.client_request.v1` from razor `@code`

**Files:**
- Modify: `crates/julie-extractors/src/base/framework_structural_facts/http_clients/csharp.rs` (accept allowed embedded-C# byte ranges while preserving full-file offsets)
- Modify: `crates/julie-extractors/src/base/framework_structural_facts/http_clients/mod.rs` (Razor-specific entry point)
- Modify: `crates/julie-extractors/src/base/framework_structural_facts/mod.rs` (invoke the Razor HTTP-client entry point and update emitted-pattern IDs)
- Modify: `crates/julie-extractors/src/base/structural_fact_registry.rs` (add razor to `http.client_request.v1`)
- Modify: `docs/contracts/structural-fact-patterns.json`
- Test: `crates/julie-extractors/src/tests/razor/client_request.rs`

**Interfaces:**
- Produces: `http.client_request.v1` facts (same shape as csharp: client=httpclient, verb, url) from HttpClient calls inside `@code`/`@functions` blocks; `http.client_request.v1` added to razor's supported structural_facts list.

**What to build:** Reuse the C# HTTP-client scanner through the framework-fact pipeline, not the Razor symbol extractor. Derive allowed byte ranges from named Razor `@code`/`@functions` C# nodes, scan only those ranges while preserving absolute spans, and keep the normal csharp full-file path unchanged.

**Acceptance criteria:**
- [ ] Fixture with `await Http.GetFromJsonAsync<Foo>("/api/foo")` in `@code` emits a fact with verb GET and the url
- [ ] Identical text in Razor markup, strings, or comments outside embedded-C# ranges emits nothing
- [ ] csharp fact output unchanged (no double emission for `.cs` files)
- [ ] Registry conformance passes with razor added to `http.client_request.v1` and the exported contract JSON is synchronized

### Task 8: test_container detection closure (csharp/vbnet/razor)

**Files:**
- Modify: `crates/julie-extractors/src/test_detection.rs:86-163` (`detect_csharp`)
- Test: golden test-detection fixtures for the three languages

**Interfaces:**
- Produces: `test_container` classification for xUnit/NUnit/MSTest test classes (`[TestFixture]`, `[TestClass]`, classes containing `[Fact]`/`[Theory]`/`[Test]` members).

**Contract inputs:** the existing closure plan `docs/plans/2026-07-09-test-detection-golden-closure-implementation-plan.md` — execute its csharp/vbnet/razor scope here; do not duplicate its other languages.

**Acceptance criteria:**
- [ ] NUnit `[TestFixture]` class and attribute-less class with `[Test]` members classify as containers
- [ ] The open `test_detection → test_container` capability gap rows for csharp/vbnet/razor close in the capability snapshot
- [ ] Lifecycle members (`[SetUp]`, `[TearDown]`) unaffected

### Task 9: Synthetic fixtures, certification, release

**Files:**
- Create: `fixtures/extraction/razor/{code-behind,imports,scoped-assets,typeparam,rendermode}/`
- Modify: `fixtures/extraction/capabilities.json` (certified kind_coverage + structural_facts for razor)

**Interfaces:**
- Consumes: Tasks 4–8 output.
- Produces: the julie-extractors release Miller pins (Lane 2).

**What to build:** Fixtures for shapes Terraform lacks: `.razor` + `.razor.cs` code-behind identity inputs, `_Imports.razor` namespace/import inputs for downstream inheritance resolution, scoped-asset adjacency (fixture documents `Foo.razor.css` belongs to `Foo` — extraction-level association only), constrained `@typeparam`, render modes, cascading parameters. Certify all new facts/kinds and prove the registry export is synchronized. Prepare the release per the repo's release flow; publishing, tagging, or pushing requires separate explicit approval. File the T-SQL parse-quality issue (283 errors + 1 missing across six Terraform SQL files, 225 in `db/baseline.sql`) as a separate tracked item.

**Acceptance criteria:**
- [ ] All five fixture groups pass the semantic gate
- [ ] Razor certified kind_coverage includes class + property (today: import/method/variable only)
- [ ] Capability snapshot certifies `razor.route_reference.v1`, `blazor.component_reference.v1`, razor `http.client_request.v1`
- [ ] `docs/contracts/structural-fact-patterns.json` is byte-synchronized with the registry and all new metadata keys are declared
- [ ] Release prepared; version + rev recorded for Miller's pin bump
- [ ] T-SQL issue filed
