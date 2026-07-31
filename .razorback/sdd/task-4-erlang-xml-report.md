# Task 4: Erlang identifiers + calls (Batch A) — report

**Status:** DONE (edits left in the working tree — parallel-lead-commit, no `git add`/`git commit`)
**Commit SHA:** none - parallel-lead-commit
**Worktree state:** path `/Users/murphy/source/julie-extractors/.worktrees/erlang-xml-language-support`,
branch `erlang-xml-language-support`, HEAD `964f2ba`, working tree dirty with exactly the files in §9.

---

## 1. Worktree guard (step 0)

```
pwd    = /Users/murphy/source/julie-extractors/.worktrees/erlang-xml-language-support
branch = erlang-xml-language-support
HEAD   = 964f2ba
```

`git worktree list` showed three worktrees; the other two
(`/Users/murphy/source/julie-extractors` @ `4bee2fe [main]`,
`~/.config/razorback/worktrees/julie-extractors/csharp-locals-params` @ `90542e0`) were not touched.

---

## 2. Miller calls and what each confirmed

Workspace `julie-extractors-91c17adbdab9` (indexes the MAIN checkout, so erlang is absent there by design;
elixir was the model and was read through Miller, erlang files were read raw).

| Call | Confirmed |
| --- | --- |
| `context(query="how the elixir extractor emits call and reference identifiers")` | Pivots `ElixirExtractor` (`src/elixir/mod.rs:28`), `extract_identifiers` (`src/elixir/mod.rs:67`), the `src/elixir/identifiers.rs` module, and `elixir_call_target`/`elixir_call_arguments` in `base/code_structural_facts.rs`. Established that the identifier layer is a standalone module wired through one `extract_identifiers` entry point. |
| `inspect(target="crates/julie-extractors/src/elixir/identifiers.rs", depth="overview")` | The approved shape: `extract_identifiers` → `walk_tree_for_identifiers` → `extract_identifier_from_node` (a `match node.kind()` dispatch) plus `find_containing_symbol_id`. Erlang mirrors this exactly. |
| `search(query="create_identifier", mode="source")` | Usage convention across scala/sql/csharp/gdscript/swift/dart/javascript/go: `base.create_identifier(&node, name, IdentifierKind::X, find_containing_symbol_id(...))`, anchored on the *name* node, not the enclosing expression. |

Raw reads (Miller cannot serve them — the module exists only in this worktree):
`crates/julie-extractors/src/erlang/{mod,helpers,definition_forms}.rs`,
`crates/julie-extractors/src/tests/erlang/mod.rs`, `crates/julie-extractors/src/tests/capability_matrix.rs`,
`crates/julie-extractors/src/tests/golden.rs`, `fixtures/extraction/capabilities.json`.

---

## 3. API-shape evidence (repo-internal)

| Shape | Where proven |
| --- | --- |
| `create_identifier(node, name, kind, containing_symbol_id)` | `base/creation_methods.rs:89`. `Identifier` has **no metadata map** — `id`, `name`, `kind`, `language`, `file_path`, span, `containing_symbol_id`, `target_symbol_id`, `confidence`, `code_context` only. Every distinction this task had to record therefore had to land in `name` or `kind`. |
| `IdentifierKind` vocabulary | `base/kinds.rs:34` — exactly `Call | VariableRef | TypeUsage | MemberAccess`. |
| Containing-symbol lookup | `base/creation_methods.rs:234` `find_containing_symbol_from_map` — innermost same-file symbol whose span contains the node's start position. |
| Depth guards | `crate::tree_traversal::{should_visit_tree_depth, child_tree_depth}`, as used by `elixir/identifiers.rs:30`. |
| Golden regeneration | `UPDATE_GOLDEN=1` + `cargo test -p julie-extractors --features test-golden --lib golden` (the exact command `xtask test golden` runs, `xtask/src/test_tiers.rs:233`). |
| Identifier claims in the matrix | `tests/capability_matrix.rs:1316` `assert_supported_kind_claims(..., "identifier", ...)`: every `kind_coverage.identifiers.supported` entry must be observed in a golden, kinds must parse via `IdentifierKind::try_from_string`, and a kind may not be classified twice (supported **and** open gap). `identifiers` is not in the bidirectional `assert_golden_domain_claims_match` set, but the claim was filled to exactly the observed set anyway. |

