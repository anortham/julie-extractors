# ASP.NET Minimal API, htmx, and Alpine Structural Facts Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Emit versioned structural facts for ASP.NET minimal API routes, htmx request attributes, and Alpine directives so downstream tools can index and bridge these web-stack facts without owning parser policy.

**Architecture:** Extend the existing `structural_facts` row family; do not add a new SQLite/JSONL schema domain. Add content-aware framework fact collectors beside the current generic node-kind collector, then publish exact supported pattern ids through the existing capability snapshot. Miller consumes these facts and owns cross-file route linking.

**Tech Stack:** Rust 2024, tree-sitter C#, tree-sitter HTML, tree-sitter Razor, `julie-extractors`, `julie-extract` CLI contracts, SQLite/JSONL v3 `structural_facts`, fixture-backed capability evidence.

**Architecture Quality:** Medium risk. The caller-facing interface is public `structural_facts` rows and capability metadata, but artifact schema shape stays unchanged. Framework recognition must stay local to extractor-side fact collection; downstream bridge/search behavior remains out of scope for this repo.

---

## Source Documents

- `AGENTS.md` and `CLAUDE.md`: product boundary, test discipline, and extractor-only scope.
- `docs/testing-strategy.md`: default, language, capability, contract, and changed-path test tiers.
- `docs/plans/2026-06-09-structural-facts-design.md`: existing structural facts contract and downstream boundary.
- `fixtures/extraction/capabilities.json`: source of truth for advertised pattern coverage.
- `crates/julie-extractors/src/base/structural_facts.rs`: current generic structural-facts collector.
- `crates/julie-extractors/src/registry.rs`: canonical place where source regions, structural facts, and complexity metrics are added after language extraction.

## Architecture Quality

**Affected modules:** `crates/julie-extractors/src/base/`, `crates/julie-extractors/src/registry.rs`, C#/HTML/Razor fixture directories, `fixtures/extraction/capabilities.json`, extractor structural-fact tests, CLI capability contract tests, and structural-facts design docs.

**Caller-facing interface:** `ExtractionResults.structural_facts`, artifact `structural_facts` rows, `metadata_json`, `kind_coverage.structural_facts.supported`, and `julie-extract languages --json`.

**Depth/locality check:** Parser-specific matching lives in extractor-side collectors. Artifact writer, JSONL, reports, and SQLite schema should not change because the existing row shape already carries `pattern_id`, span, confidence, and metadata.

**Test surface:** Tests must prove behavior through `extract_canonical`, golden fixtures, capability-matrix checks, and CLI `languages --json`. Private helper tests are acceptable only as support for those public surfaces.

**Seams/adapters:** Add one internal collector seam for content-aware framework facts. Do not add a Miller adapter, route linker, search table, dashboard, query language, or bridge provider in this repo.

**Rejected shortcuts:** Do not rely on HTML display signatures for htmx/Alpine. Do not emit opaque framework blobs. Do not claim broad ASP.NET MVC/controller support. Do not synthesize cross-file links between htmx and ASP.NET endpoints here.

**Architecture risk:** Medium. The schema is stable, but new pattern ids become public artifact contract data and must be fixture-backed.

## Contract Shape

All new rows use the existing `StructuralFact` / `ArtifactStructuralFact` shape:

- stable `structural_fact_id`
- `pattern_id`
- `capture_name`
- matched `node_kind`
- optional `containing_symbol_id`
- normalized source span
- `confidence`
- optional `metadata_json`

Every new row must include:

```json
{
  "pattern_version": 1,
  "query_family": "web_framework"
}
```

### `aspnet.minimal_api.route.v1`

**Language:** `csharp`

**Emits for:** Static ASP.NET minimal API route declarations using:

- `MapGet`
- `MapPost`
- `MapPut`
- `MapPatch`
- `MapDelete`

**Does not emit for this slice:** MVC controllers, Razor Pages handlers, SignalR hubs, `MapGroup` prefix composition, `MapMethods`, non-string route templates, interpolated route templates, route constants, extension-method wrappers, or endpoint filters.

