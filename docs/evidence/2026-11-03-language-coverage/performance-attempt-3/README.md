# Final controlled comparison

The v3 run is inconclusive: four of six workloads pass. References fresh-scan
spread fails for both binaries, and references update spread fails for the
baseline. Both exhaust their one permitted repeat. No correctness, health,
latency-budget, RSS or artifact-size gate reports a failure.

The campaign ran on ordinary Btrfs at clean `e1d14ef1`, with code identical to
the clean `e1c18a1e` candidate build. It retained all frozen binary/input hashes
and used the reviewed four-second calibration target, two-second sample floor
and unchanged budgets. The full command ran for 2,513.437986 seconds and exited 1.

[Artifacts and checksums](artifacts.json) preserve every final/prior sample and
calibration round, command, source state, build metadata and the
[independent accounting review](final-campaign-review.md.gz).

A quieter Linux reference host is required before another comparison. Do not
rerun on this host, tune thresholds, refresh the baseline, or count this as a
performance pass. The goal remains open.
