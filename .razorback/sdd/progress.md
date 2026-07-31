# SDD progress — erlang-xml-language-support

Plan: docs/plans/2026-07-31-erlang-xml-language-support-plan.md
Branch: erlang-xml-language-support (worktree .worktrees/erlang-xml-language-support)
Scope note: entries below belong ONLY to this plan. Reports use the suffix
`task-N-erlang-xml-report.md` because `.razorback/sdd/` is tracked and holds files
from earlier, unrelated plans — do not treat those as this plan's progress.

- Task 1: complete (commit fd962a50, Lead inline review clean). Phase-0 gate PASS; XML entry point = tree_sitter_xml::LANGUAGE_XML (DTD is a separate grammar); tree-sitter-erlang ships no tags/locals queries (hand-written walking as planned).
- Task 2: complete (commit eb7cb130, Lead inline review clean). Erlang registered at 37, symbols tier honest (SYMBOLS_ONLY_CAPABILITIES). Plan mismatches recorded in task-2 report §7: migration-plan doc is the open-capability registry; negative fixture required at registration when target relationships=true; per-language parity guards force source_regions/marker/body.rs/operations_contract touches; complexity_metrics left as typed open gap (unowned). Task 3 prompt must carry §7.3 file list.
- Task 3: complete (commit 8df10321, Lead inline review clean). XML registered at 38, DATA_ONLY honest (target==actual). Cardinality: 4,000 anon elements → 3 symbols. 4th parity guard found: capability_matrix DOMAIN_LANGUAGES const. Matrix lacks a not_targeted status (open rows overstate intent) — surface in final report. Next: Batch A (T4 ∥ T5, parallel-lead-commit).
- Task 5: complete (parallel-lead-commit, Lead inline review clean, lead commit e8bf8a4). Oversized transition removes rows on update+scan; update status no_change→unsupported for oversized (RELEASE-NOTE + check Miller ExtractReportLog handling at pin bump). cli.md touched outside ownership (justified, accepted).
