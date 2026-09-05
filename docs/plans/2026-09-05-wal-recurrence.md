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

- [x] Normal completion truncates WAL despite another idle connection staying open.
- [x] Held snapshot defers cleanup, returns committed success and a diagnostic.
- [x] Multiple committed writes remain intact; after reader release a replay/no-change
  command truncates WAL while an idle connection stays open.
- [x] Include both databases; no cleanup side effects on read-only commands.
- [x] Record exact tests, red/green evidence and platform limitations.

Miller implementation runs independently in another repo. Commit mode is
parallel-lead-commit: do not stage, commit, merge, or push. Lead owns final review,
Windows and broader gates. Security scope: none declared. No external reviewer.

## Completed verification

Production changes are committed as 60ae102b and 0b7ef7cc on fix/wal-recurrence.
Linux default tier passed in 89 seconds after the existing missing-generation JSON
failure test caught a redundant stderr diagnostic. The fix preserves that existing
test unchanged. Import contracts: 39 passed. Maintenance contracts: 19 passed.

Exact-full-SHA Windows NTFS sync of 0b7ef7cc91a9775d4d4b03a60b243a7cf466d1b9:
`cargo test --locked -j 4 -p julie-extract-cli --test store_import_contract --test store_maintenance_cli_contract`
passed 38 import and 18 maintenance tests. All three new WAL tests ran on Windows.
Log: `/home/murphy/.local/share/win-test/logs/20260905T123719Z-julie-extractors-2571805.log`.

A failed command whose layout cannot be opened retains its original failure output;
this is not a new cleanup failure to report twice. Valid layouts still checkpoint
after failed commands, and successful commands still report cleanup failure.

No merge, push, release or version change. This branch includes J1 and must not be
adopted as an unrelated 2.39 patch. Coordinate release/pin adoption with Miller M1.
No hard WAL size guarantee: pinned readers and large transactions can retain WAL;
cleanup is explicit, observable, nonfatal and retryable on the next write command.
