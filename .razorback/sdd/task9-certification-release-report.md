# Task 9 certification and release report

## Outcome

- Prepared the initial `julie-extractors` feature release `2.13.0` at candidate revision
  `ff05b298e4f8fa510b31a9f0be22cc0fd3eb143b`.
- Task 4 originally certified `tree-sitter-razor` revision
  `99354a050c5a5190c04b9b07bf4f66d4eae0a6ba`; final review integration pins
  `f82b737c77f5e3ef26bd655eda622b281479bbbc`.
- Did not push, tag, publish, or release.

## RED evidence

- The first full golden run failed on `razor:basic` because the Task 6 component
  fact was not yet recorded in its expected output.
- Registering the new fixture paths before expected generation failed on the
  missing scoped-asset golden, proving the new rows participated in the gate.
- The first capability run found two integration debts: C# had not advertised
  the new `razor.route_reference.v1` registry fact and its property annotations
  were not certified.
- The first strict quality run rejected the Task 4 semantic files named
  `expected.json` because only the new full `golden.json` paths were registered.
- Workspace Clippy found four collapsible conditionals in Task 5 navigation and
  Task 6 Razor relationship code.

## GREEN changes

- Registered all five Task 4 attribute-expression sources as full goldens while
  preserving their focused assertions as `evidence.json`.
- Regenerated `razor:basic` with its component-reference fact.
- Added five Task 9 fixture groups: code-behind, imports, scoped assets,
  constrained type parameters, and render modes.
- Registered both `Widget.razor` and `Widget.razor.cs` identity inputs and both
  `ScopedPanel.razor` and adjacent `ScopedPanel.razor.css` extraction inputs.
- Added focused semantic tests for component class and property identity,
  `_Imports.razor` without synthetic identity, scoped Razor/CSS adjacency,
  constrained `@typeparam`, render mode, cascading parameters, navigation,
  component references, and Razor HTTP requests.
- Certified Razor `class` and `property` symbol kinds plus
  `razor.route_reference.v1`, `blazor.component_reference.v1`, and
  `http.client_request.v1`.
- Added the required C# registry and property-annotation capability claims with
  registered fixture evidence.

## Architecture review

- Component facts remain unresolved extraction evidence. No workspace resolver,
  guessed `external` flag, watcher, search, or Miller-owned resolution behavior
  was added.
- `_Imports.razor` contributes namespace/import inputs but no synthetic
  component class.
- Scoped Razor and CSS assets are adjacent, independently extracted inputs; no
  unsupported cross-file association contract was invented.
- The existing structural-fact IDs and metadata contracts were reused and the
  checked-in registry export remained byte-synchronized.

## Fixture and capability matrix

| Group | Semantic evidence |
| --- | --- |
| Attribute expressions | implicit, explicit, modifiers, directives/render fragment, explicit render mode |
| Code-behind | `.razor` component class/property/facts and `.razor.cs` partial class/property/navigation |
| Imports | namespace/import symbols; no synthetic component identity |
| Scoped assets | Razor component/reference plus adjacent CSS selector fact |
| Type parameter | constrained `TItem : IEntity` and static generic component argument |
| Render mode | parse-clean `@rendermode`, page/component facts, cascading property |

The strict quality report records 36 languages, `silent_cells: 0`, and
`quality_bar_debts: 0`.

## Verification

- `cargo fmt --check`: pass
- `cargo clippy --workspace --all-targets --all-features --no-deps --offline -- -D warnings`: pass
- `cargo xtask test default`: pass; 2,864 extractor tests, 7 ignored, plus artifact and CLI suites
- `cargo xtask test contract`: pass, including downstream path-dependency smoke
- `cargo xtask test certification`: pass; capability 39/39, pending-shape 1/1, and parser upgrade 2/2
- `cargo xtask test golden`: pass; 3/3
- `cargo xtask test capability`: pass; capability 39/39 and pending-shape 1/1
- `node scripts/language-data-quality-report.mjs --strict`: pass; 0 silent cells and 0 quality-bar debts
- Structural-fact registry unit/export tests: pass; 10/10
- CLI structural-fact registry publication contract: pass
- `cargo deny --all-features check`: advisories, bans, licenses, and sources pass; only the repository's accepted duplicate/wildcard warnings remain
- `git diff --check`: pass

## Release preparation

- GitHub showed `v2.12.1` as the published latest release on 2026-07-11, so the
  additive feature release is `v2.13.0`.
- Updated all three crate manifests and `Cargo.lock` to `2.13.0`.
- Added `docs/release-notes/v2.13.0.md`, local release-prep evidence, and updated
  `docs/release.md`.
- `cargo xtask release preflight --version 2.13.0`: pass; 4 targets and 22 inputs.
- `cargo xtask release package-list`: pass.
- Built `target/release/julie-extract`; `--version` reports `2.13.0`.
- Staged the local `aarch64-apple-darwin` package successfully under `target/`.

## Live corpus and T-SQL issue

A clean current CLI scan of `/Users/murphy/source/Terraform` processed 418
paths, extracted 388 supported files, reported 30 unsupported paths, and had
zero failed files. The final-review scan used the fresh artifact
`target/blazor-review-fixes/terraform-f82b737.sqlite`; its release binary was
built offline against `f82b737c`. Parse diagnostics were:

- Razor: 0
- SQL: 283 errors and 1 missing node across 6 files
- `db/baseline.sql`: 225 errors

No open or closed duplicate was found by GitHub searches for T-SQL,
`baseline.sql`, or Terraform SQL parser quality. Filed the separate issue:
https://github.com/anortham/julie-extractors/issues/10.

## Miller evidence

- Miller context identified Task 9, release documentation, the fixture README,
  and capability matrix as the owned integration surfaces.
- Miller searches located the Razor golden, semantic-gate, registry, and
  version surfaces before edits.
- Miller diff impact identified capability snapshot/CLI consumers and the
  focused Razor semantic-gate tests; all likely affected gates passed.

## Repository state

- Worktree: `/Users/murphy/source/julie-extractors/.worktrees/blazor-review-fixes`
- Branch: `codex/blazor-review-fixes`
- Verified base before the final integration commit: `5ae311000d7247e0de3a2ddef927d49a769e0a96`
- Primary checkout: `main`, one existing commit ahead of `origin/main`, with the
  unrelated untracked T-SQL plan left untouched
- Grammar checkout: `codex/blazor-review-fixes` at
  `f82b737c77f5e3ef26bd655eda622b281479bbbc`, with pre-existing untracked
  `.julieignore` and `.miller/`
- Approval boundary: the grammar commit must be pushed before a portable
  downstream pin or release can resolve it; no push was attempted.
