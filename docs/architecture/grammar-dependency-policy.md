# Grammar Dependency Policy

## Decision

Julie Extractors uses exactly one Rust Tree-sitter runtime, declared and locked
at `0.26.11`. Parser dependencies are selected independently and must resolve
from either crates.io or an approved, exact Git commit.

Tree-sitter CLI `0.26.11` is the required generator for future checked-in parser
artifacts until a reviewed policy change replaces it. Historical plans and
generation records keep the tool version that actually produced their files;
they are not rewritten to match the current requirement.

## Registry And Git Sources

Use a crates.io release when it provides the required parser behavior. A
registry dependency must use a reviewed version requirement and resolve through
the approved crates.io index in `Cargo.lock`.

Use Git only when the published crate does not contain required,
parser-evidenced behavior or when an approved parser has no suitable registry
release. Every Git parser dependency must:

- use `rev` with one lowercase 40-character commit ID;
- reference a commit already pushed to the declared remote;
- list that remote in `deny.toml`;
- resolve the same commit in `Cargo.lock`;
- avoid branches, tags, local paths, and unpushed commits.

Changes maintained by this project require an `anortham`-owned fork. An
unchanged external Git parser may remain on another approved remote when its
ownership, exact commit, and review rationale are recorded. Depending directly
on an unreleased upstream branch is not accepted.

## Generation Records

New generated parser changes record the grammar source commit, the exact
generation command, Tree-sitter CLI `0.26.11`, and the reviewed generated-file
diff. Generator changes and grammar changes stay in the grammar repository;
Julie Extractors consumes only a published crate or pushed exact commit.

## Semantic Evidence

A newer crate version or Git commit proves dependency freshness, not language
support. A parser change may update extraction or capability claims only after
golden fixtures demonstrate useful emitted rows, valid fixtures have no
`error` or `missing` diagnostics, malformed controls remain diagnostic, and the
strict language data-quality report has no silent cells or quality-bar debt.

Runtime changes are cross-language changes. They require the full
language/default/contract verification selected by the implementation plan,
even when focused dependency-policy checks pass.

## Audit Cadence

Audit parser sources before each release, before every runtime or grammar
change, and at least monthly during active development. Compare registry
versions and Git default heads with `Cargo.toml` and `Cargo.lock`, review every
reported drift row, and record why a dependency is updated or retained. Task 6
of the parser-runtime and grammar-freshness plan replaces the manual comparison
with `node scripts/grammar-freshness-report.mjs`.

The freshness report detects version and commit drift only. It does not change
dependencies, validate parser semantics, or establish a support claim. Support
changes remain gated by parser diagnostics, canonical extraction fixtures,
capability evidence, and the applicable language certification tier.
