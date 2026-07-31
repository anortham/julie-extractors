# Task 3: XML registration + symbols + identifiers — report

**Status:** DONE
**Commit:** `8df10321867c616408b4ed99c81f86e902966a2e` (`feat(xml): register xml and ship the symbol + identifier tier`)
**Worktree state at commit:** path `/Users/murphy/source/julie-extractors/.worktrees/erlang-xml-language-support`,
branch `erlang-xml-language-support`, parent commit `7ef5299d`, working tree clean after commit
(`git status --short --branch` → `## erlang-xml-language-support` only).

---

## 1. Worktree guard (step 0)

```
pwd    = /Users/murphy/source/julie-extractors/.worktrees/erlang-xml-language-support
branch = erlang-xml-language-support
HEAD   = 7ef5299  (as expected)
```

`git worktree list` showed three worktrees; the other two (`/Users/murphy/source/julie-extractors` @ `4bee2fe [main]`,
`~/.config/razorback/worktrees/julie-extractors/csharp-locals-params` @ `90542e0`) were not touched.
Every Bash call re-`cd`s because the harness resets cwd to `/Users/murphy/source/miller` after each call.

---

## 2. Miller calls and what each confirmed

All calls used `workspace_id=julie-extractors-91c17adbdab9` (main checkout — the base for orientation).

| Call | Confirmed |
| --- | --- |
| `context(query="how the yaml extractor emits key symbols with parent chains")` | Pivots `YamlExtractor` (`src/yaml/mod.rs:25`), `extract_mapping_pair` (`:95`), `walk_tree_for_symbols` (`:47`). Showed the exact parent-chain shape XML mirrors: a recursive walk carrying `parent_id: Option<String>`, pushing a symbol then passing `Some(sym.id)` to children, plus the container-vs-leaf `SymbolKind::Module`/`Variable` split (`:123-129`) and the `should_visit_tree_depth`/`child_tree_depth` guard. |
| `inspect(target="crates/julie-extractors/src/html/elements.rs")` | The element-filtering model: `should_extract_element(tag_name, attributes) -> bool` at `:24` returning true first for `id`/`name` attributes, and `ElementExtractor::extract_element` at `:68` returning `Option<Symbol>` so filtered elements emit nothing while their children keep being walked. |

Everything after that was direct file reads: the remaining questions ("what exactly does this gate assert")
need exact text rather than ranked pivots, and the Erlang additions from Task 2 exist only in this worktree,
which the index does not cover.

---

## 3. API-shape evidence (repo-internal)

