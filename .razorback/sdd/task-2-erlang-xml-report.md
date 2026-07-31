# Task 2: Erlang registration + symbols — report

**Status:** DONE
**Commit:** `eb7cb1306360b88cba624743b40ddfde9f54646d` (`feat(erlang): register erlang and ship the symbol extraction tier`)
**Worktree state at commit:** path `/Users/murphy/source/julie-extractors/.worktrees/erlang-xml-language-support`,
branch `erlang-xml-language-support`, parent commit `0741190a`, working tree clean after commit
(`git status --short --branch` → `## erlang-xml-language-support` only).

---

## 1. Worktree guard (step 0)

```
pwd    = /Users/murphy/source/julie-extractors/.worktrees/erlang-xml-language-support
branch = erlang-xml-language-support
HEAD   = 0741190a  (Task 1 grammar pin, as expected)
```

`git worktree list` showed three worktrees; the other two (`/Users/murphy/source/julie-extractors` @ `4bee2fe2 [main]`,
`~/.config/razorback/worktrees/julie-extractors/csharp-locals-params` @ `90542e0d`) were not touched.

---

## 2. Miller calls and what each confirmed

All calls used `workspace_id=julie-extractors-91c17adbdab9`.

| Call | Confirmed |
| --- | --- |
| `context(query="how the elixir extractor emits symbols, visibility, and doc comments")` | Pivots `ElixirExtractor::extract_symbols` (`src/elixir/mod.rs:51`), `extract_doc_comment_for_node` (`src/elixir/attributes.rs:194`), `extract_def` (`src/elixir/calls.rs:118`), `extract_defguard` (`src/elixir/definition_forms.rs:7`). Showed the `create_symbol` + `SymbolOptions` call shape, `normalize_annotations(&[...], "elixir")` usage, and the `metadata.insert("is_test", …)` pattern the Erlang module now mirrors. |
| `inspect(target="crates/julie-extractors/src/elixir/mod.rs", depth="overview")` | The approved module surface: `new`, `extract_symbols`, `extract_relationships`, `extract_identifiers`, `infer_types`, `traverse_node`, `traverse_children`, plus the `get_*` accessors. `ErlangExtractor` exposes the same public entry points so the registry dispatch shape stays uniform. |

Everything after that was direct file reads, because the remaining questions were "what exactly does this
macro/test assert", which needs exact text rather than ranked pivots. Notably the capability-matrix
contract (`crates/julie-extractors/src/tests/capability_matrix.rs`, 2050 lines, 39 tests) had to be read
line-by-line — its assertions are what drove most of the design decisions below.

---

## 3. API-shape evidence (repo-internal)

| Shape | Where proven |
| --- | --- |
| Registry dispatch table | `crates/julie-extractors/src/registry.rs:528` `EXTRACTORS: &[(&str, ExtractFn)]`; the four `define_*_extractors!` macros at `:30/:70/:109/:147` all fill `relationships`/`identifiers`, so a symbol-only language needs a hand-written fn — the same shape `extract_toml` (`:400`) and `extract_json` (`:434`) already use. `extract_erlang` follows that precedent. |
| `LanguageSpec` row | `crates/julie-extractors/src/language_spec/mod.rs:38-45` (`name`, `aliases`, `extensions`, `parser_crate`, `capabilities`, `parser`, `doc_comment_styles`); `spec(...)` rows in `specs.rs`. |
| `ParserFn` | `parser!` macro at `language_spec/mod.rs:206`; `parser!(parser_erlang, tree_sitter_erlang::LANGUAGE)` follows `parser_elixir` at `:237`. |
| Symbol creation | `BaseExtractor::create_symbol` at `crates/julie-extractors/src/base/creation_methods.rs:19`; `body_span`/`body_hash` are inferred there, and `doc_comment` falls back to `find_doc_comment(node)` only when `SymbolOptions.doc_comment` is `None`. |
| Doc-comment block selection | `select_doc_comment_block` at `base/extractor.rs:450` — walks preceding comment siblings and gates on `LanguageSpec::is_doc_comment` / `continues_doc_comment`, i.e. a `DocCommentStyle` is required for `%%` blocks to resolve. |
| Golden harness contract | `crates/julie-extractors/src/tests/golden.rs:258` `golden_fixtures_match_canonical_extraction`; fixtures are discovered from `fixtures/extraction/capabilities.json`, run through `pipeline::extract_canonical`, normalized by `normalize()` (`:365`), and compared to `expected.json`. **Blessed regeneration path: `UPDATE_GOLDEN=1`** (`golden.rs:261`, message at `:311`). `expected.json` was never hand-authored. |
| Parse diagnostics | Produced centrally by `pipeline::parse_diagnostics_for_tree` (`pipeline.rs:174`), not by extractors — every registry fn sets `parse_diagnostics: Vec::new()`. So Erlang parse-error degradation is a pipeline-level test, not extractor code. |
| Structural-fact contract regeneration | `UPDATE_CONTRACT_JSON=1` (`tests/structural_fact_registry.rs:300`). |

