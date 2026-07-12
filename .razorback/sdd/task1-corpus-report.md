# Task 1 corpus spike report

## Result

- Live baseline: 235 Razor diagnostics, 232 `error` and 3 `missing`.
- Classification: implicit 30, explicit 164, modifier 0, other 41.
- Corpus: 26 desired-semantics cases; `07eab9c` passes 2 and leaves 24 RED.
- Existing corpus invariant: 84/84 pre-task parses remain green with Tree-sitter CLI 0.26.10.
- RED invariant: 23 cases contain current `ERROR`/`MISSING` nodes; the mixed HTML case is semantically RED because only the identifier before `.Class` survives.

## Miller evidence

- `workspace(onboarding, workspace_id=89389279...)`: confirmed `/Users/murphy/source/tree-sitter-razor` and advised context-first exploration.
- `context("Razor grammar attribute values ... corpus conventions", workspace_id=89389279...)`: identified `razor_attribute_directive`, `razor_attribute_value`, and the embedded C# grammar.
- `inspect(grammar.js)`: listed the live rule symbols before source reads.
- `inspect` full on `razor_attribute_value`, `razor_attribute_name`, `razor_attribute_modifier`, `component_attribute_value`, `component_attribute`, `_component_attribute_name`, `razor_typeparam_directive`, and `razor_rendermode_directive`: proved exact rule bodies and node names.
- `trace(mode=refs)` on the attribute value/name/modifier symbols: proved their `razor_html_attribute` and component call sites before relying on rule ownership.
- Miller found no indexed symbols for corpus text files, so targeted corpus reads followed the documented fallback.

## Verification and judgment calls

- In this worktree, `npx tree-sitter --version` resolves `/Users/murphy/source/tree-sitter-razor/node_modules/tree-sitter-cli/tree-sitter` as 0.24.7. Both `npx tree-sitter test --overview-only` and `npx tree-sitter parse .../App.razor --quiet --stat` reject the generated ABI 15 parser with `Expected minimum 13, maximum 14`. A plain `npx` success from another shell is reproducible only after recording which version and path it resolved.
- Successful corpus and targeted parse evidence uses the exact binary `/Users/murphy/.npm/_npx/fc82e01b08b7a8ed/node_modules/tree-sitter-cli/tree-sitter`, version 0.26.10. The commands are `.../tree-sitter test --overview-only`, per-original-file `.../tree-sitter test --file-name <file> --overview-only`, and `.../tree-sitter parse -n 1..26 --cst`.
- All 26 cases were parsed individually; current diagnostic counts were recorded before running the full test suite.
- The 35 `HomePage.razor` rows map to the interpolated component value because branch parsing shows that first failure expands into a file-wide root `ERROR`; this includes all 3 missing rows.
- Generic `TOption="string"`/`TValue="string"` was expected to be covered by `07eab9c`, but the minimal branch parse still emits errors. It is recorded as O1 and added to Task 2 closure.
- O6 render-fragment literals in switch expressions are assigned to Task 3 so the zero-diagnostics gate cannot close with them outstanding.
- FluentUI-only E10 comes from the official `AutocompleteCustomized.razor` example and was confirmed absent from Terraform with `rg -n '=\"@\\(async \\(\\) => await' /Users/murphy/source/Terraform/src -g '*.razor'`.
- The row-addressable appendix groups identical path/case mappings but retains every `line:column:kind`; its checked multiplicities are `total=235 I=30 E=164 M=0 O=41`.

## Repository state before commits

- Grammar: `/Users/murphy/source/tree-sitter-razor`, branch `fix/attribute-value-expressions`, start `07eab9cff90d462571c05526520686abb077dc4d`; pre-existing untracked `.julieignore` and `.miller/` were not touched.
- Extractors: `/Users/murphy/source/julie-extractors/.worktrees/blazor-razor-support`, branch `codex/blazor-razor-support`, start `f5ba7637d6b42da242633231d91600f17d96efee`.