---

## 4. Grammar node kinds (derived from real parse trees, not memory)

Three scratch dump tests (`tests/erlang/scratch_dump.rs`, written, run, **deleted** before reporting) printed
the full `tree-sitter-erlang` 0.20.0 tree for calls, type signatures, control flow, and preprocessor forms.
Every node kind the new code switches on comes from that output:

| Construct | Node kinds observed |
| --- | --- |
| `g(X)` local call | `call` → `atom` + `expr_args` |
| `lists:reverse(X)` remote call | `remote` → `remote_module` (→ `atom` + `:`) + `call` (→ `atom` + `expr_args`) |
| `?MODULE:g()` | `remote` → `remote_module` → **`macro_call_expr`** (no `atom`) |
| `Fun(X)` dynamic call | `call` → `var` + `expr_args` (no `atom` child) |
| `fun g/1` | `internal_fun` → `atom` + `arity` |
| `fun mod:h/2` | `external_fun` → `module` (→ `atom` + `:`) + `atom` + `arity` |
| `fun(A) -> A end` | `anonymous_fun` → `fun_clause` (names nothing) |
| `?LOG("hi")` | `macro_call_expr` → `var` + `macro_call_args` |
| `?PI` | `macro_call_expr` → `var` (no `macro_call_args`) |
| `#rec{a = 1}` | `record_expr` → `record_name` (→ `#` + `atom`) + `record_field`* (→ `atom` + `field_expr`) |
| `R#rec{a = 3}` | `record_update_expr` → `var` + `record_name` + `record_field`* |
| `R#rec.a` | `record_field_expr` → `var` + `record_name` + `record_field_name` (→ `.` + `atom`) |
| `#rec.a` | `record_index_expr` → `record_name` + `record_field_name` |
| `-import(lists, [reverse/1])` | `import_attribute` → `atom` + `fa`* (→ `atom` + `arity` → `integer`) |
| guard `when is_integer(N)` | `guard` → `guard_clause` → `call` |
| `try … catch error:Reason` | `catch_clause` → **`try_class`** (→ `atom` + `:`) — *not* `remote_module`, so no false module reference |
| `-ifdef(X). f() -> ok. -endif.` | `pp_ifdef` / `pp_endif` are flat siblings; `fun_decl` stays a direct `source_file` child |

**Load-bearing discovery:** type signatures spell type names with the *same* `call` node a real call uses —
`-spec f(...) -> list().`, `-callback init(Args :: term())`, `-opaque tok() :: binary().`, and
`-record(r, {a :: integer()})` all contain `call → atom "integer"/"term"/"binary"/"list"`. Walking the whole
tree (the elixir shape) would have emitted `Call` identifiers named `integer`, `term`, `binary`, `list` —
exactly the class of bogus row this task forbids. The walk therefore starts from the two **executable**
top-level forms only: `fun_decl` and `pp_define` (minus its `macro_lhs`). Verified by
`type_signatures_do_not_emit_call_identifiers`, which asserts a file made entirely of signatures emits zero
identifiers.

---

## 5. What was built

`crates/julie-extractors/src/erlang/identifiers.rs` (new, ~240 lines), wired through the existing
`ErlangExtractor::extract_identifiers` entry point (`erlang/mod.rs`) and through `registry.rs` (§8.1).
No `calls.rs` split: elixir's `calls.rs` is about *symbol* extraction from `def` macro calls, not identifiers,
so the split does not transfer. One module, one dispatch, no cross-file resolution logic.

### Emitted model

