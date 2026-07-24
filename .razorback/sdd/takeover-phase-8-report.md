# Miller takeover Phase 8: reference-resolution closure

Date: 2026-07-23

## Workspace

- Repository: `/Users/murphy/source/julie-extractors/.worktrees/miller-takeover-resolution`
- Branch: `codex/miller-takeover-resolution`
- Base commit: `ca61aa8367c1a9b559348abfd839a1fc7bfbc68d`
- Source version is prepared as `2.17.0`.
- No push, tag, package publication, or release was performed.

## Result

The extractor and artifact contracts now provide the conservative, provenance-preserving evidence Miller needs to replace Julie's deterministic reference workflows.

- Resolution metadata version is `2`.
- SQLite schema remains `4`.
- Extract contract remains `3`.
- JSON report schema remains `3`; the reference-resolution section is additive.
- The seven canonical relationship kinds are `calls`, `extends`, `implements`, `imports`, `instantiates`, `references`, and `uses`.
- Every report cell identifies language, origin, raw kind, canonical kind, outcome, tier, method, span presence, and count.
- Evidence origins are distinct: `identifier`, `relationship`, and `pending_relationship`.
- Aggregate totals are explicitly evidence-row totals and are also split by
  origin so consumers do not mistake overlapping identifier and pending rows
  for unique source-reference counts.

## Resolver behavior

- `variable_ref` participates in resolution.
- A variable reference resolves only through its scope chain or a unique compatible same-file value symbol.
- Compatible value symbols are variable, constant, field, and property.
- Variable references never fall through to workspace-global name-only resolution.
- `member_access` uses receiver resolution only when receiver context exists and never falls through to the global tier.
- Receiver context is persisted in identifier metadata for `.`, `::`, `->`,
  and `?.` member separators, including sigiled receiver names.
- Structured pending evidence is matched by compatible kind and the narrowest
  containing occurrence. Conflicting textual and structured receiver evidence
  drops receiver and import context instead of guessing.
- Resolution confidence is capped by the source evidence confidence.
- Tier-2 gating is reported only when the edge actually has an import tier.
- Direct relationship propagation preserves the relationship confidence instead of replacing it with the tier maximum.

## Span and occurrence identity

- `Relationship` carries an optional normalized six-coordinate span.
- AST-backed relationship producers emit the relevant node span.
- Non-AST producers use exact content ranges or an exact line match.
- Vue template references use their regex match offset, avoiding ambiguity when the same name occurs twice on one line.
- Relationship IDs include all span coordinates when a span exists, so repeated same-line occurrences remain distinct.
- Normalization offsets relationship spans and refreshes occurrence identity.
- A conservative language-agnostic fallback fills a missing relationship span
  only when exactly one target identifier of a compatible reference kind
  occurs on the relationship line.
- The canonical pipeline owns fallback span inference; the CLI does not repeat
  or widen that decision.
- CSS comment masking preserves byte positions, and Razor/GDScript fallback
  relationships use exact namespace or base-class occurrences rather than
  root, class, or directive-line spans.
- CSS animation relationships carry their parsed segment offset, so a name
  such as `fade` cannot point into an earlier `fadeIn` occurrence.
- Golden fixtures serialize relationship and structured-pending spans.

## Coverage artifact

The committed artifact is `fixtures/extraction/reference-resolution-coverage.json`; its contract is `docs/contracts/reference-resolution-coverage-v1.md`.

- Languages: `36`
- Exact cells: `689`
- Silent cells: `0`
- Quality-bar debts: `0`
- Total evidence rows: `3268`
- Attempted: `1021`
- Resolved: `227`
- Ambiguous: `0`
- Missing: `0`
- No context: `0`
- Unresolved pending: `794`
- Unattempted evidence rows: `2247` (`2209` identifiers and `38`
  non-resolvable pending imports/references)
- Span present: `2830`
- Span missing: `438`
- Direct relationships with spans: `224/224`
- Identifiers with spans: `2212/2212`
- Pending relationships with spans: `394/832`

The 438 span-missing rows are explicit pending evidence, not inferred from a
nearby identifier and not silently missing direct relationships. The strict
report fails on registry drift, stale fixture digests, unmapped canonical
kinds, canonical-kind vocabulary drift, missing summaries, silent cells,
invalid per-cell counts, invalid pending-kind outcomes, or resolved identifiers
outside the live method vocabulary.

## Tests added or strengthened

- Conservative same-file variable resolution wins over an identical symbol in another file.
- A workspace-global-only variable candidate remains unresolved.
- Receiver-bearing member access uses tier 3 and cannot use tier 4.
- Receiver-free member access reports no context.
- Source confidence caps tier confidence.
- Canonical mapping covers all seven advertised kinds.
- C# receiver metadata survives extraction through SQLite and resolves at tier 3.
- JavaScript `member_access` receiver metadata survives extraction through SQLite.
- Nested same-name member calls preserve the receiver belonging to each
  occurrence.
