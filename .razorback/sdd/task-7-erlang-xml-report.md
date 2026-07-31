# Task 7 — Erlang types + test roles → FULL

**Status:** COMPLETE
**Commit:** `32d8baf`
**Branch:** `erlang-xml-language-support`

---

## 1. Worktree guard (step 0)

```
pwd    /Users/murphy/source/julie-extractors/.worktrees/erlang-xml-language-support
branch erlang-xml-language-support
HEAD   4486a0d (clean at start)  →  32d8baf (this task's commit)
```

`git worktree list` at start:

| path | HEAD | branch |
| --- | --- | --- |
| `/Users/murphy/source/julie-extractors` | 4bee2fe | main |
| `/Users/murphy/.config/razorback/worktrees/julie-extractors/csharp-locals-params` | 90542e0 | feature/csharp-locals-params |
| `/Users/murphy/source/julie-extractors/.worktrees/erlang-xml-language-support` | 4486a0d | erlang-xml-language-support |

All work happened in the erlang worktree. Nothing was stashed. The only file
left uncommitted in this worktree is this report (per instruction).

---

## 2. Miller calls and what each confirmed

Workspace `julie-extractors-91c17adbdab9` (MAIN checkout; erlang files exist only
in the worktree, so those were read raw guided by the prior reports).

| Call | What it settled |
| --- | --- |
| `context(query='how type facts are extracted and attached to symbols')` | The whole type pipeline in one bundle: `BaseExtractor.type_info: HashMap<String, TypeInfo>`, `base/types.rs:458 TypeInfo`, `factory::convert_types_map`, `julie-extract-cli/src/extraction.rs:703 map_type_facts`, and `resolution.rs:321 TypeFact` (the downstream consumer). Disposition `partial` / `discovery_implementation_present`. |
| `inspect(target='crates/julie-extractors/src/test_detection.rs')` | Symbol listing: `is_test_symbol` at :62-101 with the language match arm, plus `detect_*` per language and `is_test_lifecycle`, `apply_callable_test_metadata`, `mark_*_test_containers`. Told me exactly where the `"erlang"` arm goes and that container marking is a post-pass, not part of `is_test_symbol`. |
| `inspect(target='crates/julie-extractors/src/elixir/types_inference.rs', depth=full)` | One function, `infer_types(specs: &HashMap<String,String>, symbols: &[Symbol]) -> HashMap<String,String>` — the weight class. Confirms the contract is `symbol_id → type string`, matched by name. |
| `search(query='test_container', mode='source')` | The classification shape across 9 languages: metadata booleans `is_test` / `test_container` / `test_lifecycle`, canonicalised in `test_calls.rs:9`. Also surfaced the vue test asserting `role(lifecycle, "is_test")` — the reason lifecycle hooks carry BOTH flags. |

**API-shape evidence proved by these calls:**

1. **Type-fact creation/attachment shape.** Extractors return `HashMap<symbol_id, type_string>`;
   `factory::convert_types_map(types, language)` lifts it to `HashMap<String, TypeInfo>` with
   `generic_params: None, constraints: None, is_inferred: true, metadata: None`. The registry
   macro `define_structured_full_language_extractors` calls `ext.infer_types(&symbols)` and passes
   the result through `convert_types_map`; hand-written extractors (erlang, json, toml) must do the
   same by hand. Golden serialisation is `{symbol_key, resolved_type, generic_params, constraints,
   is_inferred, language, metadata}` (verified against `fixtures/extraction/elixir/basic/expected.json`).
2. **Test-role classification shape.** Three metadata booleans on `Symbol.metadata`. Per
   `capability_matrix.rs:1822 observed_test_detection_roles` (test-evidence-v1): `test_case`
   evidence requires `is_test` WITHOUT `test_lifecycle`; a lifecycle hook sets both. Containers
   set `test_container` and must NOT set `is_test`.
3. **`detect_*` dispatch in `test_detection.rs`.** `is_test_symbol(language, name, file_path, kind,
   annotation_keys, doc_comment)` gates on `is_callable(kind)` then matches `language` to a
   `detect_<lang>` function, falling through to `detect_generic`. It is `pub` and re-exported at
   `lib.rs:135`, so its signature is a public contract — arity cannot be threaded through it.

No Glob→Read→Grep chains were used for orientation.

---

## 3. Grammar node kinds (derived from real parse trees)

A scratch dump test (`tests/erlang/scratch_dump.rs`, written, run twice, **deleted before commit** —
verified absent in `git status`) printed real `tree-sitter-erlang` 0.20.0 trees.

