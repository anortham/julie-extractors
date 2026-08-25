# TODO

Lightweight tracker for open, agreed-but-not-yet-done work on the extraction
product. One section per item, each with a concrete file reference, why it
matters, and the proposed fix. No "later" placeholders.

Status legend: `open` (verified present), `partial` (partly done), `idea`
(proposed, not committed to), `done` (implemented and verified).

---

## 1. No `cargo-deny` supply-chain / license / advisory gate — done

- **Where:** `deny.toml`; `.github/workflows/ci.yml`.
- **What changed:** Added a cargo-deny policy covering advisories, SPDX license
  allow-listing, duplicate/wildcard warnings, exact git-source allow-listing,
  and an explicit compatibility decision for `md5@0.7.0`.
- **Verification:** `cargo deny check` passes locally. Duplicate versions and
  path/git wildcard requirements are warnings; advisories, license policy, and
  unknown dependency sources are hard gates. CI runs
  `EmbarkStudios/cargo-deny-action@v2` with `--all-features`.

## 2. Evaluate migrating standalone `md5` 0.7 to RustCrypto `md-5` — done

- **Where:** `crates/julie-extractors/Cargo.toml:74` (`md5 = "0.7"`); usages in
  production ID/hash paths:
  `crates/julie-extractors/src/base/{extractor.rs,types.rs,body.rs,results_normalization.rs}`;
  expected-value helpers also use it under `crates/julie-extractors/src/tests/`.
- **Decision:** Keep `md5@0.7.0` as an explicitly allowed compatibility
  dependency. The MD5 output is part of stable legacy extraction IDs and body
  hashes, so changing crates would be an artifact identity migration rather than
  a supply-chain cleanup.
- **Verification:** `deny.toml` explicitly allows only `md5@0.7.0` with the
  compatibility reason above; strict workspace clippy and the path-identity /
  contract gates pass locally.

## 3. Legacy `julie-extractors` clippy warnings not gated (residual of F20) — done

- **Where:** `.github/workflows/ci.yml`.
- **What changed:** CI now runs
  `cargo clippy --workspace --all-targets --all-features --no-deps -- -D warnings`,
  covering the core extractor crate and test targets.
- **Verification:** strict workspace clippy passes locally.

## 4. Execute the 2026-06-05 project hardening review plan — done

- **Where:** `docs/plans/2026-06-05-project-hardening-review.md`.
- **What changed:** F1-F19 are implemented and marked fixed in the plan.
  Regression coverage was added for artifact correctness, JSONL atomicity,
  parser provenance, language detection, release preflight, default-suite
  tripwires, and workflow guardrails.
- **Verification:** `cargo xtask test default`, `cargo xtask test contract`,
  `cargo test -p xtask`, and strict workspace clippy pass locally.

## 5. TypeScript generic client-call URL literals are missing — done

- **Where:** `crates/julie-extractors/src/typescript/identifiers.rs`;
  `crates/julie-extractors/src/tests/typescript/literals.rs`;
  `crates/julie-extract-cli/tests/operations_contract.rs`.
- **Root cause:** For `await axios.get<T>(...)`, tree-sitter TypeScript reports
  the call callee through an awaited node whose fallback text was
  `await axios.get`. The literal was captured, but carrier classification only
  recognizes `axios.get`, so the language-policy pass dropped the row before it
  reached SQLite/JSONL.
- **What changed:** TypeScript callee carrier normalization now strips the
  `await ` prefix before carrier classification. Regression coverage verifies
  raw extractor output and the real CLI SQLite `literals` table for generic
  `axios.get<T>` and `axios.put<T>` calls.
- **Verification:** Focused raw TypeScript literal test passes; focused CLI
  operations contract passes; a MyraNext services smoke scan now persists
  `/api/messages/active` with `kind = url`, `carrier = axios.get`, and
  containing symbol `getActiveMessages`.

## 6. OpenClaw-scale SQLite writer perf guard for current schema — done

- **Where:** `xtask/src/performance.rs`; `xtask/Cargo.toml`;
  `xtask/tests/performance_baseline_contract.rs`;
  `xtask/tests/commands_contract.rs`; `docs/testing-strategy.md`.
- **What changed:** Added a non-default
  `cargo xtask performance writer-current-schema` command that generates a
  deterministic current-schema artifact workload, writes it through the real
  `ArtifactWriter`, and records `artifact.sqlite` plus
  `writer-current-schema-summary.json` under `target/`.
- **Coverage:** The generated workload includes files, symbols,
  symbol annotations, identifiers, relationships, pending relationships,
  type facts, type argument usages, type arguments, literals, source regions,
  structural facts, complexity metrics, parse diagnostics, and revision file
  changes. The default run wrote 10,000 files, 80,000 symbols, 240,000
  identifiers, 120,000 source regions, 10,000 structural facts, 90,000
  complexity metrics, and a 270,106,624-byte SQLite artifact. Elapsed write
  time, rows/sec, and artifact size are report-only metrics, not CI thresholds.
- **Verification:** Focused parser/runner tests and top-level routing tests pass.
  The manual evidence run completed:
  `cargo xtask performance writer-current-schema --out-dir target/performance/writer-current-schema`.
  Branch-gate verification passes with `cargo test -p xtask`,
  `cargo xtask test default`, and `cargo xtask test contract`.

## 7. Structural tree-sitter query facts for downstream tools — complete

- **Where:** New contract/design under `docs/contracts/` and/or `docs/plans/`;
  likely extractor surfaces under `crates/julie-extractors/src/base/`,
  `crates/julie-extractors/src/language_spec/`, and per-language modules;
  artifact surfaces under `crates/julie-extract-artifact/src/{schema.rs,writer.rs,jsonl.rs,model.rs}`.
