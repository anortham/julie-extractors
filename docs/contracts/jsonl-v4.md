# JSONL Contract v4

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
catalog fingerprint in `sqlite-schema-v5.catalog.sha256` is the authority for the corresponding
artifact tables.
