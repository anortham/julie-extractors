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

`regex.conditional.v1` records the written condition and branch count
for numeric, relative numeric, named, assertion and reserved conditional
forms. Capture conditions reference the declared group without changing
capture numbering. The conditional separator is not a second alternation.

The parser is the licensed local `tree-sitter-regex 0.25.0-julie.1` patch
under `vendor/tree-sitter-regex`. Generated parser sources ship with the
crate; consumer builds need no parser generator. The `conditionals`
fixture covers conditional syntax and capture references.
Atomic groups `(?>...)` parse without changing the numbers of captures nested
inside them and contribute to nesting depth.

`regex.branch_reset.v1` records a PCRE2 branch-reset group `(?|...)` and
its direct `branch_count`. Each branch restarts capture numbering at the same
number, and captures after the group continue from the widest branch, so
`(?|(a)(b)|(c))(d)` numbers `a`, `c` as 1, `b` as 2 and `d` as 3. Capture
facts and symbols carry these shared numbers. A numeric or named reference to
a shared number or name references every group that holds it. Relative
conditions count from the captures opened so far, as PCRE2 does. The
`branch_reset` fixture covers nesting, named branches and relative conditions.

## Complexity

A conditional adds one decision. Every group and lookaround adds one nesting level. A scoped inline flag group
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
