# Claude Qt/QML review follow-up

Review date: 2026-09-22

## Scope

This records an external Claude review of the unreleased Qt/QML review patch on
`review/qt-release-quality`. The review inspected a pre-fix snapshot and did
not rerun after the fixes described below. It is evidence for triage, not a
claim that Claude approved the final tree.

The user expressly requested this Claude review. No external-model policy was
declared. A redacted diff and evidence bundle was sent to Anthropic under that
authorization through the local review workflow:
[`claude-cli` skill](</home/murphy/.codex/plugins/cache/razorback/razorback/0.44.3/skills/claude-cli/SKILL.md>).

The normalized result is preserved at
[`claude-qt-review.json`](./claude-qt-review.json). It contains seven findings:
three high, two medium, and two low.

## Findings and disposition

| Severity | Finding | Lead disposition | Status |
| --- | --- | --- | --- |
| High | `Q_NULLPTR` could be blanked in ordinary expressions, producing parse errors. | Agreed. | Fixed. It is excluded from preprocessing; C++ macro tests passed 47 cases. |
| High | QML `parent`/`this` binding lookup could select a component rather than the exact nested object. | Agreed. | Fixed. QML relationships passed 30 focused tests; golden and strict quality passed. |
| High | Narrowed Qt macro recognition could drop declaration-position macros that the released scanner handled. | Agreed. | Fixed. Released broad declaration recognition was restored; C++ macro tests passed 47 cases. |
| Medium | Visible artifact changes lacked a version and output-change ledger entry. | Agreed. | Addressed by the unreleased 3.3.1 preparation in this patch. |
| Medium | Same-line inline-component `extends` attribution still used line-level matching. | Agreed. | Fixed with byte-exact ownership; covered by the same QML verification. |
| Low | Qt scanner comments/dead branch and the C++ `goto`-label limit were stale. | Agreed. | Fixed. C++ scanner cleanup and documentation correction landed. |
| Low | QML resolver scans might add per-call linear work. | Plausible hypothesis, not source-proven as the measured cost. | Do not apply blindly; profile before changing. |

Six correctness, contract, and documentation findings were addressed. The low
performance proposal remains un-applied because the measured corpus cost does
not attribute a cause to that hypothesis.

The accepted corrections reuse existing mechanisms: declaration-context macro
handling, exact source-byte ownership, `ContainingSymbolIndex`, and structured
pending relationships. They do not add a schema, dependency, global resolver,
or new abstraction.

The C++ correction preserves `Q_NULLPTR` as original source, rather than
rewriting it, so spans, literals, and type facts remain exact. It restores broad
all-caps declaration recognition while preserving runtime calls only for
`Q_ASSERT`, `Q_ASSERT_X`, `Q_CHECK_PTR`, `Q_ASSUME`, `Q_LIKELY`, `Q_UNLIKELY`,
`Q_UNREACHABLE`, and the `Q_UNUSED` wrapper. Typed `Q_D`, `Q_Q`, and `Q_FOREACH`
remain declaration-preprocessed because retaining their raw typed forms produced
diagnostics. This does not implement Qt macro expansion or typed-loop facts.
The C++ symbols suite passed 28 cases and scoped C++ golden verification passed.
Corpus verification found Kirigami at 102 files, zero failures, and 31
diagnostics, equal to v3.3.0. Plasma-workspace had 1,050 files, zero failures,
and 297 diagnostics versus 298 in v3.3.0 after removing one `Q_DECL_EXPORT`
diagnostic. Restored `Q_PRIVATE_SLOT`, `Q_OBJECT_BINDABLE_PROPERTY`, `Q_ENUMS`,
and `Q_LOGGING_CATEGORY` locations were diagnostic-free.

## Release-readiness result

The version/ledger finding is a release-readiness gap, not an already published
3.3.0 defect. The 3.3.1 preparation adds a compatible ledger declaration and
requires artifact rebuilds because canonical rows change. Published 3.3.0
release pointers and release evidence remain historical records.

The earlier Qt release review and performance dataset are the 3.3.0 baselines:

- [`3.3.0 review baseline`](./2026-09-22-qt-release-review.md)
- [`3.3.0 performance baseline`](./qt-release-performance.json)

The current review report links here. Lead verification, rather than an
external re-review, verifies the accepted corrections.

## Final verification

Lead verification on the frozen 3.3.1 tree passed formatting and diff checks,
the default tier (4,537 passed, zero failed, seven ignored across 30 targets),
the contract tier (187 passed, zero failed across 12 targets), the syntax API
contract (8 passed), and the xtask tier (119 passed, zero failed across 10
targets). The strict language report covered 40 languages with
`silent_cells=0` and `quality_bar_debts=0`; its 47 open-gap backlog entries
pre-date this patch. Logs are retained under
`target/qt-release-review/claude-review/*-final.log`.

Final quiet corpus evidence is in
[`2026-09-22-v3-3-1-post-claude-corpus.json`](./2026-09-22-v3-3-1-post-claude-corpus.json).
It uses one warm scan and five quiet-machine full-scan samples per release,
with a fresh SQLite artifact and one worker. Quality scans found zero failures;
QML diagnostic spans match v3.3.0, Kirigami C++ diagnostics remain 31, and
Plasma C++ diagnostics fall from 298 to 297. The aggregate p95 cost rises from
7.62 s to 8.68 s for Plasma C++, 0.77 s to 0.88 s for Kirigami C++, 1.22 s to
1.25 s for Kirigami QML, and 0.17 s to 0.18 s for Quickshell QML. Those scans
do not isolate a causal repair. The adversarial 169 KB single-line Qt class
improves from 3.73 s to 2.21 s p95 through the line-start-index correction.
