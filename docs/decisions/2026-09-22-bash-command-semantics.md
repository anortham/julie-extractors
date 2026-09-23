# Bash command semantics

Date: 2026-09-22. Plan: [language gap closure](../plans/2026-09-22-language-gap-closure.md),
wave 2 (the `bash.*` gaps: wrapped commands, declarations, doc comments,
test conventions, HTTP clients, heredoc SQL).

## Decision

- **Wrapped commands.** `sudo`, `doas`, `exec`, `time`, `timeout N`, `nohup`,
  `nice`, `setsid`, `env`, `command`, `builtin`, and `xargs` run their first
  non-option argument. That command gets the call identifier, the call edge,
  and the literal carrier (`sudo curl "https://..."` records a `curl` URL). The
  wrapper gets its own call identifier but no pending call. `command -v x`
  only looks `x` up, so it runs nothing.
- **Registered handlers.** `trap HANDLER SIG...` calls the handler word, or
  the first word of a quoted handler (`trap 'notify "x"; exit 1' INT`), at the
  exact byte range of that word. `complete -F fn` calls `fn`.
- **Test-file words.** In a test path (bats `.bats`, `*_test.sh`,
  `*_spec.sh`, `test/`, `spec/`), bats `run X` and ShellSpec `When call X`
  run `X`, bats `load` and ShellSpec `Include` import a file, and ShellSpec
  hooks (`BeforeEach 'fn'`, `AfterAll fn`, ...) become lifecycle symbols that
  call their functions. `bats_load_library` imports everywhere. A function the
  file defines under a wrapper name (`run() { ...; }`) stays a plain call.
- **Builtins and dynamic names.** Shell builtins (`trap`, `shift`, `getopts`,
  `let`, `time`, `wait`, `ulimit`, ...) emit no pending calls. A command name
  with an expansion (`"$DIR/tool"`, `$cmd`) emits no call identifier or
  pending call. A quoted static name (`"./bin/tool"`) loses its quotes.
- **Declarations.** A declaration command emits one symbol per name, with no
  second copy from the assignment. `readonly` and a `-r` flag make a
  `constant`; `export` and a `-x` flag make it `public`. Bare names
  (`local x`, `declare -A map`) are symbols. A bare `export NAME` or
  `readonly NAME` after `NAME=...` in the same scope updates that symbol.
  `-g` declares a global, so the symbol has no parent. `-a` and `-A` type the
  name as an `array`. `-f`, `-p`, and `export -n` declare nothing. Command prefixes
  (`IFS= read`) and element writes (`map[k]=v`) declare nothing. A plain
  ALL_CAPS assignment stays a `constant`.
- **Export facts.** `bash.export_declaration.v1` covers every exporting
  declaration (`export`, `declare -x`, `local -x`) and lists all names in
  `variable_names`.
- **Doc comments.** A doc comment is the `#` block directly above a
  declaration. A blank line ends it, a comment after code on its line is not
  part of it, and the `#!` line never is. Only comments in such a block are
  `doc_comment` regions; every other comment is a `comment` region with its
  containing symbol.
- **Body spans.** Functions keep their compound body. Assignments use their
  value. Imports, bare declarations, and positional parameters have none.
- **HTTP clients.** `curl` and `wget` calls with a static URL emit
  `http.client_request.v1`. The verb comes from `-X`/`--request`/`--method`
  (`attested`), from `-I`, `-G`, `-T`, or a data flag (`attested`), and is
  otherwise `GET` (`default`). Only request arguments are URL literals, so a
  `-H` header is not.
- **Heredoc SQL.** A herestring and a heredoc body are string literals of
  their command, after its arguments. Expansions in an unquoted heredoc
  become `{}`. A literal that is only holes (`"$DB"`) is dropped.
- **No receivers.** The CLI does not derive a text receiver for Bash
  identifiers: Bash has no member access, and the text scan read a comment's
  final `.` as a separator.

## Why

The old rules made `trap cleanup EXIT` handlers look dead, added a fake
`IFS` constant to almost every script, emitted two conflicting symbols for
each declared name, and attached the shebang and file banners as the doc
comment of the first declaration. Test wrappers and hooks carry the edges that
link a test to the code it runs, but the same words are ordinary commands in
production scripts, so the test-path gate keeps them apart.