- **Done:** The current v3 artifact has a `structural_facts` SQLite table,
  `structural_fact` JSONL records, report row counts, writer coverage, CLI scan
  coverage, and current-schema performance coverage. The extractor now emits a
  representative fixture-backed pattern set: Rust unsafe blocks, Go goroutine
  launches and defer statements, Python decorated definitions,
  JavaScript/JSX/TypeScript/TSX await expressions, and C/C++ preprocessor
  definitions.
- **Capability metadata:** `fixtures/extraction/capabilities.json`, Rust
  capability snapshot APIs, persisted SQLite `language_capabilities`, and
  `julie-extract languages --json` now publish exact
  `kind_coverage.structural_facts.supported` pattern ids.
- **Guardrail:** Do not add a generic downstream search engine or a Miller/Eros
  query DSL here. This repo should emit versioned extraction facts. Interactive
  querying, ranking, dashboards, and commercial workflows belong downstream.
- **Verification:** Contract tests prove SQLite/JSONL/report shape, writer tests
  prove row persistence and counts, extractor tests prove every advertised
  structural pattern, capability-matrix tests prove fixture-backed evidence,
  the CLI operations contract proves non-empty scan output and metadata
  publication, and `cargo xtask performance writer-current-schema` exercises
  the row family.

## 8. Cross-language AST/code complexity metrics — complete

- **Done:** The current v3 artifact has a `complexity_metrics` SQLite table,
  `complexity_metric` JSONL records, report row counts, writer coverage, CLI
  scan persistence, current-schema writer guard coverage, and contract docs.
- **Done:** Extractors now emit primitive parser-backed metrics with
  `algorithm_id = julie-ast-complexity-v1`: `file` and `symbol` scope,
  covered lines/bytes, decision count, loop count, max nesting depth, and
  parameter count where applicable.
- **Done:** The first fixture-backed matrix covers `c`, `cpp`, `go`,
  `javascript`, `python`, `rust`, and `typescript`. Capability evidence
  advertises supported scopes through
  `kind_coverage.complexity_metrics.supported`; unsupported languages publish
  no metric-scope claims.
- **Guardrail kept:** The extractor emits facts only. It does not emit a single
  opaque quality score, severity, ranking, threshold, or dashboard decision.
- **Evidence:** `docs/release-evidence/2026-06-09-complexity-metrics-dogfood.md`
  records a real-repo scan with 6943 `complexity_metrics` rows and counts by
  language and metric scope. Focused extractor, artifact, CLI, capability, and
  xtask tests pass for this slice.

## 9. Clone-ready body fingerprints beyond current `body_hash` — done

- **Where:** Existing normalized body hash logic in
  `crates/julie-extractors/src/base/body.rs`; symbol model fields in
  `crates/julie-extractors/src/base/types.rs` and
  `crates/julie-extract-artifact/src/model.rs`; artifact writers in
  `crates/julie-extract-artifact/src/{schema.rs,writer.rs,jsonl.rs}`;
  docs in `docs/contracts/{sqlite-schema-v3.md,jsonl-v3.md}`.
- **Finding:** Symbols already expose `body_hash` when body spans are available.
  That hash is useful for exact normalized-body matches. The exact-hash contract
  now has a documented algorithm id, normalization rules, and guardrails in the
  SQLite/JSONL contracts.
- **Why it matters:** Miller can consume exact duplicate facts cheaply, and Eros
  can build higher-level clone/risk workflows, but only if the extractor emits
  stable, documented fingerprints instead of forcing downstream tools to
  re-tokenize source.
- **Completed slice:** Kept the existing `body_hash` field, defined algorithm
  id `julie-normalized-body-md5-v1`, made normalization ignore whitespace and
  language-appropriate comments, and added tests for exact-match stability and
  contract wording.
- **Decision:** Do not add a separate machine-readable fingerprint surface in
  the current v3 artifact. Token counts, SimHash, token n-grams, and other
  near-duplicate candidate signals need a named downstream requirement before
  they become public SQLite/JSONL contract surfaces.
- **Decision record:** `docs/decisions/0002-clone-fingerprint-scope.md`
  captures the clone-fingerprint scope boundary and future-entry criteria.
- **Guardrail:** Do not make the extractor decide product-level duplicate
  severity. Emit stable exact fingerprints; Miller/Eros can group, rank,
  threshold, and present duplicates downstream.
- **Verification:** Focused tests prove whitespace/comment-stable exact hashes,
  intentional non-matches when executable tokens differ, quoted string
  preservation, and SQLite/JSONL v3 contract stability. The remaining deferred
  surface was closed by Decision 0002, not by adding speculative artifact rows.

## 10. ASP.NET minimal API, htmx, and Alpine structural facts — done

- **Where:** `docs/plans/2026-06-09-aspnet-htmx-alpine-structural-facts.md`;
  planned extractor surfaces under
  `crates/julie-extractors/src/base/framework_structural_facts.rs`,
  `crates/julie-extractors/src/registry.rs`, C#/HTML/Razor fixture directories,
  and `fixtures/extraction/capabilities.json`.
- **Why it matters:** C#, Razor, HTML, and JavaScript parser support already
  covers the syntax, but downstream tools need durable framework facts for
  minimal API routes, htmx request attributes, and Alpine directives before
  Miller can reliably index and bridge this stack.
- **Completed slice:** Emits `aspnet.minimal_api.route.v1`,
  `htmx.attribute.v1`, and `alpine.directive.v1` through the existing
  `structural_facts` row family with fixture-backed capability metadata. The
  extractor records static ASP.NET minimal API route templates, htmx attributes
  with request metadata where applicable, and Alpine long-form/shorthand
  directive metadata. htmx-to-ASP.NET route linking remains downstream in
  Miller, not in this repo.
- **Verification:** Focused structural-fact tests pass; `cargo xtask test
  language csharp`, `cargo xtask test language html`, and `cargo xtask test
  language razor` pass; capability-matrix structural checks pass; golden
  fixture checks pass; `languages --json` capability snapshot test passes; a
  real three-file CLI smoke scan writes `aspnet.minimal_api.route.v1`,
  `htmx.attribute.v1`, and `alpine.directive.v1` rows; `cargo xtask test
  default` and `cargo xtask test contract` pass.

