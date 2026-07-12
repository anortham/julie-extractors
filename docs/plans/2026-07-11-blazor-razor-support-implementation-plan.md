# Blazor/Razor Extraction Support Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Make the razor extractor parse real-world Blazor markup cleanly (zero diagnostics + correct semantics) and emit the fact families Miller needs for Blazor bridging.

**Architecture:** Grammar-first fix in the tree-sitter-razor fork (repo `~/source/tree-sitter-razor`, branch `fix/attribute-value-expressions` is the starting point; julie pins rev `cf7b0e5` at `crates/julie-extractors/Cargo.toml:55`). Two new additive fact families (`razor.route_reference.v1`, `blazor.component_reference.v1`) plus `http.client_request.v1` for razor. All additive; certified through the capability snapshot.

**Tech Stack:** Rust, tree-sitter (grammar.js + generated parser), julie golden-fixture harness.

**Architecture Quality:** Approved shape per the umbrella design (`/Users/murphy/source/eros/docs/plans/2026-07-11-dotnet-blazor-stack-support-design.md`): grammar work stays in the fork; extractor work stays in `crates/julie-extractors/src/razor/` and `src/base/framework_structural_facts/razor.rs`; no new extractor seams. Rejected: ERROR-node regex recovery as the primary fix; diagnostics-only acceptance gate. Risk: medium (grammar variance — bounded by the Task 1 spike; fallback architecture is nested tree-sitter-c-sharp parsing of attribute expressions, which is structured parsing, not regex recovery).

## Global Constraints

- New fact families and symbol kinds must land in the certified capability snapshot (`fixtures/extraction/capabilities.json` kind_coverage / structural_facts), not just code.
- All fact additions are additive — no changes to existing fact family shapes.
- Existing razor test suite (~59 tests in `crates/julie-extractors/src/tests/razor/`) stays green throughout.
- Existing ERROR-node recovery (`razor/mod.rs:255-297`, `@inherits`/`@rendermode` regex) stays as tail safety net.
- Component identity: `_Imports.razor` and `_ViewImports.cshtml` are NOT components; `App.razor` is.
- `languages/razor.toml` carries extraction policy only (its own header rule).
- Reference corpus source: Terraform diagnostics (232 razor errors; query `~/source/Terraform/.miller/symbols.db` table `parse_diagnostics WHERE language='razor'`) plus FluentUI documentation patterns.

## Verification Strategy

**Project source of truth:** `README.md` (contract test commands), `AGENTS.md` (source-verification rule), golden fixture harness under `fixtures/extraction/`.

**Worker red/green scope:** `cargo test -p julie-extractors razor` (razor test modules) for extractor tasks; `tree-sitter generate && tree-sitter test` in `~/source/tree-sitter-razor` for grammar tasks.

**Worker ceiling:** `cargo test -p julie-extractors`.

**Worker gate invariant:** each grammar construct proves BOTH zero parse diagnostics AND expected identifiers/calls/literals/relationships — opaque consumption of an expression is a gate failure even when diagnostics are clean (the grammar has opaque fallbacks, `grammar.js:480,497`).

**Lead affected-change scope:** `cargo test -p julie-extractors` + `cargo test -p julie-extract-artifact --test schema_contract --test jsonl_contract`.

**Branch gate:** full contract suite per README: `cargo test -p xtask`, artifact schema/jsonl contracts, `cargo test -p julie-extract-cli --test cli_contract`, plus a re-extraction of `~/source/Terraform` showing razor diagnostics ≈ 0.

**Escalation triggers:** any change to `src/base/http_boundary.rs` or fact-envelope code → run the full artifact contract suite; grammar rev bump → run the entire razor golden corpus.

**Assigned verification failure:** Workers stop and report when assigned verification fails.

**Verification ledger:** Record invariant, command, scope label, commit SHA, result, timestamp per task.

## Parallel Execution Contract