| Construct | Shape |
| --- | --- |
| `-spec open(integer()) -> {ok, account()}.` | `spec` → `atom` + `type_sig` → [`expr_args`, *return node*] |
| `-callback init(A) -> R.` | `callback` → `atom` + `type_sig` → [`expr_args`, *return node*] |
| `-type result(T) :: {ok, T}.` | `type_alias` → `type_name`(`atom` + `var_args`) + *declared form node* |
| `-opaque token() :: binary().` | `opaque` → same shape as `type_alias` |
| `-include_lib("eunit/include/eunit.hrl").` | `pp_include_lib` → `string` |
| `-include("x.hrl").` | `pp_include` → `string` |

Three discoveries that drove the implementation:

1. **`when` guards are a trailing sibling.** `-spec guarded(X) -> Y when X :: integer().` gives
   `type_sig` → [`expr_args`, `var(Y)`, `type_guards`]. Taking the *last* named child would
   return the guard. The return type is **named child index 1**, always.
2. **A multi-clause spec emits one `type_sig` per clause.** `-spec route(get) -> read; (post) -> write.`
   gives `spec` → `atom` + `type_sig` + `type_sig`. `find_child_by_type` returns the first, which is
   the intended behaviour (see judgment 3).
3. **A `-spec` return type is full of `call` nodes.** `{ok, account()} | {error, term()}` parses as
   `pipe` → `tuple` → `call`. This is the concrete reason Task 6's concern 4 exists and why the
   type walk is separate — see §5.

---

## 4. What was built

### 4.1 `crates/julie-extractors/src/erlang/types.rs` (new, 134 lines)

A separate type-tier walk. `collect(base, declarations)` visits `spec` / `callback` /
`type_alias` / `opaque` top-level declarations and fills a `DeclaredTypes` struct with three
maps keyed by `(name, arity)`:

| map | source | value |
| --- | --- | --- |
| `specs` | `spec` | declared return-type text |
| `callbacks` | `callback` | declared return-type text |
| `aliases` | `type_alias`, `opaque` | declared right-hand form |

`infer_types(declared, symbols)` then matches each symbol to its map by kind:
`SymbolKind::Type → aliases`, `SymbolKind::Function` with `callback` metadata → `callbacks`,
other `SymbolKind::Function` → `specs`. Arity comes from the `arity` metadata the symbol tier
already records. Text is whitespace-collapsed so a multi-line spec normalises to one line.

**Three maps, not one, because Erlang gives specs, callbacks and types separate namespaces.**
A module can legally declare `-callback handle(term()) -> A.`, `-spec handle(term()) -> B.` and a
function `handle/1` in the same file; a single map would silently collide. Asserted in
`spec_and_callback_of_the_same_identity_stay_separate`.

### 4.2 Erlang test roles, centralised in `test_detection.rs`

Task 2's local predicate (`erlang/helpers.rs:130 is_eunit_test_name`) was **deleted** and lifted
into `test_detection.rs` as:

- `detect_erlang(name)` — the `"erlang"` arm of `is_test_symbol` (name-only, see judgment 2).
- `ErlangTestModule { eunit, common_test }` + `classify(module_name, includes_eunit_header)`
  and `is_test_container()`.
- `ErlangTestRole { Case, Lifecycle }` + `erlang_test_role(module, name, arity, exported)`.
- Constants `COMMON_TEST_LIFECYCLE_NAMES` (the six specified hooks, verbatim),
  `COMMON_TEST_CONFIG_NAMES` (`all`, `groups`, `suite`), `COMMON_TEST_CASE_ARITY = 1`.

The rules, exactly:

| Signal | Role |
| --- | --- |
| module `*_tests`, or includes `eunit/include/eunit.hrl` | `test_container` |
| module `*_SUITE` | `test_container` |
| `*_test/0` or `*_test_/0`, any module | `is_test` (case) |
| `init_per_*` / `end_per_*` inside a `*_SUITE` | `is_test` + `test_lifecycle` |
| exported `/1` in a `*_SUITE` that is not a config or lifecycle callback | `is_test` (case) |

### 4.3 Wiring

- `erlang/mod.rs` — `test_module: ErlangTestModule` and `declared_types: DeclaredTypes` fields,
  both filled in the existing pre-scan next to `collect_exports`; `classify_test_module` reads
  the `module_attribute` atom and any `pp_include` / `pp_include_lib` string; `infer_types`
  delegates to `types::infer_types`.
