# JSONL Contract v4

> **Retired 2026-08-18 by [JSONL Contract v5](jsonl-v5.md).** v5 removes the
> reference-resolution overlay keys. This page stays as the historical v4
> authority. Do not reinterpret v4 records as v5.

Every record envelope has `jsonl_schema_version: 4` and `extract_contract_version: 4`. Version 4
has no aliases or accepted version range.

## Reference site record

`reference_site` records are emitted after `symbol_annotation` and before evidence rows:

```json
{
  "reference_site_id": "reference_site-...",
  "file_id": "file-...",
  "path": "src/example.rs",
  "language": "rust",
  "containing_symbol_id": "symbol-...",
  "span": {
    "start_line": 4,
    "start_column": 11,
    "end_line": 4,
    "end_column": 17,
    "start_byte": 52,
    "end_byte": 58
  },
  "is_exact": true,
  "provenance": "target_token"
}
```

For a spanless site, `span` and `containing_symbol_id` may be `null`; `is_exact` is `false` and
`provenance` is `spanless`.

`identifier`, `relationship`, and `pending_relationship` records each add the required
`reference_site_id`. All other record payloads retain their version-3 shape. The checked-in SQLite
catalog fingerprint in `sqlite-schema-v6.catalog.sha256` is the authority for the corresponding
artifact tables.

## Identifier `code_context`

The `identifier` payload still carries a `code_context` key, and the `identifiers.code_context`
column still exists, but the producer no longer populates either: every exported identifier record
has `"code_context": null`. Per-identifier context snippets were write-only — no consumer read them
— and they accounted for roughly half of all identifier bytes in both the scan spool and the
artifact. Symbol records dropped their equivalent field in version 1; this is the same disposition
for identifiers. Consumers must treat `code_context` as always `null`; the key and column are
retained only so existing readers keep parsing.
