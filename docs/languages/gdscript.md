# GDScript support

`gdscript` handles `.gd` files with `tree-sitter-gdscript`. This page records
the type-fact rules for variables. Receiver facts are in
`docs/decisions/2026-09-08-receiver-type-facts-wave-2.md`.

## Variable type facts

A `var`, `const`, `@export var`, or `@onready var` gets at most one type fact.
The first rule that applies wins:

1. A written type (`var x: Node = ...`) is a declared fact. It always wins.
2. `expr as T` records `T` as an inferred fact.
3. The initializer resolves to a type from declarations in the same file. The
   result is an inferred fact.

`Array[T]` and other generic returns record the base type (`Array`) with the
full text in the `declared` metadata, the same as a written annotation.

## Initializers that resolve

A "scope" below is the script class or one inner class. Each class is known by
its full path from the script (`Outer.Inner`), not by its bare name, so two
inner classes with the same name never share functions.

- `Foo.new()`: records `Foo` when the file declares a class or `class_name`
  named `Foo`.
- `f()` and `self.f()`: records the declared return type of `f` in the scope
  that holds the variable.
- `Foo.f()`: records the declared return type of `f` in class `Foo`. `Foo`
  must mean exactly one same-file class at the call site: an inner class of
  the current scope or of a scope around it, an inner class that one of
  those scopes inherits from a same-file base class, or the script's
  `class_name`. No parameter or local of the function or accessor
  (`set(value):`, `get:`), and no other member (const, var, enum, signal,
  function) of those scopes or their same-file base classes, can have the
  name `Foo`.
- `await expr` and `(expr)` pass the type of `expr` through.

## Initializers that record nothing

- The callee has no return type, or returns `void`.
- `await` on a call that returns `Signal`. Awaiting a signal gives the signal
  arguments, not a `Signal`. The call without `await` records `Signal`.
- Duplicate declarations of one function in one scope that disagree on the
  return type.
- A bare call to a function that only a base class declares, also when the
  base class is in the same file. The call records only a function that the
  scope itself declares.
- `Foo.f()` or `Foo.new()` when the name `Foo` means more than one same-file
  declaration once same-file base classes count. For example, in
  `class Derived extends Base`, `Item.make()` records nothing when both the
  script and `Base` declare a class `Item`, and `Foo.make()` records nothing
  when `Base` has `var Foo`. A return type such as `Kind` records nothing when
  `Kind` means a class of `Base` at the call site but a different class where
  the function is written.
- Any `Foo.f()` or `Foo.new()` inside a class whose same-file `extends`
  cannot be resolved to exactly one class, such as `extends Base.Missing`, or
  whose `extends` chain loops. The extractor cannot know which names that
  class inherits.
- A bare call from an inner class to a function of an outer class, or from the
  script to a function of an inner class.
- A bare call `f()` when `f` is a built-in type (`String`, `Vector2`), a
  GDScript utility function (`load`, `range`, `len`), or a `@GlobalScope`
  utility function (`str`, `max`, `print`). Godot resolves these names
  before any method, so `func load()` in the class does not change what
  `load()` returns. `self.load()` calls the method and records its type.
- `Foo.f()` when two same-file classes named `Foo` are visible at the call
  site, or when `Foo` does not declare `f`.
- `Foo.f()` or `Foo.new()` when a parameter, a local, a `for` variable, or a
  class member other than a class has the name `Foo`. Godot resolves the
  nearer name first, so `Foo` is not the class there. A local declared
  anywhere in the function counts, also one in another block.
- `Foo.f()` whose return type names a class, enum or const that means a
  different declaration, or no same-file declaration, at the call site. For
  example, `A.make() -> Result` where `Result` is `A.Result` records nothing
  at the script level, because `Result` there means a different class or a
  class in another file. The same holds for `A.kind() -> Kind` where `Kind`
  is an enum of `A`, and for `const Res = preload(...)` in `A`.
- Any other receiver: locals, `super`, other files' classes, and qualified
  receivers such as `a.Inner.build()`.
- A chain that ends in any other call, such as `load_thing().open()`, and
  global functions such as `load()`.

Other files are out of scope because each file is extracted alone.
