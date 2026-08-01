# Pre-merge fix round — erlang-xml-language-support

Fixes for the three findings the lead verified from the Codex pre-merge review.
Finding 2 (`pp_define` macro bodies walked as executable) was explicitly out of
scope and is untouched.

- Worktree: `/Users/murphy/source/julie-extractors/.worktrees/erlang-xml-language-support`
- Branch: `erlang-xml-language-support`
- Start HEAD: `4910144` (clean)
- End HEAD: `1285687`
- Toolchain: every cargo invocation prefixed `RUSTUP_TOOLCHAIN=1.97.1`; the global
  default was not touched.

| Finding | Commit |
| --- | --- |
| 1 — multi-clause erlang functions cover only their first clause | `ab3a43f` |
| 3 — recovery can resume inside multiline quoted atoms | `afa8416` |
| 4 — generic XML attributes become false type references | `1285687` |

Three commits, one per finding. The golden regen did not entangle them: Finding 1
moved only the erlang goldens, Finding 3 moved none, Finding 4 moved only the xml
goldens plus the reference-resolution coverage report.

---

## Finding 1 (HIGH) — multi-clause erlang functions cover only their first clause

### Fix design

`tree-sitter-erlang` emits one `fun_decl` per clause. `extract_symbols_from`
created the symbol from the FIRST `fun_decl` of a name/arity and suppressed the
rest via `emitted.insert(clause.identity)`, so span, body span, body hash, and
symbol-scope complexity all stopped at clause one.

Two changes:

1. **Extent.** `ErlangExtractor::clause_run_extent` (`erlang/mod.rs:321`) walks
   forward from the creating declaration's index through the contiguous sibling
   run of `fun_decl`s whose `function_clause` yields the same `(name, arity)`,
   and returns a `NormalizedSpan` from the first clause's start to the last
   clause's end. The run stops at the first declaration that is not another
   clause of the same function — Erlang requires a function's clauses to be
   adjacent. `extract_symbols_from` now iterates `declarations.iter().enumerate()`
   to have that index. The symbol is built through
   `BaseExtractor::create_symbol_from_span` with that extent, so `symbol_map`
   carries the correct span from creation rather than being patched afterwards.

2. **Body span.** `FunctionClause` gained a `body_start` field, read from
   `function_clause`'s `body` field. `apply_clause_body_span`
   (`erlang/definition_forms.rs:106`) replaces the inferred body span with
   `body_start .. symbol.end_byte` and recomputes `body_hash`, then re-inserts the
   symbol into `base.symbol_map` so the map copy does not drift.

### Why this seam

`infer_body_span` cannot reach the erlang body: `fun_decl` has no `body` field and
no `BODY_NODE_KINDS` child (the `body` field lives on `function_clause`, one level
down), so it falls through to `infer_body_span_from_span_with_line_starts` →
`brace_body_span`, which returns the first `{`…`}` run in the declaration. For
`open(Id) -> #account{id = Id}.` that is the record literal `{id = Id}`, which is
what Task 11's report observed. Teaching `body.rs` to descend a `clause` field
would be an erlang rule in a language-generic module; the existing precedent for a
language-specific body-span fixup is `sql/body_spans.rs::finalize_sql_callable_symbol`,
so this follows it.

The golden confirms both halves. `fixtures/extraction/erlang/basic`:

| symbol | before | after |
| --- | --- | --- |
| `deposit/2` lines | 31–32 | 31–34 |
| `deposit/2` body span | bytes 790–831 (clause 1 body only) | bytes 771–868 (both clauses) |
| `deposit/2` body hash | `ea100cd3…` | `8669530b…` |
| `open/1` body span | bytes 632–641 (`{id = Id}`) | bytes 617–642 (`-> #account{id = Id}.`) |
| `deposit/2` symbol complexity span | lines 31–32 | lines 31–34 |

### The complexity special case — kept, and why

Task 11 added an erlang arm in `base/complexity_metrics.rs::complexity_span_for_symbol`
that returns the declaration span instead of `body_span`. It is **not** subsumed by
the body-span fix, so it stays — but its justification changed and the comment was
rewritten to match.

