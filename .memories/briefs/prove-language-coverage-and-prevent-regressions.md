---
id: prove-language-coverage-and-prevent-regressions
title: Prove language coverage and prevent regressions
status: active
created: 2026-11-04T00:15:04.316Z
updated: 2026-11-04T15:08:29.351Z
tags: []
---

Finish docs/plans/2026-11-03-language-coverage-goal.md without lowering its acceptance criteria. Local extraction implementation is complete: 42 languages, 630 cells, 47 fixed defects, 465 goldens and 11 retained boundaries. Linux and Windows gates pass; warm default median is 45.434524 seconds. Three local controlled campaigns remain inconclusive; no further local timing run is permitted.

The resumed goal prepared a concrete GitHub Actions route. It builds immutable baseline f9a1b6cf and candidate e1c18a1e with Rust 1.97.1/release/one Cargo job, records truthful new hashes, and proves every rebuilt baseline snapshot against the unchanged frozen receipt before timing. The original semantic baseline, manifest, allowance, budgets and one-repeat cap stay fixed. Untimed rehearsal reproduced both frozen binary hashes, passed all 42 corpus rows plus controls and all ten baseline snapshots. Nineteen runner contracts, 33 existing comparator contracts, actionlint and a mutation check pass. Harness pin is 7a31fa57.

The CI approval package is docs/evidence/2026-11-03-language-coverage/ci-reference-host/README.md with exact dispatch inputs. Explicit approval is required before pushing docs/language-coverage-plan and dispatching one CI comparison with new reference-host build identities. No push/dispatch or new timing has occurred. Keep the goal active and incomplete until the approved controlled comparison passes all six workloads. A supplied quiet Linux host remains an alternative. Work stays in the current task worktree; no release, resolver, context guessing or code-kb changes.
