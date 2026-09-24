---
id: infer-local-types-from-call-initializers-in-every-
title: Infer local types from call initializers in every language
status: active
created: 2026-09-23T23:59:14.530Z
updated: 2026-09-24T00:46:40.294Z
tags:
  - type-facts
  - code-kb
  - all-languages
---

# Infer local types from call initializers in every language

## Goal

Record an inferred type fact (`is_inferred = 1`) for a local variable with no written type when its value comes from a call whose return type is known. Do this for every general-purpose language, not only Rust.

## Why

- code-kb resolves `x.method()` calls only through `type_facts`. A local with no type fact makes code-kb miss real callers in `find_references` and `blast_radius`.
- code-kb is used daily on C#, TypeScript, Python, Java, Kotlin, Swift, Go, and others, not only on Rust. The gap costs real results in all of them.
- The first report came from Rust: `let workspace = resolve_root(..)?` at code-kb `crates/code-kb-cli/src/mcp/server.rs:259`.

## Status

- **Rust: done.** Committed as `5c60e2d8` on `feat/call-initializer-type-facts`, the integration branch. Not pushed.
  - The code-kb rescan gives `Workspace` for line 259, and `refs nested_project_root` lists server.rs:275.
  - Unresolved receiver calls in code-kb went from 193 to 142.
- **The other 23 language units: in progress.** Workflow run wf_820b31db-470 works on them.
  - The first run (wf_4d0c483c-7ce) crashed the machine by using up its memory, and no unit finished.
  - Each unit has a worktree under `.claude/worktrees/` and a branch `feat/type-infer-<lang>`.
  - C# covers Razor, and JavaScript covers QML.

## Next steps (user instructions, 2026-09-24)

1. When the workflow finishes, merge all 23 `feat/type-infer-*` branches into `feat/call-initializer-type-facts`. Then run the full checks.
2. Have Codex review all the work through the `razorback:codex-cli` skill. Fix every Codex finding and verify the fixes.
3. Prepare a release: version bump, release notes, and the repo's release checks.
   - Do not tag, push, or publish before the user approves.
   - Do not bump the pin in `code-kb/scripts/julie-pins.json`.

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
- Parallel cargo work: at most 5 agents at once, with `CARGO_BUILD_JOBS=4 RUST_TEST_THREADS=4`. 16 at once crashed the 62 GB machine.

## Done means

- Each language has tests for the positive cases and the failure cases.
- The default and contract tiers, clippy, and `language-data-quality-report.mjs --strict` pass.
- There is a before-and-after count on a real repo for each language, local in `~/source` or cloned.
- Codex review findings are fixed, and the release is prepared but not tagged.