| Shape | Where proven |
| --- | --- |
| `LanguageSpec` row + `spec()` helper | `language_spec/mod.rs:38-45`, `language_spec/specs.rs:1` (`LANGUAGE_SPECS`), tail `const fn spec(...)`. The xml row was appended after `yaml`. |
| `DATA_ONLY_CAPABILITIES` | `language_spec/mod.rs:139` — `symbols: true, identifiers: true`, rest false. Used verbatim. |
| `parser!` macro | `language_spec/mod.rs:206`; `parser!(parser_xml, tree_sitter_xml::LANGUAGE_XML)` added next to `parser_yaml` (`:262`). |
| Registry dispatch | `registry.rs:538` `EXTRACTORS`; `("xml", extract_xml)` appended. Hand-written `extract_xml` follows `extract_toml` (`:400`) / `extract_erlang` (`:435`), since no macro fits "symbols + identifiers, no relationships/types". |
| `extract_for_language` fills derived domains centrally | `registry.rs:612-695` — `source_regions`, all structural-fact families (including `collect_marker_structural_facts`), and `complexity_metrics` are attached AFTER the per-language fn, which is why the extractor returns empty vectors for them. |
| Parse diagnostics come from the pipeline | `pipeline.rs:36` sets `results.parse_diagnostics = parse_diagnostics_for_tree(&tree)` (`:174`); every registry fn returns `Vec::new()`. So XML degradation is tested through `extract_canonical`, not the extractor. |
| Symbol creation | `base/creation_methods.rs:19` `create_symbol` (body_span/body_hash inferred there); `SymbolOptions` at `base/types.rs:481`. |
| Identifier creation | `base/creation_methods.rs:89` `create_identifier(node, name, kind, containing_symbol_id)`; `IdentifierKind` variants at `base/kinds.rs:34` are exactly `Call | VariableRef | TypeUsage | MemberAccess`. |
| Golden harness | `tests/golden.rs:257` `golden_fixtures_match_canonical_extraction`; fixtures discovered from `fixtures/extraction/capabilities.json`, run through `pipeline::extract_canonical`, normalized, compared. **Blessed regeneration: `UPDATE_GOLDEN=1`** (`:260`). `expected.json` was never hand-authored. |
| Language test filter | `xtask/src/test_tiers.rs:197` — `cargo xtask test language xml` maps to `tests::xml::`, which is why the focused tests live in `src/tests/xml/`. |
| Structural-fact contract regeneration | `tests/structural_fact_registry.rs` `UPDATE_CONTRACT_JSON=1`. |
| No extension collision | `grep '"xml"|"xsd"|"wsdl"'` across `crates/` returned nothing before this task; `detect_language_for_source` (`language_spec/mod.rs:300`) is extension-driven with a single `.h` content-sniff exception. |

---

## 4. Grammar node kinds (derived from real parse trees, not memory)

A scratch dump test (`src/tests/xml/scratch_dump.rs`, written, run twice, then **deleted** before commit)
printed full `tree-sitter-xml` 0.7.0 trees. Kinds the extractor switches on, all observed:

| Construct | Node kinds observed |
| --- | --- |
| Root | `document` |
| `<?xml …?>` + leading comments | `prolog` → `XMLDecl`, `Comment`, `doctypedecl`, `StyleSheetPI` |
| `<a>…</a>` | `element` → `STag` + `content` + `ETag` |
| `<a/>` | `element` → `EmptyElemTag` |
| tag name | `Name` (first `Name` child of `STag`/`EmptyElemTag`) |
| attribute | `Attribute` → `Name` + `=` + `AttValue` |
| attribute value | `AttValue` text **includes the quotes**; may contain `EntityRef` children; single quotes also valid |
| element children | `content` → `CharData`, `element`, `Comment`, `CDSect`, `PI` |
| comment | `Comment` (both in `prolog` and inside `content`) |
| unclosed tag | wrapped in `ERROR`; the `STag` survives as a direct `ERROR` child |

**Load-bearing discoveries:**

1. **A single unclosed tag can promote the whole enclosing element to `ERROR`.** `<root><open></root>` parses
   as one `ERROR` node containing two bare `STag`s — no `element` node at all. Walking only `element` nodes
   would have dropped everything. Hence the orphan-tag recovery in §6.2.
2. **`<?target data?>` inside element content is an ERROR on this grammar** (bare `<?target?>` parses as `PI`).
   That is a grammar limitation, not an extractor one; the fixtures deliberately contain no processing
   instructions with data, and the four goldens parse with **zero** ERROR/MISSING nodes.
3. `xs:` prefixes are part of the `Name` text — the grammar does no namespace splitting, which is consistent
   with v1 recording QNames raw.

---

## 5. What was built

### `crates/julie-extractors/src/xml/`

- `mod.rs` — `XmlExtractor` with the same public surface as every other extractor
  (`extract_symbols`, `extract_identifiers`, `extract_relationships`, `infer_types`, `get_literals`,
  `get_type_argument_usages`). Two recursive walks, both depth-guarded by `should_visit_tree_depth`.
- `elements.rs` — tag/attribute accessors, name promotion, symbol emission.
- `identifiers.rs` — the QName reference attribute list and identifier emission.

### Emitted model

