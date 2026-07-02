---
id: julie-extractors-post-v2-5-10-roadmap-http-boundar
title: "julie-extractors Post-v2.5.10 Roadmap: HTTP Boundary Facts and Hardening"
status: completed
created: 2026-07-01T22:46:22.563Z
updated: 2026-07-02T14:49:42.578Z
tags:
  - julie-extractors
  - roadmap
  - http-boundary-facts
  - structural-facts
  - refactor
  - miller-bridge
---

## Goal

Extend julie-extractors from route-navigation facts to full cross-boundary tracing evidence for Miller/Eros, via four sequenced lanes:

1. **Web collector cleanup** — split `base/web_structural_facts.rs` (3,676 lines) into focused modules, dedupe helpers shared with `framework_structural_facts.rs`, add convention-test guardrails. Pure refactor, no contract change.
2. **HTTP boundary facts** — new fact families for client HTTP requests (`fetch`/`axios`-style, import-gated, verb + literal URL + normalized template) and API route-handler definitions (Next.js `route.ts` verb exports, Nuxt `server/api/**`, ASP.NET controller attribute routes `[Route]`/`[HttpGet]`). Completes both sides of the fetch↔API-route join. Also close the htmx-in-JSX/TSX/Vue scanning gap.
3. **Per-pattern metadata schemas** — machine-readable pattern registry (pattern_id → metadata keys/types) with a contract test that emission matches the registry; ends the M4-style doc-drift class. Publishes via checked-in JSON + `languages --json`.
4. **Engineering-health pass** — nightly `specialist-gates.yml` schedule, `registry.rs` extract-wrapper macro, writer/`sql/schemas.rs` unwrap audit.

## Why Now

The 2026-07-01 project review confirmed the previous brief's goal is met: 36 languages, 0 silent cells, 0 quality-bar debts, 0 open_gaps; route facts for htmx/Vue/React/Next/Nuxt shipped and hardened in v2.5.10. The biggest remaining gap for Miller's bridge tracing is the HTTP boundary: no client-request facts and no API-handler definition facts exist, so fetch→API-route impact analysis has nothing to join on either side.

## Constraints

- Product boundary unchanged: `source tree -> versioned extraction artifact`; facts only, joining/resolution stays in Miller.
- Sequencing: lane 1 before lane 2 (new families land on the split modules, not the god module). Lanes 3 and 4 are order-independent.
- Contract changes bump `EXTRACTION_CONTRACT_VERSION` marker + `api_surface.rs` list; capability claims need golden fixtures; strict data-quality report stays at 0/0.
- Dynamic/unresolvable URLs stay silent (no guessed routes) — existing doctrine.
- Default suite stays under the 90s tripwire.
- Releases require explicit user approval.

## Success Criteria

- `web_structural_facts.rs` split with convention tests; no behavior change (goldens byte-identical).
- Client-request and API-handler fact families emit fixture-backed rows with join-ready metadata (`target_path`/`route_path`, `normalized_route_template`, attested verbs); Miller companion plan can pin-bump and bridge fetch↔handler.
- Pattern registry is contract-tested against actual emission and published in `languages --json`.
- Nightly specialist gates run; writer paths return typed errors instead of panicking on the audited hot spots.

## Status

All four implementation plans are written (2026-07-01) and awaiting user review/approval before execution.

## References

- Review checkpoint: checkpoint_f0014da5 (2026-07-01 review findings)
- Lane 1: `docs/plans/2026-07-01-web-structural-facts-module-split.md`
- Lane 2: `docs/plans/2026-07-01-http-boundary-facts.md`
- Lane 3: `docs/plans/2026-07-01-structural-fact-pattern-registry.md`
- Lane 4: `docs/plans/2026-07-01-engineering-health-pass.md`
- Prior art: `docs/plans/2026-07-01-web-route-facts-hardening.md`, `docs/plans/2026-06-30-framework-route-facts-for-miller-bridge.md`, `docs/decisions/0003-domain-coverage-via-kind-coverage.md`
