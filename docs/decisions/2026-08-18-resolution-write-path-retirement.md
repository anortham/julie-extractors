# Decision: retire the resolution write path

Date: 2026-08-18

## Context

`julie-extract` used to materialize workspace-global reference resolution:
`store resolve`, resolution sessions, immutable bases, cumulative deltas, pins,
a scope journal, and standalone-artifact overlay tables. Miller was the only
consumer. That write path cost gigabytes on family stores, wedged cold imports
when language classification disagreed, and still under-resolved some sessions.

Miller now computes resolution at query time from the published fact tables.
Policy lives in Miller as `docs/contracts/resolution-policy-v6.md`. Spike
evidence there showed 100.000% parity at Miller scale and 99.9997% at
aspnetcore scale. The remaining aspnetcore gaps were this repo's bounded
session under-resolving, not Miller's query-time path.

## Decision

julie-extract stops producing reference resolution.

- The CLI has no `store resolve` verb.
- Scan, update, delete, export, and import write facts only.
- Family store and coordinator stay schema v2. Writer open drops leftover
  resolution objects in place. A schema bump to 3 would refuse every existing
  family store and force a full re-extract.
- Standalone artifacts bump to schema v7 and drop `identifier_resolutions` and
  `pending_resolutions`.
- JSONL bumps to contract v5 and drops the overlay keys. v4 is not redefined.
- View `resolution_*` columns stay so migrated stores keep their values. This
  product does not bind them.
- `JULIE_RESOLUTION_*` and `JULIE_STORE_RESOLUTION_*` flags are gone.

The missing overlay tables are an intentional compatibility break. Fact tables
stay the compat gate. See
[extraction-output-changes.md](../contracts/extraction-output-changes.md).

## Consequences

- Miller query-time resolution is the replacement. Do not add a write path here.
- Existing family stores migrate on first writer open and reclaim disk.
- A leftover v6 artifact opens for read. Write access and `--strict-schema`
  refuse it. Recovery is a whole-workspace `scan`.
- Historical plans, findings, and the fleet-safety flag decision that named the
  retired env flags are superseded. They stay in tree as history.

## Pointers

- Implementation plan: [2026-08-18-resolution-write-path-removal-plan.md](../plans/2026-08-18-resolution-write-path-removal-plan.md)
- Miller policy: Miller repo `docs/contracts/resolution-policy-v6.md`
- Miller phase-1 plan: Miller repo `docs/plans/2026-08-18-query-time-resolution-phase1-plan.md`
