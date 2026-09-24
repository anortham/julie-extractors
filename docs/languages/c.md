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
  declarations of the name must agree. A same-named macro, variable, or
  parameter anywhere in the file blocks the fact, because the call can then
  reach something other than the function. A name in a declaration with a
  parse error also blocks the fact, because its type cannot be trusted. This
  includes C23 `auto make = pick();` and a local whose initializer uses a
  macro with a type argument, such as `container_of(p, struct holder, node)`.
  A local that tree-sitter-c reads as a call does not block the fact. Examples
  are a parenthesized declarator such as `fp_t (make);` and a variable that a
  macro declares, such as `LIST_HEAD(make);`.
- Some valid declarations make tree-sitter-c emit error nodes or expression
  statements instead, for example `gadget_fn *__attribute__((unused)) make =
  pick;`, `_Atomic(gadget_fn *) make = pick;`, or a prototype with an
  object-like macro such as `LIBAPI struct gadget *API make(void);`. So these
  names block the fact anywhere in the file: a name inside an error node, a
  name in an expression statement with a parse error, the target of an
  assignment (`make = pick;`), any name in a file-scope expression statement
  (C has none, so the parser made it from a broken declaration), and the
  misread type of a function definition that has no function declarator. A
  file-scope macro call such as `EXPORT_SYMBOL(make);` therefore also blocks
  the fact for `make`.
- These cases record no inferred fact: a member call (`ctx->make()`), a
  parenthesized call, any non-call initializer, a pointer declarator
  (`__auto_type *w`), a function that returns a function pointer, and a callee
  in another file. Other files are out of scope because each file is
  extracted alone.
- A function declared or defined with a macro between the return type and
  the name (`struct node *attr_pure find(void);`) gives no fact, because
  tree-sitter-c misparses that declaration. The name is then unknown anywhere
  in the file, so a same-named prototype elsewhere gives no fact either. A macro with arguments in that place
  (`struct node *NONNULL(1) find(void *p);`, `__attribute__((malloc))`,
  `__declspec(dllexport)`) makes the name unknown anywhere in the file, so
  the name gives no fact.
- A written type always wins: `auto int n = count();` records `int` as a
  declared fact.
- tree-sitter-c 0.24.2 has no rule for C23 `auto` inference. It reads
  `auto w = make(x);` as a prototype of `make` that returns the type `w`. The
  extractor reads it back as the variable `w`, a call to `make`, and a
  variable reference to `x`. The misread `w` and the `__auto_type` keyword are
  not type usages and make no `uses` relationships.
