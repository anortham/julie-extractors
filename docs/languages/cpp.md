# C++ support

Julie extracts C++ symbols, calls, member accesses, types, includes, test roles,
and source locations from the tree-sitter C++ syntax tree. Qt projects use the
same `cpp` registration; see [`cpp-qt.md`](cpp-qt.md) for Qt macro handling and
Qt-specific rows.

## Variable access facts

`variable_ref` and assignment-target `member_access` identifiers carry an
optional `metadata.access`: `read`, `write`, or `read_write`. Plain assignments,
initializers represented by a variable-reference row, increments, and
decrements are writes; compound assignments are read/write. For
`box->value = next`, `value` is a write and `box` is a read. Array bases and
indexes remain reads on assignment. Other classified occurrences, including
call arguments, are reads. Declaration names already represented by symbols do
not gain duplicate identifier rows. Calls and type usages have no access key.
C++ has no general pattern-matching binding syntax; structured-binding names
represented by symbols do not gain duplicate identifier rows.
