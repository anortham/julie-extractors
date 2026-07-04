# GLM Review Findings Remediation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use razorback:subagent-driven-development when subagent delegation is available. Fall back to razorback:executing-plans for single-task, tightly-sequential, or no-delegation runs.

**Goal:** Fix the confirmed findings from `docs/reviews/2026-07-04-glm-review.md` across structural-fact extraction, contract honesty, and artifact writer/export scale behavior. Downgraded or unconfirmed claims become regression locks, not speculative rewrites.

**Architecture:** Keep the work inside the extraction product boundary: source tree -> versioned extraction artifact. Server/client route facts stay in `base/framework_structural_facts/`, web route/reference facts stay in `base/web_structural_facts/`, data/SQL/domain facts stay in the language collectors, and SQLite/JSONL performance work stays in `julie-extract-artifact`. The caller-facing surface is still the `julie-extract` CLI plus SQLite/JSONL rows; no MCP, daemon, search, watcher, dashboard, or editing behavior is added.

**Tech Stack:** Rust, tree-sitter, rusqlite, SQLite, JSONL export, golden fixtures, focused Rust tests, `node scripts/language-data-quality-report.mjs --strict`.

**Architecture Quality:** Risk is high because this touches shared extraction contracts and high-volume writer paths. The plan favors local collector fixes, shared helper reuse, and caller-facing artifact tests over private-only tests. Add new abstraction only where it removes duplicated silence logic or repeated writer work. Rejected shortcuts: guessed dynamic routes, `INSERT OR IGNORE` as a blanket data-loss mask, capability claims without fixture evidence, and broad schema/contract changes where a local collector fix is enough.

## Global Constraints

- `julie-extractors` owns extraction only. Do not add service, MCP, daemon, search, embedding, watcher, dashboard, or editing-tool behavior.
- SQLite is the primary durable output; JSONL is the secondary export/streaming output.
- `julie-extract` is the primary integration surface. Verify behavior through emitted SQLite/JSONL artifact rows when the bug is caller-visible.
- Silence doctrine: dynamic or ambiguous route/URL expressions emit nothing. A false positive is worse than a miss.
- `normalized_route_template` remains the server-side join key. Do not create another join key.
- Capability claims must be backed by golden fixture evidence and recorded in `fixtures/extraction/capabilities.json`.
- Unsupported or not-yet-implemented constructs must be represented as `open_gaps` with reason, required closure, and planned closure task.
- After fixture or capability changes, run `node scripts/language-data-quality-report.mjs --strict`; `silent_cells` and `quality_bar_debts` must remain `0`.
- Default tests must stay fast. Add focused tests for each language or subsystem instead of broad corpus gates.
- Regenerate contract JSON only when emitted pattern ids or metadata contracts change, and commit the regenerated artifact in the same slice.
- Do not alter `AGENTS.md` or `CLAUDE.md` in this plan. If later work touches them, run `scripts/check-agent-doc-sync.sh`.

## Scope and Triage

**Fix by implementation:**

- P0/P1 silence and contract defects: ASP.NET, Spring, Java/Ruby clients, Django `re_path`, Vue script scanning.
- Confirmed route extraction gaps: Express middleware labeling, Spring adjacency/prefix loss, Rails parenthesized/multi-resource/member/collection routes, Actix direct routes, Go `var mux`, Razor normalized routes.
- Confirmed web/data/SQL correctness gaps: React/Vue path false positives, JS string escapes, Next/Nuxt file-route bugs, SQL nullability/subquery/join/recursive flags, JSON/TOML paths, YAML flow collections, Markdown and CSS edge cases.
- Artifact writer/export scale risks that compound with the new fact volume.

**Fix by regression lock only unless a new failing test proves otherwise:**

- SQL trigger-name claim.
- `index: true` token-boundary claim.
- HTML commented htmx artifact claim.
- "Zero unit tests" claim.
- Current CLI duplicate-ID writer crash claim.

