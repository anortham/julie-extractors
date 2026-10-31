# Bash support

`bash` handles shell scripts and `.bats` files with `tree-sitter-bash`.

## Imports

- `source path` and `. path` are import rows named by the file stem
  (`source ./lib/common.sh` is `common`). In a test file, bats `load` and
  ShellSpec `Include` are import rows too, and so is `bats_load_library`.
- Each row has `source` and `form`. `form` is the command (`source`, `.`,
  `load`, `Include`, `bats_load_library`). `source` is the path as written
  after quote removal: `"$DIR"/common.sh` is `$DIR/common.sh`. Variables and
  command substitutions keep their text.
- The older keys stay: `sourceCommand` is the command, and `sourcePath` is the
  argument with only its outer quotes removed.
- `alias ll='ls -l'` is an import row named `ll` with `aliasTarget`, `command`,
  and `form: alias`. It loads no file, so it has no `source`.

## Test labels

A test or a test container named by a string literal takes the text that the
literal writes: bats `@test "adds \"three\""` and the ShellSpec `Describe` and
`It` forms. The label in `"adds \"three\""` is `adds "three"`. In a
double-quoted label, `\` before `$`, `` ` ``, `"`, or `\` writes that character,
and every other `\` stays as written. A single-quoted label keeps its text as
written. An expansion is `{}`.

## Variable access facts

Bash reference identifiers receive access metadata when the syntax is clear. Reassignment and arithmetic assignment targets are writes; compound assignments and increments are read/write. Loop bindings are writes. Variable expansions in unquoted words leave the key absent because word splitting can change which value each token names. Expansions passed to `eval` also leave the key absent. Quoted expansions, scalar assignment values, test expressions, and subscript indices remain reads. An initial assignment represented by a variable symbol does not add a duplicate reference.
