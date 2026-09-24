# VB.NET support

Julie registers `vbnet` for `.vb` files. VB.NET shares the .NET test-role
rules with C# (see `csharp.md`).

## Continuous testing

Run the language target when changing VB.NET extraction:

```bash
cargo xtask test language vbnet
```

The command runs the VB.NET unit-test modules and the golden extraction test
with `JULIE_GOLDEN_LANGUAGE=vbnet`.

## Type facts

- Parameters, fields, properties, `Dim` locals with an `As` clause,
  and `Function` return types record declared type facts. `As New T(...)`
  records `T`.
- A `Dim`, `Static`, or `Using r = ...` local with no `As` clause gets an
  inferred type fact from its initializer:
  - `New T(...)` records `T` when the file declares class or structure `T`.
  - A call records the declared return type of a same-file `Function` or
    `Declare Function`. The call can be unqualified (`Load()`), on
    `Me`/`MyClass` (`Me.Load()`), or on a same-file class, structure, or
    module name (`Loader.Create()`). Names match without regard to case.
  - An unqualified call uses the innermost enclosing type that declares the
    name. If that type does not declare it, the search moves out. A type with
    an `Inherits` clause stops the search, because the base can declare the
    name. Same-file module functions come last.
  - `Await` removes one `Task(Of T)` or `ValueTask(Of T)` layer.
    `ConfigureAwait(...)` keeps the task type.
- These initializers record no fact:
  - Same-named candidates with different return types, a `Sub`, a `Function`
    without `As`, or a property, event, or field with the called name.
  - A return type that is a type parameter of the method or of an enclosing
    class.
  - A chain that ends in any other member (`Load().Name`), a bare name without
    parentheses (`Load`), and `MyBase.Load()`.
  - A called name or qualifier that is also a local, a parameter, or the
    enclosing member, because VB resolves the name to that variable first. A
    qualifier that is also a member of an enclosing type is skipped for the
    same reason.
  - A callee in another file. Each file is extracted alone, so a type from
    another file can go stale.
- A written `As` type always wins over inference.
