# Task 12 — Erlang structural facts + strict gate green

**Worktree:** `/Users/murphy/source/julie-extractors/.worktrees/erlang-xml-language-support`
**Branch:** `erlang-xml-language-support`
**Start HEAD:** `b4f6e9d` (clean)
**Toolchain:** every cargo invocation prefixed `RUSTUP_TOOLCHAIN=1.97.1`

**Exit criterion: MET.** `node scripts/language-data-quality-report.mjs --strict` exits **0**,
`## Quality-Bar Debt` reads `none`, and `structural_facts` reports
`applicable_closure: 38/38 complete` with `unclassified_gaps: 0`.

---

## 1. Spec set chosen, and why

Five pattern specs, all module-header shapes a `.erl` file declares before any function body:

| pattern_id | node kinds | query_family | metadata keys |
| --- | --- | --- | --- |
| `erlang.module_attribute.v1` | `module_attribute` | `module` | `module` (str/always) |
| `erlang.behaviour_declaration.v1` | `behaviour_attribute` | `otp` | `behaviour` (str/always), `attribute` (str/always) |
| `erlang.callback_declaration.v1` | `callback` | `otp` | `callback_name` (str/always), `arity` (num/always) |
| `erlang.export_attribute.v1` | `export_attribute`, `export_type_attribute` | `module` | `export_kind` (str/always), `exported_count` (num/always) |
| `erlang.include_directive.v1` | `pp_include`, `pp_include_lib` | `imports` | `include_kind` (str/always), `path` (str/always), `application` (str/optional) |

This is exactly the candidate list the task text named (behaviour, includes, `-export` groups,
`-callback`, `-module`), sized to the density other code languages carry (elixir 5, lua 5, zig 5,
gdscript 5, r 3). Design decisions worth recording:

- **Every key is a scalar.** Miller's `patterns` surface filters with a top-level metadata equality
  filter (`where key=value`) and facets on a single metadata key. A `StringArray` of exported
  `name/arity` entries would be unfilterable there, so `-export` emits `exported_count` instead of
  the names. The names are not lost: exported functions and types already carry `Visibility::Public`
  on their symbols (`erlang/attributes.rs:159-163` for types), so "what does this module publish" is
  answerable without duplicating the list into fact metadata.
- **One pattern per shape, kind discriminated by a metadata key** — `include_kind`,
  `export_kind` — following Task 9's `xml.xsd.import.v1` / `import_kind` precedent rather than
  minting `erlang.include_lib_directive.v1` as a separate id.
- **`attribute` records the source spelling.** The grammar folds `-behaviour` and `-behavior` into a
  single `behaviour_attribute` node (`grammar.js:325-332`, `choice(atom_const('behaviour'),
  atom_const('behavior'))`), so the spelling only survives in the raw text. Both spellings are valid
  Erlang and the distinction is otherwise unrecoverable downstream.
- **`application` is optional and `include_lib`-only.** `-include_lib("stdlib/include/assert.hrl")`
  resolves through an OTP application's lib directory, so the leading segment names a real
  dependency. A plain `-include` path and a separator-free `include_lib` path name no application,
  so the key stays absent rather than repeating the bare filename.
- **Emission is gated on the value that would be invented otherwise.** A macro-spelled include
  (`-include(?HDR).`) carries no string literal, so it emits nothing — no fact with a guessed path.
  Same rule for a macro-spelled module/behaviour/callback name. This is what makes the name and path
  keys legitimately `Always` under the registry's presence rule ("when a key is derived from a value
  that gates emission it is `Always`", `structural_fact_registry/mod.rs:26-29`).

**Deliberately excluded** (not padding the set): `record_decl` (records already emit Struct + Field
symbols), `import_attribute` (already a structured pending `Imports` edge; rare in idiomatic Erlang),
`optional_callbacks_attribute` (would need a whole-file back-scan per callback for one boolean),
`spec` (already a type fact), per-`fa` export entry facts (duplicates symbol visibility and would
multiply corpus rows).

