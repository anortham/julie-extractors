# Task 5: Blazor navigation facts

## Status

Implemented `razor.route_reference.v1` for C# and Razor through the canonical framework structural-fact pipeline.

## Behavior

- Emits `NavigationManager.NavigateTo` and `NavigateToLogin` when the first argument is a direct static string literal.
- Emits Razor element `href` values only when the parsed element carries an internal static path.
- Preserves `target_path` exactly as written inside the literal.
- Emits `source_kind` as `navigate_to`, `navigate_to_login`, or `href`.
- Emits `route_source=string_literal`, `framework=blazor`, and the standard base metadata.
- Rejects dynamic and interpolated call arguments, unproven receivers, receiver names shadowed by another type, external and fragment hrefs, protocol-relative paths, and dynamic Razor href values.
- Preserves existing `razor.page_directive.v1` optional and catch-all templates and flags.

## Receiver proof

The collector walks typed C# and embedded Razor declarations. At each member-access invocation it resolves the nearest visible declaration for the receiver name by AST scope. The fact emits only when that declaration's type is `NavigationManager` or a qualified `.NavigationManager`. A same-name parameter or local of another type suppresses an outer field or Razor `@inject` declaration. Bare method-name matching and file-wide name attestation are not used.

## TDD evidence

Initial RED:

- Razor navigation calls: expected 2 facts, observed 0.
- C# navigation calls: expected 2 facts, observed 0.
- Razor internal href: expected 1 fact, observed 0.
- Shadowing regression: both C# and Razor incorrectly emitted 1 `/shadowed` fact before scoped receiver resolution.

Focused GREEN:

- Navigation positive and negative tests: 4 test functions passed.
- Razor structured href test: 1 passed.
- Razor raw optional/catch-all page-template fidelity test: 1 passed.
- Positive facts exercised: Razor `NavigateTo`, Razor `NavigateToLogin`, C# `NavigateTo`, C# `NavigateToLogin`, and Razor internal `href`.
- Negative classes exercised: external HTTP/HTTPS hrefs, fragment href, dynamic Razor href, dynamic and interpolated call arguments, unproven receivers, and typed receiver shadowing.

## Verification

- `cargo test --offline -p julie-extractors razor`: 72 passed, 0 failed.
- `cargo test --offline -p julie-extractors csharp`: 143 passed, 0 failed.
- `UPDATE_CONTRACT_JSON=1 cargo test --offline -p julie-extractors --features test-capability-matrix structural_fact_registry`: 10 passed, 0 failed; regenerated the checked-in JSON contract.
- `cargo test --offline -p julie-extractors structural_fact_patterns_json_matches_checked_in_contract`: 1 passed, 0 failed.
- `cargo test --offline -p julie-extractors --features test-capability-matrix structural_fact_registry`: 10 passed, 0 failed, including C# and Razor emitted-pattern union conformance.
- `cargo test --offline -p julie-extractors`: 2,836 passed, 0 failed, 7 ignored; doctests 1 passed.
- `cargo fmt --all -- --check`: passed.
- `git diff --check`: passed.

## Miller evidence

- Workspace `facd497e3541cca323c780bd925d78c37901f0757736c6e51a62d8704c18e41d` refreshed to revision 12.
- `collect_framework_structural_facts` proved the C# and Razor dispatch arms and canonical caller path.
- `razor_page_directive_fact` proved raw route metadata and route-parameter serialization.
- `collect_nextjs_route_references` proved the `target_path` / `source_kind` / `route_source` vocabulary.
- `static_route_arg` and its C# arm proved the direct-literal acceptance boundary.
- `StructuralFactPatternSpec` and `structural_fact_patterns_contract_json` proved registry and byte-sync shapes.
- Final trace found exactly the module import plus the C# and Razor dispatch calls for `collect_blazor_navigation_facts`.
- Final impact selected framework dispatch, registry serializer/conformance, and the canonical extraction pipeline; all selected gates passed.

## Architecture quality

- Complexity stays local in one internal collector shared by the two language dispatch arms.
- The caller-facing interface remains `collect_framework_structural_facts`; no new public API was added.
- Tests exercise canonical extraction output rather than private helpers.
- The shared seam earns its keep because C# and Razor emit one identical contract while Razor adds structured markup collection locally.
- No speculative extension interface, regex recovery, route normalization, symbol-extractor routing, or workspace classification was added.
- Scoped typed-declaration resolution fixes the structural false-positive cause instead of suppressing one test shape.

## Worktree

- Path: `/Users/murphy/source/julie-extractors/.worktrees/blazor-razor-support`
- Branch: `codex/blazor-razor-support`
- Starting commit: `8aa60bdb5dcd3ac453728a7c6bae2e31642fe383`
- Concerns: none.
