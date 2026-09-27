# Vendored tree-sitter-regex

This parser is based on `tree-sitter-regex` 0.25.0 from
`tree-sitter/tree-sitter-regex`, upstream commit
`b2ac15e27fce703d2f37a79ccd94a5c0cbe9720b` (MIT license). The upstream
grammar did not parse conditional or atomic groups. `grammar.js` adds
conditional-group rules, an atomic-group rule and a branch-reset rule for
`(?|...)`. The atomic rule keeps atomic
patterns inside the root `pattern` node while the conditional rule handles the
shared `(?` prefix. `src/parser.c` and `src/node-types.json` are generated from
that grammar with tree-sitter CLI 0.26.11 using:

```sh
CARGO_TARGET_DIR=/home/murphy/source/julie-extractors/target TMPDIR=/home/murphy/source/julie-extractors/target/reference-integrity-tmp /usr/bin/tree-sitter generate
```

The command ran with `vendor/tree-sitter-regex` as its working directory. The
CLI used parser ABI 14 because the vendored grammar does not need a
`tree-sitter.json` file.

The local package version `0.25.0-julie.1` marks this grammar patch. The
upstream license is preserved in `LICENSE`.
