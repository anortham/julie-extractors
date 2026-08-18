# JSONL Contract v5

Every record envelope has `jsonl_schema_version: 5` and `extract_contract_version: 4`. Version 5
has no aliases or accepted version range.

Version 5 keeps the version-4 record kinds and payload shapes except for the retired
reference-resolution overlay fields. A v4 document remains the historical contract; do not
reinterpret v4 records as v5.

## Artifact record

The `artifact` record no longer carries these keys:

- `reference_resolution_status`
- `reference_resolution_version`
- `reference_resolution_last_full_revision`

Those keys existed only while standalone scan/update/delete wrote an overlay. New artifacts
omit them. A prior artifact that still stores the old metadata keys exports without them.

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
`reference_site_id`. Identifier `target_symbol_id` stays on the payload. New writes leave the
resolution tables empty, so that field is `null`. A prior artifact that still has overlay rows
may still export a target through the leftover table join.

The checked-in SQLite catalog fingerprint in `sqlite-schema-v6.catalog.sha256` is the authority
for the corresponding artifact tables.

## Identifier `code_context`

The `identifier` payload still carries a `code_context` key, and the `identifiers.code_context`
column still exists, but the producer no longer populates either: every exported identifier record
has `"code_context": null`. Per-identifier context snippets were write-only — no consumer read them
— and they accounted for roughly half of all identifier bytes in both the scan spool and the
artifact. Symbol records dropped their equivalent field in version 1; this is the same disposition
for identifiers. Consumers must treat `code_context` as always `null`; the key and column are
retained only so existing readers keep parsing.
