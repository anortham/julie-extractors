# Task 3 — Blazor directive grammar report

## Result

- Grammar repository: `/Users/murphy/source/tree-sitter-razor`
- Branch: `fix/attribute-value-expressions`
- Base revision: `03238493cbf131354873e634017535bcd538440b`
- Task revision: `d24d075afe5b18eae56c4386046ed5e6e3902795`
- Exact generator: `/Users/murphy/.npm/_npx/fc82e01b08b7a8ed/node_modules/tree-sitter-cli/tree-sitter` 0.26.10
- No tag, push, publish, or release was performed.

## RED evidence

Before grammar edits, the approved CLI reproduced exactly the eight planned failures:

- `tree-sitter test --file-name directive-modifiers.txt --overview-only`: 1/4 passed; M2, M3, and M4 failed.
- `tree-sitter test --file-name other.txt --overview-only`: 1/6 passed; O2, O3, O4, O5, and O6 failed.
- M1 remained green.

## Implementation

- Extended `razor_attribute_modifier` with `:event`, `:format`, `:get`, `:set`, and `:after`. Existing `:culture`, `:preventDefault`, and `:stopPropagation` remain unchanged.
- Added `rendermode` to Razor directive-attribute names and allowed structured explicit and implicit Razor expressions in `razor_attribute_value`.
- Preserved the three built-in render-mode literals while allowing custom identifiers plus explicit and implicit expressions in `razor_rendermode`.
- Extended `razor_typeparam_directive` with the inherited C# `type_parameter_constraints_clause`, retaining named `type_parameter_constraint` and `constructor_constraint` nodes.
- Overrode only the inherited `switch_expression_arm` shape to add `razor_template` as a value. `razor_template` is a Razor marker followed by the existing structured `element` rule. The general C# `expression` rule was not overridden.
- Corrected four Task 1 expected trees to match the inherited C# caller-facing contract: named arguments expose their identifier field without a `name_colon` wrapper, constructor constraints remain inside `type_parameter_constraint`, an `@code` member is a `method_declaration`, and the component render-mode tree had one excess closing parenthesis.

## GREEN evidence

- `tree-sitter test --file-name directive-modifiers.txt --overview-only`: 4/4 passed.
- `tree-sitter test --file-name other.txt --overview-only`: 6/6 passed.
- `tree-sitter test --overview-only`: 110/110 passed, including all original 84 cases and all Task 2 cases.
- The new expected trees contain no `ERROR` node and no opaque text substitution for M2–M4 or O2–O6.
- Syntax-highlight assertions remained 52/52 green on targeted and full runs.
- `git diff --check` passed before commit.

## Generator determinism and parser cost

The exact CLI generated successfully without new conflict warnings. Two consecutive generations produced byte-identical outputs:

| Output | SHA-256 |
| --- | --- |
| `src/grammar.json` | `d429d840591d5665935c111b13a04af3ffb9cb031cd160f10163d18dcd2f09da` |
| `src/node-types.json` | `ec461c6a0e4a7b846317150b6ab993dada3bf292e43dccc09a506d024a9f199e` |
| `src/parser.c` | `6f5cc62e8350f55b34e53ecd4d2b838d8e82e94b3ff2cf578b768d6c6bc2b05c` |
| `src/tree_sitter/parser.h` | `180b893c8734778fd32f372dfbc27bd6ad1cd2221f26150b31256ff6716320d2` |

| Metric | Task 2 | Task 3 | Delta |
| --- | ---: | ---: | ---: |
| `STATE_COUNT` | 19,636 | 19,674 | +38 (+0.19%) |
| `LARGE_STATE_COUNT` | 7,486 | 7,487 | +1 (+0.01%) |
| `src/parser.c` bytes | 57,646,051 | 57,683,937 | +37,886 (+0.07%) |

Five warmed full-corpus samples were 5,157, 5,685, 5,963, 5,969, and 5,972 bytes/ms, averaging 5,749 bytes/ms. That is +446 bytes/ms (+8.4%) from the Task 2 lead baseline of 5,303 and +893 bytes/ms (+18.4%) from the Task 2 worker baseline of 4,856. The state and size increases are bounded and there was no throughput regression.

## Miller evidence

Workspace: `89389279b01fd18b1c88b62bb2afb27eb0ecfac41cf5bc5506561c93964ca590` (`/Users/murphy/source/tree-sitter-razor`).

Exact orientation calls:

- `workspace(operation="onboarding", workspace_id=...)`
- `workspace(operation="health", workspace_id=...)` — fresh revision 3 before edits; 14,307 symbols.
- `context(query="directive attributes razor_rendermode_directive razor_typeparam_directive switch_expression_arm razor_template embedded C# extension points", entry_symbols=["razor_rendermode_directive","razor_typeparam_directive","switch_expression_arm","razor_template"], token_budget=5000, workspace_id=...)`
- Symbol searches for `razor_rendermode_directive`, `razor_typeparam_directive`, `switch_expression_arm`, `razor_template`, `component_attribute_name`, `component_attribute_value`, `razor_implicit_expression`, `razor_explicit_expression`, `type_parameter_constraints_clause`, `constructor_constraint`, and `_html_attribute_name`.

Pre-edit full inspections used exact symbol IDs:

- `inspect(target="f0dfc4c4461b9f6d169dd207aa00c62d", depth="full", workspace_id=...)` proved `razor_typeparam_directive` only accepted `$._name`.
- `inspect(target="8d60e42d54410c0d22982010d61d3202", depth="full", workspace_id=...)` and `inspect(target="d4004442feb785fe50b357b0d8aad468", depth="full", workspace_id=...)` proved render modes were a closed three-literal choice.
- `inspect(target="7f301f9bf29f225cca28aa3109c5f223", depth="full", workspace_id=...)` proved the inherited `switch_expression_arm` sequence was `pattern`, optional `when_clause`, `=>`, `expression`.
- `inspect(target="cdc85937df9be4696b82f0d6b4025789", depth="full", workspace_id=...)`, `inspect(target="854d5333daf17f48fa909edccc44034c", depth="full", workspace_id=...)`, and `inspect(target="609395c0108aa579fa7dd837b4ecd21c", depth="full", workspace_id=...)` proved the inherited named C# constraint structure.
- `inspect(target="6115211efde2bf2f2209c92c6bbdd2be", depth="full", workspace_id=...)` and `inspect(target="83be1a0416ed6f11eb28f8c75ffa71c3", depth="full", workspace_id=...)` proved the existing structured implicit/explicit Razor expression rules.
- `inspect(target="bc9a43e8ad470ec77f7e5bca3f8f60c5", depth="full", workspace_id=...)`, `inspect(target="b233669c3c9bec872ff378055d957f9c", depth="full", workspace_id=...)`, and `inspect(target="8471c0c036fe9f6ba5d08255e172b4b2", depth="full", workspace_id=...)` proved component and HTML attribute boundaries.
- `inspect(target="883d6a05161c2fad4f4320a3ce8cd21b", depth="full", workspace_id=...)` and `inspect(target="655d06d7630061ba58f6a8b8597352f5", depth="full", workspace_id=...)` proved the `_csharp_nodes` and `_node` extension paths.

Reference and impact calls were run for `_component_attribute_name`, `component_attribute_value`, `razor_implicit_expression`, `razor_explicit_expression`, `type_parameter_constraints_clause`, `type_parameter_constraint`, `constructor_constraint`, `_html_attribute_name`, `_csharp_nodes`, and `_node` using `trace(mode="refs")` and `impact(target=...)`. The broadest proven path was `_node` to 24 Razor composition dependents, which is why O6 used a narrow switch-arm override instead of changing `_node` or the general C# expression rule.

Post-edit calls:

- `workspace(operation="refresh", workspace_id=...)` — refreshed revision 4.
- `inspect(depth="full")` on exact refreshed function IDs `98c9a2f4f28c4f3990c50547058dd508`, `8d9e5cec8c2b3160fbcbbbe681ea3145`, `a6e2107bde0aaa81fe4840db7e686a76`, `ea7c4cc92161eec3edaf26e5ccc464f3`, `ce763c8bcb4469233bee6986ad1fc81d`, `e2af994d6512868d4e26a86904866d44`, and `ad46713306004d3ea0a3a318c1683a6e` confirmed the final rule bodies and caller edges.
- `impact(git=true, max_depth=2, limit=100, workspace_id=...)` identified 11 impacted Razor composition symbols; full-corpus verification covered them.

## Final state and concerns

- Grammar task commit contains only `grammar.js`, deterministic generated `src/grammar.json`, `src/node-types.json`, `src/parser.c`, and `test/corpus/blazor-attributes/other.txt`.
- Pre-existing untracked `.julieignore` and `.miller/` were not staged or modified.
- `directive-modifiers.txt` already contained the RED expectations from Task 1 and required no content change.
- Concern: `src/parser.c` has a large generated line diff because parse-table numbering shifts, but the actual byte growth is only 37,886 bytes and repeat generation is byte-identical.
- Blockers: none.
