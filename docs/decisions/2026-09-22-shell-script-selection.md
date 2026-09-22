# Shell script selection without a `.sh` extension

Date: 2026-09-22. Plan: [language gap closure](../plans/2026-09-22-language-gap-closure.md),
wave 1 (`bash.bats-extension-unsupported`,
`bash.extensionless-and-bats-files-not-discovered`).

## Decision

- `.bats` is a `bash` extension. The bash grammar parses bats files, and the
  bash extractor already models `@test` blocks.
- An extensionless file selects `bash` when its name is a shell startup file
  (`.bashrc`, `.bash_profile`, `.bash_login`, `.bash_logout`,
  `.bash_aliases`, `.profile`, `.envrc`) or its first line is a `sh`, `bash`,
  or `bats` shebang (`#!/bin/bash`, `#!/usr/bin/env sh`). Other extensionless
  files stay unsupported.
- `julie-extract` discovery reads at most the first 256 bytes of an
  extensionless file to see the shebang. Files with an extension are never
  opened during selection.

## Why

Most shell applications keep their entry points in `bin/` without an
extension. Before this change those scripts, and every bats test file, were
reported as unsupported, so their functions and calls were missing from the
artifact.

## Windows

Discovery opens the file read-only and drops the handle before it returns, so
no handle stays open during the scan. Path output is unchanged.