**Row span:** the matched invocation/call node that owns the route declaration.

**Capture name:** `minimal_api_route`

**Metadata fields:**

- `framework`: `"aspnet"`
- `api_style`: `"minimal_api"`
- `verb`: uppercase HTTP verb derived from the method name
- `route_template`: decoded static route string, preserving route parameters such as `{id}`
- `route_source`: `"static_string_literal"`
- `handler_kind`: `"lambda"`, `"method_group"`, or `"unknown"`
- `handler_name`: present for method-group handlers
- `handler_symbol_id`: present only when the named handler resolves to an extracted symbol in the same file

**Confidence:** `1.0` for static literal routes. Skip ambiguous dynamic cases instead of emitting lower-confidence rows in this slice.

### `htmx.attribute.v1`

**Languages:** `html`, `razor`

**Emits for:** HTML/Razor attributes whose names start with `hx-`.

**Row span:** the matched attribute node.

**Capture name:** `htmx_attribute`

**Metadata fields:**

- `framework`: `"htmx"`
- `attribute_name`: raw attribute name
- `attribute_value`: raw decoded value, or empty string for boolean attributes
- `is_request_attribute`: true for request attributes
- `http_verb`: uppercase verb for `hx-get`, `hx-post`, `hx-put`, `hx-patch`, and `hx-delete`
- `target_path`: attribute value for request attributes when the value is non-empty

**Confidence:** `1.0` when the attribute name is syntactically present. Empty `target_path` values still emit the attribute fact but set `is_request_attribute` according to the attribute name.

### `alpine.directive.v1`

**Languages:** `html`, `razor`

**Emits for:** Alpine directive attributes:

- long form: names starting with `x-`
- event shorthand: names starting with `@`
- binding shorthand: names starting with `:`

**Row span:** the matched attribute node.

**Capture name:** `alpine_directive`

**Metadata fields:**

- `framework`: `"alpine"`
- `attribute_name`: raw attribute name
- `attribute_value`: raw decoded value, or empty string for boolean attributes
- `directive`: normalized directive name, such as `x-data`, `x-show`, `x-on`, or `x-bind`
- `argument`: directive argument when present, such as `click` for `x-on:click` or `@click`
- `modifiers`: array of modifier names after the argument, such as `prevent` and `debounce`
- `expression`: same decoded value as `attribute_value`
- `shorthand`: true for `@...` and `:...`

**Confidence:** `1.0` when the attribute name is syntactically present.

## File Structure

- Create: `crates/julie-extractors/src/base/framework_structural_facts.rs`
- Modify: `crates/julie-extractors/src/base/mod.rs`
- Modify: `crates/julie-extractors/src/registry.rs:987`
- Modify: `crates/julie-extractors/src/tests/structural_facts.rs:64`
- Modify: `crates/julie-extractors/src/tests/capability_matrix.rs:1414`
- Modify: `crates/julie-extract-cli/tests/operations_contract.rs:1396`
- Create: `fixtures/extraction/csharp/aspnet_minimal_api/source.cs`
- Create: `fixtures/extraction/csharp/aspnet_minimal_api/expected.json`
- Create: `fixtures/extraction/html/htmx_alpine/source.html`
- Create: `fixtures/extraction/html/htmx_alpine/expected.json`
- Create: `fixtures/extraction/razor/htmx_alpine_fragment/source.razor`
- Create: `fixtures/extraction/razor/htmx_alpine_fragment/expected.json`
- Modify: `fixtures/extraction/capabilities.json`
- Modify: `docs/plans/2026-06-09-structural-facts-design.md`
- Modify: `TODO.md`

Do not modify `crates/julie-extract-artifact/src/schema.rs`, `writer.rs`, `jsonl.rs`, or report row shapes for this slice. If implementation cannot represent these facts in the existing `structural_facts` table, report a plan mismatch before changing the artifact schema.

## Verification Strategy

