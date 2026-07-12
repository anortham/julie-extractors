# Task 8: .NET test containers

## Scope

- Worktree: `/Users/murphy/source/julie-extractors/.worktrees/blazor-razor-support`
- Branch: `codex/blazor-razor-support`
- Starting commit: `a388e7f71b6f155d103f63d93efa9fde0279e07c`
- Task commit: the commit containing this report; exact SHA is recorded in the lead handoff
- Scope was limited to C#, VB.NET, and Razor `test_container` closure. The open `test_lifecycle` rows remain open.

## Miller and architecture

- Miller context and symbol inspection identified `is_test_symbol`, the three language `extract_symbols` entry points, C#/VB/Razor class extraction, and the existing golden fixtures.
- Miller impact showed the shared detector is broadly referenced, so container promotion is isolated to a new postpass called only by the C#, VB.NET, and Razor extractors.
- The postpass uses normalized parsed annotations and direct symbol parent IDs. It recognizes class markers `testfixture`/`testclass` and direct member markers `fact`/`theory`/`test`.
- VB.NET type attributes live on the grammar's `type_declaration` wrapper, so class extraction now persists those normalized annotations before the postpass runs.

## TDD ledger

- RED: `cargo test --offline -p julie-extractors test_containers -- --nocapture` failed in all three languages because positive classes lacked `test_container`; the three lexical/unrelated-attribute negatives passed.
- GREEN: the same command passed 6/6 after the parsed annotation and symbol-hierarchy implementation.
- Golden RED: `cargo --offline xtask test golden` reported the expected first mismatch at `csharp:test_roles` because `ManagedTestRoles` gained `test_container=true`.
- Golden outputs were regenerated and reviewed for exact positive and negative rows in all three registered `test_roles` fixtures.

## Verification ledger

| Scope | Command | Result | Timestamp |
|---|---|---|---|
| Focused | `cargo test --offline -p julie-extractors test_containers` | 6 passed | 2026-07-12T04:07:11Z |
| C# | `cargo --offline xtask test language csharp` | 107 passed | 2026-07-12T04:07:11Z |
| VB.NET | `cargo --offline xtask test language vbnet` | 83 passed | 2026-07-12T04:07:11Z |
| Razor | `cargo --offline xtask test language razor` | 80 passed | 2026-07-12T04:07:11Z |
| Golden | `cargo --offline xtask test golden` | Task 8 outputs passed after regeneration; checkout-wide gate remains Task 9-blocked by prior Razor structural-fact golden drift | 2026-07-12T04:07:11Z |
| Capability, Task 8 | `cargo test --offline -p julie-extractors --features test-capability-matrix capability_matrix_test_detection` | 2 passed | 2026-07-12T04:07:11Z |
| Annotation evidence | `cargo test --offline -p julie-extractors --features test-capability-matrix capability_matrix_annotation_claims_have_fixture_evidence` | 1 passed | 2026-07-12T04:07:11Z |
| Package ceiling | `cargo test --offline -p julie-extractors` | 2,853 passed, 7 ignored; doctest passed | 2026-07-12T04:07:11Z |
| Formatting | `cargo fmt --all -- --check` and `git diff --check` | passed | 2026-07-12T04:07:11Z |

## Deferred integration gates

- Full capability: 37/39 passed. The two failures are Task 9-owned certification for Task 5-7 structural facts (`razor.route_reference.v1`, `blazor.component_reference.v1`, and Razor `http.client_request.v1`), not test detection.
- Strict data-quality report is Task 9-blocked because the five Task 4 Razor attribute-expression goldens are on disk but not yet registered in `capabilities.json`.
- No push was performed.
