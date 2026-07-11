#!/usr/bin/env bash
# Fail loudly when a release is in flight or abandoned.
#
# Catches the failure mode where a `chore(release): prepare vX` commit exists
# but the vX tag was never pushed and published, leaving main (local or
# remote) ahead of the last completed release with nobody noticing.
#
# Run at session start and as part of release closeout:
#   scripts/check-release-state.sh
#
# Exit codes:
#   0  release state is reconciled
#   1  unfinished release or unsynced main detected

set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

fail=0

say() { printf '%s\n' "$*"; }

git fetch origin --tags --quiet

version="$(sed -n 's/^version = "\(.*\)"/\1/p' crates/julie-extractors/Cargo.toml | head -1)"
if [[ -z "$version" ]]; then
  say "ERROR: could not read version from crates/julie-extractors/Cargo.toml"
  exit 1
fi
tag="v$version"

# 1. The version declared in the source tree must have a pushed tag.
if ! git ls-remote --exit-code --tags origin "refs/tags/$tag" >/dev/null 2>&1; then
  say "UNFINISHED RELEASE: source declares $version but tag $tag is not on origin."
  say "  Finish the release: push main, wait for CI, tag $tag, push the tag."
  fail=1
fi

# 2. Local main must not sit ahead of origin/main.
if git show-ref --verify --quiet refs/heads/main; then
  ahead="$(git rev-list --count origin/main..main)"
  if [[ "$ahead" -gt 0 ]]; then
    say "UNPUSHED MAIN: local main is $ahead commit(s) ahead of origin/main:"
    git log --oneline origin/main..main | sed 's/^/    /'
    fail=1
  fi
fi

# 3. Local release tags must all exist on origin.
while IFS= read -r local_tag; do
  if ! git ls-remote --exit-code --tags origin "refs/tags/$local_tag" >/dev/null 2>&1; then
    say "UNPUSHED TAG: $local_tag exists locally but not on origin."
    fail=1
  fi
done < <(git tag --list 'v*')

# 4. The pushed tag for the current version must be reachable from origin/main.
if [[ "$fail" -eq 0 ]] && ! git merge-base --is-ancestor "$tag" origin/main 2>/dev/null; then
  say "DETACHED RELEASE: tag $tag is not an ancestor of origin/main."
  fail=1
fi

if [[ "$fail" -eq 0 ]]; then
  say "Release state reconciled: $tag is pushed and reachable from origin/main."
fi

exit "$fail"
