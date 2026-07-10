# 0006: Embedded Test Calls Use Shared Vocabulary with Host Remapping

## Context

HTML and Vue can contain JavaScript or TypeScript whose test roles are expressed
as call-style DSLs. Parsing those calls against the whole host document gives
incorrect node text and positions. Copying the JS/TS vocabulary into each host
extractor would also let role semantics drift.

Vue already owns language-local extraction for its component declarations, so
running a full JavaScript or TypeScript symbol extractor and publishing every
embedded symbol would duplicate the host extractor's output.

## Decision

Embedded-language adapters parse the embedded source with its native grammar,
route recognized test calls through the shared `test_calls` materialization
seam, and publish only the additive role symbols needed by the host extractor.

The adapter must use section-local source for AST text, then remap the complete
symbol span, body span, stable ID, and container parent ID into the host file
before returning rows. It must retain nearby declaration and qualified
member-call negatives. Capability support is promoted only after a registered
host-language golden proves the remapped rows.

## Consequences

- Framework vocabulary and role metadata stay centralized.
- Host-language symbols keep one owner; embedded adapters do not duplicate the
  host extractor's declaration surface.
- Position and parent correctness is an explicit adapter obligation and test
  surface.
- A new host language must supply a grammar-specific traversal and offset
  mapping, but it does not invent a second test classifier.

## Applies To

- `crates/julie-extractors/src/test_calls.rs`
- `crates/julie-extractors/src/html/scripts.rs`
- `crates/julie-extractors/src/vue/test_calls.rs`
- Registered HTML or Vue test-role goldens

## Future Agents

Do not classify embedded test roles by scanning host text or copying the JS/TS
name lists. Reuse the shared call seam, parse section-local source, remap every
published location and parent reference, and prove ordinary lookalikes stay
negative before changing capability claims.
