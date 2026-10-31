# CSS support

`css` handles `.css` files and the `<style>` blocks of HTML and Vue files.

## Custom functions

tree-sitter-css has no grammar for CSS custom functions (`@function`), and it
splits a dashed name that holds a non-ASCII code point or an escape. The
extractor reads the names and calls from the source with the CSS Syntax
tokenizer rules instead:

- An ident code point is an ASCII letter, a digit, `-`, `_`, or any code point
  at or above U+0080, as browsers parse it. So `--名_css` and `-- _css`
  (U+2000 EN QUAD) are whole names. The current CSS Syntax editor's draft
  narrows the non-ASCII ident code points to listed ranges that leave out
  U+2000; the extractor follows the browsers.
- Whitespace is space, tab, and the newlines. A non-ASCII space is part of the
  name or text it is in, also in the `prelude` and `ruleName` metadata of
  every at-rule.
- A name is written with its escapes decoded: `--foo\31 ` is `--foo1`, and
  `--escaped\ name` is `--escaped name`. CSS compares names after decoding, so
  a declaration and a call match when they decode to the same string. The raw
  spelling stays in the row's source span and in the signature. Zero, a
  surrogate, and a value above U+10FFFF decode to U+FFFD.

An `@function --name(...)` rule is a `function` symbol named by its decoded
dashed name (`--anchor_css`). Its signature is the rule text, and its metadata
keeps `type: at-rule`, `atRuleType: function`, `prelude`, and `ruleName`
(`@function --anchor_css()`). A rule whose prelude does not open with a dashed
name and a `(` stays a `namespace` symbol named by its rule name.

A dashed function token (`--name(` with no space before the `(`) outside a
comment, a string, or an unquoted `url()` is a custom function call:

- It is a `call` identifier named by the decoded name, with the span of the
  raw name. It is recorded in every value, including a `result:` descriptor
  of another custom function and a nested call.
- Its metadata holds `arg_count`: the comma-separated arguments at the top
  level of its parentheses. Commas inside `()`, `[]`, `{}`, strings, and
  comments do not count, and an empty list has 0. An argument list with no
  closing `)` gives no `arg_count`. Other CSS calls (`calc()`, `var()`) carry
  no `arg_count`, because their arguments are not comma-separated by rule.
- A `calls` relationship goes from the innermost symbol around the call to
  each `@function` declaration with the same name in the file, with the
  metadata `referenceType: custom-function` and `referenceName` (the decoded
  name).
- Custom functions are global across the loaded stylesheets. So a call with
  no `@function` of that name in the file is a pending `calls` relationship:
  the target name is the decoded name, the from-symbol and the caller scope
  are the innermost symbol around the call (the rule `.card`, or the
  `@function` whose `result:` holds it), the span is the raw name, and the
  confidence is 0.7. A call with a same-file declaration has its `calls`
  relationship and no pending row.
- A call with no symbol around it (a declaration outside every rule, such as
  `width: --f();` at the top of a file) keeps its identifier with no
  containing symbol, and has no relationship and no pending row, the same as
  a top-level call in Python, JavaScript, or Lua.

### Known limits

- An `@function` rule with a `returns` clause (`@function --a(--x) returns
  <length> { ... }`) does not parse as an at-rule in tree-sitter-css, so it has
  no symbol. Its calls are still identifiers.
- Literal arguments of a custom function call are recorded only when
  tree-sitter parses the call as a call expression.

## Imports

- `@import "base/reset.css";`, `@import url("theme.css");`, and
  `@import url(plain.css);` are import rows named by the target path.
- Each row has `source` (the path without quotes or `url()`) and
  `form: @import`. The older keys stay: `url` is the same path, and
  `ruleName`, `atRuleType`, and `type` describe the at-rule.
- Text after the target, such as media queries, `layer(base)`, or
  `supports(...)`, is in `media`.

## Variable access facts

Variable access classification is not applicable to native CSS. Custom-property
declarations and `var()` references describe stylesheet values rather than
program-variable assignment sites; their `metadata_json.access` stays absent.
