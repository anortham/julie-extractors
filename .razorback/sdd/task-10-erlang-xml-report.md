# Task 10 — Branch gates + repo docs

**Status:** COMPLETE with one non-green gate escalated (see §5).
**Worktree:** `/Users/murphy/source/julie-extractors/.worktrees/erlang-xml-language-support`
**Branch:** `erlang-xml-language-support`
**HEAD at start:** `0f7d53c` · **HEAD at end:** `e2d39c0`
**Commit:** `e2d39c0` — `chore: close the erlang/xml branch gates and record parity guards`
**Dirty state at end:** clean except the untracked `.razorback/sdd/task-10-erlang-xml-report.md` (this file, intentionally uncommitted).

Note on the assignment's context: `.razorback/sdd/task-{8,9}-erlang-xml-report.md` were already
committed (in `0f7d53c` and `f3c2f52`), not untracked. Nothing of theirs was touched.

---

## 1. Part A — cleanup

### A1. Erlang residual gap pointers normalized

Four `planned_closure_task` strings in `fixtures/extraction/capabilities.json` pointed at
`docs/plans/2026-07-31-erlang-xml-language-support-plan.md Task 8: Erlang real-world corpus gate`,
a task that is done and never owned them:

| Domain | Kind(s) |
| --- | --- |
| `structural_facts` | `erlang.behaviour_declaration` |
| `complexity_metrics` | `file`, `symbol` |
| `literals` | `other` |

All four now point at
`docs/plans/2026-05-31-julie-code-migration-implementation-plan.md Task 13: Erlang Capability Closure`,
byte-mirroring the form XML's kind rows already use for its Task 14.

Task 13's text previously described only the four capability rows (relationships, pending,
identifiers, types) that the branch plan closed. It was extended to name the three
`kind_coverage` residuals explicitly plus a fifth acceptance criterion, so the registry entry
actually covers what now points at it. Task 14 was extended the same way for XML
attribute-value literals (§5 explains why that gap matters now).

Verified: `cargo xtask test capability` green (39 + 1 + 2 tests, exit 0). The
`capability_matrix_open_rows_have_planned_closure_task` guard resolves every pointer against the
migration plan, so a bad string would have failed here.

### A2. Corpus gate wired into the xtask real-world tier

`real_world_release_plan()` in `xtask/src/test_tiers.rs` is the right tier — Task 8's
recommendation checks out against the existing structure (`real-world` and `real-world-release`
are aliases for it; `real-world-smoke` is the deliberately narrower profile). Added:

```
cargo test -p julie-extract-cli --features test-real-world --test erlang_corpus
```

Two assertions in `xtask/tests/test_tiers.rs` were updated:

- `test_real_world_smoke_and_release_profiles_are_separate` — asserts the exact release command
  list, so the new command had to be added there or the tier change would not compile past it.
- `test_real_world_tier_selects_every_real_fixture_gate` (~:241) — previously only asserted that
  `real-world` and `real-world-release` resolve to the same plan, which does not match its name.
  Added a `contains` assertion for the corpus gate so the test earns its title. This is the
  assertion the task description pointed at; the exact-list one is the sibling.

Proof the wiring runs: `cargo xtask test real-world` exit 0, with

```
+ cargo test -p julie-extract-cli --features test-real-world --test erlang_corpus
     Running tests/erlang_corpus.rs
test result: ok. 3 passed; 0 failed; ... finished in 3.58s
```

### A3. Convention guard mirrored (cheap — done)

`crates/julie-extract-cli/tests/perf_gate_convention.rs` gained
`erlang_corpus_gate_is_feature_gated_out_of_default_suite`, a 20-line copy of the existing
perf-gate shape: asserts `test-real-world = []` exists in the crate manifest and that
`tests/erlang_corpus.rs` starts with `#![cfg(feature = "test-real-world")]`. Same file rather
than a sibling, because the two guards share the `read()` helper and the same rationale.

