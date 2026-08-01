# Task 9: XML schema/WSDL structural facts — report

**Status:** DONE
**Commit:** `6639f1cf030db3ff81bc28f7caf359a01173c526` (`feat(xml): ship document, XSD, and WSDL structural facts`)
**Worktree state at commit:** path `/Users/murphy/source/julie-extractors/.worktrees/erlang-xml-language-support`,
branch `erlang-xml-language-support`, parent `890db84`, tree clean apart from the untracked
`.razorback/sdd/task-8-erlang-xml-report.md` (left alone) and this report.

---

## 1. Worktree guard (step 0)

```
pwd    = /Users/murphy/source/julie-extractors/.worktrees/erlang-xml-language-support
branch = erlang-xml-language-support
HEAD   = 890db84  (as expected)
status = ?? .razorback/sdd/task-8-erlang-xml-report.md   (untouched)
```

Every Bash call re-`cd`s; the harness resets cwd to `/Users/murphy/source/miller` after each one.

---

## 2. Miller calls and what each proved

Workspace `julie-extractors-91c17adbdab9` (MAIN checkout — the xml module and Task 2–8 work exist only in
this worktree, so orientation was Miller-on-main plus raw reads of the worktree-only files).

| Call | Confirmed |
| --- | --- |
| `context(query="how document structural facts are registered and collected for json yaml toml markdown")` | Pivots `collect_markdown_structural_facts` / `collect_yaml_structural_facts` (`base/data_structural_facts.rs:133`, `:1154`), neighbour `collect_data_structural_facts` (`:98`) as the language match-arm router, `YAML_DOCUMENT_PATTERN_ID` (`:34`) as the pattern-id const convention, `structural_fact_patterns_json` (`julie-extract-cli/src/capability_snapshot.rs:405`) as the contract serializer, and `markdown_emits_document_structural_facts` (`tests/markdown/structural_facts.rs:37`) as the focused-test shape. This is the routing shape the task asked me to discover: emission goes through the **data collector**, not the `XmlExtractor`. |
| `search(query="document.v1", mode="source")` | The real document-family pattern ids and where the SPECS live: `yaml.document.v1` in `base/structural_fact_registry/data.rs:365`, `markdown.frontmatter.v1` at `data.rs:18`, `html.link.v1` in `web/html.rs:15`. Confirmed `<language>.<thing>.v1` in the data family and non-language dialect prefixes in the framework/web families. |

Everything else was direct file reads (exact gate text, and worktree-only files the index does not cover) plus
the Task 3 report's §4 node-kind table.

---

## 3. API-shape evidence (repo-internal)

| Shape | Where proven |
| --- | --- |
| Central collector attach point | `registry.rs:700-708` — `results.structural_facts.extend(collect_data_structural_facts(language, tree, file_path, content, &results.symbols))`. Nothing was added to `XmlExtractor`. |
| Language routing | `base/data_structural_facts.rs:108` match arm; `attach_containing_symbols` + `sort_structural_facts` run once for every arm (`:114-116`), so xml facts get containing-symbol binding and start-byte ordering for free. |
| Spec type + helpers | `structural_fact_registry/mod.rs:78` `StructuralFactPatternSpec`, `:100` `key()`, `:116/:122` `K_PATTERN_VERSION`/`K_QUERY_FAMILY`, `:95-98` `ALWAYS`/`OPT`/`STR`/`NUM`/`BOOL` aliases. |
| Family-module registration | `mod.rs:150-160` `all_specs()`; `xml::SPECS` inserted after `data::SPECS`. |
| Per-language claim authority | `base/structural_facts.rs:136-149` `structural_fact_pattern_ids_for_language` unions the five collectors; `data_structural_fact_pattern_ids_for_language` (`data_structural_facts.rs:120`) is the data arm. Marker patterns are excluded from this union by construction. |
| Bidirectional matrix gates | `tests/capability_matrix.rs:924` `..._structural_fact_claims_have_fixture_evidence` (claims ⇔ live extraction over the fixture sources) and `:1001` `..._structural_fact_claims_match_registry` (claims ⇔ `structural_fact_pattern_ids_for_language`). |
| Registry conformance over goldens | `tests/structural_fact_registry.rs:156` `structural_facts_conform_to_registry` — declared pattern, declared keys with matching value types, every `Always` key present. |
| Contract regeneration | `tests/structural_fact_registry.rs:295` `structural_fact_patterns_json_matches_checked_in_contract`, regen path `UPDATE_CONTRACT_JSON=1`. |
| Golden regeneration | `tests/golden.rs` `UPDATE_GOLDEN=1` (Task 3 §3). |
| Family line ceiling | `tests/structural_fact_registry.rs:379` `REGISTRY_FAMILY_CEILING = 700`. **This decided the module layout** — see §5.1. |

