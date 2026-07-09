# Continuous-Testing Evidence Boundary

## Decision

`julie-extractors` stops at versioned extraction evidence. It emits test-role
columns, capability evidence, file status, and parse diagnostics. It does not
become a continuous-testing runtime.

The ownership boundary is:

| Product | Ownership |
| --- | --- |
| `julie-extractors` | emitted test roles plus capability and diagnostic evidence |
| Miller | deterministic graph candidates over those extracted facts |
| Eros | runner inventory, scheduling, results, freshness, and verdicts |

## Data Flow

`julie-extractors` publishes `symbols.is_test`, `symbols.test_container`, and
`symbols.test_lifecycle`, together with
`kind_coverage.test_detection`. Miller may use those facts to compute
deterministic graph candidates. Eros combines candidates with runner and result
state to make freshness and execution decisions.

Each layer preserves the uncertainty from the layer before it. Unsupported or
`failed_preserved` files, relevant parse diagnostics, and missing capability
evidence remain unknown. Miller must not convert unknown extraction evidence
into a complete impact set, and Eros must not treat an empty candidate set as a
proof that no tests are impacted.

## Consequences

- No runner inventory, watcher, scheduler, result store, or verdict logic is
  added to `julie-extractors`.
- No second test classifier is added outside the emitted role columns.
- The `test_detection` capability is additive inside existing JSON objects; it
  requires no SQLite schema, JSONL, or extraction contract version bump.
- Semantic test-impact completeness belongs to later analysis and runtime
  evidence, not to extraction.