- `erlang/attributes.rs` — `extract_module` sets `test_container` when the module classifies.
- `erlang/definition_forms.rs` — `extract_function` hoists the `exported` bool it already
  computed for visibility and passes it to `erlang_test_role`, setting `is_test` and, for a
  lifecycle hook, `test_lifecycle`.
- `registry.rs` — `extract_erlang` now calls `ext.infer_types(&symbols)` and
  `convert_types_map(types, "erlang")` (Task 4/6 precedent for the hand-written arm). The
  "Erlang does not yet ship a type tier" doc comment was replaced with the real reason the
  function is hand-written (structured pending relationships).
- `language_spec/specs.rs` — erlang `PENDING_NO_TYPES_CAPABILITIES` → `FULL_CAPABILITIES`.

### 4.4 Fixtures

Two new fixture rows (single source file each, mirroring the elixir `test_roles` layout):

- `fixtures/extraction/erlang/test_roles/source.erl` — EUnit. Container both ways over
  (`bank_tests` AND `eunit.hrl`), cases `balance_test/0` and `deposit_test_/0`, plus negative
  controls `check_test/1` (right suffix, wrong arity) and `setup/0` (ordinary helper).
- `fixtures/extraction/erlang/test_roles_common_test/source.erl` — Common Test. Container
  `bank_SUITE`, six lifecycle hooks, two cases, plus negative controls `all/0`, `groups/0` and
  `format_report/2` (exported, in the suite, wrong arity).

Both carry a `-spec` so the goldens also register type facts on test-role files.
`basic` and `cross_file` goldens regenerated to pick up their existing `-spec`/`-type`/`-opaque`/
`-callback` declarations.

### 4.5 Capability matrix

- `capabilities.types` `false` → `true`; the `types` capability gap removed
  (`capability_gaps` is now empty for erlang, so `capabilities == target_capabilities == FULL`).
- `kind_coverage.test_detection.supported` `["test_case"]` → `["test_case", "test_container",
  "test_lifecycle"]`; both open gaps removed.
- Two fixture rows registered.
- Diff is exactly 16 insertions / 30 deletions — no whitespace churn elsewhere in the 4,000-line file.

---

## 5. Task 6 concern 4 — the separate walk, and why

Task 6 §350 warned: *"Task 7 should add a separate type-tier walk rather than lifting the
relationship walk's `fun_decl`/`pp_define` restriction, which exists to stop `integer()` in a
`-spec` being read as a call."*

Followed exactly. `erlang/relationships.rs` and `erlang/identifiers.rs` are **unchanged by this
task** (`git diff HEAD~1 --stat` shows neither). `types.rs` re-reads the same top-level
declaration list independently and emits only type facts.

The concern is real, not theoretical: the dump in §3 shows `-spec open(integer()) -> {ok,
account()} | {error, term()}.` contains three `call` nodes (`integer()`, `account()`, `term()`)
that are type applications, not call sites. A regression test pins it —
`spec_types_do_not_leak_into_call_identifiers` asserts no identifier named `term` is emitted
from a module whose only `term()` occurrences are inside `-spec` / `-callback`. The
pre-existing `type_signatures_emit_no_relationship_or_pending_rows` (Task 6) still passes.

---

## 6. Judgment calls (ambiguity → plan-consistent choice)

1. **`erlang/types.rs:94` — `resolved_type` carries the declared RETURN type, not the argument
   list.** The task text says "argument/return types as declared text", but `TypeInfo` exposes a
   single `resolved_type: String` and `convert_types_map` nulls `generic_params` / `constraints` /
   `metadata`, so there is exactly one string per symbol. Chose the return type because (a) it
   matches elixir's `types_inference.rs`, the stated weight class, which stores `integer()` for
   `@spec run(...) :: integer()`, and (b) `julie-extract-cli/src/resolution.rs:1194
   unique_type_symbol` resolves `resolved_type` against workspace type NAMES — storing
   `(integer()) -> ok` would make every erlang type fact unresolvable noise. Argument types remain
   visible in the symbol's own signature (`open/1(Id)`). **Not a scope reduction:** no shape in the
   contract can hold both, and building one would be the type system the task forbids.