| XML construct | Emission |
| --- | --- |
| element with `name` (preferred) or `id`, non-blank value | Symbol named by that value |
| element with neither (`<xs:sequence>`, `<item>`, `<row>`, `<xs:schema>`) | **nothing** (children still walked) |
| element with child elements | `SymbolKind::Module` |
| leaf element (text-only or empty) | `SymbolKind::Variable` |
| parent | nearest **named** ancestor (anonymous levels are transparent) |
| signature | the start tag, whitespace-collapsed: `<xs:complexType name="AddPhone">` |
| metadata | `tag` (qualified tag name), `name_attribute` (`name` or `id`) |
| visibility | `Public` (XML has no visibility construct; matches html) |
| `type` / `ref` / `base` / `element` attribute value | `IdentifierKind::TypeUsage` identifier, QName raw |
| containing symbol of an identifier | the symbol of the element that owns the attribute |

---

## 6. Judgment calls

1. **`src/xml/mod.rs:11` — no synthetic document/root symbol.** The task allowed one "if yaml/json precedent
   does the same". It does not: `src/yaml/mod.rs:87-88` explicitly skips `document` as noise. So `<xs:schema>`
   (no `name`) emits nothing and its named children become top-level symbols. Follows the precedent found.

2. **`src/xml/elements.rs:80-89` — orphan start tags in ERROR regions are recovered.** Discovery §4.1 means
   one missing end tag would otherwise zero out a file. An `STag`/`EmptyElemTag` whose parent is not an
   `element` gets a symbol (kind `Variable`, span = the tag). Orphans do **not** become parents for following
   siblings — they are recovery artifacts, and inferring nesting from a broken tree would be guessing.
   Proven by `tests::xml::parse_errors::malformed_documents_still_yield_the_elements_that_parsed`, which
   asserts the exact list `["parts", "bolt", "nut"]`.

3. **`src/xml/mod.rs:116-144` — identifier binding is threaded through the walk, not resolved with
   `find_containing_symbol`.** That shared helper (`base/creation_methods.rs:295-303`) prioritises by
   SymbolKind (`Module` = 5 beats `Variable` = 10) before span size, so `<xs:element name="number"
   type="xs:string"/>` bound its `xs:string` reference to the enclosing `AddPhone` module rather than to
   `number`. The walk already knows which element owns the attribute, so the symbol is looked up by
   `start_byte` and passed down. Verified by
   `tests::xml::identifiers::references_bind_to_the_containing_named_element` and by the golden
   `containing_key` values.

4. **`src/xml/identifiers.rs:8` — all four reference attributes emit `TypeUsage`.** `ref=` and `element=`
   name element declarations, `type=`/`base=` name type declarations; all four are "reference to a named
   schema component", and `TypeUsage` is the only one of the four `IdentifierKind` variants that fits.
   `call`/`member_access`/`variable_ref` are recorded `not_applicable` in the matrix.

5. **`src/xml/elements.rs:67` — attribute matching is on the local name.** `xsi:type="tns:Concrete"` is a real
   type reference, and `xml:id` is a real id. Prefixes are dropped only to *recognise* the attribute; recorded
   values keep their prefix. Test: `prefixed_reference_attributes_match_on_their_local_name`.

6. **WSDL `message=` and `binding=` are NOT identifiers.** The task named exactly four attributes; adding more
   is Task 9's call. The wsdl golden shows this explicitly (3 identifiers, not 6).

7. **`language_spec/specs.rs` xml row uses `EMPTY` doc-comment styles.** HTML claims `HtmlBlock`, which makes
   every `<!-- -->` a doc comment. XML has no doc-comment convention, so claiming one would make every comment
   a `doc_comment` source region and force a `doc_comments` supported claim the goldens would have to carry.
   Consequence: `kind_coverage.doc_comments` is `not_applicable` and source regions are `comment` +
   `string_literal` (same shape as the json row).