| Erlang construct | Rows | Anchored on |
| --- | --- | --- |
| `g(X)` | `Call g` | the callee `atom` |
| `lists:reverse(X)` | `TypeUsage lists` + `Call reverse` | the module `atom`, then the callee `atom` |
| `reverse(X)` with `-import(lists, [reverse/1])` | `Call reverse` + `TypeUsage lists` | both on the callee `atom` |
| `length(X)` / `self()` / `is_list(X)` (auto-imported BIF) | `Call length` **only** | the callee `atom` |
| `Fun(X)` | *(nothing)* | — |
| `fun g/1` | `VariableRef g` | the function `atom` |
| `fun lists:reverse/1` | `TypeUsage lists` + `VariableRef reverse` | module `atom`, function `atom` |
| `?LOG(X)` | `Call LOG` | the macro `var` |
| `?LIMIT` | `VariableRef LIMIT` | the macro `var` |
| `#rec{a = 1}` (construction **or** head pattern) | `TypeUsage rec` + `MemberAccess a` | record `atom`, field `atom` |
| `R#rec{a = 3}` | `TypeUsage rec` + `MemberAccess a` | same |
| `R#rec.a` / `#rec.a` | `TypeUsage rec` + `MemberAccess a` | same |

Containing symbol: resolved once per top-level declaration. A multi-clause function is a run of sibling
`fun_decl` nodes but a single symbol spanning only the *first* clause, so later clauses reuse the scope
resolved for the first clause of the same name/arity (`function_scope`). Without that, every identifier in
clause 2+ would have carried `containing_symbol_id: null`. Asserted by
`later_clauses_bind_identifiers_to_the_same_function_symbol`. Identifiers inside a `-define` body bind to the
macro's `Constant` symbol (`macro_body_calls_bind_to_the_macro_symbol`).

---

## 6. Judgment calls (each one a plan-consistent choice, with the reason)

1. **Remote call = `TypeUsage` module row + `Call` callee row** (`erlang/identifiers.rs:131`).
   The task spec says remote calls are kind `Call`; elixir records a remote receiver as its **own** row
   (`elixir/identifiers.rs:75-95`: `alias` → `TypeUsage`, then the member). Mirroring elixir's *shape* (a
   separate receiver row) while keeping the task's *kind* (`Call` for the callee, as go/rust/csharp also do
   for `obj.Method()`) satisfies both. Elixir uses `MemberAccess` for the callee of a qualified call; that was
   deliberately **not** copied, because the task spec is authoritative and because `MemberAccess` is already
   spoken for by record fields here.

2. **Fun references are `VariableRef`** (`erlang/identifiers.rs:207`). `Identifier` has no metadata map, so
   the call/fun-ref distinction had to be a `kind`. Elixir has no capture (`&f/1`) arm to copy. Within the
   four-kind vocabulary, a `fun g/1` *reads the function as a value* rather than invoking it, which is exactly
   what the repo-wide "variable_ref complement arm" already means in go/csharp/elixir (a bare identifier used
   as a value). Locked by `fun_reference_is_distinguishable_from_a_call_to_the_same_function`, which asserts
   `fun g/1` and `g(X)` produce two rows with the same name and different kinds.

3. **BIFs attribute to no module** (`erlang/identifiers.rs:147`). `length/1`, `self/0`, `spawn/1`, `is_list/1`
   are auto-imported from `erlang`, but synthesising a `TypeUsage erlang` row would invent a module reference
   the source never wrote and that no workspace symbol resolves. They emit a bare `Call` row and nothing else.
   Asserted by `auto_imported_bif_calls_emit_no_module_reference` (which also asserts *zero* `TypeUsage` rows
   in that file) and visible in both goldens (`self` in `basic`, `length` in `negative`).

