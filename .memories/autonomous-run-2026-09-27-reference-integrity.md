# Reference integrity and extraction coverage — completion report

Status: complete. Plan: `docs/plans/2026-09-27-reference-integrity.md`.
Branch: `fix/reference-integrity`; implementation: `14f4510cdb0cb55ab5d71929c92f7d433ff5094c`.
All three approved work areas and final verification are complete. No end-to-end duration was measured.

## What changed and why

- Local call targets require visible bindings and correct receiver ownership. Shadowed, overloaded or unproven targets retain pending occurrences instead of false concrete edges.
- Direct incremental scans match fresh artifacts through additions, edits, malformed-to-valid recovery, renames, deletes and repeated no-op scans. No scan/writer defect was found.
- Ten of the 21 original capability gaps close, with 26 new fixture groups. Eleven gaps retain explicit context or consumer-join requirements. A separately discovered PCRE2 branch-reset gap has a named follow-up.
- Existing fact tables carry the added framework/testing data. The extraction contract changed; existing consumer artifacts require forced re-extraction after upgrading the extractor.

## Decisions and review

Lead reviewed each delegated source change, positive/negative tests, generated facts and contracts. A read-only Astra plan review preceded implementation. No external CLI review campaign was requested or run; external invocations: 0. Security scopes: none declared. No actionable review finding remains within the approved scope.

Unknown inheritance targets remain pending. Route declarations and mount sites remain separate to support multiple mounts. Benchmarks and body checks retain their own meanings instead of becoming test cases. Regex uses one licensed patched tree-sitter grammar, with provenance and generated sources included.

## Verification

- Linux default: 6,804 passed, zero failed, seven intentionally ignored; 29 seconds.
- Normal golden tier: eight checks passed. All 26 new fixture groups have zero parse diagnostics; no new duplicate pending rows, resolved/pending overlaps or dangling structural-fact owners.
- Strict quality: 42 languages, zero silent cells, zero quality-bar debts, 12 explicit gaps.
- Native Windows default on the implementation SHA: passed, 190 seconds including rebuild. Incremental-equivalence target: four passed in 6.56 seconds. Guest checkout is clean.
- Formatting, whitespace checks and grammar-freshness script tests passed. Final documentation/memory changes preserve the verified source tree.

The first Windows attempt failed before tests with LNK1180, insufficient disk. Package-only `cargo clean -p julie-extractors` removed 38.8 GiB of rebuildable cache; the same source then passed. No unresolved blocker remains.

## Source control and authority

The user's implementation approval and approved plan authorize local commits and integration. Push, PR creation and release are outside this task; none occurred. Main was fast-forwarded to the verified implementation. Final evidence and brief completion follow in a documentation commit.

The baseline-to-implementation diff covers 232 files (64,691 added and 2,757 removed lines), including generated parser code and golden output. Earlier `fix-data-correctness` and `refactor-receiver-facts` worktrees are clean and already merged. All worktrees remain in place; no unaccounted changes or stranded source commits were found.

## Remaining work outside this slice

The plan names closure tasks for the eleven context/composition gaps and the branch-reset grammar gap. This session does not modify code-kb. Consumers must upgrade and re-extract to use the corrected facts.
