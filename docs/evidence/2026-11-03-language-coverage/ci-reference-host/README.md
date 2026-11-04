# CI reference-host approval package

Ready for approval; no push, dispatch or remote timing run has occurred.

The local untimed rehearsal rebuilt both release binaries from clean sources
and reproduced the frozen hashes `79eb21fa…` and `c236e361…`. The candidate
passed all 42 language samples and two controls. All ten rebuilt baseline
snapshots matched the original receipt, including its exact receipt hash.
The rehearsal recorded `comparison_started=false`.

The runner has 19 passing contracts, 33 existing comparator contracts pass, and
YAML, shell and actionlint checks pass. A private mutation that removed the
canonical fact comparison was rejected. [Evidence and checksums](artifacts.json)
preserve the builds, records, receipts and verification logs.

The proposed CI job builds these immutable revisions:

- Baseline: `f9a1b6cfbd6c51e9aecad39a19610695f210ad05`.
- Candidate: `e1c18a1eb6b76eb930117235fa19cfce9477e32a`.
- Harness: `7a31fa573f99328946c6104b9de6fa7a82261d43`.

A follow-up regression fixes evidence-directory creation on a fresh checkout
and protects existing evidence. The rehearsal candidate was `d5de48b8`; its Rust code and manifests match `e1`,
and its resulting binary matched `c236e361` byte-for-byte. CI retains the
previously verified `e1` candidate source. Rust 1.97.1, release profiles and one
Cargo job are required. The workflow allows 120 minutes for builds, receipt
proof and comparison.

Approval covers pushing `docs/language-coverage-plan` and dispatching one
source-build comparison with new, truthfully recorded host-build hashes. The
original semantic receipt, corpus, exact row allowance, performance budgets and
single-repeat limit stay pinned. Any baseline semantic difference stops before
timing. Hosted-runner noise can still leave the required gate inconclusive.

Execute these commands only after approval, from the task checkout:

```bash
git push origin HEAD:refs/heads/docs/language-coverage-plan
gh workflow run performance.yml --repo anortham/julie-extractors --ref docs/language-coverage-plan --json < docs/evidence/2026-11-03-language-coverage/ci-reference-host/dispatch-inputs.json
```

[Dispatch inputs](dispatch-inputs.json) record every source and evidence pin.
The goal remains open until the controlled comparison passes all six workloads.
