# Annotations at the references level and variable write sites (julie 3.11.0)

code-kb asked for two facts on 2026-10-31; its plan is
`~/source/code-kb/docs/plans/2026-10-31-one-call-answers.md`, task 5. Both change
extraction output, so each needs a marker in `EXTRACTION_CONTRACT_VERSION`, an entry in
`src/tests/api_surface.rs`, and a `## 3.11.0` section in
`docs/contracts/extraction-output-changes.md` before the merge. Publishing needs the
user's approval.

## Why

- Annotations: every extractor already builds `AnnotationMarker` rows
  (`normalize_annotations` in `base/annotations.rs`, called from about 30 language
  files), and `strip_to_level` in `base/results_normalization.rs` clears them for every
  level but `full`. code-kb builds at `references` and so never sees them. code-kb
  wants them for a "conventions of the siblings" section: the two native failures in
  its 2026-10-27 edit study were a missing C# `[Pure]` attribute that every sibling
  method carried.
- Write sites: `variable_ref` identifiers carry no read-or-write fact
  (`metadata_json` is empty on all 93,880 rows of the code-kb index). "Who writes this
  field" is the data-flow question a coding agent asks most and the one `grep` answers
  worst. The only write fact today is the R superassignment row of 3.10.0.

## Task A: `annotations-at-references-v1`

Owner files: `base/types.rs` (`ExtractionLevel`), `base/results_normalization.rs`,
`registry.rs:1055`, `docs/contracts/cli.md` (the `--level` paragraph),
`docs/architecture/` level page if one exists, each `docs/languages/<lang>.md` that
states what `references` drops.

- Split `includes_complexity_and_annotations` into `includes_annotations` (`References`
  and `Full`) and `includes_complexity_metrics` (`Full`). `strip_to_level` clears
  annotations only when `!includes_annotations()`. `complexity_metrics` stays `full`
  only.
- `cli.md`: `references` is `facts` plus call and variable-reference identifiers plus
  `symbol_annotations`; `complexity_metrics` stays `full` only. Record the size effect
  on the hermes-agent tree (the 3181 MB figure moves) and on this repository.
- Ledger classification: `compatible`. A 3.10.0 reader of a `references` artifact sees
  rows in a table it already knows, with the same shape. The capability snapshot
  fingerprint changes, so an existing `references` artifact is refreshed with
  `scan --force` before incremental use; say so, as the 3.10.0 entry does.
- Tests: the level tests that assert empty `symbol_annotations` at `references` flip;
  one test per level that annotations are present at `references` and `full` and
  absent at `symbols` and `facts`; the contract test on `cli.md`.

## Task B: `variable-write-sites-v1`

Owner files: the identifier module of each language in scope (`<lang>/identifiers.rs`
or where the language calls `create_identifier` with `IdentifierKind::VariableRef`),
`base/creation_methods.rs` if a shared helper is added, `docs/languages/<lang>.md`,
`docs/contracts/extracted-data-v4.md` (a new "Variable access facts" section; the
contract stays v4 because the key is additive).

Fact: `identifiers.metadata_json.access` on a `variable_ref` identifier, one of:

| Value | Meaning |
| --- | --- |
| `write` | the name is the target of an assignment, a declaration with an initializer, a destructuring target, an increment or decrement, or an `out`/`ref`/`&mut` argument where the language marks one |
| `read_write` | a compound assignment (`x += 1`, `x ||= y`, Swift `inout`) |
| `read` | every other occurrence the extractor classified |
| absent | the extractor did not classify the site |

Rules:

- The fact describes the written syntax, not data flow: `a.b = 1` writes the member
  access `b`, and `a` is a read. A `member_access` identifier gets the same key when it
  is the assignment target. `call` and `type_usage` identifiers never carry it.
- A declaration (`let x = 1`, `int x = 1`, `x := 1`) is a write only when the language
  also emits a `variable_ref` identifier for it; do not add identifiers for
  declarations that have a `symbols` row.
- Pattern bindings (`match`, `case`, `for x in`, `catch e`) are `write`.
- A language that cannot tell (`Bash` word splitting, a dynamic language's `eval`) leaves
  the key absent. Absent is never `read`.
- One shared helper in `base/` takes the identifier node and returns the value from
  the parent node kinds the language names, so each language adds a table of node
  kinds and field names, not a walk.

Languages, two waves. Wave 1 is the set the code-kb reach gate and edit study cover:
Python, Rust, Go, TypeScript, TSX, JavaScript, Java, Kotlin, C#, C, C++, Ruby, PHP,
Swift. Wave 2 is every other language with a `variable_ref` identifier; a language with
no assignment construct (JSON, YAML, TOML, Markdown, CSS) is not applicable and its
language page says so.

- Ledger classification: `compatible`. A new optional key.
- Tests: for each wave-1 language, one test with a declaration, a plain assignment, a
  compound assignment, a member write, a read in a call argument, and a pattern
  binding, asserting the value on each identifier; the golden fixtures under
  `fixtures/extraction` are refreshed and the diff reviewed row by row; the capability
  snapshot adds `identifiers.access` to `supported` for each covered language.

## Parallel execution

Base: `main` at 44cd0e92 or later. Worktrees under `.worktrees/<task>`, branches
`feat/annotations-level` and `feat/write-sites-<wave>`. Task A is one session. Task B
wave 1 can split across two workers by language (JVM and .NET; the rest); wave 2
follows in one session. Workers do not edit `EXTRACTION_CONTRACT_VERSION`, the marker
list, the ledger, the version, or release notes; the lead adds them at integration.

## Integration and release (lead)

1. Cherry-pick onto `release/3.11.0`; add markers `annotations-at-references-v1` and
   `variable-write-sites-v1`, the `## 3.11.0` ledger section, release notes
   `docs/release-notes/v3.11.0.md`, version 3.11.0.
2. `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --all-features`,
   `cargo test -p xtask`, `cargo xtask test default`, `cargo xtask test contract`,
   `cargo xtask compat-check` against 3.10.0.
3. Build the release binary and copy it into the code-kb worktree `.tools/` for its
   dogfood round (code-kb plan task 5).
4. User approval: push and release 3.11.0. Then code-kb pins it.

## Effort

For an agent: task A one session; task B wave 1 two parallel sessions plus one
integration session; wave 2 one session; release one session. Human time: the fixture
diff review and the release approval.

## Execution record

Integration is in `.worktrees/release-3.11.0` on `release/3.11.0`. Independent
workers share this worktree with separate file ownership; the lead owns commits.

- [x] Task A implementation and focused verification (14 level tests, SQLite persistence, CLI help).
- [x] References artifact size measurements on this repository and Hermes.
- [x] Task B wave 1, shared classifier, and per-language tests.
- [x] Task B wave 2 and per-language documentation.
- [x] Golden fixture refresh, row review, and strict capability report.
- [x] Version/contract integration and all verification gates.
- [x] Release binary and code-kb dogfood copy.
- [ ] User approval for push and release.

All required gates passed. The default suite took 76 seconds. Compatibility
against 3.10.0 produced the declared `compatible` notice. The strict quality
report has zero silent cells and zero quality-bar debts.

The final row audit preserves existing identifier kinds and non-access facts.
It covers 274 changed existing fixtures, 14 new fixtures, and 3,692 access rows.
Review regressions cover duplicate member/call ownership, C# conditional
receivers, Swift discards, Erlang record labels, QML Behavior references, and
Vue call promotion. The GDScript receiver integration test now selects the
intended member kind while separately asserting its new unqualified write.

Storage measurements and binary provenance are recorded in
[release evidence](../release-evidence/v3.11.0-access-facts.md).
