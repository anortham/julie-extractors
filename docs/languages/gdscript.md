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
  the current scope or of a scope around it, or the script's `class_name`.
- `await expr` and `(expr)` pass the type of `expr` through.

## Initializers that record nothing

- The callee has no return type, or returns `void`.
- `await` on a call that returns `Signal`. Awaiting a signal gives the signal
  arguments, not a `Signal`. The call without `await` records `Signal`.
- Duplicate declarations of one function in one scope that disagree on the
  return type.
- A function that only a base class declares, also when the base class is in
  the same file. `class B extends A` inherits from `A`, but `B` can also
  override the function or `A` can be another file's class. The extractor does
  not follow `extends`.
- A bare call from an inner class to a function of an outer class, or from the
  script to a function of an inner class.
- `Foo.f()` when two same-file classes named `Foo` are visible at the call
  site, or when `Foo` does not declare `f`.
- `Foo.f()` whose return type names a class that means a different class, or
  no same-file class, at the call site. For example, `A.make() -> Result`
  where `Result` is `A.Result` records nothing at the script level, because
  `Result` there means a different class or a class in another file.
- Any other receiver: locals, `super`, other files' classes, and qualified
  receivers such as `a.Inner.build()`.
- A chain that ends in any other call, such as `load_thing().open()`, and
  global functions such as `load()`.

Other files are out of scope because each file is extracted alone.