4. **`-import`-ed calls carry the module on the callee atom** (`erlang/identifiers.rs:174`). An imported call
   is semantically identical to a remote call, so it gets the identical two-row shape; the only honest anchor
   available is the callee atom, because the source does not spell the module at the call site. The `-import`
   *attribute itself* emits nothing — identifiers are usage rows, and the import edge belongs to Task 6's
   relationship tier (the same reason `-export`, `-behaviour`, and `-include_lib` emit nothing). Attribution is
   arity-sensitive: `-import(lists, [reverse/1])` does not attribute `reverse(X, Y)`
   (`import_attribution_is_arity_sensitive`). The one visible consequence in the golden is two rows at the same
   span (`basic` L43 c10: `type_usage lists` + `call reverse`); that is intended, not a duplicate.

5. **Macro usage splits on arity** (`erlang/identifiers.rs:~215`): `?LOG(X)` is a `Call`, `?LIMIT` is a
   `VariableRef`. Both name the `-define` symbol; the split records "invoked" vs "read", consistent with
   judgment 2. `?MODULE:` in a module-qualifier position falls out as a `VariableRef MODULE` (predefined macro,
   no `-define` in file) and — importantly — produces no module `TypeUsage`, so no bogus module reference.

6. **Type signatures are skipped wholesale** (§4). `-spec`, `-callback`, `-type`, `-opaque`, and `-record`
   field types are not walked. A record reference inside a `-spec` (`#account{}`) is therefore *not* emitted;
   that is a deliberate deferral to Task 7 (types), preferred over walking those subtrees in a restricted mode
   that suppresses `call` nodes. `type_usage` is still an honest supported claim because record expressions in
   function bodies and heads emit it.

7. **`DATA_ONLY_CAPABILITIES` reused instead of a dedicated const** (`language_spec/specs.rs:192`). It is
   value-identical (symbols + identifiers) and it is what **xml** already uses after Task 3
   (`specs.rs:304`), so erlang and xml now describe the same tier with the same const. The name is a leftover
   from data languages; introducing a second const with the same value would have been worse than the naming
   oddity. See §8.2 for the consequence.

---

## 7. Fixtures (hand-reviewed, not just regenerated)

### `erlang/basic` — extended to carry the whole identifier vocabulary

Added `-import(lists, [reverse/1]).`, exported `history/1`, and a `history/1` function exercising an imported
call, a record field read, a bare macro read, an internal fun reference, an external fun reference, and a BIF
call. Symbol side effect: one new `function` symbol (`history`, Public, `-doc` attached, `clause_count: 1`);
all 14 pre-existing symbols are unchanged in kind, visibility, signature, doc, and metadata.

All 23 identifier rows, hand-checked line by line:

```
L13 c18 type_usage io          in=LOG          <- -define(LOG(Msg), io:format(...)) body
L13 c21 call       format      in=LOG
L25 c 5 type_usage account     in=open         <- #account{id = Id}
L25 c13 member_access id       in=open
L28 c 9 type_usage account     in=balance      <- head pattern #account{balance = B}
L28 c17 member_access balance  in=balance
L32 c 9 type_usage account     in=deposit      <- Acct#account{...} (update)
L32 c17 member_access balance  in=deposit
L32 c32 type_usage account     in=deposit      <- Acct#account.balance (field read)
L32 c40 member_access balance  in=deposit
L38 c 5 call       LOG         in=audit        <- ?LOG(Acct)
L43 c10 type_usage lists       in=history      <- -import attribution (same span as the call)
L43 c10 call       reverse     in=history
L43 c24 type_usage account     in=history      <- Acct#account.id
L43 c32 member_access id       in=history
L44 c13 variable_ref MAX_BALANCE in=history    <- ?MAX_BALANCE
L45 c17 variable_ref balance   in=history      <- fun balance/1
L46 c16 type_usage erlang      in=history      <- fun erlang:length/1
L46 c23 variable_ref length    in=history
L47 c32 call       self        in=history      <- BIF, no module row
L50 c 8 call       balance     in=balance_test
L50 c17 type_usage account     in=balance_test
L50 c25 member_access id       in=balance_test
```

