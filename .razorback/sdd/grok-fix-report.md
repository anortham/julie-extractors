# Grok third-opinion fix round — Erlang parse-error recovery

Worktree `/Users/murphy/source/julie-extractors/.worktrees/grok-review-fixes`, branch
`grok-review-fixes`, from `4aae042` (clean). Toolchain pinned to `RUSTUP_TOOLCHAIN=1.97.1` on every
cargo invocation; the global default was never touched.

| Finding | Commit |
| --- | --- |
| G1 — phantom symbols from form-shaped lines inside unclosed strings | `d8053db` |
| G2 — silent recovery-budget exhaustion | `436a254` |

---

## G1 — phantom symbols from form-shaped lines inside unclosed strings

### What was actually happening

`recovery::resume_points` excluded offsets strictly inside a literal, but it took the literal map
from `literal_ranges(&primary.root_node())` — a walk of the parse tree whose failure recovery exists
to work around. Dumping the primary tree for the lead's probe file makes the causal chain explicit:

```
PRIMARY: (source_file (module_attribute name: (atom)) (export_attribute …)
          (ERROR (atom) (expr_args) (UNEXPECTED '\0')))
RESUME POINTS: [0, 16, 36, 57, 78, 101]
  57 -> "ghost() -> not_code."
  78 -> "-record(ghost, {id})."
 101 -> "real() -> ok."
RECOVERED: (source_file (fun_decl …) (record_decl …) (fun_decl …))   # ghost, ghost, real
```

The unclosed `"` never becomes a `string` node — it lands under `ERROR` — so the tree-derived map is
empty for it, offsets 57 and 78 pass the filter, `blank_before` erases the opening quote, and the
re-parse mints `ghost/0`, a `ghost` record, and its `id` field out of string text. `real/0` was
produced by that same recovered tree, which is why a naive "reject everything after the open quote"
fix would have dropped it.

### Fix design and why this seam

New module `crates/julie-extractors/src/erlang/lexical.rs`: a tree-independent scan that reads the
source the way the lexer does, **from byte 0**. Starting at the file start is what makes it
authoritative — there is no earlier state it can be wrong about, and the token rules it applies are
the grammar's own, so the scan is as trustworthy as the text. `recovery.rs` now takes both its
resume-point filter and its recovered-declaration filter from that scan; the tree-based
`literal_ranges` / `collect_literal_ranges` / `LINE_SPANNING_LITERAL_KINDS` are deleted rather than
kept alongside, because keeping a demonstrably blind second source of truth is what caused the bug.
The four pre-existing literal tests (comment block, triple-quoted string, multiline quoted atom,
quoted-atom form head) are the evidence that the lexical map covers everything the tree map did —
they were kept unchanged and stayed green.

The filter is applied in two places, per Grok's recommendation:

- `resume_points` — a cut may not land strictly inside a literal.
- `merge_declarations` (via `Recovery::is_literal_text`) — a pass that resumes legally can still
  leave an unresolved error region whose re-parse invents declarations further down inside one.

Scope note: the post-filter is applied to **recovered** declarations only, not to the primary tree's
own children. That matches the finding (the phantoms came from recovery) and keeps clean-file
behaviour — the thing goldens capture — provably untouched.

### `LiteralSpans::contains_strictly`

Only offsets **strictly** inside a literal are rejected, unchanged from before: `'quoted name'(X) ->
X.` is a legal Erlang function, so a literal that *begins* at the offset is that form's own head.
`a_quoted_atom_that_heads_a_form_stays_a_resume_point` guards it.

### Scanner edge cases handled

Verified against the pinned grammar at
`~/.cargo/registry/src/index.crates.io-*/tree-sitter-erlang-0.20.0/`:

