# Capability Ledger Policy (2026-07-17)

> Superseded for the 28 test-role cells by the [2026-08-20 named-contract
> closure decision](2026-08-20-test-role-contract-closure.md). The governing
> rule remains unchanged: external conventions require a named contract,
> negative controls, and a registered golden before they become `supported`;
> the contracts listed in that decision now satisfy that rule.

## Context

The 2026-07-17 project review found that the language-data-quality gate can stay
green (`silent_cells=0`, `quality_bar_debts=0`) while dozens of `open_gaps`
remain. Reviewers also noted that `types`, `type_argument_usages`, and
`pending_relationships` are fixture-counted in
`scripts/language-data-quality-report.mjs` but absent from
`kind_coverage` in `fixtures/extraction/capabilities.json`, and that data-language
test-role classifications look inconsistent (css/regex `not_applicable` vs
html/json/toml/yaml/markdown/sql `open_gaps`).

## Decision

### 1. Soft-NA / test-role applicability

Keep the classification from
[`docs/findings/2026-07-09-test-detection-applicability-audit.md`](../findings/2026-07-09-test-detection-applicability-audit.md):

| Bucket | Languages | Rule |
| --- | --- | --- |
| `not_applicable` | css, regex | Grammar and product surface cannot host test roles |
| `open_gaps` | html, json, toml, yaml, markdown, sql | External framework/schema conventions can assign roles; no named golden yet |

Do **not** reclassify those open roles as `not_applicable` merely to shrink the
gap count. Empty extraction is never applicability evidence. Closing a role
requires a named framework/schema contract, negative controls, and a registered
golden before moving to `supported`.

### 2. Observed domains outside `kind_coverage`

`types`, `type_argument_usages`, and `pending_relationships` remain
**observed domains**:

- Counted from golden fixtures in the quality report (`OBSERVED_DOMAINS`)
- Applicability classified in script-local `DOMAIN_APPLICABILITY`
- **Not** mirrored as `kind_coverage.<domain>` cells

Rationale: `kind_coverage` tracks fixture-proven kind inventories for the eleven
core domains. Folding the three observed domains into every language row would
duplicate script applicability metadata without improving the silent-cell gate.
Future agents must update `DOMAIN_APPLICABILITY` when adding languages or when
fixture evidence changes those domains — not invent parallel `kind_coverage`
stubs.

### 3. Gap backlog metric

`scripts/language-data-quality-report.mjs` reports `open_gap_backlog` (total and
per-domain counts) separately from `silent_cells` / `quality_bar_debts`.

- `--strict` continues to fail only on silent cells and quality-bar debts.
- `open_gap_backlog` is informational product debt for prioritization; it does
  not fail CI by itself.

## Consequences

- Capability honesty stays: open gaps are explicit, not silent zeros.
- Gate health and backlog health are no longer conflated.
- Soft-NA "inflation" on data-language test roles is intentional and documented.
- Observed-domain applicability stays in one place (the report script).

## Applies To

- `fixtures/extraction/capabilities.json`
- `scripts/language-data-quality-report.mjs`
- `docs/findings/2026-07-09-test-detection-applicability-audit.md`
- Future language/capability edits

## Future Agents

1. Do not convert html/json/toml/yaml/markdown/sql test roles to NA without
   reopening the applicability audit with grammar evidence.
2. When closing an open gap, move the kind to `supported` with a golden — never
   delete the gap without evidence.
3. Prefer reducing `open_gap_backlog` via real closes; do not weaken
   `silent_cells` semantics to hide debt.
4. Keep `types` / `type_argument_usages` / `pending_relationships` out of
   `kind_coverage` unless a later decision explicitly expands the matrix.