| Task | Parallel batch | File ownership | Serialization required | Dependency reason |
|---|---|---|---|---|
| Task 1: Corpus spike | None - serial | Create: `~/source/tree-sitter-razor/test/corpus/blazor-attributes/*`, `docs/plans/2026-07-11-blazor-corpus-classification.md` | Yes | Everything downstream is scoped by the classification. |
| Task 2: Attribute-value expressions (grammar) | None - serial | Modify: `~/source/tree-sitter-razor/grammar.js`, generated parser, corpus tests | Yes | Depends on Task 1 corpus; grammar edits conflict file-level with Task 3. |
| Task 3: Directive-attribute modifiers + rendermode/typeparam (grammar) | None - serial | Modify: `~/source/tree-sitter-razor/grammar.js`, generated parser, corpus tests | Yes | Same grammar.js as Task 2. |
| Task 4: Pin bump + semantic gate | None - serial | Modify: `crates/julie-extractors/Cargo.toml:55`; Create: `crates/julie-extractors/src/tests/razor/semantic_gate.rs`, `fixtures/extraction/razor/attribute-expressions/*` | Yes | Needs the released grammar rev from Tasks 2–3. |
| Task 5: razor.route_reference.v1 | Batch A | Modify: `src/base/framework_structural_facts/razor.rs`, `crates/julie-extractors/src/razor/identifiers.rs`; Test: `crates/julie-extractors/src/tests/razor/structural_facts.rs` (route-reference tests only) | No | None - safe parallel batch. |
| Task 6: blazor.component_reference.v1 | Batch A | Modify: `crates/julie-extractors/src/razor/mod.rs:74-107` (component identity), Create: `src/base/framework_structural_facts/blazor.rs` (new module — do NOT touch `razor.rs`, Task 5 owns it); Test: new file `crates/julie-extractors/src/tests/razor/component_reference.rs` | No | None - safe parallel batch. |
| Task 7: http.client_request.v1 from @code | Batch A | Modify: `crates/julie-extractors/src/razor/csharp.rs`, `languages/razor.toml` (carrier reuse only); Test: new file `crates/julie-extractors/src/tests/razor/client_request.rs` | No | None - safe parallel batch. |
| Task 8: test_container closure | Batch A | Modify: `crates/julie-extractors/src/test_detection.rs:86-163`; Test: golden test-detection fixtures for csharp/vbnet/razor containers | No | None - safe parallel batch. |
| Task 9: Synthetic fixtures + certification + release | None - serial | Create: `fixtures/extraction/razor/{code-behind,imports,scoped-assets,typeparam,rendermode}/*`; Modify: `fixtures/extraction/capabilities.json` | Yes | Integrates Tasks 4–8; certification is last. |

Tasks 5–8 do not depend on the grammar tasks (facts read constructs that already parse); they may run before or in parallel with Tasks 2–4 as Batch A under `parallel-lead-commit`. Tasks 1–4 and 9 are `serial-worker-commit`.

---

### Task 1: Corpus spike and classification

**Files:**
- Create: `~/source/tree-sitter-razor/test/corpus/blazor-attributes/` (one corpus file per construct class)
- Create: `docs/plans/2026-07-11-blazor-corpus-classification.md`

**Interfaces:**
- Produces: the classified construct list with per-class counts and the re-estimate for Tasks 2–3. Corpus files in tree-sitter test format (parse-tree expectations).

**Contract inputs:** Terraform diagnostics via `sqlite3 -readonly ~/source/Terraform/.miller/symbols.db "SELECT path, start_line, start_column FROM parse_diagnostics WHERE language='razor'"`; read the cited source spans from `~/source/Terraform`.

**What to build:** Harvest every distinct failing construct into minimal repro snippets; classify (attribute-value implicit expression / explicit expression with lambda-ternary-collection / directive-attribute modifier / other). Add FluentUI doc patterns not present in Terraform. Record which classes the existing `fix/attribute-value-expressions` branch (07eab9c) already fixes.

**Approach:** Run each snippet through the branch parser (`tree-sitter parse`) before classifying — do not classify against the pinned rev. Anything already fixed by the branch goes in the classification as "covered by 07eab9c".

