# Upstream provenance

The grammar is vendored from `https://github.com/LuisCarreras/tree-sitter-vb-dotnet` at commit `25dca4ac1456c691e2381a2d76151ef432aefc9e`, the revision pinned before vendoring. The upstream package version is `0.1.0` and its MIT license is retained in `LICENSE`.

The local package version is `0.1.0-julie.1`. Its only grammar change accepts repeated parameter attribute blocks, matching VB declarations such as `<[In]> <Out> ByRef value As Integer`. The generated `src/grammar.json`, `src/node-types.json`, and `src/parser.c` use Tree-sitter CLI `0.26.11`.

The Cargo manifest declares a standalone workspace so Cargo metadata and formatting work inside linked checkouts without inheriting the parent repository's workspace.

SHA-256 identities:

- `grammar.js`: `41cd46e63ae1edcfc4ef4580490ea9bec7fe8a2e53c621eaf880552c1515f946`
- `src/grammar.json`: `925b3ad6aaddef5dec1652d230e25a3bd218009494f659c1ae8aeae8c1e5d699`
- `src/node-types.json`: `873cc7b36a4be88125dd25a76eb1bc8a46efda13eb9689f80e6b4cd72c2f50aa`
- `src/parser.c`: `92403fb32545defd5f8c3529875e22be533ba6570df779d348f2463e3dcdf5b2`
- External scanner: none; `src/scanner.c` is absent.

Two consecutive runs of the command below with Tree-sitter CLI `0.26.11` produced identical hashes for all four files. The captured verification is `../../.razorback/sdd/2026-11-03-language-coverage-goal-54245db20e09/logs/task-3-native-vb-generator-repro.log`.

Run the pinned generator with:

```sh
cd vendor/tree-sitter-vb-dotnet
tree-sitter generate
```