8. **`base/source_regions.rs` xml config maps `AttValue` to `string_literal`** — the direct analogue of html's
   `quoted_attribute_value`. This is why the cardinality fixture's repeated elements carry **no attributes**:
   attributes on 4,000 rows would have produced 4,000 source-region rows in the golden.

9. **Signature is the whole start tag, whitespace-collapsed, uncapped.** Truncation would need a cap constant
   and a rule nothing else in the repo has. The wsdl golden's longest signature (the `<definitions …>` root
   with two xmlns attributes) is 96 chars.

---

## 7. Plan mismatches (lead action needed)

### 7.1 The forced-open relationships row applies to XML too — the task text says it should not

The task brief said: *"Because target == actual, NO open gap rows and NO migration-plan registry entry should
be needed … if a matrix test forces an open row anyway, that is a plan mismatch to report."* **It is forced.**
Two gates combine:

- `capability_matrix.rs:1341 test_capability_matrix_records_known_gaps_for_languages_with_unfixed_findings`
  requires a `capability_gaps` row for **every** capability whose `target_capabilities` value is `false`.
  Failure observed verbatim: *"xml sets target_capabilities.relationships = false but has no matching
  capability_gaps entry."*
- `capability_matrix.rs:284 capability_matrix_requires_relationship_fixture_evidence` asserts
  `row.capabilities.relationships || exception.is_none()` — so that row may **not** be `status: "exception"`.
- `requires_target_capabilities:346` restricts status to `open | exception`, so `closed` is not available
  either.

Therefore the relationships row must be `status: "open"`, and `capability_matrix_open_rows_have_planned_closure_task:561`
resolves `planned_closure_task` against the hardcoded `docs/plans/2026-05-31-julie-code-migration-implementation-plan.md`.

**Action taken (mirrors Task 2 §7.1):** appended `### Task 14: XML Reference Edge Closure` to that plan,
stating explicitly that the entry exists because the test treats that file as the repository's open-capability
registry, and that XML shipped with `capabilities == target_capabilities`. `relationships` and
`pending_relationships` reference it; `types` is a genuine `exception` (XML has no static type system —
same posture as the json/yaml rows).

**Net effect on honesty:** the matrix has no vocabulary for "not targeted, and documented why", so
`status: open` overstates intent for these two rows. The gap `reason` text says plainly what is and is not
emitted. Recommend the lead decide whether a `not_targeted` status is worth adding to the matrix; three
languages have now been bent through this hole.

### 7.2 Per-language parity guards — one more than Task 2 found

Task 2 §7.3 listed three forced guards. All three applied again, plus a fourth:

| Guard | File it forced | What was added |
| --- | --- | --- |
| `tests::source_regions::supported_languages_with_source_region_syntax_emit_regions` | `base/source_regions.rs:586` + `tests/source_regions.rs` fixture | xml `RegionLanguageConfig` (`Comment`, `AttValue`) |
| `tests::marker_structural_facts::marker_language_matrix_covers_every_supported_comment_language` | `base/structural_fact_registry/marker.rs:37` + `tests/marker_structural_facts.rs` fixture | `"xml"` in the `code.marker.v1` language list. **No change needed to `base/marker_structural_facts.rs`** — `<!--`/`-->` are already in the decoration lists (`:105`, `:119`) from html/markdown. |
| `crates/julie-extract-cli/tests/operations_contract.rs:145` | that file | `open_reference_resolution_gaps` 106 → **109** |
| **NEW:** `capability_matrix.rs:1129 capability_matrix_code_languages_require_resolved_test_detection` | `crates/julie-extractors/src/tests/capability_matrix.rs:1130` | `DOMAIN_LANGUAGES` `[&str; 8]` → `[&str; 9]` with `"xml"` added. Without it, XML is treated as a *code* language and its three `test_detection` open gaps (identical in shape to json/yaml/toml) fail the "add supported or not_applicable evidence" assertion. |

