# Controlled CI performance comparison

The six-workload comparison passes without budget overrides. The user approved
this reference-host build route before branch push and dispatch. The three
inconclusive local campaigns remain historical evidence; none is counted as a
pass. The comparison and every job step passed. GitHub aggregate status remained
`in_progress` at the recorded final observation; this is reported separately
from the verified command result, and overall workflow success is not claimed.

[Run 41856042545](https://github.com/anortham/julie-extractors/actions/runs/41856042545)
used branch commit `150a29d5661f54c50ba62360a5b19f2cd99c115f`, baseline source
`f9a1b6cfbd6c51e9aecad39a19610695f210ad05`, candidate source
`e1c18a1eb6b76eb930117235fa19cfce9477e32a`, and harness source
`7a31fa573f99328946c6104b9de6fa7a82261d43`.

Both release builds use Rust 1.97.1, one Cargo job and the same 267 feature
pairs. The reference host is Linux x86_64, Intel Xeon 6975P-C, 16,463,421,440
bytes RAM, and `/dev/nvme0n1p1` on ext4. Builds and correctness preparation
finish before timing; the OS page cache is retained. Both binaries run on this
same host with the existing alternating-pair and fixed-operation procedure.

| Level | Workload | Operations | Baseline median ms | Candidate median ms | Change |
| --- | --- | ---: | ---: | ---: | ---: |
| references | fresh | 16 | 4,401 | 4,440 | +0.89% |
| references | no change | 256 | 5,337 | 5,390 | +0.99% |
| references | single-file update | 128 | 5,230 | 5,146 | −1.61% |
| full | fresh | 32 | 9,469 | 9,290 | −1.89% |
| full | no change | 256 | 5,433 | 5,350 | −1.53% |
| full | single-file update | 128 | 5,730 | 5,752 | +0.38% |

These are batch times. The complete comparison retains all samples, normalized
per-operation/per-file values, calibration probes, min/median/max, memory,
artifact size and exact semantic row deltas. References fresh scans used the
single permitted repeat after baseline spread failed; the first attempt is
retained and the repeat did not reduce its operation count. The other five
workloads pass on their first attempt. All final samples satisfy the timing
floor and spread limits. No latency, peak-memory, size, health or correctness
gate fails.

The actual baseline executable SHA-256 is
`3eba4cf46d38664fc18fe3b65c50aafd0bcdfb08ad3ef3d6bcaa711f49daba2a`;
the candidate is
`525dd9f25c35e4cf5d84d8e70dbf7894458cf11ca4876573efda8fc53aeb06a4`.
Every rebuilt baseline snapshot matches the original frozen semantic receipt:
ten snapshots and four byte-identical raw semantic dumps. Only the authorized
binary identity differs. The original receipt, manifest, output ledger,
3,090-entry exact row allowance, budgets and repeat cap remain unchanged.
Both original and updated source states are checked. No failed/unsupported
files, skips or warnings are introduced, and all 42 corpus rows plus controls
pass.

The lead independently recomputed sample statistics, floors, spread and resource
budgets and verified all exact allowances and build evidence. A separate
read-only reviewer independently accepted the same result; see
[review](review.md) and the [audit/index](artifacts.json).

The GitHub artifact ZIP SHA-256 is
`de743175637ee7cad4f016ed6ac60775130248f672d875e7d3e2811fedf41c43`,
verified against the published artifact digest before extraction. Its 735-file
inventory is [members.json](members.json). The deterministic
[raw evidence archive](raw-evidence.tar.gz) retains all 733 non-executable
members, including SQLite artifacts, source states, canonical rows, all build
records and logs, original/rebuilt receipts, preparation proof and full
comparison. The two release executables are omitted from source control;
their sizes, hashes and actual build records remain recorded. Separate indexed
archives retain CI logs and independent checks. This preserves the acceptance
evidence beyond GitHub's artifact-retention period.

[Final workflow observation](workflow-observation.json) preserves the exact
status and timestamp. The original goal requires the actual comparison and
acceptance gates to pass; it does not add a terminal REST callback as a gate.

## Aggregate status finalized

GitHub subsequently reported the run `completed` with conclusion `success`,
updated at `2026-11-04T16:26:45Z`. The [terminal API record](workflow-terminal.json.gz)
is indexed with checksums. The earlier observation remains intact; no
cancellation or rerun was required.
