# SDD progress — erlang-xml-language-support

Plan: docs/plans/2026-07-31-erlang-xml-language-support-plan.md
Branch: erlang-xml-language-support (worktree .worktrees/erlang-xml-language-support)
Scope note: entries below belong ONLY to this plan. Reports use the suffix
`task-N-erlang-xml-report.md` because `.razorback/sdd/` is tracked and holds files
from earlier, unrelated plans — do not treat those as this plan's progress.

- Task 1: complete (commit fd962a50, Lead inline review clean). Phase-0 gate PASS; XML entry point = tree_sitter_xml::LANGUAGE_XML (DTD is a separate grammar); tree-sitter-erlang ships no tags/locals queries (hand-written walking as planned).