`ERLANG_CONFIG.decision_node_kinds` includes `guard_clause`, and a guard lives in a
clause HEAD (`deposit(Acct, Amount) when Amount > 0 ->`). The corrected body span
starts at the first clause's `->`, so measuring it would drop the FIRST clause's
guard while counting every later clause's guard — inconsistent. The declaration
span is head + guard + body for every clause, which is exactly the scope this
metric wants. The invariant the lead asked for holds either way; the declaration
span is strictly the more correct of the two.

### API-shape evidence

`~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/tree-sitter-erlang-0.20.0/src/node-types.json`:

```json
{"type":"fun_decl","fields":{"clause":{"types":[{"type":"_function_or_macro_clause"}]}}}
{"type":"function_clause","fields":{"args":…,"body":{"types":[{"type":"clause_body"}]},"guard":…,"name":…}}
{"type":"clause_body","fields":{"exprs":…}}
```

`fun_decl` exposes `clause`, never `body`; `body` is a `function_clause` field
holding a `clause_body`. That is the whole reason the generic inference misses.

### Tests (red → green)

`crates/julie-extractors/src/tests/erlang/symbols.rs`:

- `multi_clause_symbol_spans_through_the_last_clause` — red `end_byte` 354, green 414
  (the fixture's last clause ends at `undefined.`).
- `body_hash_moves_when_a_later_clause_changes` — red: both hashes
  `c37dd57118f93e6f225f752d1f7db71c`; green: different.
- `body_span_covers_the_clause_bodies_not_the_first_brace_run` — red body start 36
  (`#account`, i.e. the brace run); green body start 21 (`->`, the `clause_body`
  node start).

`crates/julie-extractors/src/tests/erlang/complexity.rs`:

- `multi_clause_symbol_complexity_counts_decisions_in_later_clauses` — a two-clause
  `route/1` whose first clause branches nowhere and whose second clause carries a
  head guard plus a `case` with two arms. Red `decision_count` 0, green 4. Also
  asserts the metric span reaches the last clause's end.

The expectation in the third test was authored as `#account` and corrected to `->`
after observing the real `clause_body` node start — the grammar puts the arrow
inside `clause_body`, so the fixed body span begins there. That is the extractor's
real emitted shape, not a loosened assertion.

### Golden delta

`fixtures/extraction/erlang/{basic,control_flow,cross_file,negative,test_roles,test_roles_common_test}/expected.json`
— element counts identical in every file (checked list-by-list); only spans, body
spans, body hashes, and symbol-complexity spans moved.

---

## Finding 3 (MEDIUM) — recovery can resume inside multiline quoted atoms

### Fix design

`recovery.rs::collect_literal_ranges` excluded only `string` and `comment` nodes,
while `starts_form` accepts a leading `'` or lowercase byte at column 0. A quoted
atom may straddle lines, so a form-shaped line inside one was a valid resume point;
`blank_before` then erased the opening quote and the re-parse read literal text as
declarations.

Two changes:

1. `LINE_SPANNING_LITERAL_KINDS = ["atom", "char"]` join the excluded kinds — but
   only when the node's start row differs from its end row.
2. Containment tightened from `range.contains(&offset)` to
   `range.start < offset && offset < range.end`.

### Why this seam

Excluding `atom` unconditionally would break recovery outright: every unquoted
top-level form head is an `atom` node starting at column 0, so its own resume point
would be swallowed. Two independent guards prevent that, and each earns its place:

- The multiline filter keeps the range set essentially empty on real files, so
  `resume_points`'s per-line scan does not become O(lines × atoms) on the corpus.
- The strict-interior rule keeps a multiline quoted atom that *heads* a real form
  (`'quoted name'(X) -> X.` is legal Erlang) as its own resume point.

Neither changes `string`/`comment` behaviour: `starts_form` never accepts `"` or
`%`, so no string or comment range can begin at a resume-point candidate.

### API-shape evidence

`tree-sitter-erlang-0.20.0/grammar.js:1270`:

```js
atom: $ => token(
    /([a-z\xDF-\xF6…][_@a-zA-Z0-9…]*)|('([^'\\]|\\([^x\^]|[0-7]{1,3}|x[0-9a-fA-F]{2}|x\{[0-9a-fA-F]+\}|\^.))*')/,
),
char: $ => token(/\$([^\\]|\\([0-7]{1,3}|x[0-9a-fA-F]{2}|…|\^.|\\n|\\\\|.))/),
```

The quoted alternative's `[^'\\]` matches `\n`, so a quoted atom is one token that
can span lines. A parse dump confirmed the token survives inside an ERROR region:

```
ERROR [81..183] "io:format(\"~p\", ['ghost line one\nghost() -> not_code.\n…'])\n    end."
  atom [98..172] "'ghost line one\nghost() -> not_code.\n-record(ghost, {id}).\nghost line two'"