---

## 4. Grammar node kinds (derived from real parse trees, not memory)

A scratch dump test (`tests/erlang/scratch_dump.rs`, written, run, then **deleted** before commit) printed the
full `tree-sitter-erlang` 0.20.0 tree for representative Erlang. Node kinds actually used:

| Construct | Node kinds observed |
| --- | --- |
| Root | `source_file` |
| `-module(bank).` | `module_attribute` → `atom` |
| `-doc "…"` / `-moduledoc "…"` | `wild_attribute` → `attr_name` → `atom`, plus a sibling `string` |
| `-behaviour(gen_server).` | `behaviour_attribute` |
| `-export([open/1, …]).` | `export_attribute` → `fa` → `atom` + `arity` → `integer` |
| `-export_type([account/0]).` | `export_type_attribute` → same `fa` shape |
| `-compile(export_all).` | `compile_options_attribute` → `atom` |
| `-compile([export_all, …]).` | `compile_options_attribute` → `list` → `atom`… |
| `-define(PI, 3.14).` | `pp_define` → `macro_lhs` → `var` |
| `-define(LOG(Msg), …).` | `pp_define` → `macro_lhs` → `var` + `var_args` |
| `-record(account, {…}).` | `record_decl` → `atom`, `record_field`* (each `record_field` → `atom` + optional `field_expr`/`field_type`) |
| `-type account() :: …` | `type_alias` → `type_name` → `atom` + `var_args` |
| `-opaque token() :: …` | `opaque` → `type_name` → … |
| `-callback init(A) -> R.` | `callback` → `atom` + `type_sig` → `expr_args` |
| `-spec open(…) -> …` | `spec` → `atom` + `type_sig` |
| Function clause | `fun_decl` → `function_clause` → `atom` + `expr_args` (+ optional `guard`) + `clause_body` |
| Comment | `comment` |

**Load-bearing discovery:** each clause of a multi-clause function is its **own top-level `fun_decl`**
sibling (the `;` terminator sits on the preceding one). `deposit/2` arrives as two `fun_decl` nodes.
That is why grouping is a pre-pass over `source_file` children keyed by `(name, arity)` rather than a
child walk inside one node.

Second discovery: every construct in the golden — including `-doc`, `-moduledoc`, `-include_lib`, and
`?MACRO(...)` call sites — parses with **zero** ERROR/MISSING nodes on the pinned grammar.

---

## 5. What was built

### `crates/julie-extractors/src/erlang/`

- `mod.rs` — `ErlangExtractor` + `extract_symbols`. Erlang declarations are all top-level, so extraction is a
  pre-scan (exports, `-compile(export_all)`, clause counts, `-moduledoc`) followed by one ordered pass over
  `source_file` children. `extract_relationships` / `extract_identifiers` / `infer_types` exist and return
  empty, so the public surface matches every other extractor and Tasks 4/6/7 fill them in place.
- `helpers.rs` — node helpers: `fa` list parsing, `expr_args`/`var_args` arity, atom unquoting, attribute
  signature normalization, `preceding_attributes`, and the EUnit name predicate.
