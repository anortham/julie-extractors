---
id: checkpoint_task8o_review_round1
timestamp: 2026-08-15T09:34:55Z
tags:
  - performance-recovery
  - resolution
  - architecture-review
  - api-surface
  - validated-proof
type: decision
summary: Narrowed validated-base proof construction to artifact internals after architecture review
decision: Keep the proof-producing implementation private; expose only a feature-gated hidden test wrapper and retain public proof transport/decision APIs with lifecycle docs.
alternatives:
  - Leave find_ready_with_proof and bind_base_with_proof public, rejected because it widened the safety-proof surface.
  - Rework the algorithm or threshold behavior, rejected because review requested API-surface correction only.
symbols:
  - ResolutionValidatedBase
  - ResolutionBaseCatalog::find_ready_with_proof
  - ResolutionBaseCatalog::find_ready_with_proof_for_test
  - ResolutionBindingStore::begin_convergence_with_proof
  - ResolutionBindingStore::exact_rebase_required_with_proof
impact: Production callers can obtain the proof only through the pinned convergence result; integration tests use an explicit test-only wrapper.
next: Commit this review correction separately and hand the lead the focused gate evidence.
---

## WHAT

Made `ResolutionBaseCatalog::find_ready_with_proof` and `ResolutionBindingStore::bind_base_with_proof` private. Added the `#[cfg(feature = "test-store-resolution")]` hidden wrapper `find_ready_with_proof_for_test` for integration-test proof construction and updated the artifact/CLI focused tests.

## WHY

The proof is request-local safety evidence, not a general production catalog API. Keeping its producer and binding helper private preserves the intended module boundary while leaving the convergence result as the production transport seam.

## HOW

Added concise API docs to `ResolutionValidatedBase`, `begin_convergence_with_proof`, and `exact_rebase_required_with_proof`, covering full validation, convergence-pin lifecycle, current catalog comparison, and strict fallback on drift. No algorithm, threshold, schema, counter, output, or cumulative-gap behavior changed.

## IMPACT

Exact one-pass regression, artifact binding contract, prior-overlay contract, artifact strict clippy, format, and diff checks pass after the correction.
