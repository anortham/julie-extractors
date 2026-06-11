# Caller `--ignore-file` Rules Outrank In-Tree Ignore Files

Date: 2026-06-11
Status: accepted

## Context

The first version of the "File Selection and Ignore Rules" contract gave
in-tree ignore files precedence over caller-supplied `--ignore-file` rules
below their own directory, mirroring git's treatment of caller-level
excludes. A pre-release review showed that this made the vendor-policy
mechanism from `2026-06-11-vendor-policy-consumer-side.md` unreliable in its
primary scenario: committed generated files are exactly the files most
likely to carry committed whitelist rules (`!file` is how such files get
committed past ignore patterns), so a consumer's explicit `--ignore-file`
exclusion could be silently re-included by repo content. The review also
found that the two-layer matcher implementation diverged from git semantics:
a nested whitelist could re-include a file under an excluded parent
directory, which made `scan` and `update` disagree about the same path, and
ancestor `.gitignore` patterns were anchored at the scan root instead of
their own directory.

## Decision

1. **Explicitly passed `--ignore-file` rules win.** The caller layer is
   consulted first and is decisive in both directions: a caller ignore rule
   cannot be re-included by in-tree whitelists, and a caller whitelist can
   re-include a file in-tree rules ignore. Invocation-level policy is
   operator intent; repo content must not silently override it.
2. **In-tree rules follow git semantics exactly.** Every ignore file
   (ancestors up to the git root, the scan root, and nested directories at
   any depth, including hidden directories) is anchored at its own
   directory. Deeper rules win, `.julieignore` beats `.gitignore` in the
   same directory, a file cannot be re-included when a parent directory is
   excluded, and ignore files inside excluded directories are not read.
   `scan` and `update` share one decision path, so they can never disagree
   about a file.
3. **In-tree ignore files never break the scan.** An unreadable in-tree
   ignore file is reported as a non-fatal entry in the report's `errors`
   array and skipped. An unreadable or invalid `--ignore-file` stays a hard
   CLI error, because it is operator configuration.

## Alternatives Rejected

- **Git/ripgrep-style precedence (in-tree outranks caller).** Matches git's
  own layering but defeats the consumer-side vendor policy: the preferred
  non-invasive routing (`--ignore-file`) must be reliable against committed
  whitelists, and a CLI flag the repo can override is a trust problem.
- **Keeping the depth-8 nested-ignore cap.** An arbitrary, undocumented
  cutoff that diverged from git and from discovery's unbounded traversal.
  Collection now prunes excluded directories instead, so cost is bounded by
  the scanned tree.

## Consequences

- Consumers such as Miller can rely on `--ignore-file` as the final word on
  exclusions without writing into the scanned repo.
- A caller whitelist can widen the input set relative to in-tree rules.
  Hard safety exclusions remain non-overridable by every layer.
- The contract section "File Selection and Ignore Rules" in
  `docs/contracts/cli.md` is the normative description of the layering.
