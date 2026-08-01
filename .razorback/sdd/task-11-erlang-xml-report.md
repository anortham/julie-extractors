# Task 11 — Close cheap quality-bar debts (erlang complexity + literals, xml literals)

**Worktree:** `/Users/murphy/source/julie-extractors/.worktrees/erlang-xml-language-support`
**Branch:** `erlang-xml-language-support`
**Base commit at start:** `ffe9616` (clean)
**Toolchain:** every cargo command prefixed `RUSTUP_TOOLCHAIN=1.97.1`; global default untouched.

## Result

`node scripts/language-data-quality-report.mjs --strict` now reports **one** quality-bar debt,
`erlang.structural_facts` (owned by Task 12). It was four.

```
languages: 38
silent_cells: 0
quality_bar_debts: 1
open_gap_backlog: 62      (was 66)

## Quality-Bar Debt
erlang.structural_facts open_gap
```

## Quality-bar semantics (read before building, per the task)

`scripts/language-data-quality-report.mjs:333-343` defines the bar: for each language, for each
domain in `expectedDomainsFor(language)`, a debt is recorded when
`kind_coverage.<domain>.supported` **and** `kind_coverage.<domain>.not_applicable` are both empty.
An `open_gaps` entry silences the *silent cell* check but does **not** satisfy the quality bar.
Erlang is a code language, so its expected set is `CODE_LANGUAGE_EXPECTATIONS` (8 domains); xml is
in `DOMAIN_LANGUAGES`, so its expected set is `DOMAIN_LANGUAGE_EXPECTATIONS` (identifiers, literals,
source_regions, structural_facts). So the bar is: put real kinds in `supported`, and — because the
capability gates are bidirectional — back every claimed kind with golden rows.

## Debt 1 — erlang `complexity_metrics`

**Built:** `ERLANG_CONFIG` in `crates/julie-extractors/src/base/complexity_metrics.rs`, plus the
`"erlang" => Some(ERLANG_CONFIG)` arm in `config_for_language`.

- decisions: `case_expr`, `cr_clause`, `if_expr`, `if_clause`, `try_expr`, `catch_clause`,
  `catch_expr`, `receive_expr`, `receive_after`, `maybe_expr`, `guard_clause`
- loops: `list_comprehension`, `binary_comprehension`, `map_comprehension`
- parameters: none — see "parameter_count is NULL" below

Rule encoded: Erlang branches *inside expressions*, so each branching container counts plus one per
arm (the switch-container-plus-arm convention `BASH_CONFIG`/`VBNET_CONFIG`/`POWERSHELL_CONFIG`
already use), and each `;`-separated `guard_clause` counts wherever a guard appears. Clause-based
dispatch of a *definition* (`function_clause`, `fun_clause`) is deliberately **not** counted:
`clause_count` metadata already records it, and every single-clause anonymous `fun` would otherwise
cost a decision. `try_after` is not counted — an `after` block always runs, it is not a branch.

**One extra local change inside the same function** (`complexity_span_for_symbol`): erlang returns
the declaration span instead of `symbol.body_span`. Reason, verified on the committed golden:
`tree-sitter-erlang` keeps the clause body under `function_clause`, one level below the `fun_decl`
a function symbol spans, so `infer_body_span` finds neither a `body` field nor a `BODY_NODE_KINDS`
child and falls through to text heuristics that land on the first brace/paren run. Actual
`fixtures/extraction/erlang/basic/expected.json` body spans before my change:

```
open      -> '{id = Id}'
balance   -> '{balance = B}'
audit     -> '(Acct) ->\n    ?LOG(Acct)'
history   -> '{Ids, Limit, Reader, Sizer, self()}'
```

Using those spans made symbol-scope metrics arbitrary (`classify` measured 2 decisions instead of
6 because the brace run only covered one case arm). The existing
`body_covers_meaningful_share` escape hatch (scala/vbnet) does **not** fix this reliably — `audit`'s
bogus span is 24 bytes against a ~40-byte declaration, so it passes the 50% test. `fun_decl` already
spans exactly one clause head plus body, which is the scope the metric wants, so erlang uses it
directly. See "Concerns" — the underlying `body_span` defect is pre-existing and NOT fixed here.

**parameter_count is NULL for erlang** (documented in the config comment): erlang clause heads bind
arbitrary patterns (`var`, `atom`, `tuple`, `record_expr`, `binary`, …), so there is no closed set of
`parameter_node_kinds` to enumerate; a partial list would produce wrong counts. `BASH_CONFIG` has the
same shape. Arity is already carried in the symbol signature (`open/1(Id)`) and in
`metadata.arity`.

