# Vendor Policy Is Consumer-Side; Extraction Stays Policy-Free

Date: 2026-06-11
Status: accepted

## Context

The v2.3.0 quality release multiplied structural-fact volume on data files. A
benchmark against a large real workspace (openclaw, 16k files) showed the new
data is dominated by machine-generated but git-tracked JSON/JSONL files: ten
committed i18n translation-memory `*.tm.jsonl` files produced ~39% of all
structural facts. That raised the question of whether `julie-extract` should
detect and skip vendor or generated files on its own, the way the original
Julie workspace indexer did (`analyze_vendor_patterns` plus a generated
`.julieignore`).

Miller, the first production consumer, already loads `.julieignore` files
per directory in its watcher (`WatchPathFilter`, `WorkspaceIgnorePolicy`) to
keep live update filtering consistent with delegated full scans. At the time
of this decision the scanner only honored `.julieignore` at the scan root,
so the two sides had divergent semantics.

## Decision

1. **`julie-extract` does not detect vendor or generated files.** The
   extractor stays policy-free: it extracts whatever the layered ignore
   rules let through. Smart weeding would make artifact content
   unpredictable, weaken capability claims, and second-guess consumers who
   legitimately want deep extraction of committed data files.
2. **Vendor detection belongs to consumers.** A consumer such as Miller
   detects vendor or generated files with its own heuristics and routes the
   result through the existing ignore contracts: either a generated ignore
   file passed via `--ignore-file` (consumer-owned, non-invasive, preferred
   default) or a `.julieignore` written into the workspace as an explicit
   user action (visible, editable, shared by every consumer). For this
   routing to be reliable, `--ignore-file` rules outrank in-tree ignore
   files, so a committed whitelist cannot silently re-include a file the
   consumer excluded (see `2026-06-11-ignore-rule-precedence.md`).
3. **`.julieignore` is a first-class contract, with nested support.** It is
   the committed, repo-owner-controlled exclusion layer. The scanner honors
   root and nested `.julieignore` files with the same per-directory
   semantics as nested `.gitignore`, matching what Miller's watcher already
   assumed. The layering and precedence rules are documented in
   `docs/contracts/cli.md` under "File Selection and Ignore Rules".

## Consequences

- Default scans of repos with committed generated data files produce large
  artifacts. That is working as intended; the remedy is an ignore rule, not
  an extractor heuristic.
- Nested ignore-file scoping was fixed as part of this decision: previously
  nested `.gitignore` patterns were applied root-relative, so a nested rule
  could ignore unrelated paths elsewhere in the tree. Nested `.gitignore`
  and `.julieignore` files now each match relative to their own directory,
  deepest rule wins, `.julieignore` beats `.gitignore` in the same
  directory.
- Follow-up debt: consumers can only write good ignore rules if they can see
  extraction cost. A cost-attribution surface (for example per-file row
  counts in `info` or the scan report) is the planned closure for that gap
  and is tracked as a candidate follow-up, not part of this decision.