**Fix by implementation or honest `open_gaps`:**

- Large domain-support lists where implementing every construct in this remediation pass would become a new feature line: SQL DML/procedures/window/DDL variants, Markdown footnotes/reference links/task lists/definition lists/autolinks, JSON `$ref`/`$schema`/JSON5, TOML multi-line strings, Regex advanced constructs, CSS at-rules, HTML semantic/link/media details, Vue style CSS and `#` slot shorthand.

## Verification Strategy

**Project source of truth:** `AGENTS.md`, `fixtures/extraction/capabilities.json`, `docs/contracts/jsonl-v3.md`, `docs/contracts/sqlite-schema-v3.md`, `crates/julie-extractors/src/base/structural_fact_registry.rs`, and the annotated review doc.

**Worker red/green scope:** Each worker first adds or identifies a failing focused test for its finding. Use module filters such as:

- `cargo test -p julie-extractors spring`
- `cargo test -p julie-extractors http_client`
- `cargo test -p julie-extractors python_web`
- `cargo test -p julie-extractors vue`
- `cargo test -p julie-extractors react`
- `cargo test -p julie-extractors nuxt`
- `cargo test -p julie-extractors sql`
- `cargo test -p julie-extractors yaml`
- `cargo test -p julie-extractors json`
- `cargo test -p julie-extractors toml`
- `cargo test -p julie-extract-artifact writer`
- `cargo test -p julie-extract-artifact jsonl`

**Worker ceiling:** Workers own only their focused module tests, fixture regeneration for their slice, and narrow CLI smoke tests where the bug exists only at artifact boundaries. Workers do not run the full branch gate unless asked by the lead.

**Worker gate invariant:** Each confirmed bug must have one positive test and one negative/silence test where applicable. Tests must assert emitted `structural_facts` rows, not just private helper behavior, unless the helper is the risk itself.

**Lead affected-change scope:** After each batch, run the relevant package tests plus `node scripts/language-data-quality-report.mjs --strict` if any fixture/capability changed. If a task changes contract docs or registry specs, run the structural-fact registry test with regeneration as documented in the repo.

**Branch gate:** Before claiming the remediation branch is complete:

```bash
cargo fmt --all -- --check
cargo test -p julie-extractors
cargo test -p julie-extract-artifact
cargo test -p julie-extract-cli
cargo clippy -p julie-extractors --all-targets -- -D warnings
cargo clippy -p julie-extract-artifact --all-targets -- -D warnings
cargo clippy -p julie-extract-cli --all-targets -- -D warnings
cargo build --bin julie-extract
node scripts/language-data-quality-report.mjs --strict
```

**Replay evidence:** Keep at least one CLI smoke fixture for the high-risk boundaries: dynamic route silence, Vue SFC script false positives, Django `re_path` normalization, duplicate structural-fact ID safety, and WAL checkpoint/export behavior.

**Escalation triggers:** Stop for user input only if a fix requires a schema migration, a product decision about intentionally missing Next pages-router files, accepting slower default tests, changing public CLI exit/status behavior, or weakening the documented data-quality bar.

## Parallel Execution Contract

Commit mode: **mixed serial/parallel**. Shared contract files (`fixtures/extraction/capabilities.json`, contract JSON, `structural_fact_registry.rs`) are serialized. Independent implementation modules can be worked in parallel before the contract sweep.

