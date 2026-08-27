# Vanished source files commit as deletions, not failed requests

Date: 2026-08-27
Status: accepted

## Problem

A consumer (Miller) enumerates a workspace delta, then submits one
`store update` per added/changed file. When the file is deleted between the
consumer's enumeration and julie-extract's read — routine during branch-switch
churn — the planning read failed the whole request with
`source file could not be read: No such file or directory`. The consumer's
whole delta then failed and entered scan-failure backoff, even though the
correct index state (file absent) was one delete away. Field evidence: three
failed incremental scans in the Tycho workspace on 2026-08-26.

The same window existed in `store import` planning: discovery listed a file, the
identity read raised `NotFound`, and the whole import request failed.

## Decision

- `store update`: when the planning read fails with `io::ErrorKind::NotFound`,
  enqueue a delete request for that file under the update's request id and
  idempotency key, and report it as a committed `update`. The durable outcome
  is the delete the consumer would have requested with a later enumeration.
- `store update` idempotency replay: an existing delete row under the update's
  key is adopted when its payload names exactly the update's file in the same
  family, root, and view. Anything else stays `idempotency_conflict`. Adoption
  is required: a retry after a crash mid-fallback must drain the delete row it
  enqueued, or the key is poisoned forever.
- `store import`: a file that vanishes between discovery and the planning read
  is left out of the plan. The import indexes the tree exactly as a slightly
  later discovery would have seen it.
- Only `NotFound` gets this treatment. Permission, I/O, and decode errors keep
  failing the request: those files still exist, and silently dropping them
  would serve a wrong index.

## Consequences

- The report contract is unchanged in shape. The `operation` field stays
  `update` because consumers match it against the command they invoked.
  A fallback delete reports `manifest.disposition` `created` when the file was
  indexed and `reused` when it never was.
- The cross-kind idempotency rule is narrower than before: update-over-delete
  is a conflict unless the delete row names exactly the update's file. The old
  blanket conflict made the vanished-file fallback unretryable.
- Executor drain semantics are untouched. A file that vanishes after enqueue
  still becomes a per-file failed manifest entry in a committed request, and
  the next delta lists it as deleted.

## Evidence

- `crates/julie-extract-cli/tests/store_update_vanished_file.rs`
- `planning_tests` in `crates/julie-extract-cli/src/store/import.rs`
- `update_adopts_a_delete_row_that_names_exactly_its_file` and
  `update_reports_idempotency_conflict_for_a_delete_row_naming_another_file`
  in `crates/julie-extract-cli/tests/store_operations_contract.rs`
