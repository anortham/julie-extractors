---
id: infer-local-types-from-call-initializers-in-every-
title: Infer local types from call initializers in every language
status: active
created: 2026-09-23T23:59:14.530Z
updated: 2026-09-23T23:59:14.530Z
tags:
  - type-facts
  - code-kb
  - all-languages
---

## Goal

Record an inferred type fact (`is_inferred = 1`) for a local variable with no written type when its value comes from a call whose return type is known. Do this for every general-purpose language, not only Rust.

## Why

- code-kb resolves `x.method()` calls only through `type_facts`. A local with no type fact makes code-kb miss real callers in `find_references` and `blast_radius`.
- code-kb is used daily on C#, TypeScript, Python, Java, Kotlin, Swift, Go, and others, not only on Rust. The gap costs real results in all of them.
- The first report came from Rust: `let workspace = resolve_root(..)?` at code-kb `crates/code-kb-cli/src/mcp/server.rs:259`.

## Status

- **Rust: done, not released.** The changes are uncommitted on `main`: `rust/type_facts.rs` and `tests/rust/initializer_types.rs`.
  - The code-kb rescan gives `Workspace` for line 259, and `refs nested_project_root` lists server.rs:275.
  - Unresolved receiver calls in code-kb went from 193 to 142.
- **Go:** already infers from calls to functions in the same file. Method calls may still be a gap.
- **Everything else:** open. As of 2026-09-23, most languages infer only from `new Foo()` or from constructor calls to a class in the same file:
  - C#, Java, PHP, JavaScript, TypeScript, Kotlin, Swift, Scala, Python, Dart.
  - C, C++, Zig, F#, VB.NET, Lua, R, Ruby, Elixir, Erlang, GDScript, PowerShell each need their own check.

## Rules (from the Rust design)

- Look at callees in the same file only. Each file is extracted alone, and incremental scans re-extract only changed files, so a type taken from another file goes stale. Workspace-wide resolution is also banned (CLAUDE.md).
- A wrong type is worse than no type:
  - Same-named candidates must agree on the return type.
  - Generic or type-parameter returns record nothing.
  - An unknown method at the end of a chain records nothing.
- Remove only the wrapper layers that the language itself unwraps, such as `await` on `Task<T>`/`Promise<T>` or `!!`. Each language decides its own list, and tests cover it.
- Where a language cannot declare return types, or a gap stays open, record the evidence in `capabilities.json`:
  - `not_applicable` needs source verification.
  - An open gap is `open_gaps` debt with a reason, the required closure, and the planned task.

## Done means

- Each language has tests for the positive cases and the failure cases.
- The default and contract tiers, clippy, and `language-data-quality-report.mjs --strict` pass.
- There is a before-and-after count on a real local repo for that language where one exists in `~/source`.
- No tag, publish, release, or pin bump without user approval.
