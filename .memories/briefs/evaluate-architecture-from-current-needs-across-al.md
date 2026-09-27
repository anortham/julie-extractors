---
id: evaluate-architecture-from-current-needs-across-al
title: Evaluate architecture from current needs across all supported languages
status: active
created: 2026-09-27T13:54:01.995Z
updated: 2026-09-27T13:54:01.995Z
tags:
  - architecture
  - all-languages
  - first-principles
  - code-kb
  - user-direction
---

## Goal

Reassess julie-extractors architecture, extraction design, SQLite schema, and the producer/consumer boundary from current requirements. Prevent the complexity growth that undermined Julie and Miller from recurring in code-kb.

## User direction

- Historical docs, ADRs, plans, and present code explain past choices; they do not constrain this evaluation. A documented decision can be wrong and should be reversed when a better design is supported by current evidence.
- No existing architectural choice is exempt from review, including SQLite, hand-written extraction, reference-resolution ownership, and precomputation. This authorizes evaluation and recommendations, not an unrequested rewrite or release.
- Support every language as deeply and correctly as possible. TypeScript and Python were review samples, not preferred languages. Shared improvements need evidence across applicable languages while preserving domain-specific depth.
- Evaluate total complexity across producer and consumer, not merely complexity moved out of this repo.
- Another session owns the slow-reference-query investigation in /home/murphy/source/code-kb. Do not modify that project or duplicate its investigation.

## Evaluation standard

Compare correctness, useful fact coverage, query and update cost, resource use, and maintenance burden. Distinguish observed costs from hypotheses. Decide whether work should be precomputed separately from who owns resolution. Prior failure of a large resolution subsystem is not evidence that all precomputation or producer-owned resolution is wrong.

## Status

Initial source/schema review completed on main at 3b84d04a. Its recommendation to preserve the fact-only boundary was too firm and is withdrawn pending workload/context evidence. No replacement architecture has been chosen. The existing uncommitted docs/site/extractors.html edit corrects exaggerated limits attributed to Tree-sitter queries.