## Debt 2 — erlang `literals`

**Built:** `record_call_arg_literals` + `call_carrier` in
`crates/julie-extractors/src/erlang/identifiers.rs`, called from `emit_call`.

- carrier = verbatim callee: `io:format` for a remote call, the bare atom (`audit`) for a local,
  imported, or auto-imported one. A remote call whose module is a variable (`Mod:run(...)`) names no
  module in source and falls back to the bare callee.
- `kind` stays `LiteralKind::Other`; `arg_position` counts over the whole `expr_args` list; only
  direct arguments are captured (a string nested in a list/tuple argument is not a call-arg literal).
  This matches the go and elixir legs exactly.
- The executable-forms restriction is preserved: literals ride the existing `fun_decl` / `pp_define`
  walk, so `-spec`/`-type`/`-callback`/`-moduledoc`/`-include_lib` strings produce nothing. Locked by
  `declaration_strings_are_not_call_argument_literals`.
- The `is_remote` early-return in `emit_call` still keys off `node.parent().kind() == "remote"`
  exactly as before, so the `-import` arity lookup behaviour is byte-identical.

## Debt 3 — xml `literals`

**Built:** `extract_element_references` → `extract_element_facts` in
`crates/julie-extractors/src/xml/identifiers.rs` (both call sites in `xml/mod.rs` renamed). One pass
over `elements::attributes` now records every non-empty attribute value as a literal via the existing
`base/config_literals.rs::tag_attribute_carrier`, then keeps the pre-existing `type_usage` identifier
for the `base`/`element`/`ref`/`type` subset.

- carrier keeps the tag's prefix and case (`xs:element.name`) because XML names are case-sensitive;
  `tag_attribute_carrier` lowercases the attribute half, which is why `schemaLocation` files under
  `xs:import.schemalocation`. That is the shared cross-markup helper's contract, used as directed.
- **Cardinality golden stays bounded:** the 4,000-row fixture gained **5** literals total
  (`catalog.name`, two `part.name`, two `part.type`) because the repeated `<row>`/`<cell>` elements
  carry no attributes. Golden grew 83 lines.

## Fixture and registry changes

- **New:** `fixtures/extraction/erlang/control_flow/{source.erl,expected.json}` — every erlang
  in-expression branch (case + guarded arm, if, try/of/catch/after, receive + after, old-style
  `catch`), both comprehension flavours, and remote + local string call arguments, with `handle/1`
  as an all-zero negative control and `-moduledoc`/`-include_lib` strings as literal negative
  controls. Registered in the erlang `fixtures` list.
- `capabilities.json`: erlang `complexity_metrics.supported = ["file","symbol"]`,
  erlang `literals.supported = ["other"]`, xml `literals.supported = ["other"]`; the three matching
  `open_gaps` entries removed. Remaining open pointers verified intact — erlang
  `structural_facts` and the two xml `capability_gaps` still resolve against
  `docs/plans/2026-05-31-julie-code-migration-implementation-plan.md`, whose "Task 13: Erlang
  Capability Closure" and "Task 14: XML Reference Edge Closure" headings are unchanged.
- Goldens regenerated with `UPDATE_GOLDEN=1`; all nine touched goldens are pure additions
  (the only deleted lines are `"complexity_metrics": []` / `"literals": []`).
- `docs/plans/2026-05-31-julie-code-migration-implementation-plan.md` Tasks 13/14 updated: what
  closed, by which plan/task, and per-domain acceptance boxes (erlang `structural_facts` left open).

Contract-faithful check on the regenerated `control_flow` golden — hand-tally vs emitted:

| scope | decisions | loops | depth | hand-tally |
| --- | --- | --- | --- | --- |
| file | 19 | 2 | 4 | 7 + 6 + 6 = 19 ✓ |
| classify | 7 | 2 | 3 | 2 head guards + case + 3 arms + 1 arm guard = 7; list + binary comp = 2 ✓ |
| serve | 6 | 0 | 4 | receive + arm + after + try + of-arm + catch = 6 ✓ |
| drain | 6 | 0 | 3 | if + 2 if_clause + 2 guards + catch_expr = 6 ✓ |
| audit / handle | 0 | 0 | 0 | no branch ✓ |

Literals emitted: `zero`→`audit`, `big ~p~n`→`io:format`, `failed ~p~n`→`io:format`,
`closed`→`audit`. `parse_diagnostics` is `[]`.

## Miller evidence

