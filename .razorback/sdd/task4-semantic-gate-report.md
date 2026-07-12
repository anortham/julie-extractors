# Task 4: Razor semantic gate report

## Result

- Grammar pin updated from `cf7b0e56f5ba9469c70f85d617c52e35eaffa153` to `d24d075afe5b18eae56c4386046ed5e6e3902795` in `Cargo.toml` and `Cargo.lock`.
- Added four caller-facing fixtures covering implicit expressions, explicit expressions, directive modifiers, custom render modes, generic component types, constrained type parameters, and render fragments.
- Added a public `extract_canonical` gate requiring zero parse diagnostics plus expected symbols and identifier kinds for every fixture.
- Added mirrored C# and Razor handling for conditional member binding so `value?.Property` emits `MemberAccess` and `value?.Method()` emits `Call`.

## Miller evidence

- Workspace: `facd497e3541cca323c780bd925d78c37901f0757736c6e51a62d8704c18e41d` (`blazor-razor-support-facd497e3541`).
- `context` identified `golden_fixtures_match_canonical_extraction`, the Razor test module, `extract_canonical`, and `ExtractionResults` as the relevant caller-facing path.
- `inspect(depth=full)` proved `extract_canonical(file_path, content, workspace_root)` returns public `ExtractionResults` containing symbols, relationships, identifiers, types, type-argument usages, literals, and parse diagnostics.
- `trace` proved each language walker calls its local `extract_identifier_from_node`; no public API changed.
- `inspect(depth=full)` proved both `is_razor_value_read_identifier` and `is_csharp_value_read_identifier` deliberately returned false for children of `member_binding_expression`, while neither node dispatcher handled `member_binding_expression`.
- The RED C# test printed the parser shape during root-cause investigation: property access is `conditional_access_expression -> member_binding_expression`; conditional invocation wraps that expression in `invocation_expression`. The temporary diagnostic output was removed.
- Pre-edit `impact` identified the Razor and C# identifier walkers and their type/value classification helpers as the affected code; package and language-scoped tests were selected accordingly.
- The worktree-specific Miller index was usable. Its diff-impact output included unrelated name-based neighbours for `Cargo.lock`, so verification scope followed the declared worker commands rather than those noisy suggestions.

## TDD ledger

| Invariant | Command | Revision | Result | Timestamp (UTC) |
|---|---|---|---|---|
| Old grammar rejects the previously missing attribute-expression forms | `cargo test -p julie-extractors attribute_expression_fixtures_are_clean_and_semantically_visible -- --nocapture` | `cf7b0e5` | RED: 0 passed, 1 failed; four `Error` diagnostics in the implicit fixture | 2026-07-12 |
| New grammar parses every fixture and exposes the initially expected rows | `cargo test --offline -p julie-extractors semantic_gate -- --nocapture` | `d24d075` before extractor fix | RED: 1 passed, 3 failed; all four fixtures were parse-clean | 2026-07-12 |
| Conditional access is missing member and call rows in standalone C# | `cargo test --offline -p julie-extractors conditional_access_emits_member_and_call_identifiers -- --nocapture` | `d24d075` before extractor fix | RED: 0 passed, 1 failed; `UploadFailures` absent | 2026-07-12 |
| Conditional access member/call classification works in standalone C# | same targeted C# command | `d24d075` after extractor fix | GREEN: 1 passed, 0 failed | 2026-07-12 |
| Every Razor fixture is parse-clean and semantically visible | `cargo test --offline -p julie-extractors semantic_gate -- --nocapture` | `d24d075` after extractor fix | GREEN: 4 passed, 0 failed | 2026-07-12 |
| Razor regression scope remains green | `cargo test --offline -p julie-extractors razor` | `d24d075` after extractor fix | GREEN: 68 passed, 0 failed | 2026-07-12 |
| Extractor package ceiling remains green | `cargo test --offline -p julie-extractors` | `d24d075` after extractor fix | GREEN: 2,830 passed, 0 failed, 7 ignored; doc tests 1 passed | 2026-07-12 |

## Plan-mismatch adjudication

- Initial expected evidence treated markup callback method groups as calls. The lead confirmed these are references, not invocations. The gate now requires `LookupAsync`, `ValueUpdated`, and `SetValue` as `VariableRef`; actual invocations remain required as `Call`.
- Initial expected evidence treated declared `@typeparam TItem` as a type usage. The lead confirmed the declaration name is not a use. The gate requires its constraint and signature types (`IEntity`, `RenderFragment`, `State`) as `TypeUsage` and records the current `TItem` declaration-model row as `VariableRef`.
- The missing `UploadFailures` row was a real shared extractor gap. Ownership was expanded by the lead to the mirrored Razor/C# walkers and a standalone C# regression test. The structural fix handles the grammar's `member_binding_expression` directly without regex recovery.

## Terraform re-extraction handoff

Lead-owned, not run by this worker. Repository-documented build and scan forms produce the exact handoff command:

```bash
cargo build --release -p julie-extract-cli --bin julie-extract
mkdir -p target/blazor-razor-support
target/release/julie-extract scan --root ~/source/Terraform --db target/blazor-razor-support/terraform.sqlite --force --json
sqlite3 -readonly target/blazor-razor-support/terraform.sqlite "SELECT COUNT(*) FROM parse_diagnostics WHERE language='razor';"
```

The build form is documented in release evidence; the scan contract is documented in `docs/contracts/cli.md` and the data-quality release evidence.