- `attributes.rs` — module, record (+ fields), macro, type/opaque, callback.
- `definition_forms.rs` — function clause identity and function symbol emission.
- `doc.rs` — the two documentation channels (EDoc `%%` blocks, `-doc`/`-moduledoc` attributes) and
  annotation markers.

### Emitted model

| Erlang construct | SymbolKind | Visibility | Metadata |
| --- | --- | --- | --- |
| `-module` | `Module` | Public | — |
| `-record` | `Struct` | Private | — |
| record field | `Field` (parent = record) | Private | — |
| `-define` | `Constant` | Private | `macro_arity` when parameterized |
| `-type` / `-opaque` | `Type` | Public iff in `-export_type` | `arity`, `opaque` |
| `-callback` | `Function` | Public | `callback: true`, `arity` |
| function (all clauses) | `Function` | Public iff in `-export` or `export_all` | `arity`, `clause_count`, `is_test` |

Signature format: `deposit/2(Acct, Amount)` — name/arity (Erlang's actual identity, and how docs and
`-export` refer to a function) followed by the first clause head.

---

## 6. Judgment calls

1. **`src/erlang/doc.rs` as a fourth module** (plan named `mod/helpers/attributes/definition_forms`).
   Erlang has two independent documentation channels that both `attributes.rs` and `definition_forms.rs`
   consume; keeping them in `attributes.rs` would have made that file the de-facto home of function
   documentation too. Chose a small dedicated module over a cross-importing one.

2. **`crates/julie-extractors/src/erlang/definition_forms.rs:41` — symbol span is the first `fun_decl`,
   not a synthetic span over all clauses.** A synthetic multi-clause span would need
   `create_symbol_from_span` and would make `body_span`/`body_hash` cover the gaps between clauses.
   Chose first-clause span (matches "signature from the first clause head"); `clause_count` metadata
   records that more clauses exist, so a later task can widen the span without losing information.

3. **`crates/julie-extractors/src/erlang/helpers.rs:118` — `-moduledoc` terminates the preceding-attribute
   run instead of joining it.** Without this, `-moduledoc` leaked into the annotations of the first
   declaration below `-module`. `-moduledoc` documents the module; it is resolved separately in
   `mod.rs:module_doc`.

4. **`crates/julie-extractors/src/erlang/helpers.rs:130` — EUnit detection lives in the Erlang module, not
   `test_detection.rs`.** `test_detection.rs` `_ => detect_generic` requires a `test_`/`Test` **prefix** plus a
   test path, which never matches EUnit's `*_test` / `*_test_` **suffix** convention. Task 7 owns test roles
   and `test_detection.rs`, so the predicate was kept local (10 lines) rather than editing a file that task
   owns. **Recommend Task 7 lift it into `test_detection.rs` as `"erlang" => detect_erlang(name)`.**

5. **`language_spec/mod.rs` — `DocCommentStyle::ErlangPercentBlock` matches any `%%` prefix**, not
   specifically `%% @doc`. `DocCommentStyle` is prefix-based per line and has no "contains @tag" form; this
   matches how `GoLine` treats all `//` comments as doc comments. `%%%` module banners are covered because
   they also start with `%%`; single `%` inline comments are not.

6. **`SYMBOLS_ONLY_CAPABILITIES` added** to `language_spec/mod.rs` following the existing const style
   (plan said no symbols-only const existed — confirmed, `mod.rs:99-150`).

7. **`fixtures/extraction/erlang/negative/`** was created in addition to `basic/`. See §7.2 — the matrix
   requires it once `target_capabilities.relationships = true`.

---

## 7. Plan mismatches (IMPORTANT — lead action needed)

The plan's Task 2 file-ownership list is **incomplete for what registering a language actually requires**.
Every item below is enforced by an existing repo gate, verified by running it. Files outside the assigned
ownership list were touched only where a gate made it unavoidable, and each is listed here.

### 7.1 Open capability rows must resolve against the 2026-05-31 migration plan

