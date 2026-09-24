# Infer local types from same-file call initializers

Date: 2026-09-24. Release: 3.6.0. Contract marker:
`call-initializer-type-facts-v1`.

## Context

code-kb resolves a receiver call such as `workspace.nested_project_root()`
only through `type_facts`. A local with no written type had no fact unless its
initializer was a constructor, so code-kb missed real callers in
`find_references` and `blast_radius`. The first report was the Rust local
`let workspace = resolve_root(..)?` in code-kb `mcp/server.rs`. code-kb is used
on every supported language, so the gap was not Rust-only.

## Decision

- A local with no written type gets an inferred fact (`is_inferred = 1`) when
  its initializer is a call to a callee whose declared return type is in the
  same file. Each language page in `docs/languages/` states its exact contract.
- Same file only. Each file is extracted alone, and an incremental scan
  re-extracts only changed files, so a type read from another file goes stale.
  Workspace-wide resolution stays with code-kb.
- A wrong type is worse than no type:
  - Every same-named candidate must agree on the return type.
  - A generic or type-parameter return records nothing.
  - A call that a local, parameter, import, or inherited member may bind
    instead records nothing.
  - A chain that ends in an unknown method records nothing.
  - Only unwrap layers the language itself applies are removed, for example
    `?`, `.await`, `await`, `try`, `!!`, or `.unwrap()`.
- A written type always wins over an inferred one.
- A language with no general return-type syntax reads only the declarations
  it can trust: R reads `setGeneric(valueClass =)`, Ruby reads Sorbet and RBS
  signatures, Lua reads `---@return`, and Elixir and Erlang read same-module
  specs. Remaining cases are listed per language, and Lua's are planned in
  [Lua call-initializer gaps](../plans/2026-09-24-lua-call-initializer-gaps.md).

## Rejected

- Cross-file return types. Rejected for staleness under incremental scans and
  for the product boundary: code-kb owns workspace-wide resolution.
- Convention guesses, such as reading `Type::new(..)?` as `Type` when `new` is
  not in the file. Rejected because the guess can be wrong.

## Consequences

- Readers see more inferred `type_facts` rows. The schema does not change.
- Some existing declared facts that were wrong are removed, and a few call
  edges change. The 3.6.0 entry in the
  [extraction-output ledger](../contracts/extraction-output-changes.md) lists
  them.
