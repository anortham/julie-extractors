# Razorback Policy

This file is the source of truth for razorback-specific workflow policy in this
repo.

## Default Position

Use the cheapest tier that can safely do the work, but treat these areas as
strategy-tier by default:

- public CLI contract
- SQLite schema
- JSONL schema
- report/error contract
- parser dependency changes
- language capability claims
- test-tier policy
- release packaging

## Worker Eligibility

Workers are eligible for bounded implementation only when all are true:

- the public interface is already decided
- file ownership is narrow and non-overlapping
- the verification ceiling is explicit
- the task does not reinterpret schema/report/release evidence
- the task does not modify parser dependency versions

## Mechanical Tier

Mechanical workers may handle docs, manifests, fixtures, formatting, and rote
renames only when they do not own a failing test, metric, or acceptance gate.

## Escalation Triggers

Escalate to strategy or gate-review work when:

- a public artifact schema changes
- a CLI status, exit code, or error code changes
- a language capability claim changes
- parser dependency versions change
- a test passes but evidence quality is weak
- a worker finds hidden coupling to old Julie internals
- default-suite runtime grows unexpectedly

## Verification Ownership

Workers own narrow red/green verification. The lead owns affected-change,
contract, certification, real-world, release, and budget gates.

Workers must report:

- invariant
- command
- scope label
- commit SHA
- result
- timestamp

If assigned verification fails, workers stop and report unless the plan
explicitly says to update that gate.
