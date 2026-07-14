# Parser Runtime and Grammar Freshness Design

## Goal

Bring the shared Tree-sitter runtime and the C#, Swift, and R parsers to the
current verified language surface without regressing any extraction artifact,
then add a repeatable freshness report so parser drift is discovered before it
becomes another corpus-wide quality problem.

## Why Now

- The completed T-SQL lane proves that published grammar versions and live
  language syntax can diverge materially.
- Julie Extractors declares Tree-sitter `0.26.8` and currently locks `0.26.9`,
  while Tree-sitter `0.26.11` shipped parser-reuse and query-anchor fixes.
- Julie Extractors uses `tree-sitter-c-sharp 0.23.5`. That remains the newest
  crate, but upstream C# 14 support landed after the crate release at
  `tree-sitter/tree-sitter-c-sharp@af29416d729b7a6603101b513604392d8f675e3b`.
- The upstream C# 14 work intentionally leaves .NET file-based app directives
  unrecognized even though .NET 10 uses `#:package`, `#:project`, `#:property`,
  and `#:sdk`, and current SDKs also accept `#:include` and shebang lines.
- Published parser updates exist for Swift `0.7.3` and R `1.3.0`; both change
  node inventories and therefore require migration evidence, not blind bumps.

## Design

### Integration base

The implementation branch descends from the completed T-SQL commit
`dbff11b8598e47eea867c1cc69484561b9877b3e`. T-SQL remains part of the final
fast-forward history, and the completed SQL/Razor Terraform replay remains a
hard regression gate. The protected untracked T-SQL plan in the primary
checkout is not modified during implementation.

### Runtime lane

Update the declared and locked Rust Tree-sitter runtime to `0.26.11`. Treat the
runtime change as cross-language: run the full language/default/contract gates,
not only C#, Swift, and R. Standardize future grammar generation evidence on
Tree-sitter CLI `0.26.11`; historical plan records keep the tool version that
actually generated their parser.

### C# 14 lane

Create the owned fork `anortham/tree-sitter-c-sharp` from upstream commit
`af29416d729b7a6603101b513604392d8f675e3b`. Before changing the grammar, add
corpus cases for .NET file-based app directives and prove that upstream still
reports them as errors. Add a narrow grammar node that accepts complete `#!`
and `#:` directive lines without interpreting SDK or MSBuild semantics, generate
with Tree-sitter CLI `0.26.11`, and keep all upstream corpus/highlight/binding
tests green. Push the fork commit before Julie depends on it.

Julie then pins the exact fork commit. Add a registered `csharp:csharp14`
fixture sourced from official C# 14 and .NET file-based app documentation. It
covers extension declarations, null-conditional assignment, unbound generic
`nameof`, field-backed properties, simple lambda parameter modifiers, partial
constructors/events, user-defined compound assignment, supported `#:`
directives, and a shebang. Valid fixture code must emit zero error/missing
diagnostics and useful existing extraction rows; malformed controls must remain
diagnostic. New node shapes are adapted only where canonical extraction would
otherwise lose or misclassify data.

### Swift lane

Update `tree-sitter-swift` from `0.7.2` to `0.7.3`. Freeze parser-backed cases
for consume/discard operators, typed-throws do/catch, parenthesized
`nonisolated`, conditional directives inside type bodies, bracket-qualified
nested types, and double-optional lambda parameter types. Preserve existing
symbol, identifier, relationship, type, literal, and structural-fact shapes;
adapt language-local matchers only when the new node inventory proves a real
migration need.

### R lane

Update `tree-sitter-r` from `1.2.0` to `1.3.0`. Freeze cases for `return` as a
normal identifier, hexadecimal constants with decimals, identifiers beginning
with `else`, raw-string open/content/close nodes, and CRLF comment boundaries.
The R extractor continues to emit stable public artifact shapes even where the
parser exposes more precise internal nodes.

### Freshness report

Add `scripts/grammar-freshness-report.mjs` as a networked, non-default
maintenance command. Its caller-facing interface is:

```text
node scripts/grammar-freshness-report.mjs [--format text|json]
```

The report reads `Cargo.toml` and `Cargo.lock`, queries crates.io for registry
packages, resolves GitHub default heads for Git pins, and reports runtime,
registry-grammar, and git-grammar drift separately. JSON output is a versioned
contract with deterministic ordering. Network failures return a nonzero exit
and identify the failed source. Unit tests exercise pure normalization and
comparison functions with fixtures; no network call enters the default suite.
The report detects version/commit drift but never claims semantic language
support automatically.

## Architecture Quality

**Affected modules:** dependency manifests, parser registration, C#/Swift/R
language-local extractors and tests, registered goldens/capabilities, release
dependency policy, and the new maintenance report.

**Caller-facing interface:** the existing `julie-extract` artifact contracts
remain unchanged. The only new interface is the maintenance script's text/JSON
report.

**Depth/locality check:** grammar node migration stays inside each language
module. Remote metadata handling stays inside one script instead of leaking
network/version logic into extractors or default tests.

**Test surface:** parser diagnostics and canonical extraction through the same
language/CLI interfaces used by callers, plus contract tests for the report.

**Seams/adapters:** the freshness script separates registry and Git metadata
adapters from pure comparison/rendering functions. Both adapters are real,
current sources rather than speculative extension points.

**Rejected shortcuts:** blind `cargo update`, assuming newest crate means newest
language support, pinning an unpushed/local grammar commit, treating parser-only
success as extraction support, accepting generated golden drift without review,
and running network freshness checks in the default tier.

**Architecture risk:** medium. Public interfaces do not change, but parser and
runtime node-shape changes have broad silent-regression potential.

## Acceptance Criteria

- Tree-sitter runtime and future generation floor are `0.26.11`.
- C# 14 official syntax and .NET file-based app directives parse without valid
  diagnostics from an exact pushed fork commit.
- Swift `0.7.3` and R `1.3.0` are adopted with reviewed node-shape migrations.
- Existing general-language extraction behavior, goldens, capability claims,
  structural-fact registry, artifact schema, and CLI contracts remain stable
  except for new evidence-backed fixture rows.
- SQL and Razor remain at zero diagnostics on the pinned Terraform corpus, and
  malformed T-SQL controls remain diagnostic.
- The freshness report is deterministic, tested, versioned, and outside the
  default network-free test tier.
- Full formatting, Clippy, default, language, golden, capability, contract,
  certification, strict quality, and targeted real-corpus gates pass.
- Julie Extractors is not pushed, tagged, published, versioned, or released
  without a separate explicit approval.

## Sources

- https://github.com/tree-sitter/tree-sitter/releases/tag/v0.26.11
- https://learn.microsoft.com/en-us/dotnet/csharp/whats-new/csharp-14
- https://learn.microsoft.com/en-us/dotnet/core/sdk/file-based-apps
- https://github.com/tree-sitter/tree-sitter-c-sharp/pull/429
- https://github.com/r-lib/tree-sitter-r/compare/v1.2.0...v1.3.0
- https://github.com/alex-pinkus/tree-sitter-swift/compare/0.7.2...0.7.3