Zero rows from `-module`, `-moduledoc`, `-behaviour`, `-export`, `-export_type`, `-import`, `-record`,
`-type`, `-opaque`, `-callback`, `-spec`, or `-doc`. `relationships`, `pending_relationships`,
`structured_pending_relationships`, `types`, `literals`, `structural_facts`, `complexity_metrics`, and
`parse_diagnostics` all stay empty.

### `erlang/negative` — reframed from "zero rows" to "no wrong rows"

The header comment was rewritten: the fixture no longer claims the tier emits nothing, it claims the tier
emits the *right* rows. Source extended with a BIF call, a dynamic call through a variable, and a fun
reference to a function that is also called directly. All 16 rows hand-checked:

```
L18 type_usage request / member_access id / type_usage erlang / call unique_integer / member_access payload
L19 type_usage timer / call sleep / variable_ref TIMEOUT
L20 call length (BIF - no module row) / call queue
L22 variable_ref queue                     <- fun queue/1, distinct from the two Call rows
L23 type_usage lists / call reverse / call queue
L25 type_usage request / member_access payload
```

`Fun(Request)` (dynamic call) contributes nothing. `relationships`, `pending_relationships`, and
`structured_pending_relationships` remain `[]`, so the fixture still discharges its original duty.

---

## 8. Plan mismatches / files outside the assigned list (LEAD ACTION)

### 8.1 `crates/julie-extractors/src/registry.rs` — required, not optional

`extract_erlang` (`registry.rs:435`) hardcoded `identifiers: Vec::new()`. The task's file list omits
`registry.rs`, but no golden can show an identifier row without it. One-line change:

```rust
let identifiers = ext.extract_identifiers(tree, &symbols);
…
identifiers,
```

Task 2 also had to modify `registry.rs`; the plan's Task 4 file list should list it. Nothing else in the file
was touched.

### 8.2 `crates/julie-extractors/src/language_spec/mod.rs` — orphaned const removed

Ratcheting the erlang row from `SYMBOLS_ONLY_CAPABILITIES` to `DATA_ONLY_CAPABILITIES` left
`SYMBOLS_ONLY_CAPABILITIES` (added by Task 2) with **zero** users, which produces
`warning: constant SYMBOLS_ONLY_CAPABILITIES is never used` — observed live during `xtask test capability`.
The branch gate expects zero warnings, so the 8-line const was deleted. If the lead prefers Task 2's const to
survive for a future symbols-only language, revert this hunk and add an `#[allow(dead_code)]` instead — but
leaving it as-is fails the warning gate.

### 8.3 `kind_coverage.literals` open gap now names a task that has shipped

The erlang literals gap said "literal capture is driven by call-argument analysis, which the symbol-only
Erlang tier does not perform" and pointed at **Task 4**. Call-argument analysis now happens, so that reason
became false. Literal capture itself was **not** implemented: elixir's `record_elixir_call_arg_literals`
depends on a downstream carrier gate that lives in `crates/julie-extract-cli/**` (Task 5's exclusive
ownership), so wiring erlang literals could not be verified end-to-end from inside this task's boundary.
The gap was rewritten to state the real reason and re-pointed at **Task 8 (Erlang real-world corpus gate)**.
**That re-pointing is a recommendation, not a decision** — the lead should confirm Task 8 is the right owner
(Task 6 is the alternative).

### 8.4 Gap wording refreshed for the relationship/pending/types rows

Three `capability_gaps` reasons and two `kind_coverage.relationships` gap reasons said "while the extractor is
symbol-only" / "the symbol-only registry entry". That is no longer true, so the phrasing was updated to
"while the extractor ships the symbol and identifier tiers only". No status, capability, evidence, or
`planned_closure_task` value changed, so `capability_matrix_open_rows_have_planned_closure_task` still resolves
against the migration-plan doc (which was **not** touched, as instructed — `xtask test capability` agrees).

