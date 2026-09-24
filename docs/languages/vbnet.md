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
    name. If that type does not declare it, the search moves out. An open
    type stops the search: a type with an `Inherits` clause, a type with the
    `Partial` modifier, or a type that has more than one part in the file.
    The base or the other part can declare the name. Same-file module
    functions come last.
  - A module or a qualifier type must be in scope: in the caller's namespace
    or an enclosing one, or in a namespace that an `Imports` clause without
    an alias names. A nested qualifier type is in scope only inside the type
    that declares it.
  - At least one candidate must take the call's argument count. `Optional`
    parameters can be left out, and a `ParamArray` takes any count.
  - `Await` removes one `Task(Of T)` or `ValueTask(Of T)` layer. In
    `Await LoadAsync().ConfigureAwait(False)`, the awaited result has the
    task's type argument.
- These initializers record no fact:
  - Same-named candidates with different return types, a `Sub`, a `Function`
    without `As`, or a property, event, or field with the called name.
  - A return type that is a type parameter of the method or of an enclosing
    class.
  - Arguments that no candidate takes. For a function with no parameters,
    VB applies the arguments to the result (`Load(0)` indexes the returned
    array or list), so the local does not get the return type.
  - A candidate with the `Overloads` or `Overrides` modifier in an open
    type. The base overloads stay visible, and the call can bind to one of
    them.
  - `ConfigureAwait(...)` without `Await`. The result is a configured
    awaitable, not a task.
  - A chain that ends in any other member (`Load().Name`), a bare name without
    parentheses (`Load`), and `MyBase.Load()`.
  - A called name or qualifier that is also a local, a parameter, a lambda
    parameter, or the enclosing member, because VB resolves the name to that
    variable first. A qualifier that is also a member of an enclosing type is
    skipped for the same reason.
  - An unqualified call to an `Object` member name (`ToString()`,
    `Equals()`, `GetHashCode()`, `GetType()`, `ReferenceEquals()`,
    `MemberwiseClone()`, `Finalize()`). It binds to the inherited member
    before a module function.
  - A module or a qualifier type from a sibling namespace that no `Imports`
    clause names.
  - A qualified or generic qualifier (`NS.Loader.Create()`,
    `Loader(Of T).Create()`).
  - A callee in another file. Each file is extracted alone, so a type from
    another file can go stale.
- Known limits of same-file inference:
  - VB lets one part of a partial type omit the `Partial` keyword. If this
    file holds only that part, the extractor cannot see the other part. That
    part can add an `Inherits` clause or a member with the called name, as
    the WinForms designer file does.
  - `Me.F()` and `Loader.F()` on a `Partial` type use the members in this
    file. Another part can add an overload with a different return type.
- A written `As` type always wins over inference.
