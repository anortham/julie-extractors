# Fix: C# `A * B` pointer mis-parse → both operands invisible to liveness

**Worktree:** `.worktrees/csharp-misparse-fix`
**Branch:** `fix/csharp-pointer-misparse` (base `feat/variable-ref-emission` @ `1d525f7`)
**Scope:** C# identifier extraction only — no schema, no capabilities.json, no cross-extractor coupling.

## The defect

tree-sitter-c-sharp resolves `identifier * identifier` in expression/argument
position as a **pointer-type declaration** instead of multiplication. The `*` binds
as a pointer declarator, so `A * B` parses as
`declaration_expression{ type: pointer_type{ type: identifier=A }, name: identifier=B }`.

Consequences in the extractor:
- `A` (pointee identifier) was emitted as a bogus `type_usage`.
- `B` (the declaration_expression `name`) was excluded by every arm → **emitted nothing**.

Real-world hit: `Miller.Server/Resolution/SymbolSuggestionEngine.cs:26`
`int searchLimit = Math.Max(limit * SearchCandidateMultiplier, 24);` made the const
`SearchCandidateMultiplier` invisible to all liveness signals → false dead-code positive.

## Probe evidence (2.10.0 grammar, `to_sexp`)

| Case | Parse shape | Note |
|------|-------------|------|
| `Math.Max(limit * K, 24)` (arg) | `argument(declaration_expression type: pointer_type(type: identifier=limit) name: identifier=K)` | **mis-parse** |
| `int t = limit*3` | `binary_expression left: identifier=limit right: integer_literal` | literal can't be a declarator → correct |
| `int s = limit * K` (assign) | `binary_expression left/right: identifier` | assignment position parses correctly |
| `return limit * K` | `binary_expression` | return position parses correctly |
| `unsafe void M(){ int* p=&x; }` | `local_declaration_statement(variable_declaration type: pointer_type ...)` | genuine pointer — different node (`variable_declaration`, not `declaration_expression`) |
| `unsafe { …Max(limit*K)… }` | mis-parse shape nested under `unsafe_statement` | unsafe block marker |
| `unsafe void M(){ …Max(limit*K)… }` | mis-parse shape under method with `(modifier)`=unsafe | unsafe modifier marker |

Key language fact: **C# pointer types are legal only in `unsafe` contexts.** So a
pointer-type `declaration_expression` with **no enclosing unsafe context is always a
mis-parsed multiplication** — firing the recovery there is never wrong.

## The fix (`crates/julie-extractors/src/csharp/identifiers.rs`)

New match arm evaluated **before** the TypeUsage arm:

```
"identifier" if is_csharp_misparsed_mul_operand(base, node) => emit VariableRef
```

- `is_csharp_misparsed_mul_operand` returns true for either operand identifier
  (pointee `A` inside `pointer_type`, or `name` `B` of the `declaration_expression`)
  of a pointer-type `declaration_expression`, **gated on `!is_in_csharp_unsafe_context`**.
- `is_in_csharp_unsafe_context` walks ancestors: true on an `unsafe_statement`, or on any
  ancestor declaration carrying a `modifier` child whose text is `unsafe`
  (`csharp_node_has_unsafe_modifier`).
- Because the arm runs first, `A` never doubles as `type_usage` → single `variable_ref`
  row per operand, no duplicates. Genuine unsafe pointer declarations are gated out and
  keep today's behavior (pointee `type_usage`, declarator name excluded).
- LOCKED CONTRACT doc-comment extended with a "HEURISTIC ADDENDUM" describing the recovery;
  the six original rules are unchanged (this is an appended documented behavior).

## TDD

RED (before fix): `test_csharp_pointer_misparse_multiplication_emits_variable_refs`
failed with `got ["System","s","limit","t"]` — `K` missing, `limit*K` operand still a
bogus type_usage (the lone `limit` was from `limit*3`). Correct reason.

GREEN (after fix): both new tests pass.
- `test_csharp_pointer_misparse_multiplication_emits_variable_refs` — `limit` and `K` both
  `variable_ref`, no `type_usage` for `limit`, exactly 2 `limit` rows both value reads,
  `limit*3` still a read.
- `test_csharp_unsafe_pointer_declaration_unchanged` — genuine unsafe `Node* p` keeps
  `Node` as `type_usage`; unsafe-wrapped mis-parse gate honored (`K` not a read, `limit`
  stays `type_usage`).

## Fixtures

Extended `fixtures/extraction/csharp/basic/source.cs` `Registry` with a `Scale` const and
`Scaled()` method (`Math.Max(requested * Scale, 1)`) mirroring the real Miller hit.
Regenerated golden via `UPDATE_GOLDEN=1 … --features test-golden`. Golden diff shows exactly:
- `Scale:variable_ref:83:36` (const no longer looks dead)
- `requested:variable_ref:83:24`
- no `type_usage` rows for either operand
- other rows re-anchored by the +1 line insert only.

Drift check: `git status --short fixtures/ | grep -v csharp/` → empty. **No razor / no
non-csharp fixture drift.**

## Gates

- `cargo test -p julie-extractors --lib` → **2821 passed, 0 failed, 7 ignored**
- csharp suite (`… csharp`) → 139 passed
- golden verify (no update) → pass
- `cargo clippy -p julie-extractors --all-targets` → 0 warnings
- `cargo fmt -p julie-extractors -- --check` → clean

## End-to-end proof

Release `julie-extract` built in this worktree, scanned a scratch copy of the real file:

```
$ julie-extract scan --root <scratch> --db <scratch>/symbols.db
$ sqlite3 symbols.db "SELECT name,kind,start_line FROM identifiers WHERE start_line=26"
Math|variable_ref|26
Max|call|26
SearchCandidateMultiplier|variable_ref|26   <-- previously absent (false dead-code)
limit|variable_ref|26                         <-- previously bogus type_usage
```

`SearchCandidateMultiplier` now has a single `variable_ref` row and no `type_usage`.

## Judgment calls / concerns

- Gate uses the language fact "pointer types require unsafe" rather than trying to
  distinguish parse intent structurally — the grammar markers (`unsafe_statement`,
  `(modifier)`=unsafe on ancestor declarations) are cleanly visible from the
  declaration_expression, so the check is reliable (no STOP condition hit).
- The recovery is intentionally narrow: it only fires on the exact pointer-type
  `declaration_expression` shape. Legitimate `out var`/`out int` declaration_expressions
  (non-pointer types) are untouched.
- `variable_ref` remains non-resolvable (consumed by name-match only), consistent with the
  locked contract's NON-GOALS.
