# Task 2: Razor attribute-value grammar

## Status

Complete. The grammar revision is `0323849` on `fix/attribute-value-expressions`.

## Implementation

- `component_attribute_value` now gives leading-`@` values to the existing `razor_explicit_expression` and `razor_implicit_expression` rules, so their embedded C# expression nodes remain visible.
- Non-`@` component values retain the existing expression and opaque-text paths. The new `$.type` alternative makes generic type parameters such as `TValue="string"` produce `predefined_type`.
- `_html_attribute_value` retains its original opaque fallback and adds a higher-priority mixed-text path. After a Razor transition, the opaque token has negative lexical precedence so C# continuation tokens such as `.` win when they form a structured expression.
- The collection-expression expectation had one missing closing tree delimiter. The interpolated-string expectation was aligned with the current C# grammar's named `interpolation_brace` children. Both still assert the complete structured expression tree.

## Miller evidence

Workspace: `89389279b01fd18b1c88b62bb2afb27eb0ecfac41cf5bc5506561c93964ca590` (`/Users/murphy/source/tree-sitter-razor`).

Calls before raw reads and edits:

1. `workspace(operation="onboarding", workspace_id="89389279...")`
2. `context(query="Razor component and Razor attribute value grammar, embedded C# expressions, opaque fallbacks, and corpus tests for implicit and explicit Blazor attribute expressions", workspace_id="89389279...", ensure_fresh=true, token_budget=4000, reference_mode="usage", exclude_tests=false)`
3. `workspace(operation="health", workspace_id="89389279...")`
4. `inspect(target="grammar.js", depth="summary", limit=200, workspace_id="89389279...")`
5. `search(mode="symbol", format="json", query=<target>, workspace_id="89389279...")` for `component_attribute_value`, `razor_attribute_value`, `_csharp_nodes`, `razor_explicit_expression`, `razor_implicit_expression`, and `_html_attribute_value` to obtain unambiguous symbol IDs.
6. `inspect(target=<symbol_id>, depth="full", workspace_id="89389279...")` for each target above.
7. `trace(target=<symbol_id>, mode="refs", depth=3, limit=50, workspace_id="89389279...")` for the shared value and expression rules.
8. `impact(target=<symbol_id>, max_depth=2, limit=100, workspace_id="89389279...")` for both attribute-value rules and both Razor expression rules.

The indexed API shape showed:

- `component_attribute_value` was called only by `component_attribute` and originally chose empty `""`, quoted `$.expression`, or quoted opaque `/[^"@]+/`.
- `razor_attribute_value` was called only by `razor_html_attribute` and already exposed `$.expression`.
- `razor_explicit_expression` aliases `@` and exposes `$.parenthesized_expression`; `_html_attribute_value` and `_node` reference it.
- `razor_implicit_expression` aliases `@` and exposes `$.expression`; `_html_attribute_value` and `_node` reference it.
- `_html_attribute_value` was called only by `_html_attribute`; its opaque token competed with C# continuation tokens after a Razor expression.
- Impact analysis reached `component_attribute`, `_html_attribute`, and `element`, which selected the targeted and full-corpus verification scopes.

After edits, `workspace(operation="refresh")`, `impact(git=true)`, full inspection, and reference traces confirmed the same caller boundaries. The updated component rule exposes `razor_explicit_expression`, `razor_implicit_expression`, `$.expression`, and `$.type`; the updated HTML rule preserves the original fallback and adds the lower-priority post-transition fallback.

## RED evidence

Approved binary: `/Users/murphy/.npm/_npx/fc82e01b08b7a8ed/node_modules/tree-sitter-cli/tree-sitter` (`tree-sitter 0.26.10`).

- `tree-sitter test --file-name explicit-expressions.txt --overview-only`: 0/10 passed; all E1-E10 lacked the required structured component expression.
- `tree-sitter test --file-name implicit-expressions.txt --overview-only`: 1/6 passed; I1-I4 and I6 failed, while the pre-existing bare-identifier case passed.
- `tree-sitter test --file-name other.txt --overview-only`: O1 and the five Task 3 cases failed.
- Detailed RED output showed component `@(...)` and `@identifier` values as `ERROR`, mixed HTML `@action.Class` as only `identifier`, and O1 as an error around `predefined_type`.

This reproduces the 16 assigned failures: E1-E10, I1-I4, I6, and O1.

## GREEN and regression verification

- `tree-sitter generate` with the approved 0.26.10 binary: exit 0.
- `tree-sitter test --file-name explicit-expressions.txt --overview-only`: 10/10 passed.
- `tree-sitter test --file-name implicit-expressions.txt --overview-only`: 6/6 passed, including all five assigned implicit cases.
- `tree-sitter test --include 'Generic component type attributes remain RED after 07eab9c' --overview-only`: 1/1 passed with named `predefined_type` nodes.
- `tree-sitter test --overview-only`: 102/110 passed. The only failures are Task 3 M2-M4 and O2-O6.
- All 84 original cases pass, including the `Author Figcaption Example` regression probe.
- Final warmed full-corpus run averaged 4,856 bytes/ms versus the Task 1 baseline of 4,798 bytes/ms. The only slow-parse warning was O5 `Constrained type parameter` at 542.773 bytes/ms; O5 remains an intentional Task 3 RED case.
- `git diff --check`: exit 0 before commit.

## Generator behavior and churn

- An initial duplicated mixed-value alternative produced a generator conflict at `_html_attribute_value_repeat1`; it was replaced with distinct lexical precedence for the mixed-text prefix and lower precedence only for post-transition opaque text. The final grammar generates without conflicts.
- `parser.c` grew from 52,501,188 to 57,646,051 bytes: +5,144,863 bytes (+9.8%).
- `STATE_COUNT`: 17,393 to 19,636 (+2,243, +12.9%).
- `LARGE_STATE_COUNT`: 6,948 to 7,486 (+538, +7.7%).
- `SYMBOL_COUNT`: 648 to 651; `ALIAS_COUNT` remains 16.
- Generated diff: `grammar.json` +102/-21, `node-types.json` +12/-0, `parser.c` +1,324,860/-1,169,037. No generated headers changed.
- A second identical `tree-sitter generate` produced byte-identical SHA-256 values: `71387449...` for `grammar.json`, `8827c27a...` for `node-types.json`, and `4452e487...` for `parser.c`.
- The generated-size increase is not accompanied by a parse-speed regression, and parser generation is deterministic.

## Repository state

At grammar commit time:

- Path: `/Users/murphy/source/tree-sitter-razor`
- Branch: `fix/attribute-value-expressions`
- Base before Task 2: `8ff20abea4b42f506c6085b16f88f773d91a3115`
- Task 2 commit: `0323849`
- Pre-existing untracked paths left untouched: `.julieignore`, `.miller/`

Report repository before the report commit:

- Path: `/Users/murphy/source/julie-extractors/.worktrees/blazor-razor-support`
- Branch: `codex/blazor-razor-support`
- HEAD: `bc89348bb948aa6fa91026a2e8b854add7a95c49`
- Dirty state: clean before adding this report

## Concerns

- The generated parser is larger, but generation is deterministic and the final warmed full-corpus speed is slightly above the Task 1 baseline.
- No extractor code, directive modifiers, rendermode, type-parameter constraints, or render-fragment switch literals were changed.
