# Autonomous Run Report — Web Structural Facts Module Split

- **Status:** Complete (awaiting user integration choice — pushes require explicit approval per user policy)
- **Plan:** `docs/plans/2026-07-01-web-structural-facts-module-split.md`
- **Branch:** `feature/web-structural-facts-module-split` @ ce50001 (branch gate verified at 24e613d; ce50001 is a docs/memories-only delta, confirmed via `git diff 24e613d..ce50001 --stat`)
- **Tasks:** 5/5 complete, each with lead inline review
- **External review:** none at execution (reviewer_choice: none); the plan itself was codex-reviewed pre-approval and updated (6/6 findings fixed)

## What shipped

- `crates/julie-extractors/src/base/web_structural_facts.rs` (3,676 lines) is now a directory module: `mod.rs` 149 lines (dispatch + pattern-id constants only) plus css (197), html (488), fact_builders (104), vue (718), js_imports (169), react (419), nextjs_nuxt (666), jsx_scan (255), js_object_scan (472). History preserved via `git mv`.
- New shared `base/markup_scan.rs` (210 lines) with a superset `MarkupAttribute {tag_name, name, value, start_byte, end_byte, span}`; the duplicate scanner families in both collectors are gone (−332 lines of duplication).
- Two `include_str!` convention tests lock the layout (forbidden-definition needles for mod.rs and framework_structural_facts.rs), RED-proofed by injection.
- Zero behavior change: no contract bump, no capability change.

## Commits

- 10fb8a6 docs: add post-v2.5.10 lane implementation plans
- 922649b refactor: extract shared markup scanning into base/markup_scan.rs
- b1ba0e2 refactor: split css/html/fact builders out of web_structural_facts
- 28f66bc refactor: split vue and js imports out of web_structural_facts mod
- 462e5d4 refactor: split react, nextjs/nuxt, and scanner helpers out of web_structural_facts mod
- 24e613d test: add module-layout convention guardrails for web structural facts split
- ce50001 docs: tick module-split plan acceptance criteria; record lane-1 checkpoint

## Tests / verification ledger

| Scope | Invariant | Command | Commit | Result |
|---|---|---|---|---|
| worker-red-green | emission unchanged | `cargo test -p julie-extractors structural_facts` | each task commit | 71/71 (73/73 after Task 5's +2) |
| affected-change | goldens byte-identical | `--features test-golden golden_fixtures_match_canonical_extraction` | 922649b, 462e5d4 | 1/1 pass, no UPDATE_GOLDEN |
| affected-change | capability matrix | `--features test-capability-matrix capability_matrix` | 922649b, 462e5d4 | 36/36 |
| branch-gate | fmt/clippy/default/contract/strict-report | per plan | 24e613d | all green; silent_cells 0, quality_bar_debts 0 |

## Judgment calls

1. Executed on a feature branch in the main checkout instead of a separate worktree: the session's uncommitted plan docs had to move deliberately, and a branch gives sufficient isolation for a single sequential run (worktree-discipline rule on not stranding uncommitted task changes).
2. Task 1 kept duplicate private `skip_ascii_whitespace_until` copies in both collectors — non-scanner code in each file uses them; consolidating was out of scope for zero-change.
3. Task 3 skipped the plan's suggested `pub(super)` widening — Rust child modules access parent-private items via `use super::` (proven by compile + tests); deviation reported, plan-consistent.
4. Task 4 edited one import line in `js_imports.rs` outside its file list (its `super::` imports moved to js_object_scan.rs) — unavoidable, flagged.
5. `find_tag_end` is now the web superset for both collectors; framework equivalence rests on framework inputs never containing unquoted braces in tags — documented in markup_scan.rs, gate-confirmed for the current corpus.

## Blockers hit

None.

## Next steps

- Integration: push + PR or local merge to main (user decision — see terminal).
- Lane 2 (`docs/plans/2026-07-01-http-boundary-facts.md`) builds directly on this layout.
- Optional later cleanup from Task 3's report: `parse_attr_value`/`has_boolean_attr` live in mod.rs but are vue-only.
