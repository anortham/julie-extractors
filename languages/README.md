# Language Extraction Policy

This directory contains language policy that directly changes extraction
artifact rows.

## Included

- `[literal_carriers]`: callee names whose string-literal arguments become
  `literals` rows with `kind = url`, `sql`, or `route`.

## Excluded

- Search tokenization.
- Result scoring.
- Embedding policy.
- Watcher or daemon policy.
- Dashboard, warning, and editing policy.

Language extension mapping, parser inventory, doc-comment rules, and capability
metadata live in `crates/julie-extractors/src/language_spec/` and
`fixtures/extraction/capabilities.json`.

Contributor workflow and claim definitions live in:

- `docs/contracts/extracted-data-v1.md`
- `docs/languages/new-language-checklist.md`
