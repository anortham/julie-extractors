# Language gap follow-ups

Date: 2026-09-23. Source: the five gaps that wave 2 of the
[language gap closure](2026-09-22-language-gap-closure.md) deferred for an
owner decision or a grammar change. Owner decisions from 2026-09-23:

| Gap | Decision |
| --- | --- |
| SpecFlow, Reqnroll, and Behat step definitions | Add a `step_definition` test role |
| `go.mod` / `go.sum` | Add Go module-manifest languages keyed on exact basenames |
| JSON5 | Out of scope; record the decision and retire the open gap |
| Regex conditionals | Stay a recorded grammar limit |

## Outcome

- Step-definition methods publish `test_role = "step_definition"`, and a Behat
  context class is a test container.
- `go.mod` files extract as the `gomod` language, and `go.sum` files as the
  `gosum` language.
- The JSON5 and regex decisions are recorded, and the capability ledger shows
  each remaining gap with a concrete reason.

## Tasks

1. `step_definition` role. Add `TestRole::StepDefinition`. Classify
   SpecFlow and Reqnroll `[Given]`, `[When]`, `[Then]`, and `[StepDefinition]`
   methods in a `[Binding]` class. Classify Behat `#[Given]`, `#[When]`, and
   `#[Then]` attributes and `@Given`, `@When`, and `@Then` docblock tags on
   methods of a class that implements a Behat `Context` interface. Behat
   `BeforeScenario`-style hooks become fixture hooks. Golden fixtures carry a
   control class for each framework. See the
   [decision](../decisions/2026-09-23-step-definition-test-role.md).
2. `gomod` language. Pin `camdencheek/tree-sitter-go-mod`, select the exact
   basename `go.mod`, and publish module, go, toolchain, require, replace,
   exclude, retract, tool, and ignore rows with golden fixtures.
3. Grammar forks. Done. The owner approved the forks, and both grammars are
   pinned to owned `anortham` forks. The three open gaps on the `gomod` row
   are closed:
   - `gomod.go_sum_checksums`: `anortham/tree-sitter-go-sum` accepts empty
     files and any semver pre-release. The new `gosum` row, keyed on the
     exact basename `go.sum`, publishes one `gosum.checksum.v1` fact per line.
   - `gomod.godebug_directive`: `anortham/tree-sitter-go-mod` parses the
     `godebug` directive. Each setting is a `property` symbol and a
     `gomod.godebug.v1` fact.
   - `gomod.grammar_path_tokens`: the same fork reads absolute and
     one-character paths and a last line without a newline.

   The release CLI reads all 70 local `go.sum` and all 128 local `go.mod`
   files with 0 parse diagnostics.
4. JSON5 and regex decisions. Record the
   [JSON5 decision](../decisions/2026-09-23-json5-out-of-scope.md) and remove
   the `json5_unquoted_keys` gap. Keep `regex_conditional_patterns` open with
   a grammar-change closure.
5. Integration. Move the crate version to 3.5.0, append
   `step-definition-role-v1.go-module-manifest-v1.go-sum-checksums-v1` to
   `EXTRACTION_CONTRACT_VERSION`, and declare the output changes under
   `## 3.5.0` in the output-change ledger. The site pages describe the
   published release, so their language counts move at 3.5.0 publication.

## Gates

The branch gate from the gap-closure plan: `cargo fmt --check`, workspace
Clippy with `-D warnings` on the local toolchain and on Rust 1.98,
`scripts/check-agent-doc-sync.sh`, the strict quality report, `cargo test -p
xtask`, `cargo xtask test default`, and `cargo xtask test contract`. The
Windows default tier runs because `gomod` and `gosum` add basename selection
rules.
