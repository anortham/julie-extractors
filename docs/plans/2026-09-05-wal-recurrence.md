# Standalone WAL cleanup repair

User approved WAL recurrence gap closure across the owning projects. This branch
continues J1 at ecd021c0 without modifying or merging that worktree.

## Task 1: standalone command completion

Add a bounded nonfatal checkpoint of both current-generation store.db and coord.db
after standalone store write commands finish their final coordinator writes and
dispose transactions. Explicitly consume the busy result column. Preserve committed
success if checkpoint is blocked or unavailable; emit a concise stderr diagnostic
with status and remaining WAL bytes instead of changing stdout report contracts.
Retry on a later applicable command, including idempotency replay/no-change. Do not
rely on a Miller marker. Do not add a daemon, schema, version bump, background thread,
reader termination, or manual WAL deletion. Do not write from read-only commands.
Avoid changing generic low-level writer Drop behavior or checkpointing each row.

## Architecture quality

One internal helper at the CLI command-completion boundary, consuming store layout
paths from existing APIs. A reader may block reset indefinitely; no hard size-cap
claim. Bounded busy wait no more than one second per database. Checkpoint performance
itself may depend on existing WAL size. Low risk: cleanup is nonfatal and no data
contract changes. If an existing central completion boundary is absent, report the
smallest wiring choices to the lead before making a broad refactor.

## Owned files

`crates/julie-extract-cli/src/store/` completion helper/wiring,
`crates/julie-extract-cli/tests/store_import_contract.rs`, and this plan's findings.
Read but do not edit artifact producer or J1 reader registration code without lead
coordination. Select exact completion file from Miller evidence.

## Verification and acceptance

Use TDD through the real CLI fixture, with synchronized held read transactions.
Run only `cargo test --locked -p julie-extract-cli --test store_import_contract <filter>`
for worker red/green and ceiling. Limit cargo jobs to four. No full suite.

- [ ] Normal completion truncates WAL despite another idle connection staying open.
- [ ] Held snapshot defers cleanup, returns committed success and a diagnostic.
- [ ] Multiple committed writes remain intact; after reader release a replay/no-change
  command truncates WAL while an idle connection stays open.
- [ ] Include both databases; no cleanup side effects on read-only commands.
- [ ] Record exact tests, red/green evidence and platform limitations.

Miller implementation runs independently in another repo. Commit mode is
parallel-lead-commit: do not stage, commit, merge, or push. Lead owns final review,
Windows and broader gates. Security scope: none declared. No external reviewer.