**Project source of truth:** `AGENTS.md`, `CLAUDE.md`, `docs/testing-strategy.md`, and `fixtures/extraction/capabilities.json`.

**Worker red/green scope:** Use focused tests first:

```bash
cargo test -p julie-extractors structural_facts
cargo xtask test language csharp
cargo xtask test language html
cargo xtask test language razor
```

**Worker ceiling:** Workers may run focused extractor tests, one-language gates for `csharp`, `html`, and `razor`, capability tests, and the CLI operations contract. Workers do not own real-world corpus gates, release packaging, parser dependency upgrades, or downstream Miller bridge verification.

**Worker gate invariant:** The focused tests prove that extractor output contains the three new pattern ids with correct spans, metadata, containing-symbol behavior where applicable, and no rows for unsupported dynamic cases.

**Lead affected-change scope:** After a coherent implementation batch, run:

```bash
cargo xtask test changed crates/julie-extractors/src/base/framework_structural_facts.rs crates/julie-extractors/src/registry.rs fixtures/extraction/capabilities.json
cargo xtask test capability
cargo test -p julie-extract-cli operations_contract
```

**Branch gate:** Before handoff, run:

```bash
cargo xtask test default
cargo xtask test contract
```

**Replay/metric evidence:** A tiny scan over the new fixtures must show non-empty `structural_facts` rows for `aspnet.minimal_api.route.v1`, `htmx.attribute.v1`, and `alpine.directive.v1`. Row presence and metadata shape are hard gates. Timing and artifact size are report-only.

**Escalation triggers:** Escalate if implementation needs parser dependency changes, artifact schema changes, CLI status/exit-code changes, dynamic route evaluation, route-group data flow, or cross-file htmx-to-C# linking.

**Assigned verification failure:** Workers investigate and fix focused failures within this plan. Stop only when a failure means the approved contract shape is wrong.

**Verification ledger:** Record invariant, command, scope label, commit SHA, result, and timestamp. Reuse evidence only when HEAD and scope match.

## Model Routing

**Project source of truth:** No repo-local `RAZORBACK.md` exists. Use `AGENTS.md`, `CLAUDE.md`, and the current Codex session defaults.

**Strategy tier:** planning, architecture, contract interpretation, lead review, and finding triage.
- Harness mapping: inherit current Codex session model.

**Implementation tier:** bounded extractor, fixture, and capability tasks after this plan is accepted.
- Harness mapping: inherit current Codex session model.

**Mechanical tier:** docs wording, fixture path registration, and manifest-style updates with no gate interpretation.
- Harness mapping: inherit current Codex session model.

**Gate-interpretation reviewer:** use strategy tier for capability, fixture, or contract mismatches.
- Harness mapping: inherit current Codex session model.

**Escalation tier:** schema/API changes, parser dependency changes, cross-file route-linking proposals, repeated test failures, or weak fixture evidence.
- Harness mapping: inherit current Codex session model.

**Worker eligibility:** Workers are eligible when assigned a non-overlapping slice with exact file ownership and focused verification. A worker must not reinterpret the public contract shape or expand scope to Miller bridge behavior.

**Mechanical exclusion:** Mechanical workers cannot own failing tests, replay evidence, capability interpretation, or acceptance gates.

**Unsupported harness behavior:** If the harness cannot choose models per agent, use `inherit` and continue.

## Tasks

### Task 1: Lock Contract Examples With Failing Tests

**Files:**

- Modify: `crates/julie-extractors/src/tests/structural_facts.rs:64`
- Create: `fixtures/extraction/csharp/aspnet_minimal_api/source.cs`
- Create: `fixtures/extraction/csharp/aspnet_minimal_api/expected.json`
- Create: `fixtures/extraction/html/htmx_alpine/source.html`
- Create: `fixtures/extraction/html/htmx_alpine/expected.json`
- Create: `fixtures/extraction/razor/htmx_alpine_fragment/source.razor`
- Create: `fixtures/extraction/razor/htmx_alpine_fragment/expected.json`