## 11. Per-file extraction cost attribution in reports — complete

- **Where:** `crates/julie-extract-cli` (`info` command and scan report),
  `crates/julie-extract-artifact` report surfaces, `docs/contracts/cli.md`.
- **Why it matters:** The consumer-side vendor policy
  (`docs/decisions/2026-06-11-vendor-policy-consumer-side.md`) routes
  exclusions through `--ignore-file` and `.julieignore`, but consumers can
  only write good ignore rules if they can see which files dominate artifact
  rows. On openclaw, ten committed `*.tm.jsonl` i18n files produced ~39% of
  all structural facts — discoverable today only with ad-hoc SQL against
  schema internals. A stable report surface lets consumers such as Miller
  automate vendor detection by measured impact instead of name-pattern
  guesses, and makes artifact size regressions attributable.
- **Proposed fix:** Design pass first, then implement per-file row counts by
  row family as a report contract: a top-N offender summary in the scan
  report and/or a full per-file breakdown in `info --json`. Treat the JSON
  report shape as an API contract change with docs and focused tests. This is
  the named closure for the vendor-policy decision's follow-up debt.
- **Completed slice:** Added stable `counts.file_rows` entries with `path`,
  `language`, `status`, `total_rows`, and exhaustive per-row-family `rows`
  counts. Successful scan reports include a bounded largest-file summary with
  `counts.file_rows_truncated`; `info --json` includes the full persisted
  per-file breakdown. Attribution is computed as a read-side SQLite view over
  existing artifact tables, so the writer and SQLite schema stay unchanged.
- **Verification:** RED tests failed for the missing report type/field and
  missing `info --json` attribution before implementation. After implementation,
  `cargo test -p julie-extract-artifact --test report_contract`, `cargo test -p
  julie-extract-cli --test operations_contract`, `cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets --all-features --no-deps -- -D
  warnings`, `cargo xtask test contract`, and `cargo xtask test default` pass.

## 12. Depth-bounded tree traversal — complete

- **Where:** Shared traversal helpers in
  `crates/julie-extractors/src/base/tree_methods.rs`; parse-diagnostic walks in
  `crates/julie-extractors/src/pipeline.rs`; parser-comparison walks in
  `crates/julie-extractors/src/language_spec/mod.rs`; follow-up audit for raw
  per-language recursive walkers under `crates/julie-extractors/src/**`.
- **Why it matters:** Many extractor paths recurse through tree-sitter nodes.
  Rust stack overflow can abort the process, so CLI panic isolation is not a
  complete safety boundary for adversarially deep source trees. Shared helpers
  should enforce a fixed traversal depth budget before language-specific
  cleanup happens.
- **Completed slices:** Added a shared internal traversal depth budget, enforced
  it in the base traversal helpers, parse-diagnostic walks, and C/C++ header
  parser-comparison error walks. Focused tests prove `walk_tree`,
  `traverse_tree`, and `find_nodes_by_type` do not visit nodes beyond the
  budget. A follow-up slice guarded the direct recursive symbol walkers in
  Rust, Go, Java, C#, VB.NET, and C++; the Rust regression first reproduced a
  real stack overflow from a deeply nested `function_item`.
- **Completed follow-up:** The main per-language extraction phases now use the
  shared depth budget across direct symbol, identifier, relationship, pending
  relationship, and data-language relationship walks. This includes the
  JavaScript/TypeScript canonical passes, Dart/QML pending-call walks, Vue
  script/template relationship and symbol walks, HTML identifier/resource
  pending walks, YAML alias walks, Zig relationships, and the supported
  code/data language identifier and relationship visitors.
- **Completed helper pass:** Shared structural-fact, source-region, complexity,
  type-argument, string-literal, SQL/web/framework/data structural helpers, and
  remaining language helper recursion now route through the same traversal
  budget. A stricter recursive-function audit reports zero unguarded true
  child-recursive tree-sitter walkers.
- **Verification:** The deeply nested Rust and JavaScript regressions pass.
  `cargo xtask test language` passes for Rust, JavaScript, TypeScript, C, C++,
  C#, Go, Java, Kotlin, Scala, Swift, Python, Ruby, PHP, Dart, GDScript, QML, R,
  Lua, Elixir, Bash, PowerShell, SQL, JSON, TOML, YAML, Markdown, Regex, Razor,
  CSS, HTML, VB.NET, Zig, and Vue. `cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets --all-features --no-deps -- -D warnings`,
  and `cargo xtask test default` pass.

## 13. Split CLI command orchestration — complete

- **Where:** `crates/julie-extract-cli/src/commands.rs`.
- **Why it matters:** The command module is over 3,200 lines and `scan()`
  mixes path handling, existing artifact checks, discovery, extraction spooling,
  writing, report shaping, profiling, and exit-code mapping. That makes CLI
  contract changes harder to localize and review.
- **What changed:** Split stable helper families into focused internal modules:
  `capability_snapshot.rs` owns capability/parser fingerprint and snapshot
  mapping, `reports.rs` owns report/output/error mapping, and
  `artifact_access.rs` owns read-only artifact opening, metadata/version
  checks, root checks, content-hash loading, artifact report assembly, row
  totals, and JSONL count mapping. `commands.rs` now keeps command dispatch,
  high-level scan/update/delete orchestration, extraction spooling, and
  write-flow decisions.
- **Guardrail:** Added CLI convention tests that fail if capability snapshot,
  report/error, or artifact-access helpers drift back into `commands.rs`.
- **Verification:** Focused red/green convention tests passed after each move.
  `cargo test -p julie-extract-cli --test cli_contract`,
  `cargo test -p julie-extract-cli --test operations_contract`, and
  `cargo test -p julie-extract-cli --test path_policy` pass locally.
  `cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets --all-features --no-deps -- -D warnings`,
  and `cargo xtask test default` also pass.