`capability_matrix_requires_target_capabilities` (`tests/capability_matrix.rs:321` → `validate_target_capability:1513`)
requires a `capability_gaps` row whenever `target_capabilities.X = true` and `capabilities.X = false`.
`capability_matrix_requires_relationship_fixture_evidence:284` **forbids** `status: "exception"` for
`relationships` while `capabilities.relationships = false`, so the row must be `status: "open"`.
`capability_matrix_open_rows_have_planned_closure_task:561` then requires the row's `planned_closure_task`
string to appear **inside `docs/plans/2026-05-31-julie-code-migration-implementation-plan.md`** — a hardcoded
path.

Consequence, verified empirically: **no language can ship with `capabilities.relationships = false` without
adding text to that plan.** All 36 pre-existing languages have `capabilities == target_capabilities` and
zero `status: open` rows, so Erlang is the first row to hit this.

**Action taken:** appended `### Task 13: Erlang Capability Closure` to that plan, explicitly stating that the
entry exists because the test treats that file as the repository's open-capability registry, and that the
owning plan is `docs/plans/2026-07-31-erlang-xml-language-support-plan.md` (Task 4 → identifiers, Task 6 →
relationships/pending, Task 7 → types). **Task 3 (XML) will need the same treatment** if XML also ships
below its target.

### 7.2 `target_capabilities.relationships = true` requires a negative or cross_file fixture at Task 2

`capability_matrix_negative_cases_emit_no_wrong_edges:534` requires a fixture whose name contains
`negative` or `cross_file` for any language targeting relationships. The plan schedules
`erlang/cross_file` for Task 6, so Task 2 could not be green without one.

**Action taken:** created `fixtures/extraction/erlang/negative/` — reference-shaped Erlang (remote call,
macro call, record construction, `-behaviour`, `-include_lib`) proving zero relationship, pending, and
identifier rows are emitted at the symbol tier. Task 6 should extend it to prove *wrong* edges stay absent
once real edges exist.

### 7.3 Per-language parity guards outside the assigned file list

Three existing tests fail for **any** new language until shared per-language tables are updated:

| Guard | File it forced | What was added |
| --- | --- | --- |
| `tests::source_regions::supported_languages_with_source_region_syntax_emit_regions:359` | `base/source_regions.rs:686` + `tests/source_regions.rs` fixture | erlang `RegionLanguageConfig` (`comment`, `string`) |
| `tests::marker_structural_facts::marker_language_matrix_covers_every_supported_comment_language:354` | `base/structural_fact_registry/marker.rs`, `base/marker_structural_facts.rs`, `tests/marker_structural_facts.rs` fixture | erlang in the `code.marker.v1` language list; `%%%`/`%%`/`%` comment decorations in `semantic_line` |
| `crates/julie-extract-cli/tests/operations_contract.rs:145` | that file | `open_reference_resolution_gaps` 103 → **106** (the runtime emits three `reference_resolution.*` rows per language) |

`docs/contracts/structural-fact-patterns.json` was regenerated (`UPDATE_CONTRACT_JSON=1`) because the marker
pattern's language list changed — one added line.

**Recommendation:** add these four to the plan's Task 3 (XML) file list, and to
`docs/languages/new-language-checklist.md` §2, which currently omits them.

### 7.4 Complexity metrics have no erlang config

`base/complexity_metrics.rs` has no erlang entry, so `complexity_metrics` is empty. Unlike source regions
there is no hard guard, so it is recorded as a typed `kind_coverage.complexity_metrics` open gap (scopes
`file` and `symbol`) pointing at plan Task 8. **No task currently owns closing it** — it should be assigned.

### 7.5 Body-hash comment syntax (fixed here)

`base/body.rs:comment_syntax` fell through to the empty default for erlang, so `%` comments inside a body
counted as tokens and a comment-only edit would have changed `body_hash`. Added an `"erlang" => line: ["%"]`
arm plus a regression test (`body_hash_ignores_erlang_comments`). Golden output was unaffected.

---

