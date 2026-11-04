# Verify stable calibration with a fixed timing margin

The first controlled comparison accepted a four-operation baseline calibration
at 2,203 ms. Its later batches took 1,102–2,183 ms, below the required two-second
floor and outside the spread limit. The gate rejected the run. The
[original measurements](../evidence/2026-11-03-language-coverage/performance-attempt-1/artifacts.json)
remain archived.

The second comparison required two stable probes at or above two seconds.
Five workloads passed. References single-file-update remained inconclusive
after its one repeat: accepted probes took 2,064 and 2,065 ms, but measured
baseline batches took 1,772–2,522 ms. Its
[raw result](../evidence/2026-11-03-language-coverage/performance-attempt-2/artifacts.json)
is retained with the repeat exhausted.

Separate calibration setup from acceptance. Require two consecutive baseline
probes at the same count to reach a fixed four-second target and satisfy the
existing 10%/20 ms spread limit. Otherwise double the count within the existing
4,096-operation cap. Freeze that count for both binaries throughout each
five-pair attempt. The single permitted retry starts at the previous count;
it cannot reduce the batch size.

Four seconds supplies a fixed margin over the two-second acceptance floor.
It applies uniformly, without using candidate speed or tuning counts between
samples. Keep the final floor, spread checks, latency/memory/size budgets,
correctness checks and single-retry cap unchanged. Longer batches do not
establish that host noise is solved. If the newly identified validation run
remains noisy after its allowed repeat, require an evidenced reduction in
host/storage contention or a quieter reference host before another run.

Development comparison summary format v3 records the calibration target, every
round's count and raw probes, and the nondecreasing retry policy. Baseline
receipts, build sidecars and row allowlists retain input format v1. Product
extraction and report contracts are unchanged.