## 14. Modularize artifact writer internals — complete

- **Where:** `crates/julie-extract-artifact/src/writer.rs`.
- **Why it matters:** `ArtifactWriter` earns its public interface, but the file
  combines capability snapshot sync, revision semantics, row-family insertion,
  deletion, prepared statements, and the data-loss guard. New row families have
  to touch several distant sections.
- **What changed:** Kept the public `ArtifactWriter` API and transaction
  orchestration in `writer.rs`, and moved stable private helper families into
  focused submodules. `writer/capabilities.rs` owns capability snapshot key
  loading, deletions, upserts, and JSON/boolean mapping. `writer/rows.rs` owns
  file/child row inserters, row-family insert functions, preserved-failure row
  updates, parse-diagnostic replacement, and symbol/identifier/type-argument
  lookup helpers.
- **Guardrail:** Added writer convention tests that fail if capability sync or
  row-inserter helper definitions drift back into `writer.rs`.
- **Verification:** Focused red/green convention tests passed. `cargo test -p
  julie-extract-artifact`, `cargo test -p julie-extract-artifact --test
  writer_performance`, `cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets --all-features --no-deps -- -D warnings`,
  and `cargo xtask test default` pass locally.

## 15. Reuse tree-sitter parsers per scan worker — complete

- **Where:** `crates/julie-extractors/src/pipeline.rs`.
- **Why it matters:** `parse_for_language` creates and configures a fresh
  `tree_sitter::Parser` for every file. That is a Rust-specific cold-scan cost
  on large repositories with many files of the same language.
- **Decision:** Do not add a thread-local parser cache now. The profile did not
  confirm meaningful scan-time savings, and a cache would add mutable
  thread-local state to a hot parser path for a very small measured win.
- **Evidence:** A focused local profiler compared 2,000 tiny Rust parses through
  the current `parse_for_language` path against one configured parser reused for
  all parses. Debug mode measured 41 ms vs. 38 ms; release mode measured 10 ms
  vs. 8 ms. That is at most a few milliseconds across 2,000 tiny files, so
  parser setup is not currently a top bottleneck.
- **Verification:** The profiling was run with:
  `cargo test -p julie-extractors pipeline::tests::parser_setup_profile_reports_reuse_baseline -- --ignored --nocapture`
  and
  `cargo test -p julie-extractors --release pipeline::tests::parser_setup_profile_reports_reuse_baseline -- --ignored --nocapture`.
  The temporary profiler/test scaffolding was removed after recording the
  evidence.

## 16. Identifier target resolution has no static-receiver path — partial

**Slices 1–3 shipped 2026-07-27** (`RESOLUTION_VERSION = 3` at ship). **Static-tier
multi-language certification shipped 2026-07-28** (`RESOLUTION_VERSION = 4`):
`TIER3_STATIC_TYPE_LANGUAGES` is now `csharp`, `typescript`, and `javascript`,
each with a `static_type_receiver` resolution_contract fixture. Slice 4 (C#
locals/params as symbols + real `infer_variable_type`) is still open and is the
only remaining slice that moves `variable_ref`.

Both columns below are C# on Miller's workspace from the v3 ship: "before" from
the prior artifact, "after" from a fresh full pass with the built binary.

| C# identifier kind | resolved before | resolved after | what produced the gain |
|---|---|---|---|
| `call` | 16,659 (19.4%) | **21,995 (25.7%)** | static tier 4,453 + tier 4 **+883** (slice 1 unblocked covered spans) |
| `member_access` | 96 (**0.2%**) | **5,399 (12.0%)** | static tier 5,300 + tier 3 **+4** (the `Constant`/`EnumMember` widening, on its own, is worth 4 sites) |
| `type_usage` | 10,132 (37.2%) | **10,631 (39.0%)** | tier 4 **+499** — **entirely slice 1**, zero static tier |
| `variable_ref` | 7,484 (4.9%) | 7,486 (4.9%) | noise; untouched, needs slice 4 |
| **overall** | 34,371 (11.1%) | **45,511 (14.7%)** | |

