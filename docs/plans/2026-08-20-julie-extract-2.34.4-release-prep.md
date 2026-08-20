# Julie Extract 2.34.4 release preparation

## Status

v2.34.4 is a compatible patch candidate. v2.34.3 remains the published
release until the release workflow succeeds.

## Contract

- Extraction epoch: 4
- SQLite schema: 7
- JSONL format: v5
- Store schema: 2
- Store epoch: 1

Consumers replace the binary and re-extract their source trees.

## Scope

- Windows test hardening and path/report fixes.
- Test-role closure records 21 supported capability cells and 7 source-backed N/A
  entries across C, C++, Rust, Zig, HTML, SQL, Markdown, JSON, TOML, YAML,
  and XML.

## Release gate

Keep v2.34.3 as the published pointer until the v2.34.4 release workflow
completes successfully. The workflow input defaults to 2.34.4.