2. **`test_detection.rs:518` — the `"erlang"` arm of `is_test_symbol` is name-only; the arity gate
   lives in `erlang_test_role`.** `is_test_symbol` is `pub` and re-exported at `lib.rs:135`, and is
   called by 12 other extractors, so its signature cannot grow an `arity` parameter. EUnit's real
   rule is name **and** arity zero. The extractor — the only caller that emits erlang metadata —
   goes through `erlang_test_role`, which applies both, so **Task 2's emitted behaviour is
   unchanged** (`test_suffixed_function_with_arguments_is_not_a_test_case` pins it). The arity-less
   public entry point is deliberately the more permissive approximation, which is what every other
   path-guarded `detect_*` already is. Documented in the function's doc comment.

3. **`erlang/types.rs:88` — a multi-clause `-spec` records the FIRST clause's return type.**
   `find_child_by_type` returns the first `type_sig`; `insert` uses `entry().or_insert()` so a
   repeated declaration also keeps the first. This mirrors Task 2's established rule that a
   multi-clause function takes its signature and span from the first clause head. Asserted in
   `multi_clause_spec_records_the_first_clause_return_type`.

4. **`test_detection.rs:552` — Common Test cases are gated on `arity == 1`, an addition to the
   plan's stated rule.** The plan said "classify exported functions in a `_SUITE` module that are
   not CT callbacks". Common Test invokes every case as `Case(Config)`, so without the arity gate
   any exported helper in a suite is misclassified. The gate is a strengthening in the honest
   direction and is pinned by a negative control in the golden (`format_report/2`: exported, in the
   suite, not a callback, correctly carries no role). Flagged here because it is a deviation from
   the literal wording. The six lifecycle names are kept **exactly** as specified, per instruction.

5. **`test_detection.rs:539` — `ErlangTestModule` carries a flag per framework rather than being an
   enum.** A `*_SUITE` module may also include `eunit.hrl`; an enum would force an arbitrary
   precedence. Two independent bools mean EUnit `*_test/0` detection stays module-independent
   (Task 2 behaviour) while CT case classification is scoped to `*_SUITE`.

6. **`erlang/mod.rs:29` — the eunit header is matched by substring `eunit/include/eunit.hrl`.**
   Covers `-include_lib("eunit/include/eunit.hrl")` (the documented form) and a `-include` naming
   the same path, without matching an unrelated project header called `eunit.hrl`.

