# Regex support

`regex` handles `.regex` and `.regexp` files.

## One pattern per line

The tree-sitter grammar treats newlines as whitespace, so a single parse joins
every line of a file into one pattern. The extractor does not use that joined
tree. It parses each pattern on its own and keeps file coordinates:

- Each non-blank line is an independent pattern. This is the pattern-list
  style of secret-scanner rules, validator lists, and `grep -f` files.
- A file whose first non-blank line opens with an inline flag group that
  contains `x` (`(?x)`, `(?ix)`, `(?x:`) is one verbose pattern over all of its
  lines. In that file a `#` outside a character class starts a comment that
  runs to the end of the line. Comments are not parsed as pattern text: they
  give `comment` source regions and no symbols, facts, identifiers, or
  relationships.
- Capture numbering starts at 1 on each pattern. A backreference binds only to
  a group in its own pattern.
- Structural facts, complexity metrics, and identifiers use the same
  per-pattern trees.

Outside verbose mode, a line that starts with `#` is a pattern, not a comment,
because `#` is a literal character in regex syntax.

## Symbols

- The root of each pattern is a `variable` symbol. Its name is the pattern
  text with no trailing newline. A verbose pattern is folded onto one line:
  comments and the whitespace around each line are removed.
- A named capture group is a `function` symbol named by its bare group name
  (`slug` for `(?<slug>[\w-]+)`). The full group text stays in the signature
  and the `pattern` metadata.
- An anonymous capture group is a `function` symbol only when a numeric
  backreference refers to it.
- Character classes are `class` symbols, lookarounds are `method` symbols,
  and unicode properties (`\p{Lu}`, `\P{Script=Greek}`) are `constant`
  symbols. Each one is a child of the innermost symbol around it, so a class
  inside a lookaround is a child of the lookaround.
- The `metadata.direction` and `metadata.positive` of a lookaround come from
  its opening token: `(?=`, `(?!`, `(?<=`, `(?<!`.
- Named groups have `capturing: "true"`: they capture and have a capture index.

## References

`\1`, `\k<name>`, and `(?P=name)` give a `references` relationship from the
innermost symbol to the target group. The two named forms also give a `call`
identifier with the group name.

## Literals

Each run of two or more word characters in one term is a literal row with
carrier `pattern` (`api`, `v1`, `users` in `^/api/v1/users$`). A quantifier
binds only to the character before it, so that character is not part of the
run: `colou?r` gives `colo`.

## Structural facts

- `regex.capture_group.v1`, `regex.named_capture.v1`, `regex.lookaround.v1`,
  `regex.character_class.v1`, `regex.quantifier.v1`, `regex.alternation.v1`.
  `branch_count` counts the direct branches of that alternation only.
- `regex.anchor.v1` for `^`, `$`, `\b`, `\B`, `\A`, `\z`, `\Z`, and `\G`.
- `regex.inline_flags.v1` for `(?i)` and `(?i-s:...)` with `enabled_flags`,
  `disabled_flags`, and `scoped`.
- `regex.backreference.v1` with `form` (`numeric`, `named`, `python_named`),
  the target index or name, and `resolved`.
- `regex.quoted_literal.v1` for `\Q...\E`.
- A possessive quantifier (`a++`) is a `regex.quantifier.v1` fact with
  `possessive: true`. The pinned grammar reads the second `+` as an error
  token.

The pinned grammar has no conditional group (`(?(1)a|b)`). That gap is an
`open_gaps` entry in `fixtures/extraction/capabilities.json`.

## Complexity

Every group and lookaround adds one nesting level. A scoped inline flag group
(`(?i:...)`) also nests; a bare `(?i)` does not.

## Body spans

The body span of a pattern, group, or lookaround is the node's
own span. Character classes and unicode properties have no body.

## Containment

Regex constructs sit side by side with no separator, so containment uses byte
ranges: a symbol contains a construct only when the construct's bytes are
inside the symbol's bytes. A backreference right after its group
(`(\w)\1`) is contained by the pattern, not by the group. Structural facts bind
to the innermost symbol by the same rule, so the root pattern owns the facts
for constructs that no smaller symbol holds.
