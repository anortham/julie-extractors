# Extractor gap closure design

## Goal

Close the four remaining `julie-extractors` Linear issues without coupling independent language changes or letting the F# integration delay smaller correctness fixes.

## Scope and sequencing

The work is split into four issue-owned implementation plans. Each plan must leave the repository releasable on its own.

1. BRE-16 corrects C# `internal` visibility.
2. BRE-43 emits literal Go `t.Run` child cases.
3. BRE-53 emits Rust doc-test structural facts.
4. BRE-42 adds F# source extraction for `.fs`, `.fsx`, and `.fsi`.

The first three plans may execute independently. The F# plan runs separately because it adds a parser dependency, a language registration, and a full evidence set.

## C# internal visibility

Change `csharp::helpers::determine_visibility` so the `internal` modifier produces `Visibility::Internal`. Keep `metadata_json.csharp_visibility` aligned with the enum-backed visibility column.

The change stays inside the C# extractor contract. It does not alter Miller's reachability policy or add compatibility behavior.

### Acceptance criteria

- Internal C# types, methods, properties, fields, and constructors persist `visibility = "internal"` where their extraction path uses the shared visibility helper.
- Private declarations remain private and default visibility rules remain unchanged.
- A focused unit test and canonical golden distinguish `internal` from `private`.
- The C# capability text no longer says that `internal` maps to private.

## Go `t.Run` child cases

Keep Ginkgo detection gated by Ginkgo imports. Add a separate standard-library subtest extractor in the Go test-call module, then dispatch `call_expression` nodes through both recognized call families without weakening either family's controls.

Only a selector call shaped as `t.Run("literal", function)` inside an enclosing Go test may emit a child case. The child must carry the enclosing test as its parent and use the existing test-role metadata contract. Runtime slash-selector construction remains Miller's responsibility.

### Acceptance criteria

- `t.Run("name", func(t *testing.T) { ... })` emits one deterministic `test_case` child beneath the enclosing top-level test.
- Nested literal `t.Run` calls preserve their parent-child chain.
- A dynamic name, a non-testing receiver's `Run`, a wrong callback shape, and a `t.Run` outside an enclosing test emit no child case.
- Existing Ginkgo and testify extraction remains unchanged.
- The Go golden and focused tests prove positive, nested, and negative cases.
- Capability gap `go.subtest_names` closes with fixture evidence.

## Rust doc-test facts

Add a Rust-specific collector for fenced code blocks inside `///` and `//!` documentation. It emits versioned `rust.doc_test.v1` structural facts instead of inventing symbols or test roles for comments.

Each fact is anchored to the fence span and, when applicable, the documented symbol through `containing_symbol_id`. Fact metadata records the rustdoc fence mode needed by consumers. The collector plugs into the existing structural-fact pipeline and uses the current artifact schema.

### Acceptance criteria

- Executable Rust and untagged rustdoc fences emit deterministic `rust.doc_test.v1` facts.
- `ignore`, `no_run`, and `compile_fail` modes are represented explicitly.
- `text` and non-Rust fences emit no doc-test fact.
- Inner documentation comments attach to their containing module or file context without fabricating a callable symbol.
- Fence and containing-symbol spans remain stable in canonical goldens.
- Capability gap `rust.doc_test_cases` closes with fixture evidence.

## F# source extraction

Add `tree-sitter-fsharp` as an approved exact dependency compatible with the repository's Tree-sitter 0.26 runtime. Use `LANGUAGE_FSHARP` for `.fs` and `.fsx`, and `LANGUAGE_SIGNATURE` for `.fsi`.

The language registry must select the parser by path while publishing one artifact language, `fsharp`. The parser choice stays inside language detection and parser construction; callers continue to request extraction by the stable language name.

Create a dedicated F# extractor rather than treating F# as C# or generic .NET syntax. The first supported contract includes namespaces and modules, types, unions and records, members, functions and values, imports, doc comments, spans, body hashes, relationships, identifiers, type facts, literals, source regions, complexity where meaningful, annotations, and recognized test roles. Unsupported grammar-backed domains remain explicit `open_gaps` with closure requirements.

