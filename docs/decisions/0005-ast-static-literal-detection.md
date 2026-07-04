# 0005: AST Static-Literal Detection for New HTTP-Boundary Languages

## Context

The v2.8.0 lane (`docs/plans/2026-07-03-finish-http-boundary-lane-design.md`)
adds server-route and client-request facts for four new languages —
kotlin, elixir, php, rust. Every one of those facts is gated by a single
silence-critical decision: is the route/URL argument a **static** string
literal (emit a fact) or a **dynamic** expression (emit nothing)? Under the M2
silence doctrine a false "static" is worse than a miss — it promotes a computed
path to a guessed route, corrupting the join key Miller trusts.

The shipped collectors (Js/Go/Java/C#/Python/Ruby) make this decision with a
hand-rolled byte-level source mask (`framework_structural_facts/scan.rs`,
`MaskLanguage` + `parse_*_string_literal`). Extending that mask to the new
languages would mean hand-lexing Kotlin `$`/`${...}` interpolation, PHP
`"`/heredoc/nowdoc/`.`-concatenation, Elixir `<>`-concatenation and every
`~s`/`~S` sigil delimiter (`{}`/`[]`/`()`/`<>`/`||`/`//`), and Rust raw-string
`#` runs. That is the **highest M2 risk in the release**: it moves the silence
guard into the most error-prone code we own, per-grammar, with a denylist that
fails *open* on anything it forgot. The Doubt Pass (design §9) and the Codex
review (§9b) both flagged this; the resolution is Lane B (design §4.4).

The interpolation-*child* check alone is insufficient: a collector that plucks
the first string literal out of `"/u/" + id`, `format!("/u/{id}")`, or a
`const`/identifier reference would still leak a false positive. The decision
must run on the **whole argument-expression node**, before any value is read.

## Decision

1. **Static-vs-dynamic is an AST whole-argument allowlist check, not a mask.**
   The new-language collectors pass the entire route/URL argument-expression
   node to `static_route_arg(node, content, lang)`
   (`framework_structural_facts/static_arg.rs`). It returns the literal's inner
   text **only** when `node` is *itself* an approved static string-literal kind
   for that grammar; it returns `None` for `binary_expression`/concatenation,
   `call_expression`/macro invocation (`format!`/`sprintf`), identifiers,
   member/subscript access, arrays, and any literal carrying an interpolation
   child. The tree-sitter grammars have already solved the lexing the mask would
   have to re-derive.
2. **Allowlist, never denylist.** The check names the safe node kinds per
   language; an unknown wrapper node fails **closed** to `None` (silence). Per
   design §4.4 the arms are: Kotlin `string_literal`/`multiline_string_literal`
   with no `interpolation` child; Elixir `string`/`sigil`/`charlist` with no
   `interpolation` child and (for a sigil) `sigil_name ∈ {s, S}`; PHP `string`
   or an `encapsed_string`/`heredoc`/`nowdoc` whose children are only
   `string_content`/`escape_sequence`; Rust `string_literal`/`raw_string_literal`
   (no interpolation exists, so the work is rejecting `format!`/concat/`const`
   wrappers).
3. **The `SourceMask` in `scan.rs` is NOT extended.** `MaskLanguage` keeps its
   existing six languages. A lightweight comment/string span mask may still
   drive byte-level delimiter matching for prefix tracing, but the
   static-vs-dynamic decision does not live in it and no new `MaskLanguage`
   variant is added for kotlin/elixir/php/rust.
4. **The allowlist is proven by exhaustive, grammar-verified, table-driven unit
   tests per language arm**, including negative cases for concatenation,
   `format`/`sprintf`/macro, `const`/identifier reference, comment-adjacent, and
   interpolation forms. Task 0 ships the Rust arm and its test harness; later
   framework tasks add their arm and its tests in the same slice.

## Consequences

Easier: the silence guard is one small, testable function reusing lexing the
grammars already do correctly, instead of four hand-rolled per-byte lexers. New
languages fail closed by construction — an unrecognized wrapper is silence, not
a guessed route. One shared helper is reused across all four languages (design
§4.1), so the join key stays trustworthy.

Harder: correctness now rests on **enumerating the safe argument-node allowlist
and the interpolating-child kinds per grammar** — the Elixir `sigil_name` gate
and the PHP `encapsed_string` child allowlist are the two to nail. That
enumeration is grammar-specific and must be re-verified against the tree-sitter
grammar version in `Cargo.toml` whenever a grammar is bumped; the per-language
unit-test table is the guard. Because the helper returns a borrowed source slice
(`Option<&str>`), escape sequences are left unprocessed — acceptable for route
templates, which do not use them.

## Applies To

`crates/julie-extractors/src/base/framework_structural_facts/static_arg.rs`
(the shared helper), the four new-language route/client collectors under
`crates/julie-extractors/src/base/framework_structural_facts/` and
`crates/julie-extractors/src/base/http_clients/`, and
`crates/julie-extractors/src/base/framework_structural_facts/scan.rs`
(`MaskLanguage`), which is deliberately left unextended.

## Future Agents

When adding a route- or URL-bearing framework in a new language: read the route
argument through `static_route_arg` on the **whole** argument node — never
pluck a string literal first and check it in isolation. Add your language's arm
as an **allowlist** of static string-literal node kinds plus the grammar's
interpolation guard (verify the node kinds against the tree-sitter grammar
actually vendored, not from memory), and cover it with a table-driven unit test
that asserts `None` for every concat/format/macro/const-ref/interpolation form.
Do not extend `MaskLanguage`/`SourceMask` to make the static decision, and do
not relax the allowlist to a denylist.