resume points: … 114 "ghost() -> not_code." / 135 "-record(ghost, {id})." / 157 "ghost line two'])" …
```

Offsets 114, 135 and 157 all lie strictly inside `[98..172]`.

### Tests (red → green)

`crates/julie-extractors/src/erlang/recovery.rs` (unit):

- `resume_points_skip_lines_inside_a_multiline_quoted_atom` — red produced
  `["-module(bank).", "label() -> 'first line", "-export([fake/0]).", "second line'.", "real(X) -> X."]`;
  green drops the two interior lines and keeps `real(X) -> X.`.
- `a_quoted_atom_that_heads_a_form_stays_a_resume_point` — guards the
  strict-interior rule.

`crates/julie-extractors/src/tests/erlang/parse_errors.rs` (adversarial, end to end):

- `form_like_lines_inside_a_multiline_quoted_atom_do_not_become_symbols` — a file
  with a `?WITH_STACKTRACE` parse error whose broken region contains a multiline
  quoted atom holding a function, a record, and a macro at column 0. Red produced
  `["GHOST", "first", "ghost", "ghost", "id", "p", "real"]` — four phantom
  declarations minted from literal text. Green produces `["first", "p", "real"]`.

Existing recovery tests stayed green, including the real-world corpus gate:
`telemetry_module_exposes_its_module_exports_and_behaviour_edges` (the 8/8 export
case) and `erlang_corpus_scans_every_file_against_the_committed_baseline`.

### Golden delta

None. No golden fixture has a parse error whose recovery path this touches.

---

## Finding 4 (MEDIUM) — generic XML attributes become false type references

### Fix design

`xml/identifiers.rs` created a `TypeUsage` identifier for any attribute whose local
name was in `REFERENCE_ATTRIBUTES` (`base`/`element`/`ref`/`type`), regardless of
dialect. Those are ordinary words.

`SchemaNamespaces` (`xml/identifiers.rs:43`) is scanned once per document in
`XmlExtractor::extract_identifiers` and threaded through `walk_references` into
`extract_element_facts`. It reduces the document's own `xmlns` declarations to two
questions:

- `element_is_component(tag_name)` — the tag's prefix is bound to XML Schema, WSDL
  1.1, or WSDL 2.0; or the tag is unprefixed and the default namespace is one of
  those.
- `attribute_is_component(attribute_name)` — the attribute's prefix is bound to XML
  Schema or XML Schema-instance. An unprefixed attribute is in no namespace per the
  XML spec, so it never qualifies on its own.

A reference-named attribute becomes a `TypeUsage` only if either holds.

### Why this seam

- **Grounded in declared namespaces, not the extension.** The structural-fact tier
  already derives its dialect from the file extension
  (`data_structural_facts.rs::xml_dialect`), which the lead ruled out here — a
  `.config` or `.csproj` full of `type=` attributes would still be treated as a
  schema. Namespace declarations are the document's own statement about what it is.
- **The attribute rule is not an add-on.** `elements::local_name`'s existing doc
  comment already names `xsi:type` as the motivating case; without the attribute
  rule the gate would silently drop the one genuine reference an instance document
  carries.
- **Document-wide, not scoped.** Bindings are collected across the whole tree rather
  than threaded as a scope stack. Redeclaring a prefix to a different URI part-way
  down a document is vanishingly rare, and per-element scoping would buy nothing
  any real schema, WSDL, or config document would notice. This is documented on the
  type.
- **Literals untouched.** `base.record_literal` still runs for every non-empty
  attribute value in every XML document, before the gate. Task 11's work is
  unchanged, and the golden literal counts confirm it (see below).
- **WSDL 1.1's namespace is quoted with and without its trailing slash**, so
  `normalize` trims it from both sides and the constants are written without it.

### API-shape evidence

The `xmlns` attributes are read through the same `elements::attributes` /
`elements::attribute_value` helpers the reference walk already uses, so the scan
sees exactly the `Attribute` → `Name` + `AttValue` shape
`tree-sitter-xml`'s `STag`/`EmptyElemTag` produce — no second parse of the document
and no new node-kind assumptions. `tag_of` accepts an `element` (via
`elements::tag_node`) or a bare `STag`/`EmptyElemTag`, matching the two branches
`walk_references` already handles, so an orphan tag in an ERROR region still
contributes its declarations. The scan reuses `should_visit_tree_depth` /
`child_tree_depth`, the same depth guards as the rest of the module.

The fixture corpus is the evidence that the gate matches real documents:
`xsd/source.xsd` declares `xmlns:xs="http://www.w3.org/2001/XMLSchema"` and prefixes
every element `xs:`; `wsdl/source.wsdl` declares
`xmlns="http://schemas.xmlsoap.org/wsdl/"` and leaves its elements unprefixed —
the prefix path and the default-namespace path respectively.

### Tests (red → green)

`crates/julie-extractors/src/tests/xml/identifiers.rs`:

- `generic_documents_emit_no_type_usage_from_reference_named_attributes` — the
  negative control: a `<configuration>` document with `type=`, `ref=`, `base=`, and
  `element=` attributes and no namespace declarations. Red emitted identifiers;
  green emits zero.
- `an_undeclared_prefix_does_not_make_an_element_a_schema_component` — `<xs:element
  type="xs:string"/>` with `xs` bound to nothing. Red emitted one; green emits zero.
- `a_schema_namespace_declared_on_an_ancestor_still_qualifies_its_elements` — the
  positive counterpart.

The seven pre-existing positive tests were snippets with undeclared prefixes. They
were made contract-faithful — a real XSD or WSDL always declares the namespace its
elements live in — rather than the gate being weakened to accept bare prefixes.
`prefixed_reference_attributes_match_on_their_local_name` now declares
`xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"`, which is what makes it the
`xsi:type` case it was always describing.

