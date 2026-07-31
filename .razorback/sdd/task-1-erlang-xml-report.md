# Task 1 — Grammar spike (risk-first) — Report

**Status:** DONE
**Commit:** `fd962a50`
**Worktree:** `/Users/murphy/source/julie-extractors/.worktrees/erlang-xml-language-support`
**Branch:** `erlang-xml-language-support` (HEAD at start `4ef18c10`, clean)
**Toolchain:** `RUSTUP_TOOLCHAIN=1.97.1` on every cargo invocation (global default 1.94.0 untouched)

## Outcome

Phase-0 gate **PASSES**. `tree-sitter-xml 0.7.0` — the main compatibility risk — loads and parses
cleanly under the workspace `tree-sitter = "=0.26.11"` runtime pin. No vendoring/fork decision needed.
`tree-sitter-erlang 0.20.0` likewise.

## Miller calls used (workspace_id `julie-extractors-91c17adbdab9`)

| Call | What it confirmed |
|---|---|
| `search(query="get_tree_sitter_language")` | Grammar loader lives at `crates/julie-extractors/src/language_spec/mod.rs:264`, re-exported through `crate::language` |
| `inspect(target="get_tree_sitter_language", depth=full)` | Body is `language_spec(language).map(LanguageSpec::parser_language)` — production loads grammars via a `LanguageSpec` registry; the test helper `init_parser` (`src/tests/helpers.rs:18`) drives it as `Parser::new()` → `set_language(&lang)` → `parse(code, None)`. The smoke tests mirror that exact sequence against the raw grammar constants. |
| `inspect(target="crates/julie-extractors/src/tests")` | No indexed symbols (directory, not file) — `diagnostic_class=expected_empty` |
| `inspect(target="crates/julie-extractors/src/tests/mod.rs")` | Test-mod registration is a flat alphabetical list of `pub mod <name>;` lines, with `#[cfg(feature = "...")]` gates only on optional tiers |

Miller reported `freshness: unconfirmed_lock_busy` on every call (another indexer held the lock). Results
were consistent with the raw files I then read, so orientation was sound.

## API-shape evidence (external crates — read from downloaded source, not memory)

Source of truth: `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/<crate>/bindings/rust/lib.rs`
after `cargo fetch`.

`tree-sitter-erlang-0.20.0/bindings/rust/lib.rs`:
- `:31 pub const LANGUAGE: LanguageFn` ← **used**
- `:36 pub const NODE_TYPES: &str`
- `:40 pub const HIGHLIGHTS_QUERY: &str`
- `:41-43` injections/locals/tags queries are **commented out upstream** — not available

`tree-sitter-xml-0.7.0/bindings/rust/lib.rs`:
- `:40 pub const LANGUAGE_DTD: LanguageFn`
- `:45 pub const LANGUAGE_XML: LanguageFn` ← **used**
- `:48 XML_HIGHLIGHT_QUERY`, `:51 DTD_HIGHLIGHT_QUERY`
- `:56 XML_NODE_TYPES`, `:61 DTD_NODE_TYPES`

### XML entry-point finding (recorded for Task 3)

`tree-sitter-xml` ships **two** grammars in one crate. The XML extractor must use
**`tree_sitter_xml::LANGUAGE_XML`** (`.into()` → `tree_sitter::Language`). `LANGUAGE_DTD` is the separate
DTD grammar and is **not** a fallback for `.xml` files — it parses `.dtd` documents only. Node-types JSON
for capability work is `tree_sitter_xml::XML_NODE_TYPES`.

Observed root node kinds: XML → **`document`**; Erlang → **`source_file`**.

Both crates depend only on `cc` + `tree-sitter-language` (the ABI shim) — no transitive `tree-sitter`
version is pulled in, which is why the 2024-era `tree-sitter-xml` is compatible with runtime 0.26.11.

## Changes

| File | Change |
|---|---|
| `crates/julie-extractors/Cargo.toml:46` | `tree-sitter-erlang = "=0.20.0"` |
| `crates/julie-extractors/Cargo.toml:68` | `tree-sitter-xml = "=0.7.0"` |
| `Cargo.lock` | +22 lines: two package entries + two dep-list lines. **No transitive version churn** (full diff reviewed). |
| `crates/julie-extractors/src/tests/mod.rs:33` | `pub mod grammar_smoke;` |
| `crates/julie-extractors/src/tests/grammar_smoke.rs` | new — two smoke tests |

## Judgment calls

- `Cargo.toml:46` — placed `tree-sitter-erlang` next to `tree-sitter-elixir` (BEAM neighbours) rather than
  appending at the end; the surrounding block is already grouped by family/domain.
- `Cargo.toml:68` — placed `tree-sitter-xml` in the "Documentation and configuration languages" block next
  to `tree-sitter-yaml`/`tree-sitter-json` rather than the code-grammar block. XML is a config/document
  format in this codebase's taxonomy, and Task 9 (schema/WSDL structural facts) reinforces that framing.
- Exact pins `=0.20.0` / `=0.7.0` per the task spec, matching the existing `=`-pin style used for
  `tree-sitter`, `tree-sitter-r`, and `tree-sitter-swift`.