`cargo test -p julie-extract-cli --test perf_gate_convention` — 2 passed.

### A4. Checklist §2 updated with the per-language parity guards

`docs/languages/new-language-checklist.md` §2 now carries a "shared per-language tables" block.
Tasks 2 and 3 discovered five by test failure; this task found a **sixth and a seventh** the
same way (§5, §2.11):

1. `base/source_regions.rs` `RegionLanguageConfig` + `tests/source_regions.rs` fixture —
   guard `supported_languages_with_source_region_syntax_emit_regions`.
2. `base/structural_fact_registry/marker.rs` `code.marker.v1` language list (+ decorations in
   `base/marker_structural_facts.rs`, + `UPDATE_CONTRACT_JSON=1` regen) — guard
   `marker_language_matrix_covers_every_supported_comment_language`.
3. `base/body.rs` `comment_syntax` arm — **no guard**; without it a comment-only edit silently
   changes `body_hash`. Called out as unguarded in the doc.
4. `crates/julie-extract-cli/tests/operations_contract.rs` `open_reference_resolution_gaps`
   +3 per language.
5. `crates/julie-extractors/src/tests/capability_matrix.rs` `DOMAIN_LANGUAGES` for data
   languages — guard `capability_matrix_code_languages_require_resolved_test_detection`.
6. **NEW:** `scripts/language-data-quality-report.mjs` `DOMAIN_LANGUAGES` — a *separate*
   set with the same name as (5). XML was in (5) but not (6), so the strict scorecard treated
   XML as a code language and charged it quality-bar debt for `test_detection`.
7. **NEW:** `fixtures/extraction/reference-resolution-coverage.json` must be regenerated with
   `node scripts/reference-resolution-coverage-report.mjs --write`; it is exact over the
   language registry and a new language makes it stale.

---

## 2. Part B — verification ledger

Every command run from the worktree root with `RUSTUP_TOOLCHAIN=1.97.1` unless noted. All
results below are the **final post-commit sweep** at `e2d39c0`.

| # | Gate | Result |
| --- | --- | --- |
| 2.1 | `cargo xtask test default` | **exit 0** (11.7 s wall) |
| 2.2 | `cargo xtask test golden` | **exit 0** |
| 2.3 | `cargo xtask test capability` | **exit 0** |
| 2.4 | `cargo xtask test language erlang` | **exit 0** — 99 passed |
| 2.5 | `cargo xtask test language xml` | **exit 0** — 40 passed |
| 2.6 | `cargo xtask test changed crates/julie-extractors/src/language_spec/specs.rs fixtures/extraction/capabilities.json` | **exit 0** — selects capability + parser certification |
| 2.7 | `cargo xtask test certification` | **exit 0** (required: parser deps changed this branch) |
| 2.8 | `cargo xtask test contract` | **exit 0** |
| 2.9 | `cargo xtask test real-world` (corpus gate via new wiring) | **exit 0** — 5 commands incl. `erlang_corpus` 3 passed |
| 2.10 | `cargo run -p julie-extract-cli -- languages --json` | **exit 0** — see §3 |
| 2.11 | `node scripts/language-data-quality-report.mjs --strict` | **exit 1** — see §5 |
| 2.12 | `cargo deny check` | **exit 0** — advisories/bans/licenses/sources ok; only the pre-existing path-dep wildcard warnings (task-1 report), no new errors |
| 2.13 | `cargo fmt --all -- --check` | **exit 0** |
| 2.14 | `cargo clippy --workspace --all-targets` | **exit 0** — zero warnings |
| 2.15 | `cargo test -p xtask` (tier assertions) | **exit 0** — 15 passed |
| 2.16 | `cargo test -p julie-extract-cli --test perf_gate_convention` | **exit 0** — 2 passed |

13 of 14 assigned gates green (plus two focused runs); one red, escalated in §5.

### Gate-forced file changes

