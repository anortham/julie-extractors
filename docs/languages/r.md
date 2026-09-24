# R support

`r` handles `.R` and `.r` files with `tree-sitter-r`. This page covers the
type facts that the R extractor infers. The capability rows are in
`fixtures/extraction/capabilities.json`, and the full rules with every
no-fact case are in
`docs/decisions/2026-09-08-receiver-type-facts-wave-2.md` (section "R:
declared types").

## Inferred variable types

R has no static types, so every R type fact is inferred (`is_inferred = 1`).
A variable assigned with `<-`, `=`, `<<-`, `->`, or `->>` gets a type from
its initializer in these cases:

- A same-file constructor: `Foo$new()` for an R6 or Reference Class
  generator, `new("Foo")`, or `Foo(...)` for a class that the same file
  declares with `setClass`, `setRefClass`, `R6Class`, or `new_class`.
- A call `f(...)` to a same-file S4 generic declared as
  `setGeneric("f", function(...) ..., valueClass = "X")`. R stops with an
  error unless the value of such a generic is an `X`, so the variable gets
  `X`. The class need not be declared in the same file.
- Parentheses are unwrapped. `lhs |> f(...)` counts as `f(lhs, ...)`, the
  call that R's parser makes of it.

The extractor records no type when the name might mean something else. For
example:

- `f` is rebound anywhere in the file: by an assignment, a parameter, a
  `for` variable, `assign()`, `list2env()`, a `with()` data list, a
  replacement call such as `body(f) <- v`, or an environment member such as
  `.GlobalEnv$f <- v`.
- `f` is a Reference Class method or field, which the class's methods see as
  a bare name.
- The call is inside a Reference Class definition, where a bare name can
  reach an inherited member or a built-in such as `copy()`.
- The call is inside `with()` or `within()` and the data is not a
  `list(...)` literal, for example a variable.
- The call is at top level before the first `setGeneric` of its name. R runs
  top-level code in order, so the call reaches another function.
- `Foo(...)` for a class `Foo` also has a same-file `setGeneric("Foo")`. The
  call then takes only the generic's `valueClass`, or nothing.
- The `setGeneric` may not run (under `if`, a loop, `switch()`, `&&`, `||`,
  or in a function body), binds with `where =`, has no `def` function, or
  disagrees with another declaration.
- `pkg::f(...)`, a chained call such as `f(x)$m()`, and a roxygen `@return`
  tag, which is prose.

Known limits: a name bound with a computed string (`assign(nm, ...)`) and code
run through `eval()` or `source()` record nothing. A call inside a function
body is typed even when the function sits before the `setGeneric`, because
package code defines all generics before any function runs. A script that
calls that function at top level before the declaration gets a wrong type. Each file is extracted alone, so a
generic or class in another file gives no type.
