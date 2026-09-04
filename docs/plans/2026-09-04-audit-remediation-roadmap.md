# Audit Remediation Roadmap

Date: 2026-09-04
Source: `docs/findings/2026-09-04-architecture-and-performance-audit.md` (validated).

This roadmap maps every open finding to a plan, a design task, or an explicit
"no action" decision. Run the plans in order. Each plan is independent enough
to ship on its own branch.

## Plans

| Order | Plan | Findings | Sessions (agent) | Approval gates |
|---|---|---|---|---|
| 1 | `2026-09-04-audit-1-hot-path-waste.md` | E1, E2, E3 (identifier lookup), C1, C2, A2, C4 | 2 | none |
| 2 | `2026-09-04-audit-2-ci-and-hygiene.md` | T2, T3, T4, T6, T7, T10, T11 | 1 | Task 4: worktree and branch removal |
| 3 | `2026-09-04-audit-3-query-and-loop-fixes.md` | A3, A4, A5, A6, A7, A11, C3, C5, C9, E6, E7 | 2 | none |
| 4 | `2026-09-04-audit-4-dead-code-and-api-narrowing.md` | C6, C11, E5, E9, E10, E12, A9, A10, T5 | 2 | Task 5: public API removal list |

## Needs design before a plan

These findings change module boundaries or contracts. Each needs a
`razorback:brainstorming` session, a decision record in `docs/decisions/`, and
then its own plan.

- **A1: one schema and one binder stack.** The store DDL adds `version_id`,
  `STRICT`, deferred foreign keys, and coordinator tables. The design must
  decide whether the artifact DDL is generated from the store DDL or both from
  a column table. Depends on nothing; can start after plan 1.
- **E4 and E11: one tree walk per file with collectors registered on
  `LanguageSpec`.** Design must fix the collector interface, the shared
  containing-symbol index from plan 1 Task 3, and the ordering guarantee that
  keeps `structural_facts` row order stable. Depends on plan 1.
- **C7 and A12: split `commands.rs`, `maintenance.rs`, `coordinator.rs`.**
  Mechanical, but the split lines should follow the E4 and A1 designs so the
  files are not split twice. Do after A1.
- **A8: streaming JSONL export.** Needs a `Serialize` struct per record and a
  decision on whether `metadata_json` validation stays. Contract-neutral if
  done right; verify with the JSONL golden tier.
- **E8: streaming body hash.** `body_hash` is persisted, so the token
  normalization must be preserved byte for byte or the change is a contract
  bump. Design the normalization as a byte iterator with a differential test
  over every golden source.
- **C8: overlap extraction and spool drain.** Measure the drain share first
  with plan 1's baseline tooling. Design only if it exceeds 10 percent.

## No action

- **T1** refuted: `cargo xtask test contract` already runs golden and
  capability on every push.
- **C10** (`--strict-schema` redundant on write commands): the help text
  states the behavior. Leave it.
- **C12** (`MILLER_STORE_CHUNK_VERSIONS`): documented in `docs/contracts/cli.md`.
  Leave it.
- **T8** (docs and memory weight): do not rewrite history. Apply the existing
  Goldfish rule going forward and archive plans without status when the plan
  directory is next touched.
- **T9** (`operations_contract.rs` size): split by command when next touched.

## Closure rule

A finding is closed when its fix commit is recorded in the findings document
under the item, and the plan's verification ledger shows the branch gate green
at that commit. Performance findings also need the before and after numbers in
`docs/evidence/`.