| File | Forcing gate |
| --- | --- |
| `fixtures/extraction/reference-resolution-coverage.json` | 2.11 — `report source_digest is stale; regenerate with --write` plus 38 `<lang>/<origin>/<kind>: silent cell` problems for erlang and xml. Regenerated with `--write`: 689 → 728 cells, 36 → 38 languages, silent_cells 0, quality_bar_debts 0. |
| `scripts/reference-resolution-coverage-report.mjs` | 2.11 — its failure message hard-coded "the 36-language registry"; now interpolates `expectedLanguages.length`. No test asserts the old string. |
| `scripts/language-data-quality-report.mjs` | 2.11 — added `xml` to `DOMAIN_LANGUAGES` (parity guard 6 above). Dropped quality-bar debt from 5 to 4. |

---

## 3. `languages --json` verification (gate 2.10)

```
total languages: 38
erlang  actual == target == {symbols, relationships, pending_relationships, identifiers, types} all true
        capability_gaps: 0
        kind_coverage open gaps: complexity_metrics [file, symbol], literals [other],
                                 structural_facts [erlang.behaviour_declaration]
xml     actual == target == {symbols: true, identifiers: true, relationships: false,
                             pending_relationships: false, types: false}
        capability_gaps: 3
        kind_coverage open gaps: literals [other], relationships [references],
                                 test_detection [test_case, test_container, test_lifecycle]
```

Matches the assignment exactly: erlang row FULL with zero `capability_gaps`; xml row
symbols+identifiers with its 3 typed gaps; total 38.

---

## 4. Part C — repo docs

`README.md` "Supported languages" was the only live 36-language claim in the repo (`grep` over
non-`.memories`, non-`.razorback` paths found nothing else current — the design doc's mention is
historical). Updated:

- "reports 36 languages" → "reports 38 languages".
- The enumerated list gained `erlang` (after `elixir`) and `xml` (after `vue`), preserving
  alphabetical order.
- Added two factual sentences: Erlang at the full capability tier (symbols, relationships,
  pending relationships, identifiers, types); XML at the data tier (symbols, identifiers) plus
  document, XSD, and WSDL structural facts. Both statements are exactly what §3 shows the
  branch ships — nothing about literals, complexity, or reference edges is implied.

The README paragraph immediately below states the strict scorecard "requires zero silent
capability cells and zero quality-bar debts". That statement is currently true of the gate and
false of the branch — see §5. It was left unchanged rather than weakened to match a temporary
regression.

---

## 5. BLOCKER — strict data-quality scorecard (gate 2.11)

### What fails

```
$ node scripts/language-data-quality-report.mjs --strict     # exit 1
languages: 38
silent_cells: 0
quality_bar_debts: 4

## Quality-Bar Debt
erlang.complexity_metrics open_gap
erlang.literals open_gap
erlang.structural_facts open_gap
xml.literals open_gap
```

The nested strict check it spawns — `scripts/reference-resolution-coverage-report.mjs --strict`
— is now **green** (`{"languages":38,"cells":728,"silent_cells":0,"quality_bar_debts":0}`); that
half was fixed here by regenerating the coverage artifact.

### Baseline

`main` at `4bee2fe` reports `languages: 36, silent_cells: 0, quality_bar_debts: 0, exit 0`. This
is a **regression introduced by this branch**, not a pre-existing red.

### Why it fires

`quality_bar_debts` counts, for each domain a language is *expected* to cover, the case where
`kind_coverage.<domain>` has **neither** a `supported` kind **nor** a `not_applicable` kind — a
domain with nothing positive claimed and nothing documented as inapplicable. Open gaps alone do
not satisfy it, however well-typed and well-owned they are.

Language classification decides which domains are expected:
`CODE_LANGUAGE_EXPECTATIONS` (8 domains, incl. `complexity_metrics`, `test_detection`,
`annotations`, `doc_comments`) vs `DOMAIN_LANGUAGE_EXPECTATIONS` (4: `identifiers`, `literals`,
`source_regions`, `structural_facts`).

