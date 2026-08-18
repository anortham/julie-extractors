# Reference resolution coverage v1

> **Retired 2026-08-18.** julie-extract no longer writes workspace-global
> reference resolution. This coverage contract and
> `scripts/reference-resolution-coverage-report.mjs` are historical. Miller
> owns query-time resolution policy. See
> [2026-08-18-resolution-write-path-retirement.md](../decisions/2026-08-18-resolution-write-path-retirement.md).

`fixtures/extraction/reference-resolution-coverage.json` is the historical,
registry-driven coverage artifact for reference evidence.

- Its `languages` array exactly equals the 36-language extractor registry.
- Cells are grouped by language, origin, raw kind, canonical kind, outcome,
  tier, method, span presence, and applicability.
- Origins are `identifier`, `relationship`, and `pending_relationship`.
- Canonical kinds are `calls`, `extends`, `implements`, `imports`,
  `instantiates`, `references`, and `uses`.
- Identifier `call` maps to `calls`, `type_usage` maps to `uses`, and
  `member_access`/`variable_ref` map to `references`.
- Golden identifiers with extraction-time targets use tier `1` and method
  `tier1_local`, matching the live identifier-resolution vocabulary.
- Pending `imports` and `references` rows are `unattempted` because the
  workspace resolver does not run a symbol tier chain for those kinds. Other
  unresolved supported pending kinds are `unresolved_pending`.
- Zero, open-gap, and not-applicable cells are explicit.
- Per-language summaries include total, attempted, resolved, ambiguous,
  missing, `no_context`, unresolved pending, unattempted, span-present, and
  span-missing counts.

The artifact is generated from every golden registered in
`fixtures/extraction/capabilities.json`. Its digest covers the registry and all
registered expected outputs, so fixture drift makes the strict check fail.

```bash
node scripts/reference-resolution-coverage-report.mjs --write --strict
node scripts/reference-resolution-coverage-report.mjs --strict
```

`node scripts/language-data-quality-report.mjs --strict` also runs this gate.