**Acceptance criteria:**
- [ ] Every one of the 232 Terraform diagnostics maps to a corpus snippet or a named duplicate
- [ ] Classification doc lists per-class counts, branch coverage, and a re-estimate for Tasks 2–3
- [ ] Corpus files committed to the grammar repo on `fix/attribute-value-expressions`

### Task 2: Grammar — attribute-value expressions

**Files:**
- Modify: `~/source/tree-sitter-razor/grammar.js` (attribute value rules; opaque fallbacks at lines 480, 497 must lose priority to expression parses, not be deleted)
- Test: `~/source/tree-sitter-razor/test/corpus/blazor-attributes/*`

**Interfaces:**
- Consumes: Task 1 corpus and classification.
- Produces: grammar rev where implicit expressions (`@typeof(App).Assembly`, member access, method calls, indexers) and explicit expressions (`@(...)` with lambdas, ternaries, casts, collection expressions) inside attribute values produce expression nodes, not ERROR/opaque text.

**What to build:** Extend attribute-value rules to parse `@`-expressions via the embedded tree-sitter-c-sharp rules the grammar already uses. Keep the opaque pattern only for genuinely expressionless values.

**Approach:** Work per construct class in Task 1 frequency order. If direct composition destabilizes the grammar (conflicts, state explosion), stop and report — the recorded fallback is nested C# parsing of the attribute text, a design-level decision the lead confirms before pivoting.

**Acceptance criteria:**
- [ ] All Task 1 corpus files in the implicit/explicit expression classes pass `tree-sitter test`
- [ ] Expression content appears as parsed nodes (assert node kinds in corpus expectations, not just absence of ERROR)
- [ ] Existing grammar corpus stays green

### Task 3: Grammar — directive-attribute modifiers, rendermode, typeparam

**Files:**
- Modify: `~/source/tree-sitter-razor/grammar.js` (directive attribute rules; render-mode rule at line 190 is closed to three keywords)
- Test: `~/source/tree-sitter-razor/test/corpus/blazor-attributes/*`

**Interfaces:**
- Consumes: Task 1 corpus.
- Produces: grammar rev parsing `@on{event}:{modifier}` (e.g. `@onsubmit:preventDefault`), `@bind-{Prop}:{event|format|get|set|after}`, open-set `@rendermode` arguments, constrained `@typeparam T where T : ...`.

**What to build:** Modifier suffix parsing on directive attributes; widen the render-mode rule from its closed keyword list; typeparam constraint clause.

**Acceptance criteria:**
- [ ] Corpus files for modifiers/rendermode/typeparam pass with named nodes
- [ ] Existing grammar corpus stays green
- [ ] Grammar repo released/tagged at a rev for Task 4 to pin

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
- Modify: `src/base/framework_structural_facts/razor.rs`
- Modify: `crates/julie-extractors/src/razor/identifiers.rs` (NavigateTo call-site literal capture if not already surfaced)
- Test: `crates/julie-extractors/src/tests/razor/structural_facts.rs`

**Interfaces:**
- Produces: fact family `razor.route_reference.v1` — fields: `route` (the literal target), `source` (`navigate_to` | `href`), emitted from `.razor` AND `.cs` files for `NavigationManager.NavigateTo("...")` (incl. `NavigateToLogin` variants only if literal-first-arg) and internal `href="/..."` attributes (skip external `http(s)://` and fragment-only targets). Mirrors the `nextjs.route_reference.v1` shape.
- Consumes: nothing from other tasks (constructs parse today in `@code` and `.cs`).

**Contract inputs:** Miller will pair this with `razor.page_directive.v1` in a file-route provider; the `route` string must stay the raw literal — no normalization on the reference side.

**What to build:** Fact emission for navigation targets. Also add a regression test that `razor.page_directive.v1` preserves the raw ASP.NET brace template and `route_parameters` metadata (optional/catch-all flags) verbatim — Miller's Blazor adapter consumes the raw form; julie must not strip markers from it.