`docs/contracts/structural-fact-patterns.json` was regenerated (`UPDATE_CONTRACT_JSON=1`) — one added line in
the marker pattern's language list.

`base/body.rs:113` also needed an xml arm (`<!--`/`-->`), added to the existing `"html" | "markdown" | "razor"`
match. Regression test: `tests::xml::symbols::body_hash_ignores_comment_edits`.

**Recommendation:** Task 10 should add these five (source regions, marker registry, body comment syntax, the
CLI gap count, and `DOMAIN_LANGUAGES`) to `docs/languages/new-language-checklist.md` §2 — it still omits all
of them. This is now the second consecutive task to discover them by test failure.

### 7.3 No task owns XML relationships, literals, or complexity metrics

`kind_coverage` open gaps for `relationships`, `structural_facts`, and `literals` all point at
`docs/plans/2026-07-31-erlang-xml-language-support-plan.md Task 9: XML schema/WSDL structural facts`, because
that is the only remaining XML task. Task 9 is scoped to structural facts, so **relationships and literals
have no real owner**; the `capability_gaps` rows point at the new migration-plan Task 14 instead.
Same posture as Task 2 §7.4 flagged for Erlang complexity metrics. `complexity_metrics` is recorded
`not_applicable` for xml (file and symbol scope) rather than as a gap: XML has no decision or loop
constructs, so cyclomatic complexity is genuinely undefined for it — this matches the json, toml, and yaml
rows exactly.

---

## 8. Capability matrix row (honesty audit)

- `capabilities` == `target_capabilities` == `symbols: true, identifiers: true`, rest false — matches
  `DATA_ONLY_CAPABILITIES` in the `LanguageSpec` row (enforced by `capability_matrix_matches_registry_entries`).
- `extensions` and `parser_crate` byte-identical between the spec row and the matrix row:
  `["xml","xsd","wsdl"]`, `"tree-sitter-xml"`.
- `capability_gaps`: 3 rows — `relationships` (open, §7.1), `pending_relationships` (open, §7.1),
  `types` (exception, domain limitation).
- `kind_coverage`:
  - `symbols` supported `["module","variable"]`; `body_spans` supported `["module","variable"]` (leaf elements
    with text carry a body; empty elements do not, which is why both kinds appear in both lists).
  - `identifiers` supported `["type_usage"]`, `not_applicable` `["call","member_access","variable_ref"]`.
  - `source_regions` supported `["comment","string_literal"]`.
  - `annotations`, `doc_comments`, `complexity_metrics` — `not_applicable` with language-semantics reasons.
  - `relationships`, `structural_facts`, `literals` — open gaps with reason / required_closure /
    planned_closure_task.
  - `test_detection` — three open gaps mirroring the yaml row's wording (see §7.2 for the DOMAIN_LANGUAGES
    consequence).
- Every `supported` claim is bidirectionally checked against the four goldens by
  `capability_matrix_supported_kind_claims_have_fixture_evidence` / `assert_golden_domain_claims_match`;
  nothing is claimed that a golden does not emit, and nothing a golden emits is unclaimed.
- **`structural_facts.supported = []` deliberately.** `code.marker.v1` is now registered for xml, but
  `structural_fact_pattern_ids_for_language` excludes marker patterns, so `claims_match_registry` requires
  `{}` while `claims_have_fixture_evidence` requires the goldens to emit nothing. The fixtures therefore
  contain no TODO/FIXME markers. Same posture as every other language.

---

## 9. Golden review (hand-checked after `UPDATE_GOLDEN=1`)

All four have `parse_diagnostics: []`.

