# Markdown anchors and cross-document heading links

Date: 2026-09-22. Plan: [language gap closure](../plans/2026-09-22-language-gap-closure.md),
wave 2 (the `markdown.*` gaps).

## Decision

- A heading's name is its plain text. Inline markup, ATX closing hashes, and a
  trailing `{#id}` attribute block are removed. A heading with no text gives no
  symbol.
- A heading's anchor is its explicit `{#id}`, else its GitHub slug. The slug
  keeps Unicode letters, marks, numbers, connector punctuation, spaces, and
  hyphens, then turns spaces into hyphens. The second use of a slug gets `-1`,
  the third `-2`, and so on.
- `[text](#anchor)` and `<a href="#anchor">` give a `References` edge to the
  heading with that anchor. Reference links and footnote references give a
  `References` edge to their definitions.
- `[text](path.md#anchor)` to a local path gives a structured pending
  `References` row. The terminal name is the decoded anchor and the import
  context is the path, so a consumer can match the heading anchor in that
  document. A link to a whole document or to a URL gives no pending row.
- The source of each row is the heading whose section holds the link, else the
  link symbol.
- Frontmatter keys are `Property` children of the frontmatter symbol. Only
  top-level keys count; TOML table names are keys and their entries are not.

## Why

Before this change, Markdown claimed that links cannot refer to symbols in
other documents. A link to a heading in another document is a real reference
to a heading symbol. Without a pending row, find-references on a heading could
not see links from other documents.
