# Task 1 corpus spike report

## Result

- Live baseline: 235 Razor diagnostics, 232 `error` and 3 `missing`.
- Classification: implicit 30, explicit 164, modifier 0, other 41.
- Corpus: 25 desired-semantics cases; `07eab9c` passes 2 and leaves 23 RED.
- Existing corpus invariant: 84/84 pre-task parses remain green with Tree-sitter CLI 0.26.10.
- RED invariant: cases 2-18 and 20-25 contain current `ERROR`/`MISSING` nodes; case 19 is semantically opaque because only the identifier before `.Class` survives.

## Miller evidence

- `workspace(onboarding, workspace_id=89389279...)`: confirmed `/Users/murphy/source/tree-sitter-razor` and advised context-first exploration.
- `context("Razor grammar attribute values ... corpus conventions", workspace_id=89389279...)`: identified `razor_attribute_directive`, `razor_attribute_value`, and the embedded C# grammar.
- `inspect(grammar.js)`: listed the live rule symbols before source reads.
- `inspect` full on `razor_attribute_value`, `razor_attribute_name`, `razor_attribute_modifier`, `component_attribute_value`, `component_attribute`, `_component_attribute_name`, `razor_typeparam_directive`, and `razor_rendermode_directive`: proved exact rule bodies and node names.
- `trace(mode=refs)` on the attribute value/name/modifier symbols: proved their `razor_html_attribute` and component call sites before relying on rule ownership.
- Miller found no indexed symbols for corpus text files, so targeted corpus reads followed the documented fallback.

## Verification and judgment calls

- The repository's `npx tree-sitter` is 0.24.7 and rejects ABI 15; the pinned local 0.26.10 binary was used for all reported test and parse evidence.
- All 25 cases were parsed individually with `tree-sitter parse -n 1..25`; current diagnostic counts were recorded before running the full test suite.
- The 35 `HomePage.razor` rows map to the interpolated component value because branch parsing shows that first failure expands into a file-wide root `ERROR`; this includes all 3 missing rows.
- Generic `TOption="string"`/`TValue="string"` was expected to be covered by `07eab9c`, but the minimal branch parse still emits errors. It is recorded as O1 and added to Task 2 closure.
- Render-fragment literals in switch expressions are a separate O6 gap rather than being silently assigned to attribute work.

## Repository state before commits

- Grammar: `/Users/murphy/source/tree-sitter-razor`, branch `fix/attribute-value-expressions`, start `07eab9cff90d462571c05526520686abb077dc4d`; pre-existing untracked `.julieignore` and `.miller/` were not touched.
- Extractors: `/Users/murphy/source/julie-extractors/.worktrees/blazor-razor-support`, branch `codex/blazor-razor-support`, start `f5ba7637d6b42da242633231d91600f17d96efee`.