| Task | Parallel batch | File ownership | Serialization required | Dependency reason |
|---|---|---|---|---|
| Task 0: Regression locks and validation harness | Serial | `crates/julie-extractors/src/tests/review_regressions.rs`, focused fixture additions only | Yes | Establishes which downgraded claims must not become speculative fixes. |
| Task 1: P0 silence and route normalization | Batch A after Task 0 | `framework_structural_facts/static_arg.rs`, `aspnet.rs`, `spring.rs`, `python_web.rs`, `http_clients/java.rs`, `http_clients/ruby.rs`, focused tests | Yes with Task 4 | Shares framework collector helpers and silence policy. |
| Task 2: Vue SFC script silence | Batch A after Task 0 | `web_structural_facts/vue.rs`, `http_client.rs`, Vue tests/fixtures | Yes with Task 3 | Shares web scanners and route-object helpers. |
| Task 3: Frontend route/file precision | Batch B after Task 2 | `web_structural_facts/react.rs`, `nextjs_nuxt.rs`, `js_object_scan.rs`, `jsx_scan.rs`, `js_imports.rs`, `css.rs`, web tests/fixtures | Yes | Depends on the Vue/web helper decisions from Task 2. |
| Task 4: Backend framework coverage | Batch B after Task 1 | `framework_structural_facts/node.rs`, `spring.rs`, `rails.rs`, `actix.rs`, `go_http.rs`, `razor.rs`, focused tests/fixtures | Yes | Shares framework modules with Task 1. |
| Task 5: Data/SQL/domain correctness | Batch A after Task 0 | `data_structural_facts.rs`, `sql_structural_facts.rs`, Markdown/CSS/HTML tests/fixtures | No, except global contract files | Separate collectors. |
| Task 6: Artifact writer/export hardening | Batch A after Task 0 | `crates/julie-extract-artifact/src/{writer.rs,schema.rs,jsonl.rs,writer/rows.rs}`, `crates/julie-extract-cli/src/extraction.rs`, artifact/CLI tests | No, except final gate | Separate crate boundary. |
| Task 7: Capability honesty and registry contracts | Serial after Tasks 1-6 | `fixtures/extraction/capabilities.json`, registry specs, contract docs, regenerated JSON | Yes | Centralizes global contract churn. |
| Task 8: End-to-end verification and closeout | Serial final | Review doc, plan checklist, verification ledger | Yes | Depends on all fixes landing. |

## Task 0: Regression Locks and Validation Harness

**Files:**

- Modify: `crates/julie-extractors/src/tests/review_regressions.rs`
- Optionally add small fixtures under `fixtures/extraction/**/review_regressions/` only when a golden fixture is the cleanest caller-facing proof.

**What to build:** Focused tests that lock the validation decisions before implementation starts.

**Approach:**

- Add a SQL trigger regression proving `trigger_name` and `target_table` are distinct and correct.
- Add a React/JSX regression proving `index: true` does not match without a token boundary.
- Add an HTML artifact regression proving commented htmx attributes do not emit from current artifact output.
- Add a CLI or artifact-mapper regression proving duplicate structural fact IDs do not crash the current CLI path.
- Add comments that these are validation locks, not evidence that broader related gaps are fixed.

**Acceptance criteria:**

- [ ] Downgraded claims are covered by passing tests.
- [ ] No production code changes are made in this task unless a validation lock unexpectedly fails.
- [ ] `cargo test -p julie-extractors review_regressions` passes.

## Task 1: P0 Silence and Route Normalization

**Files:**

- Modify: `crates/julie-extractors/src/base/framework_structural_facts/static_arg.rs`
- Modify: `crates/julie-extractors/src/base/framework_structural_facts/aspnet.rs`
- Modify: `crates/julie-extractors/src/base/framework_structural_facts/spring.rs`
- Modify: `crates/julie-extractors/src/base/framework_structural_facts/python_web.rs`
- Modify: `crates/julie-extractors/src/base/framework_structural_facts/http_clients/java.rs`
- Modify: `crates/julie-extractors/src/base/framework_structural_facts/http_clients/ruby.rs`
- Test: `crates/julie-extractors/src/tests/{csharp,spring,python_web,http_client,java,ruby}/`

**Interfaces:**

- Consumes whole AST argument nodes for route and URL literals.
- Produces the existing framework and `http.client_request.v1` facts only when the whole argument is static.
- Produces Django `re_path` facts with `normalized_route_template`.

