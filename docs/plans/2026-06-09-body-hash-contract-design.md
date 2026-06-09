# Body Hash Contract Design

## Context

`body_hash` already exists on extracted symbols and artifact symbols, but the
public SQLite and JSONL contracts did not define what the value means. That made
the field hard for Miller and Eros to rely on for duplicate-code workflows.

## Decision

Keep the existing `symbols.body_hash` field and define it as the exact
normalized-body fingerprint for symbols with complete body spans. Do not add a
new SQLite table or JSONL record in this slice.

The v1 algorithm id is `julie-normalized-body-md5-v1`:

- use the source bytes covered by the symbol body span;
- tokenize while preserving quoted string-like tokens;
- ignore whitespace and comments for the symbol language;
- join normalized tokens with U+001F;
- emit the lowercase MD5 hex digest.

Equal hashes are exact normalized-body match candidates. They are not
near-duplicate scores, duplicate severity, or product-level clone decisions.
Miller and Eros own grouping, ranking, thresholds, and presentation.

## Architecture Quality

- **Affected modules:** body hash normalization in
  `crates/julie-extractors/src/base/body.rs`; symbol contract docs in
  `docs/contracts/sqlite-schema-v2.md` and `docs/contracts/jsonl-v2.md`.
- **Caller-facing interface:** unchanged field names and schema shape;
  clarified semantics for existing `body_hash`.
- **Depth/locality check:** comment syntax stays local to the tokenizer instead
  of leaking comment stripping to downstream consumers.
- **Test surface:** extractor body-span tests and contract-doc tests.
- **Rejected shortcuts:** no SimHash, n-gram table, token-count column, or
  extractor-side clone severity in this slice.
- **Architecture risk:** low for schema shape, medium for hash-value churn
  because comment-only body changes now normalize to the same hash.

## Acceptance Criteria

- `body_hash` remains present only when a complete body span is present.
- Whitespace-only and comment-only body changes produce the same hash.
- Executable token changes produce different hashes.
- Comment markers inside quoted strings remain part of the hash input.
- Current SQLite and JSONL contracts name the algorithm and explicitly say the field
  does not encode duplicate severity.
- Existing schema shape stays unchanged.

## Future Work Outside TODO #9

- Machine-readable `symbol_body_fingerprints` table or JSONL record.
- Normalized token counts.
- Near-duplicate candidate fingerprints such as SimHash or token n-gram hashes.
- Dogfood duplicate-candidate reporting by language and symbol kind.

## Closure

TODO #9 is closed by the exact `body_hash` contract. Decision 0002 records that
near-duplicate candidate surfaces remain out of scope until a downstream
consumer names a concrete requirement that exact normalized-body grouping cannot
satisfy.