### Acceptance criteria

- `.fs`, `.fsx`, and `.fsi` select the correct parser while every emitted artifact row uses `language = "fsharp"`.
- Golden fixtures cover implementation files, scripts, signature files, nested declarations, type facts, relationships, literals, documentation, and at least one real F# test framework with useful role facts.
- Symbol ids, spans, parentage, and body hashes are deterministic.
- The language registry, public metadata, parser inventory, CLI extension detection, and capability matrix agree.
- A real extraction reports useful F# rows by language and kind.
- Grammar provenance, checksum or exact revision, license, and freshness policy satisfy the repository grammar dependency contract.
- The strict language data-quality report finishes with zero silent cells and zero quality-bar debts.

## Architecture quality

### BRE-16

- **Affected modules:** C# visibility helper, C# tests, C# fixture evidence.
- **Caller-facing interface:** unchanged `Visibility` and artifact schema.
- **Test surface:** canonical extraction output and focused C# tests.
- **Architecture risk:** low.

### BRE-43

- **Affected modules:** Go call extraction and Go test detection.
- **Caller-facing interface:** existing symbol and test-role metadata contract.
- **Depth/locality check:** standard-library and Ginkgo recognition remain separate private collectors behind one call-expression dispatch point.
- **Test surface:** public canonical extraction output.
- **Rejected shortcut:** broad `Run` name matching without receiver, callback, and enclosing-test validation.
- **Architecture risk:** low.

### BRE-53

- **Affected modules:** Rust-specific structural-fact collection, Rust tests, artifact evidence.
- **Caller-facing interface:** existing `StructuralFact` rows with new versioned pattern id `rust.doc_test.v1`.
- **Depth/locality check:** rustdoc parsing remains language-local and does not expand the symbol vocabulary.
- **Test surface:** canonical structural facts produced by normal extraction.
- **Rejected shortcut:** assigning `test_case` to a comment or parsing documentation in Miller.
- **Architecture risk:** medium.

### BRE-42

- **Affected modules:** parser dependency policy, path-aware parser selection, language registration, new F# extractor, fixtures, capability and language documentation.
- **Caller-facing interface:** stable `fsharp` artifact language across all three extensions.
- **Depth/locality check:** path-specific parser choice is hidden behind the existing language factory rather than exposed to CLI callers.
- **Test surface:** normal CLI and Rust extraction APIs plus canonical fixture evidence.
- **Rejected shortcuts:** claiming `.fsi` with the implementation parser, publishing a separate `fsharp_signature` language, or shipping symbol-only coverage as full support.
- **Architecture risk:** medium.

## Verification contract

- Every implementation plan uses TDD and the narrowest language-specific test command in its worker loop.
- Capability or fixture changes run `node scripts/language-data-quality-report.mjs --strict` and require `silent_cells = 0` and `quality_bar_debts = 0`.
- Canonical golden updates must be reviewed as contract changes, not accepted only because regeneration succeeded.
- The branch gate runs the repository's documented fast/default Rust tests, formatting, strict Clippy, agent-document sync check when applicable, and diff checks.
- Windows-specific verification is required only if parser selection, path handling, or file lifecycle code changes. The F# plan must include Windows path coverage because it changes extension-driven parser selection.
- Security scope is `none declared` for BRE-16, BRE-43, and BRE-53. The F# plan must run the repository's dependency and license checks for the new parser crate.

## Non-goals

- Do not add Miller provider behavior, `go test` selector construction, search, daemon, watcher, or workspace-global resolution logic.
- Do not change Miller's C# reachability policy in this repository.
- Do not convert Rust doc comments into symbols.
- Do not back-port work into the maintenance-only Julie repository.

## Plan boundaries

Each Linear issue gets its own implementation plan and acceptance ledger. Shared capability-matrix edits serialize at integration time even when implementation work runs in parallel. The F# parser-selection contract lands within BRE-42 rather than as a speculative general parser-variant framework.