### Parse-tree evidence for the node kinds switched on

A scratch dump test (`src/tests/xml/scratch_dump.rs`, written, run, then **deleted** before commit — same
method as Task 3) printed real `tree-sitter-xml` 0.7.0 trees for an XSD and a WSDL sample. Observed and
switched on:

| Construct | Node kinds (verified) |
| --- | --- |
| root | `document` |
| `<?xml …?>` | `prolog` → `XMLDecl` |
| element | `element` → `STag`/`EmptyElemTag` + `content` + `ETag` |
| tag name | first `Name` child of `STag`/`EmptyElemTag` |
| attribute | `Attribute` → `Name` + `=` + `AttValue` (value text **includes** the quotes) |
| namespace declaration | a plain `Attribute` whose `Name` is `xmlns` or `xmlns:<prefix>` — the grammar does no special-casing |
| child elements | `element` → `content` → `element` (the `content` node sits between parent and children) |

The `content` interleaving is why `xml_parent_element` walks up through `content`, and why
`xml_child_elements` descends through it.

---

## 4. What was built

### New pattern specs — `base/structural_fact_registry/xml.rs` (10 specs, all `languages: ["xml"]`)

| Pattern id | Fires for | Query family | Metadata (ALWAYS unless marked OPT) |
| --- | --- | --- | --- |
| `xml.document.v1` | every `.xml`/`.xsd`/`.wsdl` with a root element | `document_structure` | `dialect`, `root_element`, `has_xml_declaration`, `element_count`, `max_depth`, `namespace_count` |
| `xml.namespace_declaration.v1` | every `xmlns=`/`xmlns:p=` attribute | `document_metadata` | `namespace_uri`, `is_default`, `prefix` (OPT) |
| `xml.xsd.type.v1` | `.xsd` named `complexType`/`simpleType` | `schema_structure` | `type_name`, `type_kind`, `base_type` (OPT) |
| `xml.xsd.element.v1` | `.xsd` top-level `element` | `schema_structure` | `element_name`, `type_ref` (OPT) |
| `xml.xsd.import.v1` | `.xsd` `import`/`include` | `schema_structure` | `import_kind`, `schema_location` (OPT), `namespace` (OPT) |
| `xml.wsdl.service.v1` | `.wsdl` `service` | `service_structure` | `service_name`, `port_count` |
| `xml.wsdl.port.v1` | `.wsdl` `port` | `service_structure` | `port_name`, `binding` (OPT) |
| `xml.wsdl.binding.v1` | `.wsdl` `binding` | `service_structure` | `binding_name`, `port_type` (OPT) |
| `xml.wsdl.message.v1` | `.wsdl` `message` | `service_structure` | `message_name`, `part_count` |
| `xml.wsdl.operation.v1` | `.wsdl` `operation` | `service_structure` | `operation_name`, `parent_kind` (OPT), `parent_name` (OPT), `input_message` (OPT), `output_message` (OPT) |

Every `ALWAYS` key is derived from a value that **gates emission** (no `name` attribute ⇒ no fact), which is
exactly the registry's stated rule for `Always` (`mod.rs:26-29`), and is what the golden-corpus conformance
test verifies.

### Emission — `base/data_structural_facts.rs`