| Fixture | Symbols | Identifiers | What it proves |
| --- | --- | --- | --- |
| `basic` (config .xml) | 6 | 4 | `<appSettings>`, `<sinks>`, `<features>`, `<feature>`, `<path>` and a bare `<add />` all suppressed; `id`-promoted and `name`-promoted elements both present; Module/Variable split correct. |
| `xsd` | 8 | 7 | Named `complexType`/`simpleType`/`element` promoted; `<xs:schema>`, `<xs:sequence>`, `<xs:restriction>`, `<xs:complexContent>`, `<xs:extension>` suppressed; `number`'s `parent_key` is `AddPhone` **through** the anonymous `xs:sequence`; `base=`, `ref=`, `type=` all present as raw QNames. |
| `wsdl` | 11 | 3 | service/port/operation/message/binding/part names promoted; the two same-named `AddPhone` operations are disambiguated by their `parent_key` (`PhoneBookPort` vs `PhoneBookBinding`), as are the two `body` parts; `message=`/`binding=` correctly absent from identifiers. |
| `cardinality` | **3** | 2 | 114,171 bytes, 2,000 `<row>` elements each with 2 `<cell>` children = 4,000+ anonymous elements → exactly 3 symbols (`parts`, `bolt`, `nut`) and 5 source regions. Golden file is 6 KB. |

Reviewed field-by-field on `xsd/expected.json`: `key`, `parent_key`, `containing_key`, spans, `signature`,
`metadata`, `visibility` all correct; `body_span`/`body_hash` `null` exactly for empty elements.

---

## 10. Contract-version review

`EXTRACTION_CONTRACT_VERSION` (`crates/julie-extractors/src/lib.rs:128`) was **not bumped**. Adding a language
emits rows in existing domains with existing shapes; no field, kind, serialization, or normalization rule
changed for any downstream consumer. Recorded in the commit message.

---

## 11. Verification ledger

| Command | Result |
| --- | --- |
| `RUSTUP_TOOLCHAIN=1.97.1 cargo xtask test language xml` | **26 passed, 0 failed** |
| `RUSTUP_TOOLCHAIN=1.97.1 cargo xtask test golden` | **3 passed, 0 failed** — all four xml fixtures, `parse_diagnostics: []` |
| `RUSTUP_TOOLCHAIN=1.97.1 cargo xtask test capability` (worker ceiling) | **39 passed + 1 passed, 0 failed** |
| `RUSTUP_TOOLCHAIN=1.97.1 cargo xtask test changed crates/julie-extractors/src/language_spec/specs.rs fixtures/extraction/capabilities.json` | **exit 0** — default tier + capability matrix + pending shape + certification (`parser_upgrade`), zero failures |
| `RUSTUP_TOOLCHAIN=1.97.1 cargo test -p julie-extract-cli --test operations_contract` | **56 passed, 0 failed** |
| `RUSTUP_TOOLCHAIN=1.97.1 cargo run -p julie-extract-cli -- languages --json` | `languages.languages` count **38**; xml row: `actual_capabilities` == `target_capabilities` == symbols+identifiers only, `capability_gaps: 3`, `extensions: ["xml","xsd","wsdl"]`, `parser_crate: "tree-sitter-xml"`, `dependency_status: "current"`, `fixtures: 4` |
| `cargo fmt --all -- --check` | clean (after one `cargo fmt --all`) |
| `cargo clippy -p julie-extractors -p julie-extract-cli --all-targets` | zero warnings |

Not run (outside worker scope): real-world / certification-wide tiers,
`scripts/language-data-quality-report.mjs`, `cargo deny check` — Task 10's branch gate.

### Focused test inventory (26)

- `symbols.rs` (10): name promotion + signature + visibility, `id` promotion, name-beats-id, anonymous
  suppression, parent chain through an anonymous level, Module/Variable split, text-only leaf is Variable,
  blank `name=""` does not promote, metadata (`tag`, `name_attribute`), WSDL service/port/operation ordering
  and parenting, body-hash comment insensitivity.
- `identifiers.rs` (9): one test per reference attribute (`type`, `ref`, `base`, `element`), unprefixed QName
  passthrough, containing-symbol binding, non-reference attributes emit nothing, empty reference value emits
  nothing, prefixed (`xsi:type`) local-name matching.
