---
id: prove-language-coverage-and-prevent-regressions
title: Prove language coverage and prevent regressions
status: completed
created: 2026-11-04T00:15:04.316Z
updated: 2026-11-04T16:18:35.648Z
tags: []
---

Completed docs/plans/2026-11-03-language-coverage-goal.md without lowering acceptance criteria. Evidence covers 42 languages, 630 cells, 47 fixed extraction defects, 465 goldens, and 11 retained consumer/context boundaries. Linux, native Windows, certification, real-world, incremental-equivalence and writer gates pass on unchanged relevant source. Clean e2d1869c warm default median is 45.434524 seconds. Final code is 7a31fa57; product Rust, grammars, fixtures and xtask are unchanged from e1c18a1e.

The user-approved CI comparison, run 41856042545, passes all six workloads under unchanged budgets. References fresh scans used the single permitted repeat; both attempts remain archived. Lead and independent review verify all sample floors, spreads, resource limits, 3,090 exact row allowances, ten frozen baseline snapshots, four byte-identical semantic dumps and truthful build provenance. Median latency changes range from -1.89% to +0.99%. Actual CI binaries are baseline 3eba4cf4 and candidate 525dd9f2; no semantic baseline refresh or override was used.

Durable evidence is in docs/evidence/2026-11-03-language-coverage/performance-ci-41856042545. All 735 downloaded files are inventoried; 733 non-executable members are archived with checksums, including SQLite artifacts, canonical rows, receipts, build records and logs. Original ZIP digest is de743175637ee7cad4f016ed6ac60775130248f672d875e7d3e2811fedf41c43. All 239 prior indexed evidence files also match their checksums.

The actual comparison exited 0, upload succeeded, and every job step completed successfully. GitHub aggregate API still reported in_progress at 2026-11-04T16:15:26Z. This discrepancy is recorded; overall workflow success is not claimed. Review confirmed that terminal REST status is not an original acceptance criterion. Do not cancel or rerun the accepted comparison. Final scoped documentation, evidence and memory are committed on docs/language-coverage-plan and pushed under the user's approval. No merge or release is required.