- Same-line direct relationship occurrences receive distinct IDs and exact spans.
- Resolution reports exclude non-reference relationship kinds, include direct,
  pending, and identifier denominators with provenance and span state, and
  expose per-origin totals.
- Span fallback rejects a same-name identifier with an incompatible reference
  kind.
- CSS comment prefixes, Razor `@using`, and GDScript `extends` all retain exact
  occurrence spans.
- Overlapping CSS comment openers stay masked until a non-overlapping closer.
- Prefix-colliding CSS animation names keep distinct exact spans.
- Multi-record normalization offsets structured-pending spans with every other
  source-coordinate domain.
- Pending report rows use the same seven-kind filter as direct relationships;
  pending imports/references are reported as unattempted.
- Coverage uses the live `tier1_local` method vocabulary for resolved
  identifiers.
- Textual receiver inference accepts only exact `.`, `?.`, `->`, and `::`
  separators.
- The coverage artifact is exact over the 36-language registry.

## Verification

Passed:

- `cargo fmt --all -- --check`
- `cargo +1.96.0 metadata --format-version 1 --no-deps`
- `cargo +1.96.0 test -p julie-extract-artifact --test resolution_store_contract` — 28 passed
- `cargo +1.96.0 test -p julie-extract-cli --test operations_contract` — 52 passed
- `cargo +1.96.0 test -p julie-extract-cli --test resolution_contract` — 16 passed
- `cargo +1.96.0 test -p julie-extract-cli`
- `cargo +1.96.0 test -p xtask`
- `cargo +1.96.0 xtask test default`
- `cargo +1.96.0 xtask test contract`
- `cargo +1.96.0 xtask test golden`
- `node scripts/reference-resolution-coverage-report.mjs --strict`
- `node scripts/language-data-quality-report.mjs --strict`
- `cargo +1.96.0 clippy --workspace --all-targets --all-features --no-deps -- -D warnings`
- `cargo +1.96.0 build --workspace --release`
- `cargo deny check` — advisories, bans, licenses, and sources passed; existing duplicate and wildcard dependency warnings remain
- `cargo +1.96.0 xtask release package-list`
- `scripts/check-agent-doc-sync.sh`
- `git diff --check`

The Rust 1.96 Clippy component was absent and was installed with `rustup component add --toolchain 1.96.0-aarch64-apple-darwin clippy` before the Clippy gate.

## Claude review

- The first review pass produced six correctness findings; all six were
  reproduced or validated and remediated.
- The second review pass produced six additional correctness findings; all six
  were reproduced or validated and remediated.
- The focused final pass validated those twelve fixes and found one additional
  CSS keyframes prefix-collision span defect.
- That defect was reproduced with `animation-name: fadeIn, fade`, fixed by
  carrying parsed segment offsets, and covered by an exact-column regression.
- Claude's confirmation pass returned `approve` with zero findings.
- The v2.17.0 release-preparation review found five additional upgrade and
  contract defects: forced and oversized-file upgrades could escape the
  fail-closed gate, first-scan metadata totals were stale, and the CLI and
  steady-state failure contracts were incomplete. All five were fixed with
  regression coverage before the final release-candidate review.
- The fresh final release-candidate review verified all five fixes, reran the
  affected gates, and returned `approve` with zero findings.

## Release and Miller handoff

This source state declares `2.17.0` and must be released before Miller consumes
it. Release preparation and publication require explicit approval and were
intentionally not performed here.

The live Miller takeover worktree currently pins `julie-extract` `2.16.0` in
`scripts/julie-pins.json` for exactly four targets:
`aarch64-apple-darwin`, `x86_64-apple-darwin`,
`x86_64-unknown-linux-gnu`, and `x86_64-pc-windows-msvc`. The Phase 8
contract is not available to Miller until a `2.17.0` release exists and that
pin file carries the four released asset checksums.

After that release exists, Miller must:

1. Pin the new `julie-extract` release and platform checksums in `scripts/julie-pins.json`.
2. Update its documented and fixture expectation for
   `reference_resolution_version` from `1` to `2`, while leaving SQLite schema
   `4` and extract contract `3`. Miller currently surfaces this metadata but
   does not enforce it through a separate version gate.
3. Restore and verify all platform binaries.
4. Add or update source-built and released-binary Scale coverage for variable references, receiver context, relationship occurrence spans, confidence provenance, and the exact resolution report dimensions.
5. Re-run Miller fast, Scale, Release-build, and package verification before using the new evidence in `trace`, `impact`, `inspect`, or search ranking.

When Miller moves from 2.16.0 to 2.17.0, its whole-workspace scan automatically
detects the stale resolution version and re-extracts every supported file before
stamping version 2. Single-file update and delete operations fail with
`schema_migration_required` until that scan completes.

## Remaining boundary

- This phase does not make every pending relationship resolvable. It makes unresolved pending evidence explicit, countable, span-auditable, and safe for Miller to consume.
- No speculative global fallback was added for variable or receiver-qualified member references.
- No Miller code was changed in this worktree.