## 8. Capability matrix row (honesty audit)

- `capabilities`: `symbols: true`, everything else `false` — matches `SYMBOLS_ONLY_CAPABILITIES` in the
  `LanguageSpec` row (enforced by `capability_matrix_matches_registry_entries`).
- `target_capabilities`: FULL, with four typed `status: open` gaps carrying fixture evidence
  (`fixtures/extraction/erlang/basic/expected.json`, command `cargo xtask test golden`).
- `kind_coverage`:
  - `symbols` / `body_spans` supported: `constant, field, function, module, struct, type` (every symbol
    carries both `body_span` and `body_hash`).
  - `annotations` supported: `function`; `doc_comments` supported: `function, module`.
  - `source_regions` supported: `comment, doc_comment, string_literal`.
  - `test_detection` supported: `test_case`; `test_container` and `test_lifecycle` are open gaps pointing at
    `docs/plans/2026-07-09-test-detection-golden-closure-implementation-plan.md` with the
    "language-native applicability" wording the matrix requires.
  - `relationships`, `identifiers`, `structural_facts`, `complexity_metrics`, `literals` are open gaps with
    reason / required_closure / planned_closure_task.
- No flag is true because a vector is non-empty: every `supported` claim is bidirectionally checked against
  the goldens by `capability_matrix_supported_kind_claims_have_fixture_evidence` and
  `assert_golden_domain_claims_match`.

**One claim deliberately kept empty:** `structural_facts.supported = []`. The `code.marker.v1` pattern is
now registered for erlang, but `structural_fact_pattern_ids_for_language` does not include marker patterns,
so `claims_match_registry` requires `{}` while `claims_have_fixture_evidence` requires the golden to emit
nothing. A `TODO` marker was therefore removed from the fixture rather than claimed. Same posture as every
other language.

---

## 9. Contract-version review

`EXTRACTION_CONTRACT_VERSION` (`crates/julie-extractors/src/lib.rs:127`) was **not bumped**. Adding a
language emits rows in existing domains with existing shapes; no field, kind, serialization, or normalization
rule changed for any downstream consumer. Recorded in the commit message per the task instructions.

---

## 10. Verification ledger

| Command | Result |
| --- | --- |
| `RUSTUP_TOOLCHAIN=1.97.1 cargo xtask test language erlang` | **32 passed, 0 failed** |
| `RUSTUP_TOOLCHAIN=1.97.1 cargo xtask test golden` | **3 passed, 0 failed** — `erlang/basic` and `erlang/negative`, both with `parse_diagnostics: []` |
| `RUSTUP_TOOLCHAIN=1.97.1 cargo xtask test capability` | **39 passed + 1 passed, 0 failed** (worker ceiling; the matrix did complain, repeatedly) |
| `RUSTUP_TOOLCHAIN=1.97.1 cargo xtask test changed crates/julie-extractors/src/language_spec/specs.rs fixtures/extraction/capabilities.json` | **exit 0** — default tier (3 crates) + certification (`parser_upgrade`), 25 `test result: ok` blocks, zero failures |
| `RUSTUP_TOOLCHAIN=1.97.1 cargo run -p julie-extract-cli -- languages --json` | `total: 37`; erlang row: `actual_capabilities` symbols-only, `target_capabilities` FULL, `capability_gaps: 4`, `extensions: ["erl","hrl"]`, `parser_crate: "tree-sitter-erlang"`, `dependency_status: "current"`, `fixtures: 2` |
| `cargo fmt --all --check` | clean |
| `cargo clippy -p julie-extractors -p julie-extract-cli --all-targets` | zero warnings (one `collapsible_match` was found and fixed) |

Not run (outside worker scope): `cargo xtask test default` standalone, real-world tiers,
`scripts/language-data-quality-report.mjs`, `cargo deny check` — these belong to Task 10's branch gate.
Note that `test changed` already ran the default tier and certification as sub-steps.

### Focused test inventory (32)

