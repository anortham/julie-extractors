#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if ! cmp -s "${repo_root}/AGENTS.md" "${repo_root}/CLAUDE.md"; then
  echo "AGENTS.md and CLAUDE.md differ. Update both in the same commit." >&2
  diff -u "${repo_root}/AGENTS.md" "${repo_root}/CLAUDE.md" >&2 || true
  exit 1
fi

echo "AGENTS.md and CLAUDE.md are synced."