**What to build:**

- Add or complete `StaticArgLang::Java`, `StaticArgLang::CSharp`, and `StaticArgLang::Ruby` arms.
- Route ASP.NET, Java Spring, Java HTTP client, and Ruby HTTP client extraction through whole-argument static checks.
- In mapping/array annotation arms, emit static siblings and silently skip dynamic elements.
- Normalize Django `re_path` regex groups into the existing `normalized_route_template` key.

**Approach:**

- Start with failing tests for `"/api/" + name`, Java annotation arrays with mixed static/dynamic values, Java `URI.create("https://api.com" + path)`, Ruby interpolation through `Net::HTTP`, and Django named regex groups.
- Reject wrapper nodes such as binary expressions, interpolation, identifiers, constants, member/subscript access, and method calls.
- Keep static literal handling language-specific inside `static_arg.rs`, not duplicated inside each collector.
- For Django, support common named group forms such as `(?P<id>...)`; if a regex cannot be normalized honestly, emit the route fact without a normalized join key only if the pattern is recorded as an `open_gaps` limitation in Task 7.

**Acceptance criteria:**

- [ ] Dynamic ASP.NET and Java Spring route arguments emit no guessed partial routes.
- [ ] Static siblings in Java mapping arrays still emit.
- [ ] Java and Ruby dynamic client URLs emit nothing; static URLs still emit.
- [ ] Django `re_path` emits `normalized_route_template` for named regex parameters.
- [ ] Focused tests pass for `spring`, `python_web`, `http_client`, and affected language modules.

## Task 2: Vue SFC Script Silence

**Files:**

- Modify: `crates/julie-extractors/src/base/web_structural_facts/vue.rs`
- Modify: `crates/julie-extractors/src/base/web_structural_facts/http_client.rs`
- Test: `crates/julie-extractors/src/tests/vue/structural_facts.rs`
- Fixtures: `fixtures/extraction/vue/**`

**Interfaces:**

- Consumes Vue SFC `<script>` and `<script setup>` sections.
- Produces existing Vue route and web HTTP-client facts only from executable code, not comments or string literals.

**What to build:**

- Add a section-aware JS/TS syntax mask for each Vue script block, or parse the script section with the JS/TS grammar and call the same comment/string exclusion used for plain JS/TS.
- Apply that mask before fetch/axios, route-object, and `path:` scans inside Vue scripts.
- Preserve template scanning behavior while adding negative tests for commented and string-embedded calls.

**Approach:**

- Keep the byte offsets in the original `.vue` file. If parsing section text separately, translate section-local ranges back to file byte ranges.
- Treat unknown script `lang` values as silence for JS-only facts unless the existing collector already supports them.
- Add tests for `<script>`, `<script setup>`, `lang="ts"`, comments, string literals, and real fetch/route calls.

**Acceptance criteria:**

- [ ] Commented-out `fetch`, `axios.get`, and `path:` in Vue scripts emit no facts.
- [ ] String-embedded calls emit no facts.
- [ ] Real static calls still emit with original file offsets.
- [ ] `cargo test -p julie-extractors vue` passes.

## Task 3: Frontend Route and File Precision

**Files:**

- Modify: `crates/julie-extractors/src/base/web_structural_facts/react.rs`
- Modify: `crates/julie-extractors/src/base/web_structural_facts/vue.rs`
- Modify: `crates/julie-extractors/src/base/web_structural_facts/nextjs_nuxt.rs`
- Modify: `crates/julie-extractors/src/base/web_structural_facts/js_object_scan.rs`
- Modify: `crates/julie-extractors/src/base/web_structural_facts/jsx_scan.rs`
- Modify: `crates/julie-extractors/src/base/web_structural_facts/js_imports.rs`
- Modify: `crates/julie-extractors/src/base/web_structural_facts/css.rs`
- Test: `crates/julie-extractors/src/tests/{react,vue,nuxt,javascript,typescript,css}/`