**What to build:** Add tests and fixtures that define the exact public output before implementation. The fixtures must include one static ASP.NET minimal API route for each supported verb, at least one inline lambda handler, one named method-group handler, htmx request and non-request attributes, and Alpine long-form plus shorthand directives.

**Approach:** Extend the existing `supported_structural_patterns_emit_parser_backed_facts` style with cases for `.cs`, `.html`, and `.razor`. For golden fixtures, expected rows should assert pattern ids and metadata values, not merely row counts. Include negative examples for a dynamic ASP.NET route template and a non-Alpine `@` usage only when the parser shape makes the negative case stable.

**Acceptance criteria:**

- [ ] `cargo test -p julie-extractors structural_facts` fails because the new pattern ids are not emitted yet.
- [ ] Golden fixture expected output includes `structural_facts` rows for all three pattern ids.
- [ ] No fixture claims route-group composition, MVC/controller endpoints, or cross-file linking.

### Task 2: Add Content-Aware Framework Fact Collection

**Files:**

- Create: `crates/julie-extractors/src/base/framework_structural_facts.rs`
- Modify: `crates/julie-extractors/src/base/mod.rs`
- Modify: `crates/julie-extractors/src/registry.rs:987`

**What to build:** Add a collector entry point, `collect_framework_structural_facts(language, tree, file_path, content, symbols)`, that returns `Vec<StructuralFact>`. `registry::extract_for_language` should append these rows to the existing generic `collect_structural_facts(...)` output, sort using the same stable span/pattern/id ordering, and leave source regions and complexity metrics unchanged.

**Approach:** Keep the existing `collect_structural_facts(...)` signature stable. The new collector may use `tree_sitter::Node` byte ranges and source content to decode attribute values and C# route literals. Construct `StructuralFact` rows directly, set normalized spans from matched nodes, attach containing symbols using the same smallest-containing-span semantics, and call `refresh_id()` after span and pattern fields are set.

**Acceptance criteria:**

- [ ] Existing structural-fact tests still pass for Rust, Go, Python, JS/TS, C, and C++.
- [ ] New framework facts sort deterministically with existing structural facts.
- [ ] No artifact schema, JSONL, writer, or report code changes are needed.

### Task 3: Emit ASP.NET Minimal API Route Facts

**Files:**

- Modify: `crates/julie-extractors/src/base/framework_structural_facts.rs`
- Test: `crates/julie-extractors/src/tests/structural_facts.rs:64`
- Fixture: `fixtures/extraction/csharp/aspnet_minimal_api/source.cs`
- Fixture: `fixtures/extraction/csharp/aspnet_minimal_api/expected.json`

**What to build:** Detect static ASP.NET minimal API route declarations in C# and emit `aspnet.minimal_api.route.v1` facts. Support `MapGet`, `MapPost`, `MapPut`, `MapPatch`, and `MapDelete` when the first argument is a static string literal.

**Approach:** Walk C# invocation/call nodes and identify method names by terminal member identifier. Decode normal, verbatim, and raw static string literals enough to preserve the route template without surrounding quotes. For named method-group handlers, populate `handler_name` and resolve `handler_symbol_id` when an extracted symbol in the same file has that name. For inline lambdas, set `handler_kind = "lambda"` and rely on `containing_symbol_id` for the file or enclosing symbol.

**Acceptance criteria:**

- [ ] Static `app.MapGet("/todos", ...)` emits `verb = "GET"` and `route_template = "/todos"`.
- [ ] Static route parameters such as `"/todos/{id}"` preserve the braces.
- [ ] Method-group handlers include `handler_name`, and include `handler_symbol_id` when a same-file symbol exists.
- [ ] Dynamic or interpolated route templates do not emit `aspnet.minimal_api.route.v1` in this slice.

### Task 4: Emit htmx and Alpine Attribute Facts for HTML and Razor

**Files:**