### Golden delta

`UPDATE_GOLDEN=1` moved only the two generic fixtures. Per-fixture `type_usage`
identifier sets:

| fixture | before | after | removed |
| --- | --- | --- | --- |
| `xml/basic` (generic config `.xml`) | 4 | 0 | `Serilog.Sinks.Console`, `Serilog.Sinks.File`, `xs:int` ×2 |
| `xml/cardinality` (generic catalog `.xml`) | 2 | 0 | `xs:string` ×2 (from `<part name="bolt" type="xs:string"/>`) |
| `xml/wsdl` | 3 | 3 | — |
| `xml/xsd` | 7 | 7 | — |

Symbol and literal counts are unchanged in all four. The xsd/wsdl sets did not
shrink, as required. `Serilog.Sinks.Console` is a .NET class name in a logging
config — the clearest illustration that the old behaviour was minting nonsense.

`fixtures/extraction/reference-resolution-coverage.json` regenerated: xml total
16 → 10, matching the six removed references exactly. Report summary:
`{"languages":38,"cells":728,"silent_cells":0,"quality_bar_debts":0}`.

`fixtures/extraction/capabilities.json` needed no change and the capability tier
passes: xml still claims `identifiers: true` with `kind_coverage.identifiers`
`supported: ["type_usage"]` and no open gaps, now evidenced by the xsd and wsdl
fixtures instead of by false positives in generic documents.

---

## Files touched outside the three named files

| File | Forcing reason |
| --- | --- |
| `crates/julie-extractors/src/erlang/definition_forms.rs` | Finding 1 lives here as much as in `mod.rs` — the lead's own analysis named `definition_forms::extract_function` as the half that builds the span from one clause. |
| `crates/julie-extractors/src/base/complexity_metrics.rs` | Comment only. The lead directed a revisit of the Task 11 erlang special case; its stated justification ("the declaration span is exactly one clause head plus body") became false once the symbol spans every clause, so the comment was rewritten. No behaviour change. |
| `crates/julie-extractors/src/xml/mod.rs` | `SchemaNamespaces` has to be scanned once per document and threaded to `extract_element_facts`; `walk_references` is the only path there. Module doc updated to state the gate. |
| `crates/julie-extractors/src/tests/xml/routing.rs` | Its fixture document had no namespace declarations and asserted one identifier — i.e. it asserted the defect. The document now declares the schema namespace so the routing assertion still proves identifiers route, on a document that legitimately has one. |
| `crates/julie-extractors/src/tests/erlang/{symbols,complexity,parse_errors}.rs`, `crates/julie-extractors/src/tests/xml/identifiers.rs` | The tests the findings required. |
| `fixtures/extraction/**/expected.json`, `fixtures/extraction/reference-resolution-coverage.json` | Regenerated evidence, detailed above. |