| Case | Grammar evidence | Handling |
| --- | --- | --- |
| Line comment | `grammar.js` `comment: token(/%[^\n]*/)` | runs to the newline or EOF; never "unterminated" |
| String | `_sq_string: token(seq(/"/, sq_string_q_base, /"/))`, where `sq_string_q_base = /([^"\\]|\\(…))*/` — `[^"\\]` **admits a raw newline**, which is why an unclosed `"` swallows later lines | scan to the next unescaped `"` |
| Quoted atom | `atom:` second alternative `/'([^'\\]|\\(…))*'/` — also admits newlines | same scan with `'` |
| Escapes | `sq_string_base = /\\([^x\^]|[0-7]{1,3}|x[0-9a-fA-F]{2}|x\{[0-9a-fA-F]+\}|\^.)/` | `\` consumes the next byte, so `\"` and `\\` do not close; `\^` consumes **two** (a `\^"` control escape must not close a string — covered by `a_caret_escape_does_not_close_a_string`) |
| `$` char literal | `char: token(/\$([^\\]|\\([0-7]{1,3}|x[0-9a-fA-F]{2}|x[0-9a-fA-F]+|\^.|\\n|\\\\|.))/)` | `$"` and `$'` do not open a string/atom; `$\\`, `$\n`, `$\x41`, `$\x{1F600}`, `$\101`, `$\^C` consume their full escape |
| Triple-quoted string (OTP 27+) | external scanner `src/scanner.c` `TQ_STRING`: opener is 3+ `"` followed only by whitespace to end of line **and a newline**; closer is the **same count** of `"` preceded only by whitespace from start of line | delimiter count is matched exactly (`a_longer_triple_quote_run_needs_the_same_count_to_close`); a `"""` run that is not alone on its line falls back to ordinary string lexing, matching the scanner returning `false` |
| Sigil strings | `make_verbatim_sigil_string(/~[BS]/)` and `make_quoted_sigil_string(/~[bs]?/)` over the EEP-66 delimiters `() [] {} <> / \| ' " \` #` | recognised so a `"` inside `~S/a"b/` cannot open a phantom string |
| Multibyte content | — | scanning is byte-wise but every decision byte is ASCII and UTF-8 continuation bytes (0x80–0xBF) match no delimiter, so boundaries do not shift (`multibyte_content_does_not_shift_literal_boundaries`) |

### EOF-open literals and quote mis-pairing

A literal left open at end of input is bounded at **the next blank line**, not run to EOF. Two
reasons, and they are the same reason:

1. An unterminated literal only occurs in a file that is already broken. Letting one swallow the
   remainder trades phantom declarations for lost real ones — in the probe, `real/0` sits after the
   unclosed quote and running to EOF would have deleted it along with the ghosts. The blank line is
   the paragraph boundary a reader uses, and it puts the probe's cut exactly where the lead's
   acceptance puts it (ghost lines out, `real/0` in).
2. It bounds the blast radius of a mis-pairing. If the scan ever pairs a quote wrongly on syntax it
   does not model, the damage stops at the next blank line instead of silently suppressing recovery
   for the whole rest of the file. A scan that can only be wrong locally is a scan that can be
   trusted as the single source of truth — which is what let the tree-based map be deleted rather
   than unioned in.

A closed literal keeps its exact extent, blank lines included, so a doc block with a blank line in
it still hides everything between its delimiters.

### Cost on clean files

Unchanged: `recover` still returns before touching the parser when the root has no error, and
`LiteralSpans::scan` runs *after* that check. A clean parse does zero extra work.

---

## G2 — recovery budget exhaustion is silent

### Fix design and why this seam

`recover` now records `exhausted_at: Option<usize>` — set only when the loop completes all
`MAX_RECOVERY_PARSES` passes with errors still unresolved. Every other exit (recovered clean, no
resume point left, parser failure) leaves it `None`, so this cannot fire on a file that finished its
work. The offset recorded is the start of the first still-unresolved error, i.e. where the damage
that was not worked through begins.

`ErlangExtractor::parse_diagnostics()` turns it into a `ParseDiagnostic` spanning that offset to end
of file. The span is built with `NormalizedSpan::from_content_range_with_line_starts` — the same
helper `clause_run_extent` uses — so line/column agree with every other span in the artifact.