I fixed one of the original five here: XML was missing from the script's `DOMAIN_LANGUAGES`, so
it was scored as a code language and charged for `test_detection`. Adding it puts XML in exactly
the posture json/toml/yaml/html/markdown/sql already have (all of them carry three open
`test_detection` gaps and are not charged). That is a genuine parity-table miss, not a
suppression.

The remaining four are **real coverage gaps**, and the repo has no precedent for tolerating
them:

- Every other one of the 38 languages has a non-empty `literals.supported`. Erlang and XML are
  the only two without.
- Every other language has a non-empty `structural_facts.supported`. Erlang is the only one
  without.
- Every language whose `complexity_metrics.supported` is empty (`css`, `html`, `json`,
  `markdown`, `toml`, `yaml`, `xml`) records `not_applicable: [file, symbol]`. Erlang cannot
  honestly do that — `case`, `if`, `receive`, `try`, guards, and multi-clause functions are all
  decision points.

### Why I did not close them here

Each is feature work in a domain this task does not own:

| Debt | What closing it takes |
| --- | --- |
| `erlang.complexity_metrics` | An `ERLANG_CONFIG` entry in `base/complexity_metrics.rs` (a const node-kind table, ~30 lines) + correct `tree-sitter-erlang` node kinds + golden regen + capability-matrix evidence. |
| `erlang.literals` | Capture string-literal call arguments with a verbatim carrier (bare callee, or `module:function` for a remote call). Extractor change, not a config table; task-2 recorded the carrier gate as "a language-policy surface outside the extractor". |
| `erlang.structural_facts` | Register erlang pattern specs in `base/structural_fact_registry` (`-behaviour`, OTP callback sets, `-include`), emit them, regen `docs/contracts/structural-fact-patterns.json`, golden evidence. This is the same shape and size as Task 9, which consumed a whole task for XML. |
| `xml.literals` | Attribute-value literals with a `tag.attribute` carrier. The helper already exists (`base/config_literals.rs::tag_attribute_carrier`, used by html and vue), so this is the cheapest of the four — but still an extractor change plus golden and capability evidence. |

