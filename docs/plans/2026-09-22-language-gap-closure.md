# Language gap closure

Date: 2026-09-22. Source: [the language gap audit](../findings/2026-09-22-language-gap-audit.md)
at `ea0bd76a` (v3.3.1).

## Outcome

Every language closes the audit's high-rated gaps with fixture evidence, and the
capability ledger records any gap that stays open with a concrete reason.

## Wave 1: high-rated gaps

Scope: the 347 gaps rated `high` in the audit. A medium or low gap is in scope
only when the same change closes it.

Work runs on the integration branch `feat/language-gap-closure`. Each language
group gets one worktree under `.worktrees/gaps-<group>` on branch
`gaps/<group>`, cut from the integration branch. One agent owns each group and
commits once per language. At most four groups run at the same time.

| Group | Languages |
| --- | --- |
| ecmascript | typescript, tsx, javascript, jsx |
| web | html, vue, css |
| cfamily | c, cpp |
| dotnet | csharp, vbnet, fsharp |
| razorps | razor, powershell |
| jvm | java, kotlin, scala |
| scripting | python, ruby, php |
| systems | rust, go, zig |
| appgame | swift, dart, gdscript |
| beam | elixir, erlang |
| dynamic | lua, r, bash |
| qmlsql | qml, qmldir, sql, regex |
| data | json, toml, yaml, xml, markdown |

### Rules for each gap

1. Reproduce the gap with a focused failing test under
   `crates/julie-extractors/src/tests/<language>/`. A gap that does not
   reproduce, or that a recorded decision intends, is dropped and reported.
2. Fix the extractor. Keep the change inside the language module where
   possible. A shared-module change must not alter other languages' output
   unless that change is intended and shown in their goldens.
3. Add golden fixture evidence and review the `expected.json` diff as a
   contract change.
4. Update `fixtures/extraction/capabilities.json`: add newly supported kinds,
   remove closed `open_gaps`, and register new structural-fact pattern ids.
5. A gap that cannot close in this wave (grammar limit, workspace-global
   resolution, or an owner decision) becomes an `open_gaps` entry with a
   reason, the required closure, and this plan as the planned closure task.
   Effort alone does not defer a gap.

### Gates per group

- `cargo xtask test language <language>` for each language in the group
- `cargo xtask test golden` and `cargo xtask test capability`
- `cargo fmt --all --check`
- `cargo clippy -p julie-extractors --all-targets --all-features --no-deps -- -D warnings`
- `node scripts/language-data-quality-report.mjs --strict` with
  `silent_cells` and `quality_bar_debts` at 0

## Integration

The lead merges each group branch into `feat/language-gap-closure` and
regenerates goldens where two groups changed an embedded language. Then the
lead makes the output-contract changes once for the whole wave:

- append `language-gap-closure-v1` to `EXTRACTION_CONTRACT_VERSION` in
  `crates/julie-extractors/src/lib.rs`, as 3.3.0 and 3.3.1 did. The identity
  epoch stays 10: only the retired store import read it.
- move the crate version to 3.4.0 and declare the output change under
  `## 3.4.0` in `docs/contracts/extraction-output-changes.md`

The branch gate is the CI set: `cargo fmt --check`, workspace clippy with
`-D warnings`, `scripts/check-agent-doc-sync.sh`, the strict quality report,
`cargo test -p xtask`, `cargo xtask test default`, and
`cargo xtask test contract`. Windows verification runs when a group changes
paths, discovery, or file lifecycle code.

## Wave 2: medium and low gaps

Scope:

- The audit's medium and low gaps that wave 1 did not close: 685 after
  merging duplicate ids (476 medium, 209 low). Wave 1 closed or dropped 70.
- The `open_gaps` entries in `fixtures/extraction/capabilities.json` for the
  group's languages when one file holds the evidence and no owner decision is
  needed. Entries that need workspace-global resolution (cross-file route
  prefixes) or a recorded policy decision stay open with their reason.

Work runs on the same integration branch, groups, worktrees, rules, and gates
as wave 1. Each group branch fast-forwards to the integration head first.

Verification: 26 of 37 audit units stayed `unverified` because the verifier
run stopped at a usage limit. Rule 1 (reproduce with a failing test, else
drop) is the verification for those units, so no separate verifier run
happens.

Integration: 3.4.0 is not released, so wave 2 keeps the crate version, the
`language-gap-closure-v1` contract marker, and extends the `## 3.4.0` ledger
entry. The branch gate, the real-repository comparison, the Windows default
tier, and a new two-pass Codex review of the full branch run before release.

Result: the 13 groups closed 673 gaps and 22 `open_gaps` entries, dropped 15
that did not reproduce or that a decision intends, and deferred 5. The
deferred gaps need an owner decision or a grammar change:

- A step-definition test role for Behat and SpecFlow step bindings (a new
  `TestRole` value).
- JSON5 support (a new grammar dependency, or a decision that JSON5 is out of
  scope).
- A `go.mod` / `go.sum` language row (a new entry in the language registry and
  file discovery).
- Regex conditionals `(?(cond)yes|no)`: tree-sitter-regex has no node for them.

The capability ledger's open-gap backlog fell from 43 to 25.

The real-repository comparison found scan-time regressions against the wave-1
build: zod took 22% longer and Newtonsoft.Json 28% longer. Wave 2 added symbols
and owner lookups to code that found owners with upward `Node::parent` walks,
per-call indexes, or scans of every symbol. The fix keeps every artifact row
identical on 13 repositories and brings both scans below their wave-1 times.
The [wave-2 evidence](../evidence/2026-09-23-language-gap-wave2-real-world.md)
has the numbers.

## Later waves

Gaps that wave 2 defers stay as `open_gaps` entries with a reason, the
required closure, and this plan as the planned closure task.