### 8.5 No impact on the CLI contract count

`crates/julie-extract-cli/tests/operations_contract.rs:145` asserts `open_reference_resolution_gaps == 109`.
That query filters on `capability LIKE 'reference_resolution.%'`, and those rows are generated per-language
from `tier2_enabled` / `tier3_static_type_proven` allow-lists (`capability_snapshot.rs:107`) — they do not
depend on identifier emission. The gap this task removed has `capability = "identifiers"`, so the count is
unaffected. Verified by reading, **not** by running (the CLI compile is Task 5's, and
`cargo xtask test changed` was deliberately not run).

---

## 9. Files changed

Created:
- `crates/julie-extractors/src/erlang/identifiers.rs`
- `crates/julie-extractors/src/tests/erlang/identifiers.rs`

Modified (assigned):
- `crates/julie-extractors/src/erlang/mod.rs` (`mod identifiers;` + real `extract_identifiers`)
- `crates/julie-extractors/src/tests/erlang/mod.rs` (`mod identifiers;` + `extract_with_identifiers`/`named`/`only`/`identifier_inventory` helpers)
- `crates/julie-extractors/src/language_spec/specs.rs` (capability ratchet only)
- `fixtures/extraction/capabilities.json` (erlang row only)
- `fixtures/extraction/erlang/basic/{source.erl,expected.json}`
- `fixtures/extraction/erlang/negative/{source.erl,expected.json}`

Modified (forced, outside the assigned list — see §8):
- `crates/julie-extractors/src/registry.rs`
- `crates/julie-extractors/src/language_spec/mod.rs`

**Not touched:** `crates/julie-extract-cli/**`, any xml file, `base/**`, `tests/capability_matrix.rs`,
`docs/plans/2026-05-31-julie-code-migration-implementation-plan.md`.

---

## 10. Capability matrix row (honesty audit)

- `capabilities.identifiers`: `false` → **`true`**; the top-level `capability_gaps` row for `identifiers` was
  removed (an implemented capability with a recorded gap is the dishonest state the matrix guards against).
- `kind_coverage.identifiers`: `supported = ["call", "member_access", "type_usage", "variable_ref"]`,
  `not_applicable = []`, `open_gaps = []`. Every one of the four is emitted by a registered golden — `call`
  (`basic` L38, `negative` L20), `member_access` (`basic` L25), `type_usage` (`basic` L13),
  `variable_ref` (`basic` L44). The three pre-existing open gaps (call / variable_ref / type_usage) were
  removed because a kind may not be both supported and gapped.
- `LanguageSpec` row: `DATA_ONLY_CAPABILITIES` — value-identical to the matrix row, which
  `capability_matrix_matches_registry_entries` enforces.
- Everything erlang does **not** do is still recorded as a typed gap: `relationships`,
  `pending_relationships`, `types` (capability gaps); `structural_facts`, `complexity_metrics`, `literals`,
  `test_container`, `test_lifecycle` (kind gaps).
- Nothing was claimed because a vector is non-empty: `capability_matrix_supported_kind_claims_have_fixture_evidence`
  checks every claim against the goldens and passes.

---

## 11. Verification ledger

| Command | Result |
| --- | --- |
| `RUSTUP_TOOLCHAIN=1.97.1 cargo xtask test language erlang` | **49 passed, 0 failed** (32 pre-existing + 17 new identifier tests) |
| `RUSTUP_TOOLCHAIN=1.97.1 cargo xtask test golden` | **3 passed, 0 failed** — `erlang/basic` and `erlang/negative` regenerated and hand-reviewed (§7) |
| `RUSTUP_TOOLCHAIN=1.97.1 cargo xtask test capability` | **39 passed + 1 passed, 0 failed** (worker ceiling) |
| `RUSTUP_TOOLCHAIN=1.97.1 cargo test -p julie-extractors --lib` | **3106 passed, 0 failed, 7 ignored** (whole extractor crate, to catch registry/language_spec fallout) |
| `RUSTUP_TOOLCHAIN=1.97.1 cargo fmt --all -- --check` | clean |
| `RUSTUP_TOOLCHAIN=1.97.1 cargo clippy -p julie-extractors --all-targets` | zero warnings |

