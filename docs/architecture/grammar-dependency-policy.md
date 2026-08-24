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

### Current Owned Grammar Forks

| Dependency | Owned remote | Project-maintained behavior |
| --- | --- | --- |
| `tree-sitter-c-sharp` | [`anortham/tree-sitter-c-sharp`](https://github.com/anortham/tree-sitter-c-sharp) | C# 14 and .NET file-based application syntax not available in the published crate. |
| `tree-sitter-sequel-tsql` | [`anortham/tree-sitter-sql`](https://github.com/anortham/tree-sitter-sql) | Certified T-SQL identifier, DDL, batch, trigger, routine, and `MERGE` syntax. |
| `tree-sitter-razor` | [`anortham/tree-sitter-razor`](https://github.com/anortham/tree-sitter-razor) | Parser fixes required by the certified Razor and Blazor fixtures. |

### Current Approved External Grammars

External grammars may remain on their upstream repository when the repository,
exact commit, license, lockfile source, and extraction evidence are recorded.

| Dependency | Approved remote | Pinned commit | License | Rationale |
| --- | --- | --- | --- | --- |
| `tree-sitter-qmldir` | [`tree-sitter-grammars/tree-sitter-qmldir`](https://github.com/tree-sitter-grammars/tree-sitter-qmldir) | `c57e00865a1a6f1cca83340d6dad91f13df55479` | MIT | No suitable registry release; provides the qmldir module-manifest grammar used by the first-class QML extractor. |

`crates/julie-extractors/Cargo.toml` and `Cargo.lock` are authoritative for the
current full commit IDs. Update this inventory when ownership changes or an
owned fork returns to a suitable published upstream release.

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
reported drift row, and record why a dependency is updated or retained. Run:

```bash
node scripts/grammar-freshness-report.mjs
node scripts/grammar-freshness-report.mjs --format json
```

The command reads the extractor manifest and workspace lockfile, queries
crates.io for the latest non-yanked stable registry releases, and queries each
GitHub repository for its current default-branch head. Each request has a
10-second timeout and identifies its source on failure. Set `GITHUB_TOKEN` or
`GH_TOKEN` when an authenticated GitHub API allowance is needed.

JSON output uses `schema_version: 1`. `audit` records `generated_at`,
`manifest_path`, and `lock_path`. `runtime` records the dependency name, Cargo
package name, declared requirement, locked version, latest stable version, and
status. `registry_grammars` uses the same row fields. `git_grammars` records the
dependency and package names, declared remote and normalized GitHub repository,
pinned and locked commits, remote default branch and head, and status. Rows are
ordered by Cargo dependency name. `current` means the lock matches the latest
stable registry version or remote default head; `drift` means it does not.

The report command is a networked maintenance check. Its network-free contract
tests run with:

```bash
node --test scripts/grammar-freshness-report.test.mjs
```

Do not add the live report to the default, changed, contract, or certification
test tiers. Those tiers must remain deterministic and must not depend on a
registry, forge, credentials, rate limits, or network availability.

The freshness report detects version and commit drift only. It does not change
dependencies, validate parser semantics, or establish a support claim. Support
changes remain gated by parser diagnostics, canonical extraction fixtures,
capability evidence, and the applicable language certification tier.
