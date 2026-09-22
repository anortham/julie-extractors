# Qt release review: 3.1.3 to 3.3.0

Review date: 2026-09-22

Follow-up external review findings and their final dispositions are recorded in
[the Claude Qt/QML review follow-up](./2026-09-22-claude-qt-review.md). This
report remains the 3.3.0 baseline; the follow-up work is prepared for 3.3.1.
The final 3.3.1 corpus and timing comparison is in
[the post-Claude sidecar](./2026-09-22-v3-3-1-post-claude-corpus.json).

## Scope and verdict

This review covers the released ranges `v3.1.3..v3.2.0` and
`v3.2.0..v3.3.0`, with follow-up fixes applied on `review/qt-release-quality`.
The releases materially improve QML and Qt C++ extraction and retain the
SQLite/CLI product boundary. Confirmed findings cover JavaScript directive
spans, QML inline-component ownership and scope, C++ runtime macro call loss,
the Qt macro pre-pass scaling defect, and digit-separator lexical handling; the
review branch fixes these released defects. The ordinary-C++ `signals:`/`slots:`
label correction is verified. The branch also closes a Q_PROPERTY
attribute-value capability gap. None of those fixes is part of either published
release.

The reviewed evidence supports the specific fixture and corpus claims recorded
below. KDE/Kirigami material was tested for Qt C++; Quickshell measurement is
QML-only rather than a whole-project claim. Omarchy was not part of the reviewed
source set.

This is a `julie-extract` release and SQLite-artifact review. It does not verify
code-kb end-to-end query or skeleton behavior, and no code-kb source was
changed. That consumer check is follow-up work, not a release regression.

All execution evidence in this review is from Linux. Parser byte and CRLF
invariants are covered, but no native Windows or macOS runtime execution was
performed. The reviewed changes do not alter paths or file lifecycle behavior,
so this report makes no platform-runtime claim beyond Linux.

