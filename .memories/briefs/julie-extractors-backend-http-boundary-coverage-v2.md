---
id: julie-extractors-backend-http-boundary-coverage-v2
title: julie-extractors Backend HTTP Boundary Coverage (v2.7.0 lane)
status: completed
created: 2026-07-02T14:49:58.432Z
updated: 2026-08-30T22:44:42.546Z
tags:
  - julie-extractors
  - backend-http-boundary
  - structural-facts
  - miller-bridge
  - roadmap
---

## Goal

Extend both sides of the HTTP boundary to every major backend ecosystem so Miller can bridge client requests to handlers anywhere: handler-definition facts for Express/Fastify, FastAPI/Flask/Django, Spring, Go (net/http, gin, echo), and Rails, plus `http.client_request.v1` coverage for python/csharp/go/java/ruby. One lane, one release (v2.7.0).

## Why Now

v2.6.1 shipped the JS/ASP.NET boundary slice and the containing-symbol binding fix; a parallel Miller session is consuming it now. The julie-extractors→Miller update loop is slow, so the user chose one comprehensive lane over small slices. Boundary/join facts are the highest-value structural facts for AI agents (call graphs stop at the HTTP gap); syntax-flavor facts are deprioritized.

## Plan and Contracts

- Plan: `docs/plans/2026-07-02-backend-http-boundary-coverage.md` (9 tasks; Task 1 = framework_structural_facts module split + shared `base/http_boundary.rs` normalizer + ASP.NET normalized-key addition; Tasks 2–7 = one ecosystem each, independent and parallelizable; Task 8 = contract sweep; Task 9 = v2.7.0 release).
- Doctrine: `docs/decisions/0004-http-boundary-join-contract.md` — `normalized_route_template` (`:param` flavor) is the universal server-side join key; same-file prefixes resolve, cross-file prefixes emit mount facts; verb omission = not verb-restricted.
- 15 new pattern ids; contract marker `.backend-http-boundary-v1`.

## Constraints

- Product boundary unchanged: facts only, joining stays in Miller. M2 silence doctrine (static literals only, no guessed routes) applies to every family.
- Per-ecosystem branch gates; strict data-quality report stays 0/0; default suite under the 90s tripwire; releases require explicit user approval.
- Top frameworks only; documented exclusions instead of guessy detection (receiver tracing is single-assignment, same-file only).

## Status

Plan written 2026-07-02, Codex adversarial review in flight; awaiting user plan approval before execution. Implementation will run across later sessions via razorback:subagent-driven-development.
