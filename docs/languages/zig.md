# Zig support

`zig` handles `.zig` files with `tree-sitter-zig`.

## Declarations

- The initializer of a `const`/`var` decides what it declares:
  - `struct`, `opaque` -> `struct`; `union` -> `union`; `enum` -> `enum`.
    The signature names the container keywords: `const Tag = enum(u8)`,
    `const Handle = opaque`. An `opaque` container carries `isOpaque`.
  - `error{..}` (alone or in an `||` merge) -> `enum` with `isErrorSet`.
  - A function prototype value (`const Handler = fn (u32) void`) -> `type`
    with `isFunctionType`. A function-pointer value with a declared type
    (`const h: *const fn () void = &f`) stays a constant.
  - `@import("x")`, or a member chain rooted at one, -> `import`.
  - Anything else -> `constant` (`const` at container scope) or `variable`.
- Enum tags are `enum_member` symbols parented to their enum. The signature
  keeps a stated value (`debug = 0`). Members of a named error set are
  `enum_member` symbols parented to the set; an inline error set in a return
  type declares no symbols.
- Union and struct fields are `field` symbols. An empty container (`struct {}`)
  has no field row.
- A plain declaration's signature runs up to its `=`: `pub const max: usize`,
  `const a: u32 align(8)`. Without a stated type it shows the value after `=`
  (`const limit = 10`), cut to one line.
- `export` declarations are public.

## Bodies

- A function's body is its block. An `extern` prototype has no body.
- A declaration's body is its initializer node (the container for container
  declarations). A declaration without an initializer has no body.

## Doc comments

- `///` lines directly above a declaration are its doc.
- A container without `///` docs takes the `//!` lines that open its member
  list. `//!` lines are `doc_comment` regions and never document the next
  declaration.

## Annotations

`export`, `inline`, `noinline`, `extern` (with the linkage string when one is
given), `callconv(..)`, `threadlocal`, `comptime`, and the declaration's own
`align(N)` are annotations. An alignment inside a value type (`[]align(a) T`)
belongs to that type, not to the declaration.

## Types and receivers

- `@This()` and an in-scope alias of it (`const Self = @This();`) resolve to
  the container that declares the alias, in type facts and in
  `receiver_type`. A `Self` declared in an outer container names that outer
  container inside a nested one. A qualified name (`other.Self`) never
  resolves to a same-file container. At file scope the container is the file
  itself, named by its file stem (`Tokenizer.zig` -> `Tokenizer`).
- In a file-as-struct, `self.peek()` from a top-level function resolves to the
  file's own top-level `peek`.
- A `const` or `var` with no written type and a single name gets an inferred
  type fact from its initializer: a same-file container literal
  (`Store{ .. }`), or a call to a same-file function with a declared return
  type. A return type of `@This()` (also `!@This()`, `?*@This()`) names the
  container that declares the function. The callee is a bare function in an enclosing container (`load()`), a
  function of a same-file container named directly (`Store.open()`,
  `Self.open()`, `@This().open()`, `Outer.Inner.open()`), or a method through
  the typed receiver parameter (`self.next()`). The receiver is the first
  parameter of a function inside a container, or of a top-level function in a
  file-as-struct.
- A type name (`Store`, `Self`) resolves to its nearest declaration in scope.
  Each later segment of a qualified name (`Outer.Inner`) resolves to a
  declaration of the container before it. Each declaration must be a
  container or an alias of `@This()`. So an alias of another type
  (`const Store = other.Store;`) records no fact, even when a same-named
  container exists elsewhere in the file.
- Same-named candidates must agree, and a same-named non-function declaration
  blocks inference. `try` and a `catch` with a noreturn fallback
  (`unreachable`, `return`, `break`, `continue`, an unlabeled block, `@panic`,
  `@trap`, `@compileError`) remove one error-union layer. `.?` and an `orelse`
  with a noreturn fallback remove one optional layer. `Type.init(..)` on a
  same-file container with no `init` and no `usingnamespace` records `Type`.
- A return type counts only when its leading name resolves to a
  container-level declaration or to a function-local container. A parameter
  (`comptime T: type`, or `comptime cfg: struct { T: type }` in `cfg.T`), a loop capture (`|T|`), a function-local alias
  (`const V = @TypeOf(value);`), or an unresolved name can stand for another
  type on each instantiation, so it records no fact.
- These record no fact: a value fallback, a chain that ends in any other
  member, a `void` return, a function or a literal (`Self{ .. }`,
  `@This(){ .. }`) of an anonymous container, a function of
  a container declared inside a function with a `comptime`, `type`, or
  `anytype` parameter or inside an `inline for`/`inline while` body,
  `Type.init(..)` on a container with `usingnamespace` (the mixin may supply
  `init`), a local receiver, a receiver that is not the first parameter, a
  receiver qualified by an import (`other.Store.open()`), and a callee in
  another file. Other files are out of scope because each file is extracted
  alone.
- Types inside `?T`, `[]T`, `[N]T`, pointer types, generic type arguments in
  type position, and struct literals (`Node{ .. }`) are `type_usage`
  identifiers.
- An enum or decl literal (`.info`, `.empty`) has no receiver.

## Tests

Only `test` declarations are tests. A function named `test_*` or `Test*` is
never a test case, even under a test directory.

## Structural facts

- `zig.builtin_call.v1` covers every builtin call, including builtins passed as
  call arguments.
- In a file named `build.zig`: `zig.build_artifact.v1` (`addExecutable`,
  `addLibrary`, `addStaticLibrary`, `addSharedLibrary`, `addObject`,
  `addTest`), `zig.build_dependency.v1` (`b.dependency("name", ..)`),
  `zig.build_module.v1` (`addModule`, `createModule`),
  `zig.build_module_import.v1` (`addImport`), and `zig.build_step.v1`
  (`b.step("name", "description")`).
- `httpz.route.v1`: http.zig verb routes (`get`, `post`, `put`, `delete`,
  `patch`, `head`, `options`, `all`) with a static path and a handler, on a
  router from `server.router(..)` or a `group("/prefix", ..)` of one, in a
  file that imports `httpz`.
- `http.client_request.v1` (`client: std.http`): `client.fetch(.{ .location =
  .{ .url = "lit" } })` and `client.open(.VERB, uri, ..)` /
  `client.request(..)` with a URI from `std.Uri.parse("lit")`. The receiver
  must be a same-file `std.http.Client{..}` binding or a parameter typed as
  `http.Client`. `fetch` without `.method` records the std default: POST with a
  `.payload`, GET without.
