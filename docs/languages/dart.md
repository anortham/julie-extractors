# Dart support

`dart` handles `.dart` files with `tree-sitter-dart` `0.2.0`.

This page records the type-fact contract for local variables. Other Dart
behavior is covered by the golden fixtures and
`fixtures/extraction/capabilities.json`.

## Local variable type facts

A written type always wins: `Item x = load();` records `Item` as declared.

A `var` or `final` local with no written type gets an inferred type fact when
its initializer is one of these forms:

- `Type(..)`, `new Type(..)`, `const Type(..)`, or `Type.named(..)` for a
  same-file type with no same-file static member of that name: records `Type`.
  `Type(..)` and `Type.named(..)` also need that no parameter, local, local
  function, or enclosing-type member is named `Type`, because Dart finds
  those before the type. With a member named `Type`, `Type(..)` records the
  return type of that member.
- `load()`: a same-file top-level function with a declared return type. It
  may be declared before or after the use.
- `m()` inside a class, mixin, enum, named extension, or extension type: the
  enclosing type's own member first, then the library function.
- `this.m()`: a member of the enclosing class, mixin, enum, or extension
  type.
- `Type.m()`: a static member of a same-file class, enum, extension, or
  extension type.

Wrappers around those calls:

- `await` removes one `Future` or `FutureOr` layer. `await` of a
  `Future<Item>?` records `Item?`.
- `!` removes nullability, also after a parenthesized `await`.
- A cascade (`load()..touch()`) and explicit type arguments
  (`many<int>()`) keep the call's type.
- A `Future` return with no `await` records the `Future<T>` text.

These cases record no fact, because the type would be a guess:

- The callee is in another file. Each file is extracted alone.
- The return type is a type parameter of the function, class, or extension
  type, or is `void` or `dynamic` (also `await` of `Future<dynamic>`).
- `await` of a type that is not a `Future` or `FutureOr`.
- Same-named candidates in one scope disagree on the return type.
- A parameter, local variable, local function, or pattern binding anywhere
  in the file has the callee's name or the constructed type's name.
- The name is a getter, setter, field, enum constant, or extension type
  representation field in that scope. This includes abstract, external,
  `external static`, and `covariant late final` fields.
- `super.m()`, a call on any other variable, an import prefix, `this` outside
  a type, an inherited member, or a class member called from outside its
  class.
- Any call inside an unnamed extension.
- `this.m()` inside any extension. Dart looks up `m` on the on-type first,
  and the on-type is usually declared in another file or the SDK.
- A chain that ends in any other method (`Type.create().load()`, `then()`),
  a `?.` call, or a property access.
