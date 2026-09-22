# Regex support

`regex` handles `.regex` files.

## One pattern per line

The tree-sitter grammar treats newlines as whitespace, so a single parse joins
every line of a file into one pattern. The extractor does not use that joined
tree. It parses each pattern on its own and keeps file coordinates:

- Each non-blank line is an independent pattern. This is the pattern-list
  style of secret-scanner rules, validator lists, and `grep -f` files.
- A file whose first non-blank line opens with an inline flag group that
  contains `x` (`(?x)`, `(?ix)`, `(?x:`) is one verbose pattern over all of its
  lines.
- Capture numbering starts at 1 on each pattern. A backreference binds only to
  a group in its own pattern.
- Structural facts, complexity metrics, and identifiers use the same
  per-pattern trees.

A line that starts with `#` is a pattern, not a comment, because `#` is a
literal character in regex syntax.

## Symbols

- The root of each pattern is a `variable` symbol. Its name is the pattern
  text (the first line for a verbose pattern), with no trailing newline.
- A named capture group is a `function` symbol named by its bare group name
  (`slug` for `(?<slug>[\w-]+)`). The full group text stays in the signature
  and the `pattern` metadata.
- An anonymous capture group is a `function` symbol only when a numeric
  backreference refers to it.
- Character classes are `class` symbols, lookarounds and conditionals are
  `method` symbols, and unicode properties are `constant` symbols.

## Body spans

The body span of a pattern, group, lookaround, or conditional is the node's
own span. Character classes and unicode properties have no body.

## Containment

Regex constructs sit side by side with no separator, so containment uses byte
ranges: a symbol contains a construct only when the construct's bytes are
inside the symbol's bytes. A backreference right after its group
(`(\w)\1`) is contained by the pattern, not by the group. Structural facts bind
to the innermost symbol by the same rule, so the root pattern owns the facts
for constructs that no smaller symbol holds.
