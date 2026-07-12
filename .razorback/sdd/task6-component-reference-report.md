# Task 6: Blazor Component References

## RED

- Added five caller-facing canonical-pipeline tests in `tests/razor/component_reference.rs`.
- `cargo test --offline -p julie-extractors tests::razor::component_reference -- --nocapture` compiled and ran before production edits.
- Expected result: 2 passed and 3 failed. Cross-file-style and FluentUI cases emitted zero `blazor.component_reference.v1` facts, and `_Imports.razor` still emitted a synthetic component symbol.

## GREEN

- `blazor.component_reference.v1` emits once per PascalCase `element` node in non-infrastructure `.razor` components.
- Metadata carries `tag`, filename-derived `containing_component`, locally declared `@namespace`/`@using` values in `namespace_context`, and `T*` component attributes in `generic_arguments`.
- No workspace lookup or `external` classification is performed. Existing component relationship extraction was not changed.
- `_Imports.razor` and `_ViewImports.razor` no longer emit synthetic component definitions; `_ViewImports.cshtml` remains excluded and `App.razor` remains a component.
- The registry and checked-in JSON contract declare the new Razor pattern and its emitted metadata.

## Miller Evidence

- Oriented on Razor identity, framework fact dispatch, registry, and tests with `context` in workspace `facd497e3541`.
- Inspected `RazorExtractor.extract_component_symbol`, `RazorExtractor.is_razor_component_file`, `collect_framework_structural_facts`, `collect_razor_structural_facts`, `extract_element_relationships`, and `StructuralFact` before edits.
- Pre-change impact showed component identity affects `extract_symbols`; framework fact dispatch affects the canonical pipeline through `registry::extract_for_language`.
- Post-change refresh revision 16 and git-diff impact identified the Razor collector, registry serializer, emitted-pattern union, canonical registry path, and their focused tests.
- Reference tracing proved `BLAZOR_COMPONENT_REFERENCE_PATTERN_ID` is used by both the Razor emitted-pattern array and the new collector.

## Architecture Quality

**Affected modules:** Razor component identity, Razor framework structural facts, the structural-fact registry, and Razor contract tests.

**Caller-facing interface:** One new structural-fact pattern through the existing canonical extraction result; no new public Rust seam.

**Depth/locality check:** Syntax recognition stays in the existing Razor framework collector. File identity remains in the Razor extractor. Registry serialization remains the single contract source.

**Test surface:** All behavior is exercised through `pipeline::extract_canonical`, the same interface used by consumers.

**Seams/adapters:** No new adapter was needed. The grammar exposes component tags as parser-backed `element` nodes, while tag and attribute names are unnamed tokens, so bounded decoding is local to those nodes.

**Rejected shortcuts:** No whole-file tag regex, sibling-file lookup, inherited `_Imports.razor` guessing, or unresolved/external classification.

**Architecture risk:** low. Complexity remains local and the existing fact registry owns the new contract.

## Verification

- Focused component tests: 5 passed, 0 failed.
- Razor scope: 77 passed, 0 failed.
- Registry UPDATE/export gate: 10 passed, 0 failed.
- Ungated checked-in JSON sync: 1 passed, 0 failed.
- Registry conformance with capability feature: 10 passed, 0 failed.
- Package: 2,841 passed, 0 failed, 7 ignored; doctests 1 passed.
- `cargo fmt --all -- --check`: passed.
- `git diff --check`: passed.

## Worktree Evidence

- Worktree: `/Users/murphy/source/julie-extractors/.worktrees/blazor-razor-support`
- Branch: `codex/blazor-razor-support`
- Base commit before Task 6: `d8be3476fc9b7c4166b8a6dd4a6aa57e56d96513`
- Pre-commit dirty state contained only the Task 6 implementation, tests, generated contract JSON, and this report.
- No push was performed.