## Miller calls used

| Call | What it gave |
| --- | --- |
| `inspect target=crates/julie-extractors/src/erlang/mod.rs` | Symbol inventory and line ranges before editing — located `extract_symbols_from` (166–223) and `clause_counts` without reading the file blind. |
| `trace target=extract_element_facts mode=refs` | Proved the only two call sites are both in `walk_references` (`xml/mod.rs:148`, `:156`), so threading `SchemaNamespaces` misses no caller. Both exact, confidence 1.00. |
| `trace target=extract_function mode=refs scope=…/definition_forms.rs` | Returned `no_references` / `expected_empty` — the name is ambiguous across extractors and erlang emits no ref for it. |
| `search query="definition_forms::extract_function" mode=source` | The fallback the empty trace suggested: confirmed the single call site at `erlang/mod.rs:207`. |

Erlang and XML grammar node shapes are not in Miller's index, so those came from
the pinned crates: `tree-sitter-erlang-0.20.0` `src/node-types.json` (fun_decl /
function_clause / clause_body fields) and `grammar.js:1266-1272` (the `atom` and
`char` token regexes), both under
`~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/`. A `to_sexp`-style parse
dump was used to confirm the `atom` token survives inside an ERROR region.

## Gate table

Every cargo command prefixed `RUSTUP_TOOLCHAIN=1.97.1`. Exit codes are `$?` of the
command itself, never of a pipeline.

| Gate | Exit |
| --- | --- |
| `cargo xtask test default` | 0 |
| `cargo xtask test golden` | 0 |
| `cargo xtask test capability` | 0 |
| `cargo xtask test certification` | 0 |
| `cargo xtask test changed <11 touched paths + src/lib.rs>` | 0 |
| `cargo xtask test language erlang` | 0 |
| `cargo xtask test language xml` | 0 |
| `cargo test -p julie-extract-cli --features test-real-world --test erlang_corpus` | 0 |
| `cargo run -p julie-extract-cli -- languages --json` | 0 |
| `node scripts/language-data-quality-report.mjs --strict` | 0 |
| `node scripts/reference-resolution-coverage-report.mjs --write` | 0 |
| `cargo fmt --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features` | 0 |
| `cargo deny check` | 0 |

`languages --json` erlang/xml rows:

- **erlang** — `capability_gaps: 0`; `kind_coverage.identifiers` supported
  `[call, member_access, type_usage, variable_ref]`, open_gaps `[]`;
  `complexity_metrics` supported `[file, symbol]`; `body_spans` supported
  `[constant, field, function, module, struct, type]`, open_gaps `[]`.
- **xml** — `capability_gaps: 3` (relationships / pending / types, unchanged and
  intentional); `kind_coverage.identifiers` supported `[type_usage]`, open_gaps
  `[]`.

`cargo deny check`: `advisories ok, bans ok, licenses ok, sources ok`.

The erlang corpus baseline (`crates/julie-extract-cli/tests/erlang_corpus.rs`) is
unchanged and passes on all three commits: none of these fixes changes how many
symbols exist, only their spans, hashes, and which references are real.

## Concerns for the lead

1. **Finding 2 remains open.** `pp_define` macro bodies are still walked as
   executable, so a type-valued macro can still produce a false call. Flagged, not
   fixed, per the brief.
2. **`SchemaNamespaces` is document-wide, not element-scoped.** A document that
   rebinds a prefix from the XML Schema namespace to something else part-way down
   would keep the schema reading for the later subtree. No real schema, WSDL, or
   config document does this; per-element scoping is available if the contract ever
   needs it.
3. **The XSD/WSDL namespace list is closed.** XML Schema 1.0/1.1, WSDL 1.1, and
   WSDL 2.0 are covered, plus XML Schema-instance for attributes. A schema dialect
   outside that set (RELAX NG, for instance) would now emit no `TypeUsage` where it
   previously emitted false ones — a behaviour change from "wrong references" to
   "no references", not a regression against anything the fixtures claim.
4. **Body span for a clause the grammar could not resolve.** If `function_clause`
   yields no `body` field — only reachable through a damaged recovery parse — the
   symbol keeps the generic text-inferred body span rather than losing body
   coverage. Documented at the call site.
