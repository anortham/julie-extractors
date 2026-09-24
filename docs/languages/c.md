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
  reach something other than the function.
- These cases record no inferred fact: a member call (`ctx->make()`), a
  parenthesized call, any non-call initializer, a pointer declarator
  (`__auto_type *w`), a function that returns a function pointer, and a callee
  in another file. Other files are out of scope because each file is
  extracted alone.
- A written type always wins: `auto int n = count();` records `int` as a
  declared fact.
- tree-sitter-c 0.24.2 has no rule for C23 `auto` inference. It reads
  `auto w = make(x);` as a prototype of `make` that returns the type `w`. The
  extractor reads it back as the variable `w`, a call to `make`, and a
  variable reference to `x`. The misread `w` and the `__auto_type` keyword are
  not type usages and make no `uses` relationships.
