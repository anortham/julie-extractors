# Task 6 Report: Store Schema v2

## Status and state

- Status: complete; continuing directly to Task 7.
- Base: `05da8fb06f071d7508594662f000b59922604b3e`.
- Branch: `codex/index-store-ph2c`.
- Worktree: `/Users/murphy/source/julie-extractors/.claude/worktrees/index-store-ph2c`.
- Final commit: this report's containing commit.
- Push/release: not performed.

## Delivered behavior

- Raised the Store and coordinator catalogs to schema version 2 and froze their exact catalog hashes.
- Added resolution base, delta, pin, exact-gap, and manifest-binding catalog state with database-enforced coherence.
- Added typed resolution catalog models and the stable `resolve`, `export`, and `from_artifact` request kinds.
- Made manifest language required, part of manifest hash v2, checked against file-version language, and round-tripped by all producers.
- Made manifest flips invalidate an existing resolution binding before advancing the view head.
- Preserved import, update, and delete behavior on newly created schema-v2 stores.

## RED/GREEN ledger

1. Schema RED: the catalog reported version 1. GREEN: Store and coordinator catalogs report version 2 with exact documented DDL hashes.
2. Producer RED: schema-v2 manifest inserts failed the new non-null language constraint. GREEN: every producer supplies and round-trips a canonical language.
3. Coherence RED: a delta could reference the wrong manifest identity, an exact view could bind a mismatched delta, and a referenced base could leave ready state. GREEN: insert/update triggers reject all three states.
4. Manifest-flip RED: advancing an exact view failed because the old binding became incoherent before invalidation. GREEN: publication invalidates resolution in the same transaction before changing `current_generation`.
5. Catalog RED: hardening changed the canonical Store catalog. GREEN: the checked-in v2 contract records the final hash and exact schema tests pass.

## Verification ledger

- `store_schema_contract`: 16/16 passed.
- `store_manifest_contract`: 18/18 passed.
- `store_resolution_mechanism`: 12/12 passed.
- Full artifact suite with all features: passed.
- Full CLI suite with all features: passed.
- Artifact and CLI all-target/all-feature Clippy with `-D warnings`: passed.
- `cargo fmt --all -- --check`: passed.
- `git diff --check`: passed.
- `cargo +1.97.1 xtask test default`: exit 0.

## Miller evidence

- The target worktree remains unregistered; Miller onboarding returned `workspace_onboarding_empty`.
- Per the approved fallback, inspection used targeted `rg`, bounded reads, exact tests, and direct diff review. No Miller result was invented.

## Scope judgment

- Public `resolve`, `export`, and `from_artifact` execution remains owned by later Ph2c tasks; Task 6 freezes their coordinator vocabulary and database invariants only.
- Exact-gap derivation/publication remains Task 8; Task 6 freezes its durable schema and validates state coherence.