- Modify: `crates/julie-extractors/src/base/framework_structural_facts.rs`
- Test: `crates/julie-extractors/src/tests/structural_facts.rs:64`
- Fixture: `fixtures/extraction/html/htmx_alpine/source.html`
- Fixture: `fixtures/extraction/html/htmx_alpine/expected.json`
- Fixture: `fixtures/extraction/razor/htmx_alpine_fragment/source.razor`
- Fixture: `fixtures/extraction/razor/htmx_alpine_fragment/expected.json`

**What to build:** Emit one `htmx.attribute.v1` fact per `hx-*` attribute and one `alpine.directive.v1` fact per Alpine directive attribute in HTML and Razor files.

**Approach:** Walk attribute nodes in `html` and `razor` parse trees. Reuse one attribute parser for both languages: extract the raw attribute name, decode quoted/unquoted values, and normalize Alpine shorthand names. The collector should not require the owning element to be emitted as an HTML symbol; attribute facts stand on their own.

**Acceptance criteria:**

- [ ] `hx-get="/todos"` emits `target_path = "/todos"`, `http_verb = "GET"`, and `is_request_attribute = true`.
- [ ] Non-request htmx attributes such as `hx-target="#list"` emit attribute facts without `target_path`.
- [ ] `x-data="{ open: false }"` emits `directive = "x-data"` and preserves the expression.
- [ ] `@click.prevent="open = !open"` emits `directive = "x-on"`, `argument = "click"`, `modifiers = ["prevent"]`, and `shorthand = true`.
- [ ] `:class="{ active: open }"` emits `directive = "x-bind"`, `argument = "class"`, and `shorthand = true`.
- [ ] Razor fixtures prove these facts inside `.razor` markup without treating Razor component usages as definitions.

### Task 5: Publish Capability and CLI Contract Evidence

**Files:**

- Modify: `fixtures/extraction/capabilities.json`
- Modify: `crates/julie-extractors/src/tests/capability_matrix.rs:1414`
- Modify: `crates/julie-extract-cli/tests/operations_contract.rs:1396`

**What to build:** Advertise the new pattern ids only for languages with fixture-backed evidence.

**Approach:** Add `aspnet.minimal_api.route.v1` to C# `kind_coverage.structural_facts.supported`; add `htmx.attribute.v1` and `alpine.directive.v1` to HTML and Razor. Ensure capability-matrix tests reject any advertised id that lacks fixture output. Ensure `julie-extract languages --json` exposes the exact updated coverage.

**Acceptance criteria:**

- [ ] `cargo xtask test capability` passes.
- [ ] `cargo test -p julie-extract-cli operations_contract` passes.
- [ ] `languages --json` reports the new ids for `csharp`, `html`, and `razor`, and not for unrelated languages.

### Task 6: Document Scope and Run Gates

**Files:**

- Modify: `docs/plans/2026-06-09-structural-facts-design.md`
- Modify: `TODO.md`

**What to build:** Update durable docs so future agents understand that this is a framework-fact extension to `structural_facts`, not a bridge/search feature.

**Approach:** Append a concise extension section to the structural-facts design doc listing the three new pattern ids, metadata contracts, and out-of-scope downstream linking. Update `TODO.md` from open to done only after focused, capability, default, and contract gates pass. Do not create release-evidence docs unless a release plan asks for release evidence.

**Acceptance criteria:**

- [ ] Docs describe the facts/linking boundary in the same terms as this plan.
- [ ] `cargo xtask test default` passes.
- [ ] `cargo xtask test contract` passes.
- [ ] Verification ledger records commands, commit SHA, result, and timestamp.

## Out Of Scope

- Miller bridge provider implementation.
- Linking `hx-get="/todos"` to `MapGet("/todos", ...)`.
- ASP.NET MVC controllers, Razor Pages handlers, route groups, `MapMethods`, endpoint filters, route constants, or dynamic route evaluation.
- Alpine runtime semantics, expression parsing, dependency tracking, or component state modeling.
- Generic framework/plugin architecture beyond the internal collector needed for these three pattern ids.
- New SQLite tables, JSONL record kinds, report row domains, or schema version changes.