- `grammar_smoke.rs:9` — the smoke tests call `set_language()` on the raw grammar constants instead of
  routing through `get_tree_sitter_language`/`init_parser`. Chose this because Task 1 explicitly precedes
  `LanguageSpec` registration (Tasks 2/3); routing through the registry now would either fail or force
  premature registration work outside this task's file ownership.
- `grammar_smoke.rs:1-6` — module doc states the delete-me lifecycle constraint, per the task's permitted
  exception to the no-narration comment rule.

## Self-review findings

1. **Assertions could have been vacuous.** A test that goes green the instant deps compile proves nothing
   about the ERROR check. Ran a mutation check: corrupted both snippets (`-module(bank ((.` and
   `<dependencies attr=>`) and confirmed **both tests FAIL**; restored and re-ran green. The ERROR/MISSING
   assertions are load-bearing.
2. **Removed a redundant assertion.** The first draft ended `assert_no_error_nodes` with a
   `!root.has_error()` check the traversal already subsumes (`has_error` is true iff a descendant is ERROR
   or MISSING). Dropped it; the traversal also yields a better failure message (node kind + position).
3. **`src/tests` is NOT `#[cfg(test)]`-gated** (`lib.rs:142` is a bare `pub mod tests;`), so the new module
   compiles in normal builds too. Checked under warnings-as-errors: `cargo build -p julie-extractors` and
   `cargo clippy -p julie-extractors --all-targets` are both clean, **zero warnings**.
4. **File ownership respected.** `git status` before commit showed exactly the four owned paths and nothing
   else; the commit contains only those.
5. **The Erlang snippet exercises what Task 2 needs** — `-module`, `-export` with an arity list, `-record`
   with typed and defaulted fields, a function clause constructing a record, and a second clause
   pattern-matching a record in the head. Not a hello-world.

## Verification ledger

| Invariant | Scope | Command | SHA | Result | Timestamp (UTC) |
|---|---|---|---|---|---|
| Both pinned grammars load and produce ERROR-free trees under runtime 0.26.11 | worker-red-green | `RUSTUP_TOOLCHAIN=1.97.1 cargo test -p julie-extractors grammar_smoke` | fd962a50 | PASS — 2 passed, 0 failed | 2026-07-31T19:35:09Z |
| Dependency policy accepts the new deps | worker-red-green | `RUSTUP_TOOLCHAIN=1.97.1 cargo deny check` | fd962a50 | PASS — exit 0; `advisories ok, bans ok, licenses ok, sources ok` | 2026-07-31T19:33Z |
| Assertions are non-vacuous | self-imposed | mutation check: corrupt snippets → both FAIL → restore → PASS | pre-commit | PASS | 2026-07-31T19:32Z |
| No new warnings under warnings-as-errors | self-imposed | `cargo build -p julie-extractors`; `cargo clippy -p julie-extractors --all-targets` | fd962a50 | PASS — 0 warnings | 2026-07-31T19:35Z |

Red state was confirmed before wiring deps: `cargo test … grammar_smoke` failed with
`E0433: cannot find module or crate tree_sitter_erlang` / `tree_sitter_xml`.

`cargo deny check` emits pre-existing `warning[wildcard]` lines for the crate's git/loose-version deps
(razor, powershell, qmljs, sequel, vb-dotnet, and the xtask path dep). Those predate this change, are
warnings not errors, and the run exits 0. The two new deps are exact-pinned and add no wildcard warnings.

Not run (lead-owned scopes): default-wide, golden, capability, certification tiers.

## Concerns for the lead

1. **`tree-sitter-erlang` ships no locals or tags queries** — commented out upstream (`lib.rs:41-43`).
   Only `HIGHLIGHTS_QUERY` and `NODE_TYPES` exist. If Task 4's identifier extraction assumed a tags query,
   that assumption needs revisiting; manual node-kind walking will be required.
2. **XML DTD is a separate grammar, not a fallback.** Task 3 must route `.xml`/`.xsd`/`.wsdl` to
   `LANGUAGE_XML`. If `.dtd` support is ever wanted it is a second `LanguageSpec`, not a variant.
3. **Smoke tests are scheduled for deletion.** Tasks 2/3 must remove `src/tests/grammar_smoke.rs` and its
   `mod.rs` line, or the repo carries a duplicate of coverage the real extractor tests provide.
4. **`.razorback/sdd/` is TRACKED in git, not ignored scratch — and my assigned report path collided
   with an existing tracked file.** `.razorback/sdd/task-1-report.md` was already committed (last touched
   by `44c3e51e chore: prepare breaking producer 2.18.0`) and held the Task 1 report of the unrelated,
   completed `cross-repo-dogfood-repair` plan. I honoured the assigned path and overwrote the working-tree
   copy; the previous content is intact in history via
   `git show 44c3e51e:.razorback/sdd/task-1-report.md`. **Consequence: the worktree now has one
   uncommitted modification to a tracked file I do not own** (`M .razorback/sdd/task-1-report.md`). Per the
   task instruction I did not commit it. The lead should decide whether to commit, restore, or rename it —
   and note that any worker running `git add -A` will sweep it in. Recommend future task briefs use a
   distinct report path (e.g. `task-1-erlang-xml-report.md`) since `.razorback/` is version-controlled.
