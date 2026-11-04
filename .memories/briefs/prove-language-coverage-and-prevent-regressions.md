---
id: prove-language-coverage-and-prevent-regressions
title: Prove language coverage and prevent regressions
status: active
created: 2026-11-04T00:15:04.316Z
updated: 2026-11-04T12:31:35.648Z
tags: []
---

Execute docs/plans/2026-11-03-language-coverage-goal.md to full acceptance. Local implementation is complete: 42 languages across 15 domains, 630 cells, 47 fixed extraction defects, 465 passing goldens, 11 retained consumer/context boundaries, a pinned CLI corpus and exact row allowances. Linux branch, certification, real-world, rebind and Windows gates pass. Final warm default median is 45.434524 seconds; writer batching passes.

The reviewed v3 harness uses a fixed four-second calibration target, unchanged two-second floor and budgets, and one retry that cannot reduce batch count. The final controlled campaign at clean e1d14ef1 passes four of six workloads. References fresh and update timings remain inconclusive after their repeats; correctness, health, latency-budget, RSS and size checks report no failures. The NOCOW experiment failed its adoption criterion and is rejected.

The goal remains open. Access to a quieter Linux reference host is required, using frozen baseline 79eb21fa, candidate c236e361 and unchanged inputs/budgets. Do not rerun or tune on this host or refresh the baseline. Preserve all three inconclusive campaigns. Work stays in .worktrees/language-coverage-plan on docs/language-coverage-plan; code matches the clean e1c18a1e build. The final evidence ledger records every gate and blocker. No resolver, context guessing, release, push or code-kb changes.
