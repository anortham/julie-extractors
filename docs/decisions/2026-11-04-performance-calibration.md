# Verify stable calibration before selecting a batch

The first controlled comparison accepted a four-operation baseline calibration
at 2,203 ms. Its later batches took 1,102–2,183 ms, below the required two-second
floor and outside the spread limit. The gate correctly rejected the run. The
[original measurements](../evidence/2026-11-03-language-coverage/performance-attempt-1/artifacts.json)
remain archived.

Accept a fixed operation count only after two consecutive baseline probes at
that count both reach two seconds and satisfy the existing 10%/20 ms spread
limit. Otherwise double the count within the existing 4,096-operation cap.
Keep the final paired-sample floor, noise checks, budgets and single-retry rule.

Comparison summary format v2 records every calibration round, its operation
count, raw probe times and acceptance decision. Baseline receipts, build
sidecars and row allowlists retain input format v1. Product extraction and
report contracts are unchanged.

This adds calibration work while preserving the measured workload and all
acceptance limits. It addresses batch selection rather than treating an
inconclusive measurement as a performance pass.
