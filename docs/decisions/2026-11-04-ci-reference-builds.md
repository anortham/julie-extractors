# Preserve semantic expectations across reference-host builds

Status: local preparation approved by the existing goal; remote execution
requires owner approval. The performance goal remains incomplete.

The local v3 comparison exhausted its allowed repeats with two noisy workloads.
GitHub Actions is enabled, but the task branch is unpublished and no candidate
artifact is available. The registered remote performance workflow only runs
the writer floor. An isolated CI job can provide another reference host after
the owner approves the push and new build identities.

Build the immutable baseline `f9a1b6cfbd6c51e9aecad39a19610695f210ad05` and
candidate `e1c18a1eb6b76eb930117235fa19cfce9477e32a` with Rust 1.97.1, locked
dependencies, the release profile and one Cargo job. Pin the harness separately.
Record actual hashes, source trees, feature graphs, compiler versions, recipes
and logs. Different build hashes are truthful provenance, not permission to
change expected extraction output.

Capture the rebuilt baseline without timings. Compare every snapshot with the
original receipt: canonical fact hash, source state, diagnostics, health,
manifest and source identity must match exactly. Reject missing, duplicate or
changed snapshots and unexplained fields. Permit only the rebuilt binary hash
and snapshot ordering to differ. Never replace the original semantic receipt.

Run the existing comparator only after that proof succeeds. Preserve the
four-second calibration target, two-second acceptance floor, original budgets,
exact row allowances and single-repeat limit. Builds and receipt checks finish
before timing; hosted-runner noise can still block completion. No further local
timing experiment is part of this preparation.

The manual workflow remains within GitHub's
[25-input dispatch limit](https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/trigger-a-workflow).
Its 120-minute job limit covers builds and the observed 42-minute comparison,
within GitHub's [workflow timeout limits](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#jobsjob_idtimeout-minutes).
This orchestration limit does not revise an extractor performance budget.