- `symbols.rs` (13): module symbol + signature, parent chain, function signature with arity, multi-clause
  collapse + `clause_count`, same-name/different-arity separation, record + fields + parent binding, macro
  with and without args, type alias vs same-named record, opaque metadata, parameterized type arity,
  callback metadata, EUnit flagging, body-hash comment insensitivity.
- `visibility.rs` (7): export list, arity-sensitive matching, bare `export_all`, `export_all` inside an
  options list, options list *without* `export_all` (negative control), `-export_type`, records/macros private.
- `docs.rs` (5): EDoc block on a function, multi-line EDoc block on the module, `-moduledoc` fallback,
  `-doc` on function + type + callback, annotation markers.
- `headers.rs` (4): `.hrl` standalone extraction order, no module parent, record→field parenting,
  `erl`/`hrl` extension routing.
- `parse_errors.rs` (3): declarations survive a broken clause, diagnostics are reported, clean source
  reports none.

Every test asserts concrete returned values; there are no smoke-only or no-assertion tests.

---

## 11. Self-review

| Acceptance criterion | Status |
| --- | --- |
| `cargo xtask test language erlang` green | ✅ 32 passed |
| Golden `erlang/basic` passes with zero parse diagnostics | ✅ (`erlang/negative` too) |
| Count assertions updated to 37 | ✅ `registry.rs:710`, `factory.rs:60-61`, `capability_snapshot_test.rs:8`; plus the undocumented CLI count 103→106 |
| `cargo xtask test changed specs.rs capabilities.json` green | ✅ exit 0 |
| `languages --json` shows erlang with honest flags | ✅ |
| `.hrl` standalone test asserts records/macros from a header | ✅ `headers.rs` (asserts the exact symbol list `MAX_BALANCE, account, id, balance, account`) |
| Worker-scope verification passes; committed | ✅ `eb7cb130` |
| Golden proves module / exported+private with arity / multi-clause collapse / record / macro / `-type` / EDoc + `-doc` / visibility | ✅ all 14 symbols in `erlang/basic/expected.json` |

Findings fixed during self-review:
- Removed the scratch tree-dump test before commit (it was scaffolding, and `grammar_smoke.rs` was left
  untouched for Task 3 as instructed).
- Fixed record fields being emitted *before* their record (declaration order in the symbol list).
- Fixed `-moduledoc` leaking into the next declaration's annotations.
- Fixed the `body_hash` comment-syntax gap (§7.5) and added a regression test.
- Collapsed the clippy `collapsible_match` in `collect_exports`.

Known limitations, all recorded as typed gaps rather than silently absent: no relationships, pending
relationships, identifiers, types, structural facts, complexity metrics, or literals; `test_container` /
`test_lifecycle` unclassified. `-include`/`-include_lib` and `-behaviour` parse cleanly but emit no symbol —
they are edges, and belong to Task 6.

## 12. Files changed

Created: `crates/julie-extractors/src/erlang/{mod,helpers,attributes,definition_forms,doc}.rs`,
`crates/julie-extractors/src/tests/erlang/{mod,symbols,visibility,docs,headers,parse_errors}.rs`,
`fixtures/extraction/erlang/{basic,negative}/{source.erl,expected.json}`.

Modified (assigned): `language_spec/specs.rs`, `language_spec/mod.rs`, `lib.rs`, `registry.rs`,
`factory.rs`, `tests/capability_snapshot_test.rs`, `fixtures/extraction/capabilities.json`.

Modified (implied): `crates/julie-extractors/src/tests/mod.rs` (`pub mod erlang;`).

Modified (forced by repo gates — see §7): `base/source_regions.rs`, `base/marker_structural_facts.rs`,
`base/body.rs`, `base/structural_fact_registry/marker.rs`, `tests/source_regions.rs`,
`tests/marker_structural_facts.rs`, `crates/julie-extract-cli/tests/operations_contract.rs`,
`docs/contracts/structural-fact-patterns.json`,
`docs/plans/2026-05-31-julie-code-migration-implementation-plan.md`.

`crates/julie-extractors/src/tests/grammar_smoke.rs` was **not** touched.