**Not run, deliberately:** `cargo xtask test changed` (compiles `julie-extract-cli`, which Task 5 owns — the
lead runs it after both land), `cargo run -p julie-extract-cli -- languages --json` (same reason), and the
real-world/corpus tiers (Task 8 / Task 10).

### New test inventory (17, all in `tests/erlang/identifiers.rs`)

`local_call_binds_to_the_calling_function`, `remote_call_emits_a_module_type_usage_and_a_call`,
`fun_reference_is_distinguishable_from_a_call_to_the_same_function`,
`external_fun_reference_emits_a_module_type_usage_and_a_variable_ref`,
`imported_function_call_attributes_to_the_import_module`, `import_attribution_is_arity_sensitive`,
`auto_imported_bif_calls_emit_no_module_reference`,
`macro_usage_with_arguments_is_a_call_and_bare_macro_usage_is_a_variable_ref`,
`macro_body_calls_bind_to_the_macro_symbol`, `record_construction_emits_record_and_field_references`,
`record_field_access_and_update_emit_record_and_field_references`,
`record_index_expression_references_the_record_and_the_field`,
`record_patterns_in_function_heads_reference_the_record`, `type_signatures_do_not_emit_call_identifiers`,
`later_clauses_bind_identifiers_to_the_same_function_symbol`,
`dynamic_call_through_a_variable_emits_no_identifier`,
`export_and_attribute_declarations_emit_no_identifiers`.

Every test asserts concrete kinds, names, and (where relevant) containing symbol ids. TDD order was followed:
the 16 initial tests were written first and observed failing (13 failed, 3 passed vacuously against the empty
vector) before `identifiers.rs` existed.

---

## 12. Self-review

| Acceptance criterion | Status |
| --- | --- |
| `cargo xtask test language erlang` green (32 existing + new) | ✅ 49 passed |
| `cargo xtask test golden` green; regenerated expected.json hand-reviewed | ✅ §7 |
| Remote call vs fun reference emit distinguishable rows (focused test) | ✅ `fun_reference_is_distinguishable_from_a_call_to_the_same_function` + `external_fun_reference_…` |
| BIF calls don't produce bogus unresolved module references (asserted) | ✅ `auto_imported_bif_calls_emit_no_module_reference` + golden evidence |
| identifiers=true honest in matrix; `cargo xtask test capability` green | ✅ §10, 39+1 passed |
| No commit; verified diff handed to lead | ✅ working tree only |

Findings fixed during self-review:
- Deleted all three scratch dump tests before reporting.
- Caught the type-signature `call`-node collision from the parse tree **before** writing the walk, which is
  why the golden has no `integer`/`term`/`binary`/`list` rows.
- Caught `containing_symbol_id: null` for multi-clause functions and fixed it with the clause-scope map.
- Caught and removed the dead `SYMBOLS_ONLY_CAPABILITIES` warning the ratchet introduced (§8.2).
- Added `record_index_expression_references_the_record_and_the_field` after noticing `record_index_expr` was
  handled by the code but not locked by a test.

Known limitations, deliberately deferred and recorded as gaps or noted here:
- Record references inside `-spec`/`-type` bodies are not emitted (Task 7).
- Plain Erlang variable reads (`X`) emit no `variable_ref` row: the Erlang tier emits no symbols for locals or
  parameters, so those rows would be permanently unresolvable noise. `variable_ref` is claimed on the strength
  of fun references and bare macro reads.
- Call-argument string literals are not captured (§8.3).
- Anonymous funs (`fun(A) -> A end`) name nothing and emit nothing, correctly.