- `collect_xml_structural_facts` builds an `XmlDocument { file_path, content, dialect }`, walks once
  (`collect_xml_node`, depth-guarded by `should_visit_tree_depth`/`child_tree_depth` like every sibling
  collector), then appends the single document fact from the accumulated `XmlDocumentStats`.
- `xml_dialect(file_path)` maps the extension: `.xsd` → schema layer, `.wsdl` → service layer, anything else
  → generic only. Case-insensitive.
- Component matching is on the **local name** (`xs:complexType`, `xsd:complexType`, and `complexType` all
  match), mirroring the Task 3 §6.5 attribute precedent. Recorded values keep their prefix.
- `xsd_base_type` does a bounded descendant search for the first `restriction`/`extension` carrying `base=`,
  and **stops at a nested `complexType`/`simpleType`** so an inner type's derivation is never attributed to
  the enclosing one.

---

## 5. Judgment calls (plan-consistent choices, each with a reason)

### 5.1 A new `structural_fact_registry/xml.rs` family module, not an entry in `data.rs`

The task said "likely a new `document/xml.rs` or an entry in the existing document module; discover the
actual layout". There is no `document/` directory — the registry is flat sibling modules (`data.rs`,
`sql.rs`, `http_client.rs`, `marker.rs`) plus one `web/` directory. The consistency argument said `data.rs`
(registry modules mirror collector sources). The **gate** said otherwise:
`tests/structural_fact_registry.rs:379` caps a family SPECS module at `REGISTRY_FAMILY_CEILING = 700` lines
and `data.rs` is already 600. Ten specs are 271 lines, so `data.rs` would have landed at ~870 and failed the
split test. `xml.rs` (271 lines) satisfies both the ceiling and the "must declare `pub(super) const SPECS`"
rule. `mod.rs` doc comments updated to list the new family and to note that the data collector now covers xml.

### 5.2 Document facts are document-level, not per-element

