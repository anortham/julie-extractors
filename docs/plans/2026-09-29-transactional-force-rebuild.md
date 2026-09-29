# Transactional force rebuild

The release-readiness audit is authorized by the user's request to test every
platform, fix regressions, and prepare 3.7.2. Publishing requires separate
approval. The [identity decision](../decisions/2026-09-29-artifact-identity-and-force-rebuild.md)
and [CLI contract](../contracts/cli.md) define the behavior.

## Ownership and order

1. The writer task owns `writer.rs` and `writer_contract.rs`. Add
   `stage_full_rebuild(&ArtifactMetadata)`, validate the original identity and
   producer, then reset and populate inside the force-scan transaction.
2. The CLI task owns `commands.rs`, `path_policy.rs`, and
   `producer_freshness_contract.rs`. Preserve the original metadata snapshot,
   stage the rebuild, and remove pre-write unlinking. Keep watchdog aborts
   before writes.
3. The lead owns report mapping, documentation, release versions, review,
   commits, and all platform release gates. Workers hand over focused verified
   diffs and do not commit or run broad suites.

## Acceptance

- Stale identity before or after writer open rejects every mutation.
- A concurrent rebind rejects a staged rebuild before rows are reset.
- Rebuild failure preserves the complete original artifact.
- Successful rebuild keeps database file identity and creates fresh metadata,
  facts, capabilities, index level, and revision history.
- Empty rebuilds create a revision; writer reuse does not reset twice.
- Staged rebuilds reject non-force mutations and disable live bulk load.
- Parent exit and source errors during a producer refresh preserve old data.

## Verification

Workers run focused writer and CLI regressions. The lead runs default,
contract, certification, real-world smoke/release, performance, all-feature,
and ignored tests on Linux and native Windows. Release checks include strict
quality, formatting, clippy, `cargo deny --all-features check`, compatible
output against 3.7.1, repository dogfood, performance comparisons, preflight,
and local package staging. Results belong in 3.7.2 release evidence.