**Interfaces:**

- Produces existing React/Vue/Next/Nuxt route facts with corrected emission gates and metadata.
- Produces parsed string values that preserve valid JS escapes.

**What to build:**

- Restrict route-object `path:` extraction to known route contexts, not nested `redirect` or `meta` objects.
- Fix `parse_js_string_literal` so escapes are decoded correctly or rejected consistently.
- Reconcile Next.js path classification so `pages/app/page.tsx` is not treated as app-router root.
- Decide and implement the Next pages-router evidence rule: either add a safe project/file signal that covers plain `export default function Home()` pages, or record signal-free pages-router files as an `open_gaps` item in Task 7 rather than reopening known React-SPA false positives.
- Fix JSX nested `<Route>` effective parent path metadata.
- Fix NuxtLink `:to` and relative route handling where it can be resolved statically.
- Fix CSS selector splitting so commas inside attribute selectors do not change `css_selector_kind`.
- Lock `import { type Route }` as type-only, not value import evidence.

**Approach:**

- Prefer parser- or context-aware route-object walking over broad string search.
- Use the same route tree parent stack for React object routes, JSX routes, and Vue route children where possible.
- Treat Next pages-router as an explicit product decision: no guessed file routes without evidence unless project-level evidence is implemented.
- Add paired false-positive and positive tests for every gate.

**Acceptance criteria:**

- [ ] `redirect: { path: ... }` and `meta: { path: ... }` do not emit route definitions.
- [ ] JS escape tests pass for newline, tab, unicode, escaped slash, and rejected malformed escapes.
- [ ] Next/Nuxt file-route cases match documented evidence policy.
- [ ] JSX nested routes and Nuxt relative links emit parent/effective route metadata when statically known.
- [ ] CSS attribute selector commas are classified correctly.
- [ ] Focused web tests pass.

## Task 4: Backend Framework Coverage and Precision

**Files:**

- Modify: `crates/julie-extractors/src/base/framework_structural_facts/node.rs`
- Modify: `crates/julie-extractors/src/base/framework_structural_facts/spring.rs`
- Modify: `crates/julie-extractors/src/base/framework_structural_facts/rails.rs`
- Modify: `crates/julie-extractors/src/base/framework_structural_facts/actix.rs`
- Modify: `crates/julie-extractors/src/base/framework_structural_facts/go_http.rs`
- Modify: `crates/julie-extractors/src/base/framework_structural_facts/razor.rs`
- Test: `crates/julie-extractors/src/tests/{express,spring,rails_routes,actix,go_http,razor}/`

**Interfaces:**

- Produces existing backend framework route facts with corrected labels, prefixes, and normalized templates.

**What to build:**

- Express: distinguish middleware-only `app.use('/api', middleware)` from router mounts.
- Spring: handle comments between annotations and methods; stop method-body string tokens from poisoning class prefix detection.
- Rails: support parenthesized `get('/x')` and `post('/x')`; emit all `resources :users, :posts`; preserve prefixes inside `member` and `collection` blocks.
- Actix: emit direct `App::new().route("/health", web::get().to(h))`.
- Go: track `var mux = http.NewServeMux()` receivers.
- Razor: add `normalized_route_template` for `@page "/users/{id}"`.
- HTTP clients: review `csharp`, `kotlin`, and `php` receiver proof and either add local proof or record the limitation as `open_gaps`.

**Approach:**

- Use AST nodes where available; avoid adding more line-token heuristics to Spring/Rails.
- Keep prefix resolution same-file and static-only.
- For Express, only emit router mount facts when the mounted value is proven to be a router; otherwise emit middleware evidence only if an existing pattern supports it.

**Acceptance criteria:**