The direct mirror of `json.object.v1` / `yaml.mapping.v1` would be a fact per XML element. That would put
**6,004 facts** in `fixtures/extraction/xml/cardinality/expected.json` (currently a 6 KB file). The task's own
acceptance note — "cardinality golden should stay fact-free or minimal — verify and report" — plus Task 3 §6.8
(the cardinality fixture's rows deliberately carry no attributes, precisely to avoid 4,000 source-region rows)
make per-element facts the wrong shape for this language. So the generic layer is:

- one `xml.document.v1` per document, carrying the aggregate structure signal (`element_count`, `max_depth`,
  `namespace_count`, `root_element`, `has_xml_declaration`) that per-node facts would otherwise convey, and
- one `xml.namespace_declaration.v1` per `xmlns` attribute (naturally low cardinality; documents declare
  namespaces on a handful of elements, not on every row).

**Result: the cardinality golden gained exactly one fact** (from 0 to 1) and grew ~500 bytes. Verified —
see §7. A focused test (`repeated_anonymous_elements_do_not_multiply_facts`) locks this: 500 rows / 1,502
elements ⇒ the emitted pattern-id set is exactly `{xml.document.v1}`.

### 5.3 Pattern ids keep the `xml.` language prefix with a dialect segment

Two live conventions exist: `<language>.<thing>.v1` (the whole data family) and `<dialect>.<thing>.v1`
(`aspnet.*`, `nextjs.*`, `razor.*` — framework names, not languages). All three XSD/WSDL layers are the same
*language* (`xml`, for all of `.xml`/`.xsd`/`.wsdl` — one `LanguageSpec` row), so `xsd.type.v1` would have
detached the id prefix from the declared language for no gain. `xml.xsd.type.v1` keeps the data-family
language prefix and uses a middle segment, which `aspnet.minimal_api.route.v1` already establishes as
well-formed. No gate constrains the prefix (`registry/tests.rs` checks uniqueness, non-empty fields, and base
keys only).

### 5.4 `service_structure` is a new query family

Existing families were reused where they fit — `schema_structure` (already used by `json.schema.v1`/
`json.ref.v1`) for the XSD layer, `document_structure`/`document_metadata` for the generic layer. WSDL
service/port/operation/message/binding shapes are not schema, config, or document navigation, and the registry
already carries many single-use families (`interface`, `resources`, `transaction_structure`, …). `query_family`
is validated as non-empty only.

### 5.5 Dialect is extension-driven, not content-sniffed

`.xsd` ⇒ schema facts, `.wsdl` ⇒ service facts, `.xml` ⇒ generic only — the exact split the task text states
("Schema-aware facts for `.xsd`", "WSDL facts for `.wsdl`"). A `.xml` file containing a schema gets only
generic facts. This is deterministic, needs no heuristic, and matches how the language row already registers
its three extensions. Two focused tests pin it in both directions
(`schema_facts_are_dialect_scoped_to_their_extension`, `service_facts_are_dialect_scoped_to_their_extension`).

### 5.6 Only the five WSDL constructs the task named

`portType` gets no fact of its own; it appears as `parent_kind: "port_type"` / `parent_name` on the
operations it owns. The task listed services, ports, operations, messages, bindings — five — and inventing a
sixth was not mine to decide. Likewise the XSD layer is exactly named types + top-level elements +
imports/includes; `targetNamespace` on `xs:schema` is **not** captured (it would be a sixth pattern or a new
key on a pattern the task did not scope). Both are cheap follow-ups if the lead wants them.

### 5.7 `xml_local_name` is duplicated, deliberately

`src/xml/elements.rs:68` has an identical three-line helper. The data collector is intentionally
self-contained — it re-implements `node_text`, `child_text`, `count_direct_children` rather than borrowing
`BaseExtractor` helpers — and making `base/data_structural_facts.rs` depend on the `xml` extractor's private
module would invert the layering (the collector runs centrally in `registry.rs`, after and independent of the
per-language extractor). Three duplicated lines beat a new cross-module dependency.

---

## 6. Typed open gaps — verified and repointed

The task asked me to verify the existing xml gap rows still make sense and to refresh wording that referenced
"no structural facts" (Task 4 §8.4 precedent).

| Row | Before | After | Why |
| --- | --- | --- | --- |
| `kind_coverage.structural_facts.open_gaps[xml.schema_declaration]` | open, "no xml pattern specs are registered … beyond the language-agnostic comment-marker pattern" | **removed**; `supported` now lists the 10 pattern ids | The gap is closed. Its reason text was literally about this task. |
| `kind_coverage.relationships.open_gaps[references]` | `planned_closure_task: "…erlang-xml-language-support-plan.md Task 9: XML schema/WSDL structural facts"` | `planned_closure_task: "docs/plans/2026-05-31-julie-code-migration-implementation-plan.md Task 14: XML Reference Edge Closure"` | Task 9 is now done and does **not** close relationships. Task 3 §7.3 already flagged that these rows had no real owner; the `capability_gaps` rows for the same capability already point at migration-plan Task 14, so the two now agree. The gap's own reason (namespace resolution not performed) is still accurate and unchanged. |
| `kind_coverage.literals.open_gaps[other]` | pointed at Task 9 | same repoint to migration-plan Task 14 | Task 9 never owned literals either. |
| `capability_gaps` (relationships / pending_relationships / types) | — | **unchanged** | Already correct: the reasons describe QName-identifier emission and the absence of namespace resolution, never "no structural facts". Deferred schema-type relationship resolution therefore remains a typed `open_gaps`/`capability_gaps` entry, exactly as the task requires. |

`kind_coverage` open-gap rows are validated for non-empty `kind`/`reason`/`required_closure`/
`planned_closure_task` (`capability_matrix.rs:1717`), not for plan-file resolution — that stricter check
(`:561`) applies to top-level `capability_gaps` only, and those rows were left alone.

---

## 7. Golden review (hand-checked after `UPDATE_GOLDEN=1`)

All four goldens keep `parse_diagnostics: []`. Symbol and identifier counts are **unchanged** from Task 3
(basic 6/4, xsd 8/7, wsdl 11/3, cardinality 3/2) — this task added no symbols.

| Fixture | Facts | Content |
| --- | --- | --- |
| `basic` (.xml) | **1** | `xml.document.v1`: dialect `xml`, root `configuration`, `element_count 13`, `max_depth 5`, `namespace_count 0`, declaration true. Counted by hand against the source: 1 configuration + 1 appSettings + 3 add + 1 logging + 1 sinks + 2 sink + 1 path + 1 features + 2 feature = 13; deepest chain configuration→logging→sinks→sink→path = 5. ✅ |
| `xsd` | **10** | document (dialect `xsd`, root `xs:schema`, 18 elements, depth 6, 2 namespaces) + 2 namespace declarations (`xs`, `tns`, both `is_default:false`) + 2 imports (`import` with namespace+schemaLocation, `include` with schemaLocation only and **no** `namespace` key) + 1 top-level element (`AddPhoneRequest`, `type_ref` `tns:AddPhone`) + 4 types (`PhoneNumber` simple/base `xs:string`, `AddPhone` complex/no base, `AddMobilePhone` complex/base `tns:AddPhone` through `complexContent`>`extension`, `PhoneBook` complex/no base). Nested `xs:element` declarations (`owner`, `number`, `carrier`, the `ref=` one) correctly emit **no** element facts. ✅ |
| `wsdl` | **10** | document (dialect `wsdl`, root `definitions`, 15 elements, depth 4, 2 namespaces) + 2 namespace declarations (one `is_default:true` with **no** `prefix` key, one `tns`) + 1 service (`PhoneBook`, `port_count 1`) + 1 port (`PhoneBookPort`, binding `tns:PhoneBookBinding`) + 1 binding (`PhoneBookBinding`, `port_type` `tns:PhoneBookPort`) + 2 messages (`part_count 1` each) + 2 operations: the portType one carries `parent_kind port_type`, `parent_name PhoneBookPort`, `input_message`, `output_message`; the binding one carries `parent_kind binding`, `parent_name PhoneBookBinding` and **no** message keys. The two same-named `AddPhone` operations are disambiguated by their parent metadata. ✅ |
| `cardinality` | **1** | `xml.document.v1` only: `element_count 6004`, `max_depth 4`, `namespace_count 0`. 114 KB source → one fact. ✅ |

Fixture source change: `fixtures/extraction/xml/xsd/source.xsd` gained
`<xs:import namespace="urn:phonebook-common" schemaLocation="common.xsd" />` and
`<xs:include schemaLocation="phonebook-base.xsd" />` immediately after the `xs:schema` open tag (also the
schema-correct position). Neither carries `name`/`id`, so no new symbols; neither carries a reference
attribute, so no new identifiers. The only other golden effect is +3 `string_literal` source regions
(19 → 22, `AttValue` regions per Task 3 §6.8) and shifted spans below the insertion point.

`containing_key` note: the wsdl `port` fact binds to the enclosing `PhoneBook` **service** symbol rather than
the `PhoneBookPort` port symbol, because the shared `attach_containing_symbols` helper prioritises
`SymbolKind::Module` over `Variable` before span size (documented in Task 3 §6.3). This is the shared
cross-language helper's behaviour, not xml-specific, and is left alone.

---

## 8. Verification ledger

| Command | Result |
| --- | --- |
| `RUSTUP_TOOLCHAIN=1.97.1 cargo xtask test language xml` | **40 passed, 0 failed** (26 from Task 3 + 14 new) |
| `RUSTUP_TOOLCHAIN=1.97.1 cargo xtask test golden` | **3 passed, 0 failed** — includes `structural_facts_conform_to_registry` over the whole corpus |
| `RUSTUP_TOOLCHAIN=1.97.1 cargo xtask test capability` (worker ceiling) | **39 passed + 1 passed, 0 failed** |
| `RUSTUP_TOOLCHAIN=1.97.1 cargo xtask test changed fixtures/extraction/capabilities.json docs/contracts/structural-fact-patterns.json` | **exit 0**, 26 `test result: ok` blocks, zero failures (full `cargo test -p julie-extractors` = 3,162 passed) |
| `RUSTUP_TOOLCHAIN=1.97.1 cargo test -p julie-extract-cli --test operations_contract --test cli_contract` | **61 + 10 passed, 0 failed** — the CLI's `structural_fact_patterns` byte-equality check and the gap-count assertion both hold unchanged |
| `RUSTUP_TOOLCHAIN=1.97.1 cargo run -p julie-extract-cli -- languages --json` | xml lists all 10 new pattern ids plus `code.marker.v1` |
| `cargo fmt --all -- --check` | clean |
| `cargo clippy -p julie-extractors -p julie-extract-cli --all-targets` | zero warnings |

Invariants asserted by the task, both confirmed by the capability tier:
- every advertised pattern id has golden evidence, and every emitted pattern is advertised
  (`capability_matrix_structural_fact_claims_have_fixture_evidence`, bidirectional);
- claims equal the registry (`capability_matrix_structural_fact_claims_match_registry`);
- `docs/contracts/structural-fact-patterns.json` was regenerated with `UPDATE_CONTRACT_JSON=1`, never
  hand-edited — the diff is purely additive (+400 lines, 0 deletions).

### The 14 new focused tests

`every_xml_document_emits_one_document_fact`, `a_document_without_a_declaration_records_it`,
`a_document_without_a_root_element_emits_no_document_fact`,
`namespace_declarations_separate_prefixed_and_default_bindings`,
`schema_documents_emit_named_type_declarations_with_raw_base_qnames`,
`schema_documents_emit_only_top_level_element_declarations`,
`schema_documents_emit_imports_and_includes_with_their_locations`,
`service_documents_emit_services_ports_messages_and_bindings`,
`service_operations_record_their_owner_and_message_qnames`,
`schema_facts_are_dialect_scoped_to_their_extension`,
`service_facts_are_dialect_scoped_to_their_extension`,
`schema_components_are_matched_on_their_local_name`, `anonymous_schema_components_emit_nothing`,
`repeated_anonymous_elements_do_not_multiply_facts`.

Written **before** the implementation: the first run was 13 red / 1 trivially green, the post-implementation
run 14 green. Every test asserts concrete values (names, counts, raw QNames, exact pattern-id sets); none is
smoke-only.

---

## 9. Concerns and plan mismatches

### 9.1 Pre-existing red, unrelated to this task: `registry_pattern_ids_match_emitted_union_per_language`

`cargo test -p julie-extractors --features test-capability-matrix registry_pattern_ids_match_emitted_union`
**fails on the clean `890db84` baseline**, before any change of mine. `code.marker.v1` is registered for all
37 languages in `structural_fact_registry/marker.rs` but `structural_fact_pattern_ids_for_language`
(`base/structural_facts.rs:136`) unions only the five non-marker collectors, so every language reports
`not_emitted=["code.marker.v1"]` plus a global "registry pattern `code.marker.v1` is not emitted for any known
language".

- It is **not** in my assigned verification scope: `cargo xtask test capability` filters on the test name
  `capability_matrix` (`xtask/src/test_tiers.rs:251`), so this lib test never runs in any tier I was asked to
  run, and Task 3 could report the tier green while this was already red.
- My change does **not** worsen it: after the commit, the xml row still reports exactly
  `not_emitted=["code.marker.v1"]` — all ten new patterns are both registered and emitted.
- Fixing it means teaching `structural_fact_pattern_ids_for_language` about the marker collector (or excluding
  marker specs from the registry side of that comparison). That touches `base/structural_facts.rs`, which is
  outside my file ownership, and it is a 37-language decision, not an xml one. **Flagging for the lead / Task 10.**

### 9.2 Task 3 §7.3's ownership hole is now visible in the matrix

XML `relationships` and `literals` had `planned_closure_task` pointing at *this* task, which never owned them.
I repointed both to the migration plan's Task 14 (§6) so the rows stay truthful now that Task 9 is done, which
is the smallest honest fix. The underlying issue Task 3 raised is unchanged: the matrix still has no
`not_targeted` status, so `open` overstates intent for capabilities XML deliberately does not target.

### 9.3 Scope notes (deliberate, not gaps)

- `targetNamespace` / `elementFormDefault` on `xs:schema` and a `portType` fact are the two obvious
  extensions I did **not** make, because the task enumerated its three XSD and five WSDL constructs
  explicitly (§5.6).
- The XSD `base_type` key records the raw QName only. Resolving it to the declaration it names is exactly the
  deferred work the `relationships` open gap describes, and it stays deferred.
- No forcing gate made me touch a file outside the assigned list. `crates/julie-extractors/src/tests/capability_matrix.rs`,
  `docs/plans/…-migration-implementation-plan.md`, `docs/contracts/jsonl-v3.md` and `sqlite-schema-v4.md` all
  needed **no** change (the markup-table gate at `structural_fact_registry.rs:516` covers css/html/vue/razor
  patterns only).

---

## 10. Files changed

Created:
- `crates/julie-extractors/src/base/structural_fact_registry/xml.rs` (271 lines, 10 specs)
- `crates/julie-extractors/src/tests/xml/structural_facts.rs` (14 tests)

Modified:
- `crates/julie-extractors/src/base/data_structural_facts.rs` (pattern-id consts, `XML_DATA_PATTERN_IDS`,
  both `"xml"` match arms, the collector and its helpers)
- `crates/julie-extractors/src/base/structural_fact_registry/mod.rs` (`mod xml;`, `all_specs()`, doc comments)
- `crates/julie-extractors/src/base/structural_fact_registry/tests.rs` (`"xml"` added to `KNOWN_LANGUAGES`,
  which the comment requires to track the collector match arms)
- `crates/julie-extractors/src/tests/xml/mod.rs` (`mod structural_facts;`)
- `docs/contracts/structural-fact-patterns.json` (regenerated, +400 / -0)
- `fixtures/extraction/capabilities.json` (xml row only: structural-facts claims + two repointed gap tasks)
- `fixtures/extraction/xml/xsd/source.xsd` (import + include)
- `fixtures/extraction/xml/{basic,xsd,wsdl,cardinality}/expected.json` (regenerated + reviewed)

Deleted before commit: the scratch tree-dump test.

---

## 11. Self-review

| Acceptance criterion | Status |
| --- | --- |
| Pattern specs registered; contract JSON regenerated; `cargo xtask test capability` green | ✅ 10 specs, `UPDATE_CONTRACT_JSON=1`, 39+1 passed |
| XSD golden asserts type/element/import facts | ✅ 4 type + 1 element + 2 import facts |
| WSDL golden asserts service/operation/message/binding facts | ✅ all five WSDL patterns present |
| basic golden asserts generic document facts | ✅ `xml.document.v1` |
| `open_gaps` entries remain typed and truthful | ✅ closed the structural-facts gap, repointed two stale closure tasks (§6) |
| `cargo xtask test language xml` green (26 existing + new) | ✅ 40 passed |
| `cargo xtask test golden` green | ✅ 3 passed |
| Verification passes and the change is committed | ✅ `6639f1cf`, committed only owned files |
| Cardinality golden stays fact-free or minimal — verify and report | ✅ exactly 1 fact; §5.2 and §7 |

Findings fixed during self-review:
- `collect_xml_node` had eight parameters and an `#[allow(clippy::too_many_arguments)]`. Replaced the
  suppression with an `XmlDocument { file_path, content, dialect }` context struct, bringing the recursive
  walker to six parameters — matching `collect_yaml_node`'s shape — and re-ran fmt, clippy, and all three
  tiers afterwards.
- The first `capabilities.json` rewrite used `ensure_ascii=False` and silently unescaped `—`/`’`
  across **20 unrelated language rows**. Reverted and redone with ASCII escapes preserved; the committed diff
  is 15 insertions / 11 deletions, all inside the xml row.
- `element_count` in the first draft test expected 4 for a 3-element document; corrected against a hand count
  before implementing, so the test never encoded the implementation's answer.
