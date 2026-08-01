# Autonomous Execution Report - Erlang + XML Language Support

**Status:** Complete (awaiting user landing decision — pushes require explicit approval)
**Plan:** docs/plans/2026-07-31-erlang-xml-language-support-plan.md
**Branch:** erlang-xml-language-support (worktree `.worktrees/erlang-xml-language-support`, base main @ 4bee2fe2)
**PR:** not created — user approval boundary: no push without explicit approval
**Duration:** 2 working days (2026-07-31 → 2026-08-01), 12 plan tasks + 1 pre-merge fix round
**Phases:** design → plan → 12 tasks → branch gates → pre-merge codex review, all complete
**Tasks:** 12/12 complete (10 planned + 2 debt-closure tasks added by user decision 2026-08-01)

## What shipped
- Erlang as language 37 at FULL tier: symbols, identifiers (calls/member-access/type-usage/variable-ref), relationships + pending behaviour edges, types, test roles (EUnit + Common Test), doc comments, complexity metrics, string call-arg literals, 5 structural-fact patterns (module/behaviour/callback/export/include), source regions.
- Bounded parse-error resync recovery (user-approved): blank-and-reparse with 32-parse cap; telemetry.erl recovers 8/8 exports despite a macro-as-clause-head parse error; hardened against multiline quoted-atom resume points.
- XML as language 38 at DATA_ONLY tier: name-promoted symbols, QName TypeUsage identifiers **gated on declared XSD/WSDL/xsi namespaces**, attribute-value literals, 10 structural-fact patterns (document/namespace/XSD/WSDL shapes).
- Oversized-file transition fix in julie-extract-cli: a tracked file growing past 1MB now removes its rows; `update` returns `status: unsupported` (was `no_change`). **Consumer-facing: Miller's ExtractReportLog handling must be checked at pin bump.**
- Real-world Erlang corpus gate: vendored telemetry/certifi/unicode_util_compat sources (checksummed, licensed), exact baseline, wired into the real-world xtask tier with a convention guard.
- Zero-debt strict scorecard restored (user chose closing all 4 quality-bar debts over merging with documented debt).
- Repo docs: README 36→38 languages, new-language checklist extended with 7 per-language parity guards, migration-plan registry (Tasks 13/14) updated to reflect closures.

## Judgment calls (non-blocking decisions made)
- `crates/julie-extractors/src/base/complexity_metrics.rs` — erlang symbol-scope complexity measures the declaration span, not the body span, because guards live in clause heads; clause dispatch is not counted as a decision (clause_count metadata records it); parameter_count is NULL (no closed set of parameter node kinds).
- `crates/julie-extractors/src/erlang/identifiers.rs` — identifier/relationship walks restricted to executable forms so type signatures (same `call` node) don't emit false calls.
- `crates/julie-extractors/src/base/structural_fact_registry/builtins/erlang.rs` — placement under `builtins/` (mirrors code_structural_facts.rs emission source) instead of the task text's top-level path; `-export` records `exported_count`, not the name list (arrays unfilterable by Miller `where`; names already on symbol visibility).
- `crates/julie-extractors/src/xml/identifiers.rs` — SchemaNamespaces gate is document-wide, not per-element-scoped (prefix rebinding mid-document judged unrealistic); recognized namespace list closed to XSD 1.0/1.1, xsi, WSDL 1.1/2.0.
- Task 11 removed the xml literals open gap with attribute-value literals only (element-text literals not captured — matches the migration-plan registry scope; possible follow-up).

## External review (codex, adversarial)
- **Findings:** 4 (verdict needs-attention, 0 critical)
- **Verified real, fixed:** 3 (commits: ab3a43f, afa8416, 1285687)
  - Multi-clause erlang functions covered only their first clause (span/body-hash/symbol complexity) — fixed with clause_run_extent + real clause_body body spans; body hash now moves when a later clause changes.
  - Recovery could resume inside multiline quoted atoms, minting phantom symbols — fixed with multiline-only atom/char literal-range exclusion + strict-interior containment; adversarial fixture proves ghost declarations are gone.
  - Generic XML `type=`/`ref=`/`base=`/`element=` attributes emitted false TypeUsage references (e.g. `Serilog.Sinks.Console` from a logging config) — fixed by gating on declared schema namespaces; xml/basic 4→0, cardinality 2→0, xsd/wsdl sets unchanged.
- **Dismissed:** 0
- **Flagged for your review:** 1
  - `pp_define` macro bodies are walked as executable, so a type-valued macro (`-define(TYPE, integer()).`) emits a false call identifier — real, but fixing it by dropping macro-body walks would also lose real edges like `-define(LOG(X), io:format(...))`. Lead recommendation: ship as-is, restrict macro-body EDGE emission in a follow-up if dogfood shows noise.
- Cost: codex does not report per-request token counts.

## Tests
- Branch gate: all 14 gates exit 0 at 1285687 (default, golden, capability, certification, changed, language erlang, language xml, real-world corpus 3/3 with diagnostics baseline 45/2 unchanged, languages --json, strict scorecard 0 debts, coverage report, fmt, clippy zero warnings, deny). Later commits are docs-only.

## Blockers hit
- None unresolved. Two mid-run product decisions escalated to the user (both answered): build the bounded resync recovery; close all 4 quality-bar debts on-branch.

## Files changed
- 137 files changed, ~46k insertions vs main (bulk: vendored erlang corpus fixtures, generated goldens, contract JSON). Authored code: erlang extractor (9 modules incl. recovery), xml extractor (3 modules + namespace gate), complexity/literal/structural-fact registry additions, oversized-transition fix in julie-extract-cli, corpus + convention-guard tests, docs.

## Next steps
- User decision: land the branch (merge to main locally, or push + PR) — push requires explicit approval.
- Post-merge delivery chain (each step user-approved): julie-extract 2.21.0 release (4-target matrix + release notes) → Miller `scripts/julie-pins.json` bump + restore + fast/scale suites + **verify Miller's ExtractReportLog handles `update` status `no_change`→`unsupported`** → docs 36→38 in Miller README/site → reply to and close Miller issue #8 (fabricated error strings; Erlang+XML shipped; AXL giant XSDs intentionally excluded, `content import` 25MB escape hatch).
- Flagged finding: decide on pp_define macro-body walk (recommendation: ship as-is, follow-up if noisy).
- Ticket-worthy residuals: `convert_types_map` hardcodes `is_inferred:true`; capability matrix lacks a `not_targeted` status; recovery is erlang-only (language-parity note); no cap-hit diagnostic when MAX_RECOVERY_PARSES is exhausted; `registry_pattern_ids_match_emitted_union_per_language` test is red and ungated repo-wide (code.marker.v1 never unioned — pre-existing); a TODO/FIXME marker in any fixture source would break capability evidence confusingly (latent repo-wide trap); scorecard's final JSON summary line reports quality_bar_debts:0 while the text section is authoritative (display bug); element-text XML literals not captured (attribute values only).
