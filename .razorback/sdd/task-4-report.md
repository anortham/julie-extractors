# Task 4 Report: JS/TS static_type_receiver fixtures, allowlist, integration tests

## Status

**complete**

## Worktree

| Field | Value |
|---|---|
| Path | `/Users/murphy/.config/razorback/worktrees/julie-extractors/static-tier-ts-js` |
| Branch | `feature/static-tier-ts-js` |
| Base commit | `bfce2bed` (Task 3) |
| Commit SHA | recorded in git log on `feature/static-tier-ts-js` |

## What changed

### Fixtures (positives)

| Path | Proof |
|---|---|
| `fixtures/extraction/resolution_contract/typescript/static_type_receiver/{fixture,consumer}.ts` | `Fixture.create()` + `Limits.max()` → `tier3_static_type` (0.70), receiver type name, no import |
| `fixtures/extraction/resolution_contract/javascript/static_type_receiver/{fixture,consumer}.js` | ESM `export class Fixture { static create() }`; consumer `Fixture.create()` → `tier3_static_type` |

### Fixtures (negatives)

| Path | Refusal |
|---|---|
| `typescript/static_type_nonexport/` | Non-exported `Hidden`; same-file binds, cross-file `Hidden.create()` refuses |
| `javascript/static_type_instance/` | Exported class with instance `run()`; `Fixture.run()` stays unresolved |

### Allowlist

`TIER3_STATIC_TYPE_LANGUAGES` → `&["csharp", "typescript", "javascript"]` in `crates/julie-extract-cli/src/resolution.rs` (constant + doc comment only).

### Integration tests (`resolution_contract.rs`)

- `static_type_receiver_resolves_typescript_across_files`
- `static_type_receiver_resolves_javascript_across_files`
- `static_type_receiver_refuses_non_exported_typescript_across_files`
- `static_type_receiver_refuses_instance_member_javascript`

C# static tests unchanged and still pass.

## Files modified (ownership)

```
fixtures/extraction/resolution_contract/typescript/static_type_receiver/**
fixtures/extraction/resolution_contract/typescript/static_type_nonexport/**
fixtures/extraction/resolution_contract/javascript/static_type_receiver/**
fixtures/extraction/resolution_contract/javascript/static_type_instance/**
crates/julie-extract-cli/src/resolution.rs          # TIER3_STATIC_TYPE_LANGUAGES only
crates/julie-extract-cli/tests/resolution_contract.rs
.razorback/sdd/task-4-report.md
.memories/**                                        # goldfish checkpoint
```

Not modified: extractors, csharp goldens, `capabilities.json` (Task 5).

## Verification

```bash
cargo +1.97.1 test -p julie-extract-cli --test resolution_contract static_type
# → 8 passed (incl. C# static + TS/JS positives + negatives + fixture guard)

cargo +1.97.1 test -p julie-extract-cli --test resolution_contract every_static_type
# → every_static_type_language_ships_a_proving_fixture ok

cargo +1.97.1 test -p julie-extract-cli --test resolution_contract per_language_tier_parity
# → per_language_tier_parity_guard ok
```

Toolchain: `cargo +1.97.1` (crate requires rustc ≥ 1.95).

## Acceptance criteria

| Criterion | Result |
|---|---|
| TS/JS static_type_receiver resolve at `tier3_static_type` | pass |
| Negatives do not resolve (cross-file non-export; instance) | pass |
| Allowlist = csharp, typescript, javascript | pass |
| `every_static_type_language_ships_a_proving_fixture` | pass |
| `per_language_tier_parity_guard` | pass |
| C# static integration tests | pass |
| Committed (owned files only) | yes (serial-worker-commit) |

## Concerns

1. **TS enum members are not extracted as child symbols** of the enum. Fixture artifact showed `Color` enum with no `Red`/`Blue` children, so a `Color.Red` member_access cannot bind today. Task 4 kept the second positive as another static method (`Limits.max()`) rather than touching extractors (out of ownership). Enum/static-field proofs remain open for a follow-up extractor task.
2. **JS dual symbols**: ESM `export class Fixture` yields both a `class` and an `export` symbol with the same name. Static-type uniqueness still works because it filters type-like kinds only.
3. **Task 5** must update `capabilities.json` `fixture_proven_languages` and coverage artifacts; runtime gap emission already keys off the allowlist.

## Miller

Workspace: `static-tier-ts-js-e85c4b784240`. Inspected C# fixture/tests, `static_type_candidates` / `is_statically_reachable` / `static_receiver_is_reachable`, JS/TS visibility and `isStatic` emission before writing fixtures.