**Placement.** `crates/julie-extractors/src/base/structural_fact_registry/builtins/erlang.rs`, not
top-level `structural_fact_registry/erlang.rs`. The registry families mirror the emission sources
(`mod.rs:15-20`): `builtins/*` covers `base/structural_facts.rs` + `base/code_structural_facts.rs`,
`data.rs`/`xml.rs` cover `base/data_structural_facts.rs`. Erlang is a code language emitting from
`code_structural_facts.rs`, so it belongs under `builtins/`. The 700-line family ceiling did not
force a new file (`builtins/scripting.rs` is 308 lines, `extra.rs` 366), but both of those declare
their language set in the module doc header ("PHP, Ruby, Elixir, and Lua" / "R, Zig, QML, Bash,
PowerShell, GDScript, and VB.NET"), so a dedicated module matches Task 9's one-module-per-new-
language-slice precedent without rewriting a sibling's stated scope. The new file is 113 lines and
declares `pub(super) const SPECS`, satisfying
`structural_fact_registry_is_split_into_family_modules`.

## 2. Miller calls used, and what each confirmed

| Call | Confirmed |
| --- | --- |
| `context query="structural fact registry pattern specs how a language module registers pattern specs and emits structural facts"` (budget 2400) | The registry seam: `structural_fact_pattern_specs()` at `structural_fact_registry/mod.rs:167` backed by a `OnceLock` over `all_specs()`; `StructuralFactPatternSpec`/`MetadataKeySpec` at :78/:65; serializers `structural_fact_patterns_json` :218 and `structural_fact_patterns_contract_json` :266; and the six invariant tests in `structural_fact_registry/tests.rs` plus the corpus-conformance test `structural_facts_conform_to_registry`. Disposition `sufficient`. |
| `inspect target="crates/.../structural_fact_registry/xml.rs"` | Task 9's module is a single `pub(super) const SPECS: &[StructuralFactPatternSpec]` at :16-271 with one import — the exact shape to mirror for `builtins/erlang.rs`. |
| `trace target=structural_fact_pattern_specs mode=refs` | 17 exact references, all in-crate: the JSON serializer (`mod.rs:220`), eight registry invariant tests, four cross-artifact tests in `tests/structural_fact_registry.rs`, and `marker_language_matrix_covers_every_supported_comment_language` (`tests/marker_structural_facts.rs:355`). No production consumer outside the registry reads the spec list directly, so growing it cannot change extraction behaviour for any other language — it only widens what the JSON contract advertises and what the conformance tests check. That last reference is why the `code.marker.v1` finding in §7 was worth chasing. |

Non-Miller reads were used only where Miller cannot help: the pinned grammar (not in the Rust index)
and file-content reads of the specific modules Miller pointed at.

## 3. API-shape evidence

Every symbol, signature, node kind, and config shape relied on, with its source:

**Grammar (pinned `tree-sitter-erlang` 0.20.0, `Cargo.toml:46` `= "0.20.0"`).** Node kinds and field
structure read from
`~/.cargo/registry/src/index.crates.io-.../tree-sitter-erlang-0.20.0/src/node-types.json` and
`grammar.js` — **not** an s-exp dump (node-types.json is authoritative for fields and named-ness, and
grammar.js settled the two questions node-types.json cannot answer: the `behaviour`/`behavior`
alternation and whether the attribute keyword is a named node).

- `behaviour_attribute { name: _name }` — `node-types.json`; alternation at `grammar.js:325-332`.
- `module_attribute { name: _name }` — `grammar.js:323`.
- `callback { fun: _name, module?: module, sigs: type_sig+ }` — `node-types.json`; `grammar.js:506`.
- `export_attribute { funs?: fa* }` / `export_type_attribute { types?: fa* }` —
  `grammar.js:334-343` and `:367-377`; both list `fa` as direct children.
- `fa { fun: _name, arity: arity }` — `grammar.js:365`.
- `pp_include { file: _include_detail+ }` / `pp_include_lib { … }` where
  `_include_detail = string | macro_call_expr` — `grammar.js:289-320`. This is the evidence that a
  macro-spelled include has no `string` child, which is what the emission gate keys on.
- The attribute keyword (`module`, `callback`, `behaviour`, …) is an **anonymous** token:
  `node-types.json` carries `{"type":"module","named":false}`, `{"type":"callback","named":false}`,
  `{"type":"spec","named":false}` entries. That is why `first_direct_child(node, "atom")` returns the
  declared name rather than the keyword — the same assumption the already-shipped
  `erlang/relationships.rs:277` (`emit_behaviour`) and `erlang/attributes.rs:21` (`extract_module`)
  rely on.

**Rust API shapes used, all read from source before use:**

- `CodeStructuralPattern { pattern_id, capture_name, node_kinds: &[&str], query_family }` —
  `code_structural_facts.rs:12-18`.
- `matches_pattern(language, content, node, pattern_id) -> bool`, dispatched per
  `(language, pattern_id)` with `_ => true` default — `code_structural_facts.rs:1160`.
- `enrich_metadata(language, content, node, pattern_id, &mut HashMap<String, Value>)` —
  `code_structural_facts.rs:751`.
- `first_direct_child(node, kind)` walks **all** children (named and anonymous), first match —
  `code_structural_facts.rs:1085-1089`. Correct here because the target kinds (`atom`, `string`,
  `type_sig`, `expr_args`) are all named and unique-first among the direct children.
- `node_text(content, node)`, `insert_string(metadata, key, &str)`,
  `insert_number(metadata, key, u64)` — `code_structural_facts.rs:1146-1158`. No array helper was
  needed because no key is an array.
- `StructuralFactPatternSpec { pattern_id, languages, query_family, description, metadata_keys }`
  and the authoring helpers `key(...)`, `K_PATTERN_VERSION`, `K_QUERY_FAMILY`, `ALWAYS`, `OPT`,
  `STR`, `NUM` — `structural_fact_registry/mod.rs:78-127`. `BASE_KEYS` was not used since every spec
  adds keys beyond the two base ones, matching how `xml.rs` spells them out.
- Contract regeneration switch `UPDATE_CONTRACT_JSON=1` against
  `structural_fact_patterns_contract_json()` — `structural_fact_registry/mod.rs:266-271`.
- Capability-gate contract: `structural_fact_claims == structural_fact_pattern_ids_for_language(lang)`
  (`tests/capability_matrix.rs:1001-1021`) and claimed ⇔ observed over fixture sources
  (`:924-953`) — **bidirectional**, which is why `ERLANG_PATTERN_IDS` and the capabilities.json
  `supported` list must both be exactly the five ids.
- Scorecard debt rule: a debt is recorded when `supported` **and** `not_applicable` are both empty
  for a domain in `expectedDomainsFor(language)`; `structural_facts` is in
  `CODE_LANGUAGE_EXPECTATIONS` — `scripts/language-data-quality-report.mjs:44-61, 316-325`.

## 4. Gate table

Run at final worktree state (all files staged for the commit below). Exit codes are the real `$?` of
each command, not a piped tail.

| # | Gate | Exit | Invariant it proves |
| --- | --- | --- | --- |
| 1 | `cargo xtask test default` | **0** | Fast tier unbroken: no existing language's facts, symbols, or resolution tiers shifted. |
| 2 | `cargo xtask test golden` | **0** | Regenerated erlang goldens are byte-exact against canonical extraction, and `structural_facts_conform_to_registry` proves every emitted fact's pattern is declared, every emitted key is declared with a matching value type, and every `Always` key is present across the whole corpus. |
| 3 | `cargo xtask test capability` | **0** | Bidirectional claim↔evidence↔registry: the erlang `supported` list equals `structural_fact_pattern_ids_for_language("erlang")` *and* equals what the erlang fixtures actually emit. 39 capability-matrix tests + pending-shape contract. |
| 4 | `cargo xtask test certification` | **0** | Parser-upgrade gate still covers the full language inventory with variants visible. |
| 5 | `cargo xtask test changed <18 touched paths + src/lib.rs>` | **0** | The changed-path router accepted the golden-output change with `lib.rs` in the set (the gate's own requirement) and escalated to 6 cargo commands including the certification plan. |
| 6 | `cargo xtask test language erlang` | **0** | 117 erlang tests, including the 11 new structural-fact tests. |
| 7 | `cargo xtask test language xml` | **0** | 43 xml tests — Task 9's slice is untouched by the shared registry/collector edits. |
| 8 | `cargo test -p julie-extract-cli --features test-real-world --test erlang_corpus` | **0** | 3/3: hex.pm corpus scans every file against the committed baseline, checksums match, behaviour/export edges hold. |
| 9 | `cargo run -p julie-extract-cli -- languages --json` | **0** | Runtime report publishes the erlang row with all five patterns in `kind_coverage.structural_facts.supported`, `open_gaps: []`, `capability_gaps: 0`; `structural_fact_patterns` carries 210 specs including the five erlang ones. |
| 10 | `node scripts/language-data-quality-report.mjs --strict` | **0** | **The task's exit criterion.** Zero quality-bar debts, zero silent cells; `structural_facts` closure 38/38. |
| 11 | `node scripts/reference-resolution-coverage-report.mjs --strict` | **0** | The regenerated coverage digest matches the current capabilities.json + goldens (it is also re-run inside gate 10). |
| 12 | `cargo fmt --check` | **0** | Clean after one `cargo fmt` pass (rustfmt reflowed four call sites in the new code; no semantic change). |
| 13 | `cargo clippy --workspace --all-targets --all-features` | **0** | Zero warnings. |
| 14 | `cargo deny check` | **0** | `advisories ok, bans ok, licenses ok, sources ok` (the pre-existing xtask path-dependency wildcard note is informational, unchanged). |

Verification order was TDD: the 11 new tests in `tests/erlang/structural_facts.rs` were written and
run **red** first (9 failed, 2 passed vacuously), then the registry + collector landed and all 11
passed, then regeneration, then the gate sweep.

## 5. Corpus-baseline deltas

**None. The baseline was not touched.** `crates/julie-extract-cli/tests/erlang_corpus.rs` asserts
per-file `rows.symbols` and `rows.parse_diagnostics` (`:133-136`) plus aggregate file counts and
behaviour/export edges — it carries no structural-fact assertion, so five new fact rows per module
shift nothing it measures. The diagnostics baseline (45 total / 2 files) is unchanged and the gate
passed 3/3 with the file untouched. The plan's explicit allowance to update the baseline was not
needed.

## 6. Out-of-ownership touches

**One**, and it is the ownership list's "or per-ceiling placement" clause rather than a gate forcing
it: the new registry module is at
`crates/julie-extractors/src/base/structural_fact_registry/builtins/erlang.rs` instead of
`.../structural_fact_registry/erlang.rs`. Both paths are inside the owned
`base/structural_fact_registry/*` glob; the rationale is in §1.

Everything else is inside the stated ownership:
`base/code_structural_facts.rs` (emission), `base/structural_fact_registry/{mod.rs, tests.rs,
builtins/mod.rs, builtins/erlang.rs}` (wiring), `src/tests/erlang/{mod.rs, structural_facts.rs}`,
`docs/contracts/structural-fact-patterns.json`, `fixtures/extraction/erlang/*/expected.json`,
`fixtures/extraction/capabilities.json` (erlang row only),
`fixtures/extraction/reference-resolution-coverage.json`, and
`docs/plans/2026-05-31-julie-code-migration-implementation-plan.md` (Task 13).

`base/data_structural_facts.rs` was **not** touched — Task 9 needed it only because xml emits from
the data collector; erlang emits from the code collector.

Two small edits inside owned files worth naming explicitly:
- `structural_fact_registry/tests.rs`: added `"erlang"` to `KNOWN_LANGUAGES`, whose comment states it
  is "kept in sync with the collector match arms". Erlang now has a code-collector arm.
- `structural_fact_registry/mod.rs`: added `erlang` to the module-header list of languages
  `base/code_structural_facts.rs` covers.

`capabilities.json` was edited **textually**, not by a JSON round-trip. A
`JSON.parse`/`JSON.stringify` pass silently unescaped 14 unrelated `—` sequences into literal
em dashes across other languages' rows; that was reverted and replaced with a surgical edit, so the
committed diff is 8 insertions / 9 deletions confined to the erlang `structural_facts` block.

## 7. Deviations, plan mismatches, and a pre-existing finding

**No plan mismatch.** The approved architecture (the existing structural-fact registry seam, exactly
as Task 9 used it) held; no Architecture Impact.

**Deviation from the task text, deliberate:** the task's ownership list names
`structural_fact_registry/erlang.rs`; the module landed at `builtins/erlang.rs`. See §1/§6.

**Pre-existing failing test, NOT introduced here and NOT fixed here:**
`base::structural_fact_registry::tests::registry_pattern_ids_match_emitted_union_per_language`
(gated behind `--features test-capability-matrix`) fails at `b4f6e9d` and still fails identically
after this change. Measured both ways:

- at `b4f6e9d` (via `git stash`): **37** `language … mismatch` lines, all
  `not_emitted=["code.marker.v1"]`, plus `registry pattern "code.marker.v1" is not emitted for any
  known language`.
- at this commit: **37** lines, same single root cause.

The cause is that `code.marker.v1` declares 38 languages in `structural_fact_registry/marker.rs` but
`structural_facts::structural_fact_pattern_ids_for_language` unions only the built-in, code,
framework, web, data, and sql collectors — never the marker collector
(`base/structural_facts.rs:127-141`). Erlang was already one of the 37 before this task (the test
unions `spec.languages`, and marker.rs already listed erlang), so adding erlang to `KNOWN_LANGUAGES`
changed the count by zero. No xtask plan references this test — `grep registry_pattern_ids
xtask/src/test_tiers.rs` returns nothing — which is why it has been red without failing a gate.
Fixing it means either wiring `code.marker.v1` into the emitted-ids union or scoping the test to
exclude the language-agnostic marker pattern; both are contract decisions outside this task's
ownership. **Flagging, not fixing.**

Note this is also *why* erlang's `supported` list must be exactly the five erlang ids and must not
include `code.marker.v1`: the capability gate compares against
`structural_fact_pattern_ids_for_language`, which excludes it. A corollary constraint for future
work: **no erlang fixture source may contain a TODO/FIXME/HACK/XXX comment marker**, or
`code.marker.v1` would start appearing in `observed` and break
`capability_matrix_structural_fact_claims_have_fixture_evidence`. None of the six erlang fixtures
contains one today.

**No fixture source was modified.** All five patterns already had golden evidence in the existing
fixtures — `basic` carries `-module`, `-behaviour`, `-export`, `-export_type`, and `-callback`;
`cross_file` carries both `-include` and `-include_lib`; the `application` optional key is exercised
by `stdlib/include/assert.hrl`. Task 9 had to add an `xs:import`/`xs:include` to its xsd fixture;
this slice needed nothing, which is why the golden diff is +502/-15 lines of pure structural-fact
rows across six files.

**`EXTRACTION_CONTRACT_VERSION` was not bumped**, following Task 9's precedent (commit `6639f1c`
shipped ten xml patterns without touching `lib.rs`). The version string already carries
`structural-facts-v1`; adding a new language's facts under that existing shape is not a contract
shape change. The `changed` gate's requirement — include `lib.rs` in the changed path set when a
golden expected output changes — was satisfied (gate 5).

## 8. Concerns

1. **`registry_pattern_ids_match_emitted_union_per_language` is red and ungated** (§7). It is the
   test whose stated job is "no registry pattern is dead and no emitted pattern is unregistered" —
   exactly the invariant this task depends on — and it is currently proving nothing because
   `code.marker.v1` short-circuits it for all 38 languages. The capability-matrix gate covers the
   same ground per-language and *is* wired in, so the branch is not exposed; but the marker
   pattern's own registry↔emission agreement is unverified by any running gate.
2. **The "no markers in erlang fixtures" constraint is implicit.** Nothing fails fast if someone adds
   a `%% TODO` to an erlang fixture; the failure surfaces as a confusing
   `capability_matrix_structural_fact_claims_have_fixture_evidence` message about an unadvertised
   `code.marker.v1`. This is a repo-wide property of the marker pattern, not erlang-specific, and it
   is the same latent trap for the 37 other languages.
3. **`exported_count` without the names is a deliberate ceiling.** If a downstream Miller workflow
   turns out to want "which module exports `handle_call/3`", the answer today is symbol visibility,
   not a fact query. Revisiting would mean per-`fa` facts (`erlang.export_entry.v1`) and a corpus
   baseline that does move. Recording the trade so it is a decision, not an oversight.