7. **Lifecycle hooks carry BOTH `is_test` and `test_lifecycle`.** Not my invention — it is the
   canonical contract (`apply_callable_test_metadata` sets both; `capability_matrix.rs:1834`
   comments "per test-evidence-v1: lifecycle hooks also set is_test, so test_case evidence requires
   is_test without test_lifecycle"). Container symbols correspondingly do NOT set `is_test`.

---

## 7. Plan mismatches / lead action

1. **Two fixture rows, not one.** The task listed `fixtures/extraction/erlang/test_roles/`. A single
   Erlang source file is a single module, and EUnit vs Common Test roles cannot both be exercised
   honestly in one module without an artificial hybrid. Split into `test_roles` (EUnit) and
   `test_roles_common_test` (Common Test), each a single source file per row as specified. Both
   registered in `capabilities.json`. **No lead action needed** — flagged for the record.

2. **`is_inferred: true` on declared types.** `convert_types_map` hardcodes `is_inferred: true` for
   every language, so erlang's `-spec`-declared types are serialised as inferred. This is factually
   wrong for erlang (and for elixir, and for every other language using the shared helper) but
   changing it is a `factory.rs` / base change outside this task's ownership and would rewrite every
   language's golden. **Lead action: worth a ticket if the distinction matters downstream.**

3. **Migration plan Task 13 checkboxes ticked.** The task authorised this if truthful. All three now
   are: erlang `capabilities == target_capabilities` (§4.5), every closed row has golden evidence
   (§4.4), and both suites pass (§8). Ticked in
   `docs/plans/2026-05-31-julie-code-migration-implementation-plan.md:428-430`. No other text
   changed in that file; the migration plan is still the closure registry for erlang's remaining
   `structural_facts` / `complexity_metrics` / `literals` gaps, which point at Task 8 and stay open.

4. **One gate-forced file outside the listed set.** `cargo fmt --all` reformatted
   `crates/julie-extractors/src/tests/erlang/relationships.rs` — four whitespace-only hunks
   (line wrapping) that were **already unformatted at HEAD 4486a0d**, verified by piping
   `git show HEAD:<file>` through `rustfmt --check`. It is inside
   `crates/julie-extractors/src/tests/erlang/`, which this task owns, and there is no semantic
   change. Included in the commit; reported per the serial-worker-commit rule.

---

## 8. Honesty audit of the FULL claim

| Claim | Evidence |
| --- | --- |
| `capabilities.types = true` | 4 type facts in `basic/expected.json`, 1 in `cross_file`, 2 in `test_roles`, 1 in `test_roles_common_test`. `capability_matrix_type_claim_requires_type_output_in_fixtures` passes. |
| `capabilities == target_capabilities` | `capability_gaps: []` for erlang; both are `{symbols, relationships, pending_relationships, identifiers, types}` all true. |
| `test_detection.supported = [test_case, test_container, test_lifecycle]` | `capability_matrix_test_detection_claims_have_golden_evidence` passes against the two new goldens: `bank_tests`/`bank_SUITE` carry `test_container`, six hooks carry `is_test`+`test_lifecycle`, four functions carry `is_test` alone. |
| No over-claim | The gaps this task did NOT close stay open and honest: `structural_facts` (`erlang.behaviour_declaration`), `complexity_metrics` (`file`), and `literals` (`other`) all still point at Task 8. |

Negative-control coverage in the goldens (all four correctly carry **no** role):
`check_test/1`, `setup/0`, `all/0`, `groups/0`, `format_report/2`.

---

## 9. Verification ledger

Toolchain `RUSTUP_TOOLCHAIN=1.97.1` on every command.

| Command | Result |
| --- | --- |
| `cargo xtask test language erlang` | **90 passed, 0 failed** (67 pre-existing + 23 new) |
| `cargo xtask test golden` | **3 passed, 0 failed** |
| `cargo xtask test capability` | **39 passed + 1 passed** (two commands), 0 failed |
| `cargo xtask test changed <specs.rs> <capabilities.json>` | 39 + 1 + 2 passed, 0 failed |
| `cargo test -p julie-extract-cli` (read-only, capability_snapshot feed) | all suites pass (29 in resolution, 0 failed overall) |
| `cargo test -p julie-extractors --lib` (whole extractor suite) | **3148 passed, 0 failed, 7 ignored** |
| `cargo clippy --workspace --all-targets` | 0 warnings, 0 errors |
| `cargo fmt --all -- --check` | clean |

### Red→green proof (TDD)

Tests were written before the implementation. To prove the red state honestly rather than assert
it, the three behaviour sites (`infer_types`, the role match in `extract_function`, the container
flag in `extract_module`) were temporarily reverted to their HEAD behaviour, the suite run, and the
implementation restored from byte-identical backups:

```
red:   test result: FAILED. 75 passed; 15 failed
green: test result: ok.     90 passed;  0 failed
```

The 15 red failures were exactly the 8 new type assertions and 7 new role assertions. The 8 new
**negative-control** tests passed in both states, which is correct — they assert absence.
No `git stash` was used; backups were restored with `cp` and the restored files verified by
re-running the suite.

### Scratch test discipline

`crates/julie-extractors/src/tests/erlang/scratch_dump.rs` was created, run (twice, for the two
tree shapes in §3), deleted, and its `mod` line reverted before any commit. Verified absent:
`git status` shows no such file and the committed `tests/erlang/mod.rs` declares only
`docs, headers, identifiers, parse_errors, relationships, symbols, test_roles, types, visibility`.

---

## 10. File ownership audit

Committed (20 files), all within the assigned set:

```
crates/julie-extractors/src/erlang/{mod,attributes,definition_forms,helpers}.rs   modified
crates/julie-extractors/src/erlang/types.rs                                       new
crates/julie-extractors/src/tests/erlang/{mod,relationships}.rs                   modified
crates/julie-extractors/src/tests/erlang/{types,test_roles}.rs                    new
crates/julie-extractors/src/test_detection.rs                                     modified
crates/julie-extractors/src/registry.rs                                           modified
crates/julie-extractors/src/language_spec/specs.rs                                modified
fixtures/extraction/capabilities.json                                             modified
fixtures/extraction/erlang/{basic,cross_file}/expected.json                        regenerated
fixtures/extraction/erlang/test_roles{,_common_test}/{source.erl,expected.json}   new
docs/plans/2026-05-31-julie-code-migration-implementation-plan.md                 modified (authorised, §7.3)
```

Not touched: any `xml` file, any `julie-extract-cli/**` source (its suite was run read-only), any
`base/**` except `test_detection.rs`, `erlang/relationships.rs`, `erlang/identifiers.rs`.

This report is deliberately **not** committed.
