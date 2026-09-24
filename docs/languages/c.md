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
