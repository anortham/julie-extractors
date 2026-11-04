---
id: publish-julie-extract-3-12-0-after-language-covera
title: Publish julie-extract 3.12.0 after language coverage
status: active
created: 2026-11-04T16:29:41.404Z
updated: 2026-11-04T16:29:41.404Z
tags: []
---

User explicitly requested: "ok let's merge it to main and create a new release". This authorizes merging the completed task branch, pushing main/tag, publishing the six-platform GitHub release and necessary evidence closeout. No further confirmation is needed for those actions. Prepare v3.12.0 for expanded compatible extraction facts; SQLite8/report3/extractionv4/epoch10 remain. Final implementation is d4ed177b (all original coverage goal criteria proven).

Work remains in /home/murphy/source/julie-extractors/.worktrees/language-coverage-plan on docs/language-coverage-plan until committed preparation is deliberately fast-forwarded into the clean primary main checkout. All14extractor worktrees were clean initially. Main/origin start f9a1b6cf. Version metadata, output-ledger release heading and workflow default are root-owned; Luna owns only v3.12.0 notes and readiness docs.

Release guide requires downstream code-kb dogfood before tagging, then published-binary corpus verification. Code-kb main f290fdc has an unrelated user documentation commit ahead of origin, and toolkit-efficiency worktree571e7fa is unrelated and clean. Leave both untouched. Separate local verification worktree /home/murphy/source/code-kb/.worktrees/julie-3.12.0-dogfood on verify/julie-3.12.0 starts f290fdc; baseline build running with existing3.11 pin. Use a local3.12 extractor plus temporary committed pin changes only there, run exactly one existing Claude/Codex dogfood round, fix confirmed wrong answers before tagging; do not publish a code-kb release.

Performance run41856042545 has now finalized completed/success, updated2026-11-04T16:26:45Z after all steps finished16:05. User's stuck-run question is answered; no cancellation/rerun needed. Preserve the original observation and append terminal confirmation to evidence.

Next: complete release build/preflight/package and affected checks, downstream dogfood, merge clean task prep to primary main, push and wait source CI, tag/publish3.12.0, verify all6public packages and downstream published-binary corpus, update release pointer/evidence as authorized evidence-only closeout. Keep source-state checks before commits/push/tag/release; do not delete old worktrees or overwrite unrelated work.