Qt's model supports the direction of the QML work: bindings, inline functions,
and imported JavaScript run in JavaScript scope; each QML component has a
logical scope; and imports are visible in nested inline components. [Qt QML
scope and naming resolution](https://doc.qt.io/qt-6/qtqml-documents-scope.html)
documents those semantics.

## `v3.1.3..v3.2.0`: QML and QML JavaScript

### Benefit

- QML now distinguishes root, inline-component, and nested-object symbols;
  property bindings publish structural facts; and qualified type usage records
  the terminal name with receiver metadata. These refine existing QML rows.
- `qmldir` component rows now use `export` rather than `class`, preserving their
  module-export meaning.
- QML JavaScript `.pragma` and `.import` directives are blanked at their
  original byte positions before JavaScript parsing. `.pragma` becomes a
  structural fact and `.import` becomes an import symbol, so ordinary JavaScript
  after the directives continues to receive normal extraction.
- The public syntax API and the extraction pipeline use the same preprocessing
  route. The review added an indented C++ namespace header case to the source
  detection contract; it passes, confirming this shared route did not alter
  `.h` selection in that case.

### Review follow-up: inline-component scope and ownership

**Severity:** high. **Classification:** release regression in QML relationship
ownership/scope.

Same-line inline components could receive incorrect ownership. Calls could also
resolve across an inline-component boundary or ignore a local
or parameter shadow. The review branch filters the existing
`ContainingSymbolIndex` for component ownership; resolved and pending calls now
respect own/ancestor component scope and local/parameter shadowing. A known but
inaccessible target becomes a structured pending edge, including handler calls,
rather than a false resolved relationship.

Evidence is in `qml/relationships.rs`, `qml/mod.rs`,
`tests/qml/relationships.rs`, and `tests/qml/cross_file_pending.rs`. The focused
relationship suite passed 28 tests and the canonical pending suite passed 3.
The final fixes include pending handler calls and exact same-line function
ownership by `start_byte`. The intentional limit is that only own/ancestor
component symbols are locally visible; no broader QML sweep has run yet.

### Confirmed regression: directive spans included comments. Fixed.

**Severity:** medium. **Classification:** release regression.

`javascript::qml_directives::scan` blanked a trailing `//` comment but recorded
the source span through the end of the original line. Consequently a `.pragma`
fact and `.import` symbol covered comment text and acquired an unstable/wrong
location identity for the declared construct.

The new regression test
`tests::javascript::qml_directives::directive_spans_exclude_trailing_comments`
first failed for `.pragma library // shared` with end byte `25` instead of
`15`. The fix records `directive_text.len()` as the end rather than the
pre-comment line length. `qml_directives` then passed 16 focused tests, and the
CLI directive check integration test passed 2 tests.

### Quality boundary

The directive scanner deliberately recognizes directives only before the first
JavaScript statement. This preserves ordinary JavaScript parsing outside the
QML directive position. Quoted imports containing `//` are covered.

## `v3.2.0..v3.3.0`: Qt C++

### Benefit

- The C++ pre-pass handles Qt declarations the grammar cannot model directly,
  enabling extraction from Qt headers with macro-decorated classes.
- `Q_PROPERTY` now produces property facts with accessors and qualifiers;
  review follow-up preserves `DESIGNABLE`, `SCRIPTABLE`, `STORED`, `USER`, and
  `REVISION` values rather than merely recognizing their boundaries. The five
  optional string metadata keys are declared in the structural-fact registry and
  generated JSON contract.
- Class/member declarations retain more accurate return types, visibility,
  constructor/destructor treatment, and Qt-specific symbols. Fixture coverage
  includes a Kirigami-style header.

### Confirmed regression: runtime macros hid nested calls. Fixed.

**Severity:** high. **Classification:** release regression.

The broad `Q_`/`QT_`/`QML_` macro scanner blanked runtime expressions such as
`Q_ASSERT(helper())`. This removed the nested call from the parse tree and lost
its identifier row. The pre-pass now uses an explicit declaration-syntax
allowlist; the regression is covered by
`cpp::qt_macros::runtime_qt_expression_macros_keep_nested_calls`.

### Confirmed regression: long single-line macro scan had a quadratic rescan. Fixed.

**Severity:** high for generated or minified headers. **Classification:**
release regression.

Final interleaved diagnosis on a 169 KB single-line Qt class measured 3.46 s
p95 for released v3.3.0 and 2.24 s p95 for the review branch at 10,000 members.
The cause was `qt_macros::line_prefix` rescanning from byte zero for each
candidate. The review fix reuses existing line starts and removes that new
quadratic rescan. End-to-end single-line extraction still scales poorly (about
0.7 s at 5,000 members and 2.4 s at 10,000); this does not establish linear
total extraction scaling. Raw diagnosis samples are retained under
`target/qt-release-review/results/scaling-*.txt`.

Historical full-scan timing is recorded in
`docs/reviews/qt-release-performance.json`: one warm scan followed by five
fresh-artifact, single-worker scans per released binary, with p95 as the
nearest-rank maximum. The workloads and p95 seconds were:

| Workload | v3.1.3 | v3.2.0 | v3.3.0 |
| --- | ---: | ---: | ---: |
| KDE Kirigami QML, 225 files | 1.20 | 1.14 | 1.22 |
| Quickshell QML, 26 files | 0.17 | 0.19 | 0.17 |
| KDE Kirigami Qt C++, 102 files | 0.94 | 0.97 | 0.77 |
| Ordinary C++ fixture copies | 0.31 | 0.32 | 0.31 |
| Workspace Rust, 1,170 files | 15.48 | 15.57 | 15.47 |
| Ordinary JavaScript fixture copies | 0.15 | 0.15 | 0.15 |
| JavaScript QML-directive fixture copies | 0.17 | 0.14 | 0.14 |

Wall time includes process startup, parsing, extraction, and SQLite artifact
creation. The ordinary C++, JavaScript, and directive workloads are repeated
committed fixtures, not third-party corpus evidence. The v3.3.0 Kirigami QML
p95 is conservative because one sample measured 1.22 s; the other four were
1.12 to 1.14 s.

The final review-branch binary measured 1.25 s p95 for Kirigami QML, 0.17 s for
Quickshell QML, and 0.86 s for Kirigami Qt C++, compared with released v3.3.0
at 1.22 s, 0.17 s, and 0.77 s. The review-branch Kirigami C++ scan covered 102
files with zero failures and 31 parse diagnostics, matching v3.3.0's diagnostic
count. These are honest measured quality/performance tradeoffs; they do not
isolate a causal change among the quality fixes. A final candidate optimization
measured 0.87 s rather than 0.86 s and was reverted.

### Additional C++ correctness follow-up

**Severity:** medium. **Classification:** release regression in the new macro
scanner.

Digit-separator apostrophes could be consumed as character literals during the
macro scan, hiding later macros. A shared numeric-token skip now protects both
the scan and parenthesis matcher; the regression is covered by
`cpp::qt_macros::character_literals_and_digit_separators_do_not_hide_later_macros`.

### Confirmed regression: ordinary C++ labels were rewritten. Fixed.

**Severity:** high. **Classification:** release regression.

The released scanner rewrote valid function labels named `signals:` or `slots:`
outside a class body. The correction tracks class-body brace depth and
treats lowercase section aliases only within a class, struct, or union body;
`Q_SIGNALS` and `Q_SLOTS` remain syntax macros wherever they occur. Tests prove
that ordinary labels yield no scan site, `blank_macros` returns
`None`, raw C++ parsing has no errors, and canonical full extraction has no
parse diagnostics.

Targeted cases cover method-body labels and prefix-empty class detection without
regressing namespace/template headers. The focused `cpp::qt_macros` suite passed
44 tests, `cpp::qt_symbols` passed 28, and scoped C++ golden verification
passed. Line-leading `emit` remains a token-scanner ambiguity, and a macro after
other code on the same line is intentionally not treated as a declaration macro.

## Shared contracts and cross-language effects

`map_identifiers` now merges optional extractor metadata into artifact
`metadata_json`, with extractor-supplied keys overriding mapper-derived receiver
metadata. This is a reusable artifact channel, not QML-specific schema logic.
QML's qualified type usage records the terminal token and qualifier, and its
relationship ownership reuses the existing `ContainingSymbolIndex`; these keep
reference sites precise without a workspace-global resolver.

The compatibility gate remains a review limitation. Its compatible-version
ledger declaration accepts a version-level ledger entry; it does **not** map
each observed database-row difference to an allowlisted ledger record. The
release notes and release evidence now describe this accurately. Likewise,
`silent_cells=0` and `quality_bar_debts=0` from the strict language data-quality
report prove recorded fixture coverage, not semantic completeness across real
projects.

## Architecture assessment

The implementation stays within the existing product design. Private language
extractors and the shared preprocessing entry point feed the canonical
extraction path; QML reuses the existing containment index and pending-edge
factory. The CLI and SQLite artifact remain the primary contract, exercised
through canonical extraction, syntax API, golden, and CLI integration tests.
Identifier metadata is optional, with extractor values winning mapper-derived
receiver metadata. No schema migration, dependency, workspace resolver, or new
cross-language abstraction was introduced.

The principal residual design risk is lexical context in the Qt macro scanner.
The review addresses it with real-corpus diagnostics and ordinary-C++ negative
tests rather than an additional abstraction layer. A formal redesign is not
warranted by the reviewed evidence.

## Ranked follow-up work

1. **Make `xtask compat-check` diff-aware.** Require every observed row/field
   difference to match a named entry in `extraction-output-changes.md`, then
   report unmatched differences as failures. This turns the current
   version-level declaration into evidence for the release-note claim.
2. **Transfer scope-ownership fixtures to Lua caller ownership parity.** Replace
   `lua::relationships::find_enclosing_function`'s name-map lookup with the
   existing interval `ContainingSymbolIndex` already used by Lua identifiers.
   Add duplicated local-function-name and nested-scope relationship fixtures.
   GDScript's existing `ContainingSymbolIndex` plus `ScopedSymbolIndex` path is
   the reference, not a migration target.
3. **Publish qualified-reference metadata where the token model exists.** Build
   on C#'s existing `terminal_type_identifier` for qualified names and add
   qualifier metadata only for consumers that need it. This follows QML's
   terminal-token discipline without introducing a generic resolver.
4. **Add ordinary-language negative controls for preprocessing.** Exercise
   `preprocess::blanked_source` for C++ and JavaScript with byte length, CRLF,
   UTF-8, directive/macro spans, and unchanged ordinary syntax. Both
   `parse_source_with_options` and `parse_for_language` consume this function.
5. **Complete the current Qt macro performance gate.** Keep the one-line header
   workload and add an explicit budget based on the fixed measurement; retain a
   separate real-world corpus gate outside the default suite. Do not add a new
   scanner abstraction.
6. **Run a code-kb consumer check.** Verify the new Qt `property`, `event`,
   `field`, and `export` kinds plus their metadata survive code-kb queries and
   skeleton output. Keep it separate from extractor release compatibility.

## Verification status

- Passed during review: `cargo test -p julie-extractors qml_directives` (16),
  `cargo test -p julie-extract-cli --test qml_javascript_directives` (2), and
  `cargo test -p julie-extractors --features syntax-api --test
  syntax_api_contract` (8).
- C++ worker result: `cpp::qt_macros` (44), `cpp::qt_symbols` (28), and scoped
  C++ golden verification passed.
- QML worker result: focused relationships (28), canonical pending
  relationships (3), regenerated QML golden fixtures, strict quality
  (`silent_cells=0`, `quality_bar_debts=0`), and `git diff --check` passed.
- Lead default gate passed in 13 seconds on the final tree; formatting and diff
  checks were clean. `cargo xtask test default` passed 4,532 tests with zero
  failures and seven ignored across 30 test and documentation targets.
- `cargo xtask test contract` passed 187 tests with zero failures across 12
  targets, covering all-language golden fixtures, the registry, capabilities,
  pending shapes, artifact/CLI contracts, and reference identities.
- Structural-fact registry conformance passed with `test-golden`; its JSON sync
  test passed once while regenerating the checked-in contract and once without
  regeneration. The syntax API contract passed 8 tests.
- The strict language report covered 40 languages with `silent_cells=0` and
  `quality_bar_debts=0`. Its 47 `open_gap_backlog` entries are pre-existing, so
  this is not a claim of complete gap closure.
- Final command logs are retained under `target/qt-release-review/*-final.log`.
  Final paired quiet performance evidence is recorded in
  `docs/reviews/qt-release-performance.json`.

## Review branch state

All task changes are an unreleased working-tree patch on
`review/qt-release-quality` at `803948ca`. The existing feature worktrees were
clean at final state review.
