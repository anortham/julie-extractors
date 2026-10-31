# C support

`c` handles `.c` and `.h` files with `tree-sitter-c`.

## Types and receivers

- Variables, parameters, fields, and function return types record declared
  type facts. The fact keeps the base name (`struct widget *` -> `widget`),
  and `declared` keeps the written text. Multi-word sized types
  (`unsigned int`) record no fact.
- A GNU `__auto_type` or C23 `auto` variable gets an inferred type fact when
  its initializer calls a same-file function by name (`auto w = make();`).
  The fact is the declared return type of that function. All same-file
  declarations of the name must agree.
- The name must appear only in expected places anywhere in the file: as the
  name of a well-formed function declaration or definition, as the callee of
  a call, or as a struct, union, or enum tag. Any other use blocks the fact,
  because the call can then reach something other than the function.
  Examples are a same-named macro, variable, parameter, typedef, enumerator,
  assignment target (`make = pick;`), cast operand, address (`&make`), or
  macro argument (`LIST_HEAD(make);`, `EXPORT_SYMBOL(make);`).
- tree-sitter-c misparses many valid declarations, so a name in a misparsed
  place never counts as expected. This covers a name inside an error node,
  in a declaration with a parse error, in a definition with no function
  declarator, in an expression statement with a parse error, and in any
  file-scope expression statement (C has none, so the parser made it from a
  broken declaration). Examples are C23 `auto make = pick();`,
  `gadget_fn *const __attribute__((unused)) make = pick;`,
  `_Atomic(gadget_fn *) make;`, `fp_t (make);`, and a prototype with an
  object-like macro, such as `struct gadget *API make(void);` or
  `struct gadget *API (make)(void);`.
- These cases record no inferred fact: a member call (`ctx->make()`), a
  parenthesized call, any non-call initializer, a pointer declarator
  (`__auto_type *w`), a function that returns a function pointer, and a callee
  in another file. Other files are out of scope because each file is
  extracted alone.
- A function declared or defined with a macro between the return type and
  the name gives no fact, because tree-sitter-c misparses that declaration.
  The name then appears in a misparsed or unexpected place, so a same-named
  prototype elsewhere in the file gives no fact either. This holds for an
  object-like macro (`struct node *attr_pure find(void);`), a parenthesized
  name (`struct node *API (find)(void);`, `struct node *(API find)(void);`),
  and a macro with arguments (`struct node *NONNULL(1) find(void *p);`,
  `__attribute__((malloc))`, `__declspec(dllexport)`).
- A written type always wins: `auto int n = count();` records `int` as a
  declared fact.
- tree-sitter-c 0.24.2 has no rule for C23 `auto` inference. It reads
  `auto w = make(x);` as a prototype of `make` that returns the type `w`. The
  extractor reads it back as the variable `w`, a call to `make`, and a
  variable reference to `x`. The misread `w` and the `__auto_type` keyword are
  not type usages and make no `uses` relationships.

## Calls

- A call through a subscript (`handlers[i](x)`, `(*handlers[i])(x)`) names no
  callee: it writes no `calls` relationship and no pending call. The element
  that the subscript reads has no name, so the array or parameter name is not
  the callee. The call identifier still records the site.

## Test frameworks

A function is a test (`is_test`, `test_role: test_case`) in any of these cases:

- Its name starts with `test_` and its file is in a test path.
- The file includes `unity.h` (with quotes or angle brackets, in any folder),
  and the function is a non-`static` `void` definition whose name starts with
  `test`, `spec`, or `should`. These are the functions that Unity's
  `generate_test_runner.rb` collects.
- A call in the file registers it: Unity `RUN_TEST(name)` or
  `RUN_TEST(name, line)`, CMocka `cmocka_unit_test*(name, ...)`, or Check
  `tcase_add_test`, `tcase_add_loop_test`, `tcase_add_test_raise_signal`, and
  `tcase_add_exit_test`. A registration needs no include and no test path.
- It is a Check `START_TEST(name) { ... } END_TEST` block. The row is a
  private `function` named `name` with the signature `START_TEST(name)`.

`setUp` and `suiteSetUp` are `fixture_setup`, and `tearDown` and
`suiteTearDown` are `fixture_teardown`, in a test path or in a file that
includes `unity.h`. Check `tcase_add_checked_fixture` and
`tcase_add_unchecked_fixture` give their second argument `fixture_setup` and
their third `fixture_teardown`.

tree-sitter-c reads Check's `END_TEST` as the return type of the next
function. The extractor drops it, so `END_TEST Suite *make_suite(void)` has
the return type `Suite*`. The row of that function still starts on the
`END_TEST` line. The `START_TEST(name)` header and that `END_TEST` write no
identifier and no `uses` relationship.

A function named `test_mode` in a file with no test framework include, no
registration, and no test path is not a test. C++ files do not use these
rules, because Unity, CMocka, and Check are C frameworks.

## Includes

- Each `#include` is an import row named by the header path. Its metadata has
  `path`, `isSystemHeader`, `type: include`, `source` (the path without quotes
  or angle brackets), and `form: #include`. C++ shares this row.
- The path and `isSystemHeader` come from the directive's path node, so a
  trailing comment (`#include "a.h" // see <b>`) does not change them.
  `isSystemHeader` is `true` only for `<...>`.
- `#include MACRO` writes no row, because the header name is not in the source.

## Variable access facts

`variable_ref` and assignment-target `member_access` identifiers carry an
optional `metadata.access`: `read`, `write`, or `read_write`. Plain assignments,
initializers represented by a variable-reference row, increments, and
decrements are writes; compound assignments are read/write. For
`box->value = next`, `value` is a write and `box` is a read. Array bases and
indexes remain reads on assignment. Other classified occurrences, including
call arguments, are reads. Declaration names already represented by symbols do
not gain duplicate identifier rows. C has no pattern-binding construct. Calls
and type usages have no access key.