The gain is not uniformly slice 3: `tier3_static_type` contributes 9,752, and
closing the reporting leak contributes ~1,388 by itself, mostly constructors and
types whose covering pending edge had swallowed them. Identifiers carrying no
`identifier_resolutions` row went from 63,840 (C#; 69,173 all languages) to **0**.

Precision held on every case the reviews raised. `NullLoggerFactory.Instance`
binds only at the 6 sites inside its declaring file and refuses all 18 cross-file
ones; every external-receiver site stays unresolved — `Assert.Equal` (7,091),
`Assert.Contains` (2,859), `Path.Combine` (1,691), `Assert.Single` (736),
`File.WriteAllText` (416), `Directory.CreateDirectory` (390) all at zero; and
zero of the 9,752 static-tier targets is a non-static, non-enum, non-constant
member.

Slice 4 (C# locals/params as symbols, real `infer_variable_type`) **landed**
with RESOLUTION_VERSION 5 (later tightened to 6 for module-scope): locals/params are `SymbolKind::Variable` with
`metadata.role`, typed `variableType`, and type_facts; identifier `call` with a
receiver runs the tier-3 receiver path. Slice 5 stands: bare `Method`-at-tier-4
is rejected.

**Accepted debt carried by the static tier**, from the post-implementation review:

- *Public framework homonym.* The refusals cover nested-in-type and non-public
  types. A **public, top-level** workspace type whose simple name collides with a
  framework type would still bind every same-named reference workspace-wide. No
  instance exists in the measured corpus, but nothing prevents one. Closing it
  needs import/namespace corroboration, which the receiver token (one bare
  identifier) cannot supply today.
- *Visibility dependence.* Cross-file binding requires
  `visibility = 'public'`. TypeScript now records `public` on **exported** type
  declarations (fixture-proven for the static tier); non-exported types stay
  non-public. C# still maps `internal` to `private`, over-refusing ~1,556 call
  sites. Recall loss only, never a wrong edge. Recorded in
  `fixtures/extraction/capabilities.json` under
  `reference_resolution.tiers.tier3_static_type.visibility_dependency` with a
  named closure (C# `internal` remains open).
- *Static-modifier dependence.* A member only binds through a type name if it is
  statically reachable (enum members and constants are exempt): prefer
  `symbols.metadata_json.isStatic`, else a standalone `static` word in
  `signature`. That is a per-language extractor fact, not a resolver one.
  Fixture-proven languages (`TIER3_STATIC_TYPE_LANGUAGES`): **csharp,
  typescript, javascript**. Every other language emits a
  `reference_resolution.tier3_static_type` gap row at scan time, enforced by
  `per_language_tier_parity_guard` and
  `every_static_type_language_ships_a_proving_fixture`. The gate itself is
  correctness, not debt: without it `Type.InstanceMethod()` would bind, which
  does not compile.
- *Untyped local shadowing a type name.* A local variable declared in a method
  body (`var Fixture = ...`) still shadows a workspace type invisibly: local
  *declarations* are not symbols, so nothing distinguishes the receiver token from
  a type name. Parameters no longer have this hole — see the shadowing refusal
  below. Zero measured instances (every binding's receiver is PascalCase, and C#
  convention makes locals camelCase), and `tier3_receiver` already outranks the
  static tier whenever the local carries a type fact, so the exposure is untyped
  locals only. Closes with slice 4.
- *Recheck ownership.* `recheck_resolved_identifier_items` now skips only
  identifiers a co-located edge currently owns, not every co-located identifier.
  This is defensive: the hole it closes could not be reproduced, because every
  path that forces a full pass either has no prior overlay or re-extracts the
  identifier rows and cascades it away.


- **Where:** `crates/julie-extract-cli/src/resolution.rs` —
  `resolve_receiver_symbols` (:731), `tier3_candidates` (:653),
  `tier4_compatible_kinds` (:821), `tier123_compatible_kinds` (:793),
  `TIER2_IMPORT_LANGUAGES` (:77), `resolve_identifier_items` (:1341).
  Output surface is `identifiers.target_symbol_id` (consumed by Miller as
  `reference_sites.is_exact` / `provenance='target_token'`).
- **Why it matters:** span emission is healthy, but *target* resolution is not, and
  the two are easy to conflate. On a real 400k-site C# corpus (Miller's own
  workspace, julie-extract 2.18.0, schema 5), `reference_sites` carries exact spans
  for **78.4%** of C# sites — yet only **11.1%** of C# `identifiers` rows resolve to
  a `target_symbol_id`. Downstream, anything gated on exact target resolution
  (reference counts, callers/callees, rename coverage, dead-code candidates) sees a
  small fraction of the real graph.

- **Measured breakdown (C#, 308,933 identifiers):**

  | kind | rows | resolved | % |
  |---|---|---|---|
  | `variable_ref` | 151,426 | 7,440 | 4.9 |
  | `call` | 85,410 | 16,605 | 19.4 |
  | `member_access` | 44,938 | 96 | **0.2** |
  | `type_usage` | 27,159 | 10,086 | 37.1 |

- **This is not C#-specific,** and the shape is not random. Resolution rate by
  language — powershell 49.5%, bash 40.9%, javascript 28.2%, python 23.0%,
  csharp 11.1%, razor 9.8%, css/html 0.0% — tracks whether a language's callables
  are emitted as `SymbolKind::Function` (tier-4 eligible) or `SymbolKind::Method`
  (tier-4 ineligible), plus whether tier 2 is enabled for the language. C# is the
  largest corpus here, so it surfaces the gap most visibly.

### Root cause, ranked by measured impact

1. **No static/type-name receiver path.** `resolve_receiver_symbols` (:731) binds
   the receiver only by looking for a symbol *in the caller's scope chain* and then
   *file top-level*. When the receiver IS a type name — `JulieDbFixture.CreateForEdit`,
   `SomeEnum.Value` — the receiver is a type defined in another file, so it never
   binds and tier 3 declines. This is the single largest tractable gap, and it needs
   no type inference. Yield under the rule "receiver names exactly one workspace
   type-like symbol AND that type has exactly one direct child with the terminal
   name":

   | site shape | exactly one child | at least one child |
   |---|---|---|
   | `call` with a type-name receiver | **6,086** | 7,102 |
   | `member_access` with a type-name receiver | **6,669** (3,557 enum_member, 2,567 constant, 545 property) | 6,843 |

   The exactly-one column is the shippable rule; the delta to "at least one" is
   overload declines (1,016 for calls, 14.3%). Applying the safety refusals below
   brings the predicted yield to ~9,852; the shipped tier measured **9,853**.

   **This is corroboration, not proof.** The rule declines receivers with *no*
   workspace type of that simple name (`Path`, `Assert`, `StringComparison` — zero
   workspace types, safe). It does **not** decline external receivers whose simple
   name collides with a workspace type. Existence proof in this corpus:
   `NullLoggerFactory` is a `file static class` test stub defined once in
   `tests/Miller.Tests/Server/CallToolFilterTelemetryTests.cs`, and 6 of its ~24
   receiver sites are in that file — the rest are in other test files where the real
   referent is `Microsoft.Extensions.Logging.Abstractions.NullLoggerFactory`. Those
   would be wrong edges. A workspace type named `File` is latent in the same way
   (987 `File.*` sites; currently 0 exact-one matches only because the nested
   workspace `File` has no BCL-shaped children). Mitigation is mandatory, not
   optional — see the fix plan.

2. **`tier4_compatible_kinds(Call)` excludes `SymbolKind::Method`** (:823). C# has
   9,412 methods against 117 functions and 267 constructors, so ~97% of C#
   callables are ineligible for the only tier available to them (see cause 3).
   The rationale is
   `docs/plans/2026-07-06-workspace-reference-resolution-design.md:177`, from
   accepted doubt-pass finding 6 (:357-359): *"member names collide too heavily for
   global uniqueness to mean anything."* Asserted, never measured — but **measurement
   vindicates it.** Widening the filter naively is unsafe because tier 4 keys on
   terminal name + language + kind and **discards the receiver entirely** (:708-725).
   Of the 10,575 edges a bare widening would create, **3,135 (29.6%) have a receiver
   that names nothing in the workspace** — i.e. the real target is external:

   | site | would resolve to | count |
   |---|---|---|
   | `Path.Combine` | `CanaryAggregate.Combine` | 1,691 |
   | `Assert.Single` | `DiffTargetsTests.Single` | 736 |
   | `File.ReadAllText` | `EditApplier.ReadAllText` | 160 |

   The exactly-one guard does not help here: it proves the *workspace* name is
   unique, not that the call site refers to a workspace symbol at all. These would
   be confidently wrong edges, violating
   `docs/contracts/sqlite-schema-v4.md:495` ("no best-guess selection — a wrong edge
   is worse than a missing one"). Pinned by four unit tests at
   `resolution.rs:2184, 2485, 2513, 2609`; mirrored in
   `docs/contracts/sqlite-schema-v4.md:503` and
   `fixtures/extraction/capabilities.json:6267-6270`.

3. **Tier 2 is off for C#.** `TIER2_IMPORT_LANGUAGES` is `["typescript",
   "javascript"]` (:77), so `applicable_tiers` (:533) leaves C# `call` and
   `type_usage` with tier 4 as their *only* tier — the one whose kind set has no
   methods. Combined with cause 2 this is why C# `call` resolution is 19.4% and
   almost all of it comes from tier 1 (13,417 of 16,605).

4. **Tier 3 starves at the receiver→symbol link for instance receivers too.**
   Funnel over 44,959 C# `member_access` rows: 43,545 carry a receiver → **1,544**
   resolve to a symbol in scope (−96.5%) → 719 have a type fact → 466 name a unique
   type → **96** find the member. The extractor emits zero `SymbolKind::Variable`
   symbols for C# (no `local_declaration_statement` arm in
   `crates/julie-extractors/src/csharp/mod.rs:220-252`) and `infer_variable_type`
   returns `None` unconditionally (`csharp/type_inference.rs:130-132`), so
   `resolve_receiver_symbols` has nothing to bind locals to. All 12,438 C# type
   facts are `is_inferred: true` (`crates/julie-extractors/src/factory.rs:14-34`),
   capping tier 3 at confidence 0.65.

5. **`tier123_compatible_kinds(MemberAccess)` omits `Constant` and `EnumMember`**
   (:806). On its own this is worth almost nothing — replaying the *existing* tier-3
   funnel with those kinds added yields **4** extra sites, because enum and class
   symbols carry no type facts, so the receiver never binds in the first place. It
   only pays off combined with cause 1, where it accounts for 6,124 of the 6,669
   static-receiver `member_access` sites.

6. **Covered identifiers vanish from the report.** `resolve_identifier_items` skips
   any identifier "covered" by a co-located pending relationship (:1351), delegating
   it to `propagate_relationships`. When the pending edge fails to resolve, the
   identifier gets **no `identifier_resolutions` row at all** — 61,655 C# call sites
   are recorded as neither `resolved` nor `missing`, violating the "every attempted
   outcome is recorded" contract at `docs/contracts/sqlite-schema-v4.md:448`. Note
   these are not uniformly hopeless: ~888 are covered by a pending `instantiates`
   edge whose kind filter differs from the identifier `call` chain, so they would
   resolve if the identifier chain were allowed to run.

### Corrections to the original entry (2026-07-27 investigation)

- *"member_access 0.2% suggests the receiver path is not being walked at all"* is
  **refuted**. The receiver is populated for 43,545 of 44,959 rows and only 1,414
  come back `no_context`; the path is walked and fails at the receiver→symbol link.
- *"Add a conservative unique-name fallback tier"* — that tier already exists as
  tier 4 (`CONFIDENCE_TIER4 = 0.55`, `METHOD_TIER4 = "tier4_global"`, :52/:59), and
  **it should not be widened**. Name-only matching is the wrong instrument; see
  cause 2.
- The tractable `call` subset was overstated twice over: of the 17,112 unique-name
  unresolved call rows, 7,116 point at an `enum_member` (kind filter rejects them),
  and of the 10,575 that survive the kind filter, 3,135 have an external receiver.
  The entry's own caveat was right, and the answer is *not* a shadowing check — it
  is receiver corroboration.
- An earlier draft of this entry put the static-receiver call yield at 2,891. That
  number measured a **stricter filter than the proposed rule** — it additionally
  required the terminal name to be globally unique among C# callables, which the
  rule does not. The proposed rule yields **6,086**. The draft also claimed the rule
  "declines external receivers by construction"; it does not (see root cause 1).
- *"`partial` means tier-2 gating, so these measurements are from a complete pass"*
  is **unproven**. `partial` means delta **or** gating (:1027,
  `docs/contracts/sqlite-schema-v4.md:74`). The measured artifact has
  `reference_resolution_last_full_revision = 1` against head revision **104**, so it
  has been delta-maintained for 103 revisions. Per-kind coverage looks near-total,
  but the baseline must be re-measured after a forced full pass before it is used as
  a gate.

### Proposed fix, in order

1. ~~**Close the reporting leak first**~~ — **done.** So every later slice can be measured
   honestly. Give the identifier its own outcome instead of inheriting silence from
   a failed pending edge — run the identifier chain when the covering pending edge
   did not resolve, rather than stamping a fabricated `missing`. The kind filters
   differ between the two chains, so this is not a no-op (~888 C# sites).
2. ~~**Re-baseline with a forced full resolution pass.**~~ — **done.** The measured artifact is
   delta-maintained (`last_full_revision = 1`, head 104), so no yield number here is
   a valid gate until it is re-measured on a full pass. Ordered before slice 3, not
   deferred to verification.
3. ~~**Add a static/type-name receiver path**~~ — **done**, for `call` and `member_access`: when the
   receiver names exactly one workspace type-like symbol in the same language and
   that type has exactly one direct child with the terminal name, resolve to it.
   Expected yield ~12,755 C# sites. Non-negotiable implementation constraints:
   - It is a **new independent filter, not a widening of tier 3.** Tier 3 is
     receiver → scoped symbol → `type_facts` → type → member. The static path skips
     scope binding and type facts entirely, so folding it into
     `resolve_receiver_symbols` would label static bindings `tier3_receiver` and
     misreport type-fact involvement to consumers. Give it its own method string
     (e.g. `tier3_static_type`) and its own confidence (≈0.70 — below concrete
     tier 3's 0.75, well above tier 4's 0.55).
   - **`applicable_tiers` must change for `(Identifier, Call)`** — it is
     `[Import, Global]` today (:545), so the receiver path never runs for call
     identifiers at all.
   - **Homonym mitigation is required before ship**, not after: prefer same-file,
     imported, or top-level types over arbitrary unique nested/file-local ones, and
     decline a `file`-scoped type outside its defining file. Without this the tier
     reintroduces the tier-4 failure keyed on type names instead of method names.
   - Document the receiver-token limits: `receiver_before_identifier`
     (`crates/julie-extract-cli/src/extraction.rs:479-505`) scans one bare token, so
     `Cache<T>.Get` yields `None` and `foo.Bar.Baz` yields `Bar`. Generic receivers
     decline; multi-hop receivers bind the intermediate token (51 of 6,086 call
     sites, 26 of them targeting non-static signatures).
   - Ship with `Constant` and `EnumMember` added to
     `tier123_compatible_kinds(MemberAccess)` — they only pay off together.
4. **Emit C# locals and parameters as symbols** — *not started, the only open slice.* with `parent_symbol_id`, and make
   `infer_variable_type` real, distinguishing declared from inferred type facts.
   This is the only slice that moves `variable_ref` (151k rows, 4.9%) and the
   instance-receiver half of the tier-3 funnel. Largest slice; needs C# golden
   fixtures and `capabilities.json` updates. Does not block slice 3.
5. **Do not ship bare `Method`-at-tier-4.** Revisit only as a receiver-corroborated
   rule with a measured precision gate, after slices 3 and 4 have absorbed the sites
   that can be resolved correctly. Global name uniqueness alone is rejected.

- **Contract and consumer impact:** every slice that changes observable output must
  bump `RESOLUTION_VERSION` (:856) and account for full backfill on upgrade.
  Downstream, Miller classifies *any* overlay target as exact regardless of tier
  confidence (`ReferenceEvidenceReader.cs` in the Miller repo), so a 0.55-confidence
  edge is presented to users as an exact reference — precision matters more than
  recall here. `reference_sites.is_exact` / `provenance` attest span quality, not
  target correctness (`docs/contracts/sqlite-schema-v6.md`); do not conflate them.
- **Verification:** assert no `identifiers` row lacks an `identifier_resolutions`
  row after a full pass, and prove full/delta idempotence. Rate comparisons measure
  recall only — precision needs its own fixture evidence per the repo's data-quality
  bar. Required adversarial fixtures: external receiver with no workspace type
  (`Path.Combine` against an unrelated workspace `Combine`); **framework-homonym
  receiver** (a workspace `NullLoggerFactory` referenced cross-file where the real
  referent is the framework type); file-local type referenced outside its file;
  nested-type homonym; overloads; overrides and inherited members; interface and
  implementation pairs; partial classes; generic arity and generic static receivers;
  multi-hop receivers; static enum and constant access; and local-shadows-type-name.
  The numbers above were measured 2026-07-27 on Miller's `.miller/symbols.db`
  (binary_version 2.18.0, sqlite schema 5, head revision 104).

## 17. Fresh family-store recovery reports partial reference resolution — open

- **Where:** store scan/resolve orchestration under
  `crates/julie-extract-cli/src/store/`; Miller consumer validation in
  `StoreWorkspaceCoordinator.RequireCommitted`; reproduction workspace
  `/home/murphy/source/julie-extractors` with `julie-extract 2.31.4`.
- **Observed:** after the family-store directory named by `.miller/store.json`
  disappeared, Miller attempted `RootRebind` recovery 16 times. Every attempt
  failed with `resolution_input_incomplete: reference_resolution_status must be
  complete, found partial`, leaving the workspace unreadable.
- **Plan gap:** the 2026-08-11 incremental-resolution plan improves `store
  resolve` performance and fallback behavior, but does not require this fresh
  store/bootstrap recovery case to succeed.
- **Proposed fix:** identify why a fresh store recovery publishes partial
  reference-resolution input, fix the producer path, and add an integration
  regression proving missing family store → Miller refresh/`RootRebind` →
  complete resolution → readable workspace.

## 18. Store resolve still has three measured follow-ups — open

- **Where:** `crates/julie-extract-cli/src/store/resolution_session.rs`
  (`prime_identifier_children`, `prime_exact_children_keys`,
  `load_version_mini_index`, `symbol_by_id`, `effective_identifier_exists`).
- **Status:** `open`. v2.33.6 ships the file mini-index and whole-pass name
  cache. v2.33.7 ships the ubiquitous-name filter for scoped deltas. These
  three items stay out of those tags on purpose.
- **Measured leftover (Miller family-store copy, 2026-08-17):** after the
  mini-index and name cache, a full resolve still does about 9.6k child-name
  queries that read ~1M rows, 2,409 whole-file mini-index loads, and 441k
  in-memory identifier-exists checks.
- **Proposed next work, in this order:**
  1. Skip the child-name warmup SQL when that file is already in the
     mini-index. The leftover 9.6k / 1M-row cost is the warmup, not the later
     lookups. Measure before and after. Do not skip when the file is too large
     for the index.
  2. Do not load a whole file just to answer one `symbol_by_id` probe. A
     single-id SQL read is enough until the same file is used again.
  3. Batch the 441k identifier-exists checks. They are cheap in-memory SQLite
     today; batching is only worth it if a later profile still shows them on
     the wall-time path.
- **Do not:** re-add identifier name-prime (measured slower), change crossover
  first, or raise timeouts to hide the leftover cost.

## 19. `store update` bypasses scan's discovery gates — closed (fix/store-queue-hygiene)

- **Where:** `crates/julie-extract-cli/src/store/update.rs` (`execute_update`,
  ~130-137) and `store/executor.rs` (no reference to `select_file`,
  `FileSelection`, or `UnsupportedReason`).
- **Status:** `closed`. `execute_update` runs the scan discovery decision before any read; a refused file reports terminal `unsupported` (exit 0) and writes zero queue rows.
- **Evidence (2026-08-25 Miller bench-prep incident):** Miller submitted
  `store update` for tree-sitter-c-sharp's `src/parser.c` (32 MB generated C).
  `scan` excludes it (`MAX_SOURCE_FILE_BYTES`, `discovery.rs:597`), but the
  update path read, hashed, and enqueued it; every later import spent 29-54 s
  extracting it and died on the 4000 ms coordinator quantum. Live proof of the
  same hole: three vendored `.min.js` files sit `indexed` in a manifest even
  though `scan` hard-excludes `.min.js`.
- **Proposed fix:** apply the same discovery decision `scan` uses before
  enqueue: refuse an oversized or hard-excluded file and report it as
  `unsupported` so the requester gets an honest terminal state instead of a
  poison row. Regression: `store update` on an oversized file and on a
  `.min.js` must return `unsupported` and leave zero queue rows.

## 20. Backlog quantum overrun overwrites the caller's own committed state — closed (fix/store-queue-hygiene)

- **Where:** `crates/julie-extract-cli/src/store/import.rs` (~276-288) and
  `store/update.rs` (~39-52); drain in
  `crates/julie-extract-artifact/src/store/coordinator.rs` (~1356-1370).
- **Status:** `closed`. The caller's report state now reflects only the caller's own request; backlog failures surface in a `warnings` array on the report.
- **Evidence:** a committed `store import` was reported
  `state=failed, failure_class=coordinator_quantum` because a *backlog*
  request (someone else's poisoned update) blew the quantum after the caller's
  own work had already committed. Miller logged
  "coordinator_total 35173 failed" then "completed 157 true" — the revision
  advanced on a scan reported failed, and Miller's persisted scan-failure
  backoff throttled a healthy store.
- **Proposed fix:** report the caller's own request's true terminal state;
  carry backlog failures as a warning field, never as the report state.

## 21. Unschedulable requests requeue forever with no attempt counter — closed (fix/store-queue-hygiene)

- **Where:** `crates/julie-extract-artifact/src/store/coordinator.rs`
  (requeue on overrun ~1449-1462; `Update` absent from renewable kinds
  ~88-90; candidate selection prefers interactive kinds ~1583-1619).
- **Status:** `closed`. The coordinator counts overruns per request (`quantum_overruns` column, in-place ALTER) and fails the row with `coordinator_quantum` on the third; `Update` stays non-renewable on purpose.
- **Evidence:** the quantum is measured after the work finishes, so a 29 s
  update extraction completed, was thrown away, and requeued — on every
  drain, forever. Nothing counts overruns, so the row never fails out, and
  one poison update starves every later import on the family. Deleting the
  queued rows by hand dropped a 28,670 ms failing import to 17 ms committed.
- **Proposed fix:** count overruns per request; after N (e.g. 3) fail the row
  with a terminal `coordinator_quantum` state so the queue drains. `Import`
  was already added to `permits_renewable_quantum` for the same bug class —
  either make `Update` renewable or give it the counter.

## 22. Nobody reaps dead-requester queue rows; `store maintain` skips `requests` — closed (fix/store-queue-hygiene)

- **Where:** `crates/julie-extract-artifact/src/store/coordinator.rs` (lease
  takeover requeues claimed rows ~1010-1066; `requester_deadline` filters only
  `acknowledge` ~2035-2038) and `store/maintenance.rs` (prunes store-log rows
  and scratch, never `requests`).
- **Status:** `closed`. Drains and maintenance reap queued rows with dead requesters (claimed rows only when the claim owner is dead too, token `coordinator_requester_dead`); `store maintain` archives terminal rows up to the log high-water mark and prunes aged failed rows, reported as `pruned_request_rows`.
- **Evidence:** tree-sitter-razor's `coord.db` held a `claimed` update row
  whose `claim_owner` was a dead CLI pid; nothing surfaced or reaped it. A
  Miller family store held 2,163 committed update rows and 339 resolves, none
  pruned. The only reap is lease takeover, which converts a claimed poison
  row into a queued poison row.
- **Proposed fix:** reap or age out `queued`/`claimed` rows whose requester
  pid is dead (matching the existing claimed-row takeover rule), and add
  `requests` pruning to `store maintain`.

## 23. Publish discovery limits in `languages --json` — closed (fix/store-queue-hygiene)

- **Where:** `crates/julie-extract-cli/src/limits.rs`
  (`MAX_SOURCE_FILE_BYTES`) and `discovery.rs` (hard-exclude suffixes and
  directories); consumer contract surface `languages --json`.
- **Status:** `closed`. `languages --json` publishes `discovery_limits` (max_source_file_bytes, hard-exclude directories and suffixes) sourced from the real constants; report schema stays 3.
- **Why:** Miller now mirrors the 1 MiB limit and the hard-exclude sets as
  local constants (`ExtractSourceLimits`, 2026-08-25) to stop submitting
  files `scan` refuses. A mirrored constant drifts silently on the next limit
  change. Publishing the limit and both hard-exclude sets in
  `languages --json` lets Miller read them from the pinned binary instead.