Partial closure does not help: the gate is binary, so it stays red until all four land.
Implementing four unreviewed feature slices across two languages at branch close, each needing
golden regeneration and capability-matrix evidence, is a materially worse outcome than a
loudly-reported red gate — so I stopped at the boundary the assignment sanctioned ("fix or
report as blocker per taxonomy"). The product decision — expand this branch by roughly four
task-sized slices, or merge with documented debt and close it in the registry tasks — is the
lead's.

### What I did instead

Both registry entries now own the work precisely, so nothing is lost if the branch merges:

- Migration-plan **Task 13: Erlang Capability Closure** — its "What to build" now names
  `literals`, `structural_facts`, and `complexity_metrics` with the specific closure each needs,
  plus a fifth acceptance criterion. All three erlang gap pointers resolve to it.
- Migration-plan **Task 14: XML Reference Edge Closure** — now also names XML attribute-value
  literals and the `tag_attribute_carrier` helper, plus an acceptance criterion. The xml
  `literals` gap pointer already resolved to it.

**Recommended disposition:** merge with the debt and schedule Task 13 next (it carries three of
the four), or, if the zero-debt invariant must hold at merge, add a Task 11 for erlang
complexity + literals and a Task 12 for erlang structural facts, with XML literals folded into
Task 14. Either way the strict gate should be re-run before the branch is called done.

---

## 6. Checklist §9 — review questions

**Does `languages --json` match the intended capability claim?**
Yes. Erlang reports FULL (`actual == target`, all five flags true, `capability_gaps: 0`); XML
reports `DATA_ONLY` (symbols + identifiers, `actual == target`, three typed `capability_gaps`).
Both match `LanguageSpec`, enforced by `capability_matrix_matches_registry_entries`.

**Does every true capability have fixture evidence?**
Yes. `cargo xtask test capability` runs the bidirectional claim↔golden guards
(`capability_matrix_supported_kind_claims_have_fixture_evidence`,
`..._requires_relationship_fixture_evidence`, `..._type_claim_requires_type_output_in_fixtures`,
`..._pending_claim_requires_pending_output_in_fixtures`, and the per-domain literal / annotation
/ doc-comment / source-region / structural-fact / complexity / test-detection variants) — 39
passed. No flag is true because a vector is merely non-empty.

**Does the strict scorecard still report no silent cells and no quality-bar debt?**
Silent cells yes (0, and the reference-resolution coverage artifact is back to 0 as well).
Quality-bar debt **no** — 4 remain. Full analysis in §5. This is the one non-green gate.

**Does every false capability have either a domain reason or a planned closure?**
Yes. Every open row and every `kind_coverage` open gap carries `reason`, `required_closure`, and
a `planned_closure_task` that resolves against the migration plan —
`capability_matrix_open_rows_have_planned_closure_task` enforces the resolution, and A1 fixed
the four that pointed at a task which did not own them.

**Are variant languages tested independently?**
Not applicable — neither Erlang nor XML registered a variant or alias row (unlike
typescript/tsx or javascript/jsx). The two languages do have independent tiers:
`cargo xtask test language erlang` (99 tests) and `language xml` (40 tests) select disjoint
module paths.

**Are parser-specific limitations documented as evidence, not hidden in prose?**
Yes, in three places, all executable. (a) The corpus gate's committed baseline records exact
per-file symbol and parse-diagnostic counts, including the 46 `?WITH_STACKTRACE` diagnostics —
tree-sitter has no preprocessor, so that macro body cannot parse — as asserted numbers, not a
caveat. (b) The oversized-transition policy (Task 5) is a tested behavior. (c) Every
unimplemented domain is a typed `kind_coverage` gap with a machine-checked closure pointer, not
a README sentence.

**Would a non-Rust consumer understand the artifact rows without knowing tree-sitter internals?**
Yes. Erlang symbol kinds are `constant, field, function, module, struct, type`; XML structural
facts are `pattern_id`-keyed with documented metadata keys in
`docs/contracts/structural-fact-patterns.json`. Nothing in the artifact names a grammar node
kind. `cargo xtask test contract` is green.

**Did default tests stay fast?**
Yes. `cargo xtask test default` completes in 11.7 s wall clock (3 commands) on a warm target
dir. The two slow additions are both feature-gated out of it: `erlang_corpus` behind
`test-real-world` (3.58 s when run) and the perf harness behind `test-perf`, and
`perf_gate_convention.rs` now guards both gatings in the default build.

---

## 7. Self-review

- **Scope:** Part A items 1–4 done and individually verified; Part B run in full and re-run
  post-commit; Part C done. Nothing was silently narrowed — the one incomplete item is §5, and
  it is reported at the top of this document and in the returned status.
- **File ownership:** stayed inside the assigned Part A/C files plus the three gate-forced files
  named in §2. No task report other than this one was touched, and this one is deliberately not
  committed.
- **Honesty of the fixes:** the only change that made a red gate less red by classification
  rather than by coverage is `xml` → script `DOMAIN_LANGUAGES`, justified in §5 against the
  eight languages already in that set. I did not add anything to the script's `native_debt` /
  `quality_debt` buckets (which would not have affected the gate anyway) and did not weaken any
  assertion, guard, or README claim to accommodate the remaining debt.
- **Comment discipline:** the one new test carries a short doc comment stating why the gate must
  stay feature-gated, matching its sibling in the same file; no narration comments were added.
- **Reversibility:** every change in `e2d39c0` is a pointer, a table entry, a generated
  artifact, a doc line, or a new assertion. Nothing changes extraction behavior, so the branch's
  extraction goldens are byte-identical to `0f7d53c`.