Three plumbing points, all forced by the requirement that a *consumer* be able to tell truncated
from complete:

- **`base/types.rs`** — `ParseDiagnostic` gained `message: Option<String>`. The artifact's
  `parse_diagnostics` table already has a `message` column (`schema.rs:410`) and
  `ArtifactParseDiagnostic` already has the field; the extractor-side type was the only link in the
  chain without it, and `map_parse_diagnostics` hard-coded `message: None`. Without it the new
  diagnostic is indistinguishable from an ordinary tree error, which is the exact defect. The field
  is `#[serde(default, skip_serializing_if = "Option::is_none")]`, so no serialized shape changes
  for the diagnostics that do not carry one.
- **`pipeline.rs`** — line 38 did `results.parse_diagnostics = parse_diagnostics_for_tree(&tree)`,
  unconditionally discarding anything an extractor had set. New `with_tree_diagnostics` appends the
  extractor's to the tree's instead. This is the forcing constraint for touching the file: without
  it the erlang diagnostic is constructed and thrown away one call later. The same seam is applied
  to the JSONL record path so the two do not drift.
- **`registry.rs`** — `extract_erlang` passes `ext.parse_diagnostics()` instead of `Vec::new()`.
  Safe by construction: `extract_symbols` runs first in that function and is what populates the
  cached recovery.

### Diagnostic-id collision

`map_parse_diagnostics` derives `diagnostic_id` from `(path, kind, start/end line/column)`. An
extractor diagnostic can share a span with a tree one, which would collide on the table's primary
key. The message is appended to the identity **only when present**, so every existing diagnostic id
is byte-identical to before and only the new kind of row is disambiguated.

### Golden normalizer

`NormalizedParseDiagnostic` gained the same optional, skip-when-none field so goldens capture the
fact rather than silently dropping it. No fixture carries a message, so golden bytes are unchanged —
confirmed by the golden gate passing with zero regenerated files (`git status` clean apart from
source).

---

## Miller / API-shape evidence

Miller MCP calls used against workspace selector `/Users/murphy/source/julie-extractors`:

| Call | What it established |
| --- | --- |
| `inspect target=crates/julie-extractors/src/erlang/recovery.rs depth=overview` | the ten functions in the module and their exact line spans — `recover :59-104`, `resume_points :133-150`, `literal_ranges :168-172`, `collect_literal_ranges :174-191`, `first_error_start :196-210` — which is how the `literal_ranges` → `resume_points` dependency was identified before opening the file |
| `search query="ParseDiagnostic"` | the emission path: definition at `base/types.rs:26`, the two construction sites in `pipeline.rs` (`total_parse_failure_diagnostic:145`, `parse_diagnostic_for_node:209`), the collector `parse_diagnostics_for_tree:174`, the golden normalizer `tests/golden.rs:615`, and the field on `ExtractionResults` at `base/types.rs:513` |

Both were confirmed against the working tree before editing. API shapes verified directly rather than
assumed: `ParseDiagnostic` has **no** `message` field (`base/types.rs:26-34`) while
`ArtifactParseDiagnostic` **does** (`model.rs:695-706`) and the SQL column exists
(`schema.rs:404-419`) — that asymmetry is what made `message` the right carrier rather than a new
`ParseDiagnosticKind` variant, which would have touched the kind mapping, the jsonl contract and the
schema's accepted values.

Grammar rules were read from the pinned crate source, not from memory: `grammar.js` lines 96–97
(escape bases), 1251–1274 (`_sq_string`, sigils, `char`, `atom`, `comment`), and `src/scanner.c`
lines 30–160 (the `TQ_STRING` open/close contract).

---

## Tests: red → green