**Acceptance criteria:**
- [ ] `NavigateTo` in `@code`, `NavigateTo` in a `.cs` file, and internal `href` each emit one fact with correct `route`/`source`
- [ ] External and fragment `href` values emit nothing
- [ ] Raw-template fidelity test for `razor.page_directive.v1` green (`{id?}`, `{*path}` survive verbatim in the fact payload)

### Task 6: `blazor.component_reference.v1`

**Files:**
- Modify: `crates/julie-extractors/src/razor/mod.rs:74-107` (component identity: exclude `_Imports.razor`, `_ViewImports.cshtml`)
- Create: `src/base/framework_structural_facts/blazor.rs` (component-reference fact emission — new module so Task 5's `razor.rs` ownership is untouched; register it in the framework facts mod)
- Test: `crates/julie-extractors/src/tests/razor/component_reference.rs`

**Interfaces:**
- Produces: fact family `blazor.component_reference.v1` — fields: `tag` (PascalCase component name), `containing_component`, `namespace_context` (from `@namespace`/`@using`, including `_Imports.razor` inheritance where resolvable), `generic_arguments`, `external` (bool: tag not resolvable to a workspace component name is marked external, e.g. `FluentButton`).

**Contract inputs:** existing same-file `component-usage` relationship logic (`razor/relationships.rs:231`) stays; the fact family is the cross-file channel. PascalCase tag regex already exists (`COMPONENT_TAG_RE`).

**What to build:** Emit one fact per component tag occurrence. Fix component identity so infrastructure files stop producing synthetic components (Terraform currently exposes a `_Imports` component).

**Acceptance criteria:**
- [ ] Cross-file fixture: `PageA.razor` using `<SharedWidget />` emits a fact naming both sides
- [ ] `_Imports.razor` no longer yields a component symbol; `App.razor` still does
- [ ] FluentUI tags emit facts flagged `external: true`

### Task 7: `http.client_request.v1` from razor `@code`

**Files:**
- Modify: `crates/julie-extractors/src/razor/csharp.rs` (route embedded-C# through the csharp http-client fact path)
- Test: `crates/julie-extractors/src/tests/razor/client_request.rs`

**Interfaces:**
- Produces: `http.client_request.v1` facts (same shape as csharp: client=httpclient, verb, url) from HttpClient calls inside `@code`/`@functions` blocks; `http.client_request.v1` added to razor's supported structural_facts list.

**What to build:** The embedded-C# pipeline already re-parents symbols; route the same region through the csharp `http_clients` fact collector (see `src/base/framework_structural_facts/http_clients/`).

**Acceptance criteria:**
- [ ] Fixture with `await Http.GetFromJsonAsync<Foo>("/api/foo")` in `@code` emits a fact with verb GET and the url
- [ ] csharp fact output unchanged (no double emission for `.cs` files)

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

**What to build:** Fixtures for shapes Terraform lacks: `.razor` + `.razor.cs` code-behind partial identity (one component, two files), `_Imports.razor` namespace/import inheritance (qualified names must not depend solely on local `@namespace`, `razor/mod.rs:73`), scoped-asset adjacency (fixture documents `Foo.razor.css` belongs to `Foo` — extraction-level association only), constrained `@typeparam`, render modes, cascading parameters. Certify all new facts/kinds. Prepare the release per the repo's release flow (see `scripts/` release-state tripwire). File the T-SQL parse-quality issue (283 errors + 1 missing across six Terraform SQL files, 225 in `db/baseline.sql`) as a separate tracked item.

**Acceptance criteria:**
- [ ] All five fixture groups pass the semantic gate
- [ ] Razor certified kind_coverage includes class + property (today: import/method/variable only)
- [ ] Capability snapshot certifies `razor.route_reference.v1`, `blazor.component_reference.v1`, razor `http.client_request.v1`
- [ ] Release prepared; version + rev recorded for Miller's pin bump
- [ ] T-SQL issue filed
