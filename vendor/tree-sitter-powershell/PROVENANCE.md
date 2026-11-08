# Vendored tree-sitter-powershell

This package is based on the published `tree-sitter-powershell` 0.26.6 crate,
from `airbus-cert/tree-sitter-powershell` upstream commit
`65ea6919e816a7ac142acec7ee72757f89db8439`. The source archive SHA-256 is
`fa49d15e5b291030a548447445e0547992eff1438e7cd9133bfec99a9ba962a9`.
The upstream MIT license is preserved in `LICENSE`.

The local grammar addition recognizes a simple variable immediately followed
by path text as one command argument, including later adjacent variable/path
segments. The `compound_argument` node retains each variable child and the
source text, so extraction can preserve `$PSScriptRoot` and `$folder` without
expanding either. Comma-list parsing comes from upstream 0.26.6. The
argument-mode rule follows [PowerShell's parsing documentation][parsing].

The crate version is `0.26.6-julie.1`. Its consumer build uses the checked-in
parser and scanner with `tree-sitter-language` 0.1; it does not run a parser
generator.

The Rust build script is formatted with the workspace's Rust toolchain; this
does not change the grammar, generated parser, or scanner.

The scanner marks unused parameters with `(void)` casts, and its internal
helpers take no `payload` argument. This removes C compiler warnings and does
not change scanner behavior. `tree-sitter generate` does not write
`src/scanner.c`, so the recorded hash covers this hand-maintained file.

Generated files use Tree-sitter CLI 0.26.11. From this directory, the exact
command is:

```sh
tree-sitter generate
```

The command was run and repeated on 2026-11-04 UTC. The source hashes before
and after the repeat were identical:

| File | SHA-256 |
| --- | --- |
| `grammar.js` | `56c87499d6ad2788daae3237e645bf804af452f6030a24b24f822e98077144de` |
| `src/grammar.json` | `08bcc772bb04181d734c5d0ae87a6c0b61a98c3f0814987dad462fe4cb916d70` |
| `src/node-types.json` | `110df696fb332be510f98e92ea7901b5584d8b7a7cd3ee622b806603375070fc` |
| `src/parser.c` | `fc7ef4bf342c37df2ca2ec9771222ed3bd7e2e3a8fc2f3ce7fb5b162c282c297` |
| `src/scanner.c` | `7455decc1e23e759d12394682470cad0ac552771ab169bd6f6e60e8372003784` |

[parsing]: https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_parsing#argument-mode