- [ ] Express middleware no longer emits `express.router_mount.v1` unless the target is a router.
- [ ] Spring annotation adjacency and class-prefix tests pass.
- [ ] Rails parenthesized, multi-resource, member, and collection tests pass.
- [ ] Actix direct `App::new().route(...)` emits route facts.
- [ ] Go `var mux` receiver route emits.
- [ ] Razor route facts include `normalized_route_template`.
- [ ] Focused framework tests pass.

## Task 5: Data, SQL, and Domain Structural Facts

**Files:**

- Modify: `crates/julie-extractors/src/base/data_structural_facts.rs`
- Modify: `crates/julie-extractors/src/base/sql_structural_facts.rs`
- Modify as needed: `crates/julie-extractors/src/base/web_structural_facts/{css.rs,html.rs}`
- Test: `crates/julie-extractors/src/tests/{sql,yaml,json,toml,markdown,css,html,regex}/`
- Fixtures: `fixtures/extraction/{sql,yaml,json,toml,markdown,css,html,regex}/**`

**Interfaces:**

- Produces existing data/domain structural facts with corrected metadata values and paths.
- Adds new facts only when the contract and registry are updated in Task 7.

**What to build:**

- SQL: `INTEGER PRIMARY KEY` implies not nullable; subquery flags/counts are local to the subquery; chained joins have the correct left/right tables; `with recursive` detection is case-insensitive; trigger-name regression stays green.
- YAML: emit mapping/sequence/key_value facts for flow collections; either implement block scalar/tag/multi-doc path semantics or record them as honest `open_gaps`.
- JSON: make depth match the documented definition or revise the contract; include array indices in paths if the contract expects unique JSON paths; handle `$ref`/`$schema` or record `open_gaps`.
- TOML: preserve parent paths for arrays of inline tables, dotted keys, and multi-line string limitations.
- Markdown: suppress inline-link false positives inside inline code, handle nested bracket cases, fix setext heading level and frontmatter key counting.
- CSS/HTML/Regex: fix confirmed selector/comment behavior; record broader unimplemented constructs as `open_gaps` unless implemented.

**Approach:**

- For each language, add compact unit tests in the existing language test module before changing the collector.
- Keep metadata names stable where possible. If a value definition changes, update registry docs and fixture expected output in the same task.
- Do not mark a missing implementation as `not_applicable`.

**Acceptance criteria:**

- [ ] SQL P1/P2 value tests pass.
- [ ] YAML flow collection facts emit useful paths and kinds.
- [ ] JSON/TOML path tests prove unique, documented paths.
- [ ] Markdown/CSS focused edge tests pass.
- [ ] Any still-missing domain constructs are represented by honest `open_gaps` in Task 7.

## Task 6: Artifact Writer and JSONL Export Hardening

**Files:**

- Modify: `crates/julie-extractors/src/base/mod.rs` if extractor-level dedupe is chosen.
- Modify: `crates/julie-extract-cli/src/extraction.rs` for CLI artifact-mapper regression tests if needed.
- Modify: `crates/julie-extract-artifact/src/writer.rs`
- Modify: `crates/julie-extract-artifact/src/writer/rows.rs`
- Modify: `crates/julie-extract-artifact/src/schema.rs`
- Modify: `crates/julie-extract-artifact/src/jsonl.rs`
- Test: artifact writer/export tests and CLI smoke tests.

**Interfaces:**

- Preserves the SQLite and JSONL contract unless a documented index/order change is explicitly accepted.
- Improves writer safety and scale without changing extraction semantics.

**What to build:**

