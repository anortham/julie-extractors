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
