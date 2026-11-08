# Vendored tree-sitter-sequel-tsql

This crate is based on `tree-sitter-sequel-tsql` 0.4.2 from
`anortham/tree-sitter-sql`, upstream commit
`63ea933e464813d01cab5d7febcb0f77409c247b` (MIT license). The local crate
version `0.4.2-julie.1` identifies the grammar corrections maintained here.

The Rust build script is formatted with the workspace's Rust toolchain; this
does not change the grammar, generated parser, or scanner.

The scanner uses `size_t` for the dollar-quote tag index, so the index and the
buffer size compare with the same signedness. This removes a C compiler
warning and does not change scanner behavior.

The corrections extend T-SQL routine declarations and inline table-valued
functions, system-time table syntax, sequence defaults, clustered primary keys,
transaction statements, and scalar/compound `SET` assignments. The additional
expression conflicts keep clause reductions available while T-SQL `CASE`,
`EXISTS`, and unparenthesized `RETURN SELECT` expressions continue through the
shared precedence grammar. They are maintained in `grammar.js`; `src/grammar.json`,
`src/node-types.json`, `src/parser.c`, and `src/scanner.c` are generated inputs
for consumer builds.

Regenerate the parser with tree-sitter CLI 0.26.11 from this directory:

```sh
tree-sitter generate
```

The consumer build uses the checked-in C parser and scanner and does not require
the tree-sitter CLI. The upstream MIT license is preserved in `LICENSE`.
