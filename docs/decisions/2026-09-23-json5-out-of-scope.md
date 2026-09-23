# JSON5 is out of scope

Date: 2026-09-23. Plan: [gap follow-ups](../plans/2026-09-23-gap-followups.md),
task 4. Retires `json5_unquoted_keys` (json).

## Context

JSON5 adds unquoted keys, single quotes, trailing commas, comments, and
hexadecimal numbers to JSON. The `json` row parses standard JSON and JSONC with
tree-sitter-json. A `.json5` file is not selected, and JSON5 syntax in a
`.json` file gives parse diagnostics and wrong symbols.

The registry crate `tree-sitter-json5` 0.1.0 depends on tree-sitter 0.20 and
cannot link with the 0.26.11 runtime. Support needs a Git pin or an owned fork,
a new language row, and fixtures, for a format that appears mostly in a few
tool configuration files (for example `renovate.json5`).

## Decision

- `julie-extract` does not select `.json5` files and adds no JSON5 grammar.
- A `.json` file that holds JSON5 syntax is invalid JSON. Its parse diagnostics
  are correct output, not an extractor gap.
- The `json5_unquoted_keys` open gap is removed from the `json` row.

## Revisit when

A consumer needs rows from JSON5 files, or a JSON5 grammar that links with the
current tree-sitter runtime is published on crates.io.
