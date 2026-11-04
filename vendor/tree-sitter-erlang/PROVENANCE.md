# Vendored Erlang grammar

This package is based on the published `tree-sitter-erlang` 0.21.0 crate from
crates.io. The archive SHA-256 is
`8079a9cfe733c3b6749bb3c683c0e997ee68a2305f7a210800bbbc53e82d3f64`.
Its VCS metadata names commit
`0386a11c835950f5e1ebfbcbf49863d8495de4e0`. The crate's `grammar.js`,
generated parser, node types, scanner and license match commit
`62c82e67b73e13f3ef06e4c8c07ff035a8235d30` in
`WhatsApp/tree-sitter-erlang` byte for byte.

The published Cargo metadata labels the crate MIT, while its `LICENSE` and
`README.md` state Apache-2.0. This package retains that license file and uses
Apache-2.0 metadata. The upstream repository has no separate NOTICE file.

The grammar adds a catch-clause form for an opaque macro head followed by a
body without a literal arrow. It keeps the macro head in `macro_head` and body
expressions in `macro_body`; ordinary catch patterns continue to use the
existing `pat`, `class`, and `stack` fields. The generated parser and node
schema correspond to this grammar, with no scanner change.

Generate from this directory with Tree-sitter CLI 0.26.11:

```sh
TMPDIR=../../target/parser-investigation/erlang-tmp \
CARGO_TARGET_DIR=../../target tree-sitter generate
```