- `routing.rs` (2): all three extensions route to the xml extractor with identical output; the tier emits no
  relationships / pending / structured pending / types.
- `parse_errors.rs` (3): malformed document still yields the exact expected symbol list, diagnostics reported,
  clean document reports none.
- `cardinality.rs` (1): 4,000 generated anonymous elements → exactly `["parts", "bolt"]`, with a sub-1MB assert.

Every test asserts concrete returned values; there are no smoke-only tests.

---

## 12. Self-review

| Acceptance criterion | Status |
| --- | --- |
| `cargo xtask test language xml` green | ✅ 26 passed |
| Goldens for all four fixture dirs green, zero parse diagnostics | ✅ |
| Cardinality fixture proves anonymous suppression with an exact small count | ✅ golden = 3 symbols; focused test asserts `["parts","bolt"]` against 4,000+ elements |
| Count assertions at 38 | ✅ `registry.rs:711`, `factory.rs:60-61`, `capability_snapshot_test.rs:8`; plus the CLI gap count 106→109 |
| `cargo xtask test changed specs.rs capabilities.json` green | ✅ exit 0 |
| `languages --json` shows xml with symbols+identifiers only, correct extensions/parser_crate | ✅ |
| `grammar_smoke.rs` deleted | ✅ plus its `pub mod` line |
| Worker-scope verification passes; committed | ✅ `8df10321` |

Findings fixed during self-review:
- Identifier containing-symbol binding was wrong (§6.3) — caught by a focused test, not by the goldens.
- Malformed documents dropped everything but the last element (§6.2) — caught by a focused test.
- Double identifier emission when the walk visited both `element` and its `STag` child — caught while
  restructuring `walk_references`; the reference pass now anchors on `element` (via `tag_node`) or on an
  orphan tag, never both.
- Misleading doc comment on `local_name` (said "attribute" while giving an element example) — rewritten.
- Removed the scratch tree-dump test before commit.

Known limitations, all recorded as typed gaps rather than silently absent: no relationships, pending
relationships, types, structural facts, or literals; `test_detection` unclassified. `<?target data?>`
processing instructions parse as ERROR on tree-sitter-xml 0.7.0 (grammar limitation, §4.2) — a document
containing one will report a parse diagnostic. `.dtd` is deliberately **not** registered (`LANGUAGE_DTD` is a
separate grammar, per the Task 1 handoff).

---

## 13. Files changed

Created: `crates/julie-extractors/src/xml/{mod,elements,identifiers}.rs`,
`crates/julie-extractors/src/tests/xml/{mod,symbols,identifiers,routing,parse_errors,cardinality}.rs`,
`fixtures/extraction/xml/{basic,xsd,wsdl,cardinality}/{source.*,expected.json}`.

Deleted: `crates/julie-extractors/src/tests/grammar_smoke.rs`.

Modified (assigned): `language_spec/specs.rs`, `language_spec/mod.rs`, `lib.rs`, `registry.rs`, `factory.rs`,
`tests/capability_snapshot_test.rs`, `tests/mod.rs`, `fixtures/extraction/capabilities.json`,
`base/source_regions.rs`, `base/structural_fact_registry/marker.rs`, `base/body.rs`,
`tests/source_regions.rs`, `tests/marker_structural_facts.rs`,
`crates/julie-extract-cli/tests/operations_contract.rs`, `docs/contracts/structural-fact-patterns.json`.

Modified (forced by repo gates, outside the assigned list — see §7):
`crates/julie-extractors/src/tests/capability_matrix.rs` (one const),
`docs/plans/2026-05-31-julie-code-migration-implementation-plan.md` (Task 14 registry entry).

`crates/julie-extractors/src/base/marker_structural_facts.rs` was listed in the task but needed **no** change.
