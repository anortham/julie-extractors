---
id: infer-local-types-from-call-initializers-in-every-
title: Infer local types from call initializers in every language
status: active
created: 2026-09-23T23:59:14.530Z
updated: 2026-09-24T11:28:44.571Z
tags:
  - type-facts
  - code-kb
  - all-languages
---

# Infer local types from call initializers in every language

## Goal

Record an inferred type fact (`is_inferred = 1`) for a binding with no written type when its value comes from a same-file call whose return type is known. Do this for every general-purpose language.

## Why

- code-kb resolves `x.method()` calls only through `type_facts`. Without a type fact on the binding, code-kb misses real callers in `find_references` and `blast_radius`.
- code-kb is used daily on many languages, not only Rust. The first report was Rust `let workspace = resolve_root(..)?` at code-kb `mcp/server.rs:259`.

## Status (2026-09-24): release 3.6.0 prepared, waiting for approval

- The branch `feat/call-initializer-type-facts` (HEAD `ce96a79a` or later) holds all 24 language units, the Codex fixes, and the performance fix `4974a691`. It is not merged to `main` and not pushed.
- Codex review: 8 findings. 7 are fixed, and 1 (Dart) was disproved with pinning tests.
- Release prep:
  - The version bump to 3.6.0 and the contract marker `call-initializer-type-facts-v1`.
  - The ledger `## 3.6.0` entry and `docs/release-notes/v3.6.0.md`.
  - Preflight, compat-check against 3.5.0, all tiers, and dogfood pass.
  - The Windows default tier passes on `18cdfdbd`.
- The task worktrees and their merged branches are removed.

## Next steps (need user approval)

1. Merge into `main`, push `main`, wait for CI, then tag `v3.6.0` and push the tag (`scripts/check-release-state.sh` gives the steps).
2. Optional: a tree-sitter-swift fork with `1ULL` for the Windows `try!` defect (see `docs/languages/swift.md`).
3. Do not bump the pin in `code-kb/scripts/julie-pins.json` unless the user asks.

## Rules

- Look at callees in the same file only. Workspace-wide resolution is banned.
- A wrong type is worse than no type. Same-named candidates must agree, and a generic return records nothing. Only the language's own unwrap layers are removed.
- Any per-node index walk must not call `Node::parent()` or `prev_named_sibling()` for every node. tree-sitter rescans siblings from the root on each call, so the walk becomes quadratic. Carry the context down the walk instead.
- Parallel cargo work: at most 5 agents, with `CARGO_BUILD_JOBS=4 RUST_TEST_THREADS=4`. `/tmp` is in RAM, so keep large scratch data in `~/.cache`.