| Call | What it confirmed |
| --- | --- |
| `workspace operation=list filter=julie` | the worktree was unregistered; the main checkout would not carry the branch's erlang/xml sources |
| `workspace operation=open path=<worktree>` | primed `erlang-xml-language-support-88dbac830547`, 196,638 symbols — every call below used that selector |
| `context query="how complexity_metrics node kinds are configured per language in extractors"` | disposition `partial`; pivots were generic `base/` namespaces, so it did **not** find the config table — recorded as a Miller miss, not a guess |
| `inspect crates/julie-extractors/src/base/config_literals.rs` | exact contract of the helper named in the task: `tag_attribute_carrier(tag_name: &str, attribute_name: &str) -> String`, plus `record_config_string_literal` / `is_config_string_value_node` |
| `search query="complexity" mode=file` | located `base/complexity_metrics.rs` + the per-language test convention `tests/{lua,php,r}/complexity.rs` |
| `inspect crates/julie-extractors/src/base/complexity_metrics.rs` | full symbol map: `ComplexityLanguageConfig` (14-31), `collect_complexity_metrics` (47-106), `complexity_metric_scopes_for_language` (109-115), `complexity_span_for_symbol`, `config_for_language` |
| `inspect record_literal scope=base/extractor.rs depth=full` | signature `(&mut self, node, literal_text: String, carrier: Option<String>, arg_position: u32, containing_symbol_id) -> Literal`, doc "kind is always LiteralKind::Other here; the artifact language-policy pass reclassifies and gates by carrier", and the 33-consumer fallback ref list that showed html/elixir/go as the reference integrations |
| `inspect crates/julie-extractors/src/html/identifiers.rs` | the reference `tag_attribute_carrier` consumer: 4 methods, `extract_identifier_from_node` is where the literal is recorded |
| `search query="record_string_literal"` | near-match ranking surfaced `record_literal`, `record_config_string_literal`, `vue::record_template_attribute_literal`, `LiteralKind::StringLiteral` — the whole literal-capture family in one call |
| `inspect decode_string_literal` (ambiguity result) | two definitions (`base/extractor.rs:225` method, `base/string_literals.rs:7` function); read the latter to confirm the `kind.contains("string") \|\| kind.contains("char")` gate erlang `string` nodes pass |

**Where Miller could not prove a shape** (stated rather than guessed): Miller indexes the Rust
workspace, not the vendored tree-sitter grammars, so no Miller call could establish erlang node
kinds. Those came from two non-Miller sources, both recorded: the pinned grammar's
`~/.cargo/registry/.../tree-sitter-erlang-0.20.0/src/node-types.json` (field/child shapes for
`fun_decl`, `function_clause`, `expr_args`, `guard`, `cr_clause`, `try_expr`, `receive_expr`,
`call`, `remote`, `remote_module`, `string`), and a throwaway `to_sexp()` dump test run inside the
crate and deleted afterwards. The dump is what proved the load-bearing shape the node-types file is
ambiguous about: a remote call parses as `(remote module: (remote_module module: (atom)) fun: (call
expr: (atom) args: (expr_args …)))` — the `call` is a **child** of `remote` — which is why
`call_carrier` reads the module off `node.parent()` and why the existing `is_remote` check is
`node.parent().kind() == "remote"`.

## Gate results

| Gate | Command | Invariant proved | Result |
| --- | --- | --- | --- |
| focused red/green | `cargo test -p julie-extractors -- erlang::complexity erlang::literals xml::literals` | the three new capabilities emit the documented rows/carriers and nothing else | 10 passed (9 failed first, pre-implementation) |
| golden | `cargo test -p julie-extractors --features test-golden golden` | canonical extraction of every registered fixture equals the committed golden, without `UPDATE_GOLDEN` | 3 passed |
| capability matrix | `cargo test -p julie-extractors --features test-capability-matrix capability_matrix` | claims ⇔ golden evidence in both directions, incl. `complexity_metric_claims_have_fixture_evidence` (needs a nonzero decision/loop), `literal_claims_have_fixture_evidence`, `complexity_metric_claims_match_registry`, `open_rows_have_planned_closure_task` | 39 passed |
| default suite | `cargo test --workspace` | no regression anywhere in the workspace | 32 result lines, all ok; 3,186 lib tests |
| quality bar | `node scripts/language-data-quality-report.mjs --strict` | every expected domain of every language has supported or not_applicable kinds | exit 1 with exactly `erlang.structural_facts` (Task 12) |
| resolution coverage | `node scripts/reference-resolution-coverage-report.mjs --strict` (spawned by the above) | the committed coverage report matches the golden corpus digest | 0 problems after regeneration |
| corpus | `cargo test -p julie-extract-cli --features test-real-world erlang_corpus` | real-repo erlang scan matches the exact committed baseline | 1 passed, **baseline unchanged** |
| format | `cargo fmt --check` | — | clean (ran `cargo fmt` once, then `--check` clean) |
| lint | `cargo clippy --workspace --all-targets --all-features` | warnings-as-errors repo-wide | clean |