**G1** — `form_like_lines_inside_an_unclosed_string_do_not_become_symbols` (the lead's probe verbatim),
asserting no `ghost`/`id` symbol, no symbol starting on lines 5–7, and `real/0` present:

```
FAILED: text inside an unclosed string must not become declarations,
        got ["ghost", "ghost", "id", "probe", "real"]
```

after the fix: `ok`.

**G1 control** — `a_form_after_a_closed_string_is_still_recovered` passed before and after, confirming
the guard does not swallow the file tail.

**G2** — `exhausting_the_recovery_budget_is_reported_as_a_diagnostic` (40 broken pairs plus
`tail/0`): red as a compile error, `no field 'message' on type '&ParseDiagnostic'`, then green;
asserts the message names the budget *and* a byte offset, and that `tail/0`'s absence is what the
diagnostic explains. `recovery_within_the_budget_reports_no_budget_diagnostic` guards the other
direction.

15 new `erlang::lexical` unit tests cover each scanner edge case in the table above.

### Live artifact check

Both defects re-run through the real `julie-extract scan` binary, not just the unit harness:

```
=== symbols ===
src/probe.erl|probe|module|1
src/probe.erl|real|function|9          # no ghost/0, no ghost record, no id field
src/budget.erl|budget|module|1
src/budget.erl|f0..f31|function        # 32 recovered, tail/0 absent

=== diagnostics with a message ===
src/budget.erl|error|99|923|erlang recovery budget exhausted after 32 re-parses;
  unresolved parse errors remain from byte 923, so declarations after it may be missing
```

`probe.erl` still reports its 2 ordinary tree diagnostics; `budget.erl` reports 42 (41 tree + 1
budget).

---

## Gate table

Every command prefixed `RUSTUP_TOOLCHAIN=1.97.1`. Exit codes captured directly, not through a pipe.

| Gate | Exit |
| --- | --- |
| `cargo xtask test default` | 0 |
| `cargo xtask test golden` | 0 |
| `cargo xtask test capability` | 0 |
| `cargo xtask test certification` | 0 |
| `cargo xtask test contract` | 0 |
| `cargo xtask test changed <9 touched paths + src/lib.rs>` | 0 |
| `cargo xtask test language erlang` | 0 |
| `cargo xtask test language xml` | 0 |
| `cargo test -p julie-extract-cli --features test-real-world --test erlang_corpus` | 0 |
| `node scripts/language-data-quality-report.mjs --strict` | 0 (`{"languages":38,"cells":728,"silent_cells":0,"quality_bar_debts":0}`) |
| `cargo fmt --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features` | 0 |
| `cargo deny check` | 0 (`advisories ok, bans ok, licenses ok, sources ok`) |

### Byte-identical confirmations

- **Goldens** — no fixture regenerated. `cargo xtask test golden` passed with `git status` showing
  only source files modified; had any clean-file behaviour changed, the fixture comparison would have
  failed rather than needing a refresh.
- **Corpus baseline** — `crates/julie-extract-cli/tests/erlang_corpus.rs` untouched;
  `erlang_corpus_scans_every_file_against_the_committed_baseline` passes with the committed numbers,
  so `telemetry.erl` is still 24 symbols / 45 diagnostics and `telemetry.hrl` still 13 / 2. No file
  gained a budget diagnostic — every corpus file recovers within budget.
  `telemetry_module_exposes_its_module_exports_and_behaviour_edges` still passes: 8/8 exports.
- **Capabilities / coverage** — not regenerated; the capability and certification gates pass as
  committed.

## Files touched

Owned: `erlang/lexical.rs` (new), `erlang/recovery.rs`, `erlang/mod.rs`,
`tests/erlang/parse_errors.rs`.

Beyond the ownership list, each forced by G2's requirement that the diagnostic reach a consumer:
`base/types.rs` (the `message` field the artifact column already existed for), `pipeline.rs` (it
unconditionally overwrote extractor-supplied diagnostics — without this the diagnostic is discarded
one call after construction), `registry.rs` (one-line wiring), `julie-extract-cli/src/extraction.rs`
(map the message through, collision-safe id), `tests/golden.rs` (normalizer field, skip-when-none).

No push, no merge.