- Add a lower-level duplicate structural-fact ID safety net below the existing CLI dedupe. Prefer deterministic dedupe before writer insert; do not use blanket `INSERT OR IGNORE` unless tests prove it cannot hide distinct facts with the same ID.
- Run `PRAGMA wal_checkpoint(TRUNCATE)` after a successful write transaction, with tests proving the WAL sidecar is bounded or empty after close.
- Merge spooled insert passes where safe so per-file JSON is not deserialized three times.
- Avoid JSONL metadata parse/serialize churn by emitting stored JSON metadata raw while preserving valid JSON objects.
- Add or adjust export-order covering indexes only if they match the documented export order.
- Build secondary indexes after fresh/`--force` bulk inserts if the current schema path can do so without breaking incremental scans.
- Replace child-table delete fan-out with a single FK-backed `DELETE FROM files` where tests prove cascade coverage.
- Avoid full existing-file table scans and unconditional guard counts for no-op/incremental scans.
- Batch high-cardinality inserts or temp-table symbol lookup loads where the change is local and measurable.
- Remove or justify the two non-test `.expect()` calls in writer paths.

**Approach:**

- Split this task internally into safety, WAL, export, bulk-load, incremental-delete, and batching subcommits.
- Use current artifact tests plus a small synthetic writer fixture. Record row counts and `EXPLAIN QUERY PLAN` evidence for index/export changes.
- Keep correctness first: performance changes must preserve row counts and JSONL equivalence.

**Acceptance criteria:**

- [ ] Duplicate structural-fact IDs cannot crash the lower-level writer path under the chosen policy.
- [ ] Successful writes checkpoint/truncate WAL without losing committed rows.
- [ ] JSONL export output is byte-valid and semantically unchanged except for documented ordering/index policy.
- [ ] Fresh and incremental writer tests pass.
- [ ] Performance changes have focused evidence, not just code inspection.

## Task 7: Capability Honesty, Registry, and Contract Sweep

**Files:**

- Modify: `fixtures/extraction/capabilities.json`
- Modify: `crates/julie-extractors/src/base/structural_fact_registry.rs`
- Modify: `docs/contracts/jsonl-v3.md`
- Modify: `docs/contracts/sqlite-schema-v3.md`
- Regenerate if needed: `docs/contracts/structural-fact-patterns.json`
- Update fixture expected output under `fixtures/extraction/**/expected.json`

**What to build:** A single serialized contract sweep after implementation batches land.

**Approach:**

- For each language touched in Tasks 1-5, compare actual support against capability rows.
- Add `open_gaps` entries for every real missing construct not implemented in this pass. Each entry must include concrete reason, required closure, and planned closure task.
- Register any new or changed metadata keys in `structural_fact_registry.rs`.
- Regenerate contract JSON only after the final emitted pattern set is stable.
- Run the strict language data quality report and read its output, not just the exit code.

**Acceptance criteria:**

- [ ] No known missing implementation is represented as `open_gaps: []` or `not_applicable`.
- [ ] Registry specs match emitted metadata keys and types.
- [ ] Contract docs match SQLite/JSONL output.
- [ ] `node scripts/language-data-quality-report.mjs --strict` reports `silent_cells: 0` and `quality_bar_debts: 0`.

## Task 8: End-to-End Verification and Closeout

**Files:**

- Update: `docs/reviews/2026-07-04-glm-review.md` to mark fixed/deferred findings after implementation.
- Update: this plan's acceptance checklist if the repo convention at execution time tracks completion inline.

**What to build:** Final evidence that the review findings are either fixed, regression-locked, or honestly recorded as open gaps.

**Approach:**

- Run the branch gate from the verification strategy.
- Build the CLI and run a small temp-repo smoke scan covering:
  - dynamic route silence in ASP.NET/Spring/Java/Ruby
  - Vue script comments/strings
  - Django `re_path` normalization
  - representative data/SQL fixes
  - artifact writer WAL/export behavior
- Query SQLite `structural_facts` directly for high-risk facts.
- Re-check worktree state and make sure no unrelated user changes were touched.

**Acceptance criteria:**

- [ ] Every confirmed finding in the review is fixed or has an explicit `open_gaps` entry with closure plan.
- [ ] Every downgraded/unconfirmed claim has a regression lock or documented non-action.
- [ ] Branch gate passes or any failure is fully explained with the smallest remaining blocker.
- [ ] Final report lists changed modules, verification commands, and remaining open gaps.