## Corpus-baseline deltas

**None.** `crates/julie-extract-cli/tests/erlang_corpus.rs` is untouched and passes as committed.
The baseline asserts per-file `rows.symbols` and `parse_diagnostics`, plus `files_scanned`,
`files_changed`, `files_failed`, behaviour edges, telemetry export counts, and fixture checksums —
none of which literals or complexity rows contribute to. The diagnostics baseline (45 diagnostics /
2 files) is therefore unchanged by construction and proved unchanged by the passing gate.

## Out-of-ownership touches

| File | Forcing reason |
| --- | --- |
| `crates/julie-extractors/src/base/complexity_metrics.rs` | Unavoidable and spec-directed: the erlang `complexity_metrics` open gap's own `required_closure` says "add an erlang decision/loop node configuration to base/complexity_metrics.rs". The per-language config table lives only here. |
| `fixtures/extraction/reference-resolution-coverage.json` | Forced by `scripts/reference-resolution-coverage-report.mjs --strict`, which `language-data-quality-report.mjs --strict` (my assigned gate) spawns. Its `source_digest` hashes `capabilities.json` plus every registered golden, so changing goldens makes it stale. Regenerated with the documented `--write` path; the only content deltas are erlang identifier/relationship counts from the new fixture (`resolved` 4→7, `total` 87→107, zero new ambiguous/missing). |

## Deviations from the task text

1. **XML carrier scope.** The removed xml gap's `required_closure` text said "attribute-value **and
   element-text** literals with an **element key-path** carrier". The task text overrides that with
   "capture attribute-value literals via the EXISTING helper `tag_attribute_carrier`", and I followed
   the task. Consequence: element text (`<path>logs/phonebook.log</path>`) is **not** captured. The
   quality bar and the capability gate are satisfied (`literals.supported = ["other"]` with golden
   evidence, `other` being the only kind the extractor tier emits), but if element-text literals are
   wanted they need a separate follow-up — I removed the gap entry rather than narrowing it, per the
   task's explicit instruction to remove the matching open gap entries.
2. **New erlang fixture.** The task said "regenerate goldens", not "add a fixture". `basic` alone
   would technically have satisfied both gates (its `when Amount > 0` guard yields a nonzero decision
   count and its `?LOG` macro body yields one literal), but that is thin, incidental evidence for two
   capabilities. `control_flow` makes the evidence deliberate and covers every branch form. Cost is
   one registered golden.

## Plan mismatches

None material. The task predicted the complexity work would be "a ~config-table change, no new tree
walking" — that held (no new walker; the one extra line is the erlang span selection in
`complexity_span_for_symbol`, inside the same function that already special-cases scala/vbnet).

## Concerns for the lead

1. **Erlang `body_span` is wrong on this branch, and it is pre-existing.** `capabilities.json` claims
   `body_spans.supported = [constant, field, function, module, struct, type]` for erlang, but the
   spans are text-heuristic artefacts (`'{id = Id}'` for `open/1`, `'{Ids, Limit, Reader, Sizer,
   self()}'` for `history/1`). Anything downstream that trusts an erlang `body_span` — body hashes,
   change detection, snippet extraction — is reading a brace run, not a function body. My complexity
   change routes around it; it does not fix it. The fix is to teach `base/body.rs::infer_body_span`
   that `fun_decl`'s body lives on its `function_clause` child, which would change `body_span` and
   `body_hash` on every erlang golden. Out of scope here; worth a decision before the branch merges.
2. **A multi-clause erlang function's symbol covers only its first clause** (`fun_decl` per clause,
   one symbol per name/arity). So `deposit/2`'s symbol-scope complexity measures clause 1 only. This
   is inherent to the existing symbol model, not something this task introduced, but it means erlang
   symbol-scope metrics under-report multi-clause functions. File scope is complete.
3. **`tag_attribute_carrier` lowercases the attribute name**, which is lossy for case-sensitive XML
   (`schemaLocation` → `schemalocation`). Accepted because the carrier is a cross-markup matching
   key, not payload — the literal text is verbatim. Flagging in case a downstream XML consumer wants
   an exact-case carrier.
4. **Task 12 will invalidate `fixtures/extraction/reference-resolution-coverage.json` again** when it
   adds erlang structural facts and regenerates goldens. Re-run
   `node scripts/reference-resolution-coverage-report.mjs --write --strict` after that golden regen,
   or the strict gate will fail on a stale digest rather than on a real debt.
