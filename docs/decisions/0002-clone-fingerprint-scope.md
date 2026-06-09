# Decision 0002: Clone Fingerprint Scope

## Context

Downstream tools need clone-ready symbol facts, but this repo owns extraction
facts, not duplicate-code product decisions. The existing `symbols.body_hash`
field is already available when a symbol has a complete body span, and its
contract now defines `julie-normalized-body-md5-v1` as an exact normalized-body
fingerprint.

The open question was whether TODO #9 should also add a separate
machine-readable near-duplicate surface such as token counts, SimHash, or token
n-gram hashes.

## Decision

Close TODO #9 with the exact `body_hash` contract only.

Do not add a new `symbol_body_fingerprints` table, JSONL record, SimHash, token
n-gram hashes, or clone score in the current v3 artifact. Those surfaces should
be added only after a downstream consumer names a concrete requirement that
exact normalized-body grouping cannot satisfy.

## Consequences

Miller and Eros can group exact duplicate candidates cheaply by `body_hash`
without re-tokenizing source. They still own grouping, ranking, thresholds,
presentation, and any near-duplicate analysis.

The extractor contract stays stable and conservative. It avoids publishing a
premature near-duplicate algorithm that would become a public artifact API
before its consumer semantics are clear.

## Rejected Alternatives

- **Add SimHash now:** premature without a target similarity threshold, token
  policy, or consumer acceptance test.
- **Add token n-gram hashes now:** higher artifact cost and unclear downstream
  matching semantics.
- **Emit clone severity:** outside this repo's product boundary; severity is a
  downstream workflow decision.

## Future Agents

If a future downstream consumer needs near-duplicate candidates, create a new
versioned row family instead of changing `body_hash` semantics. That future
surface must define algorithm id, tokenization, supported languages, artifact
cost, and fixture-backed evidence before it becomes part of SQLite or JSONL.
