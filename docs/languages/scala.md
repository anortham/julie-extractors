# Scala support

Scala uses `tree-sitter-scala`. The extractor reads `.scala`, `.sc`, and
`.sbt` files; an sbt build definition is Scala source and parses cleanly.

## Declarations and members

- `package a.b` and a packaging block `package http { ... }` are `namespace`
  symbols named by the package name. A `package object util { ... }` is a
  `namespace` named `util`, and its definitions are its members.
- A `val` or `var` in a class, object, trait, enum, given, or package object
  body is a `property`. Inside a callable, block, lambda, or case body it is
  a `variable` with no visibility. A top-level `val` is a `constant`; a
  top-level `var` is a `variable`.
- A pattern or multi-name definition binds every name: `val (host, port)`,
  `val Config(user, pass)`, and `val a, b` make one symbol per name, each
  spanning its own identifier, with the pattern in the signature.
- Every primary-constructor parameter list gives `property` symbols, as the
  primary-constructor rule in
  `docs/decisions/2026-09-08-receiver-type-facts-wave-2.md` requires. A case
  class or enum case parameter is a public `val`. Any other parameter
  without `val`/`var` is private state: visibility `private`, no binding in
  the signature, and `binding` metadata `none`. An enum and an enum case
  with parameters (`case Custom(name: String)`) own their parameters.
- An enum case keeps the doc comment and annotations written before its
  `case` keyword, and it `extends` its enum when it says so.
- Annotations are `symbol_annotations` rows on classes, traits, objects,
  enums, enum cases, methods, and `val`/`var` members. They never appear in
  modifiers or signatures. `@throws[E]` is `throws`, and the
  `scala.annotation.v1` fact names the last segment (`tailrec`).
- A `given` is a `variable` symbol with a `givenType` metadata key and a type
  fact. An anonymous given takes the compiler's synthesized name:
  `given Show[List[Int]]` is `given_Show_List`. A given with a template body
  (`given Show[Int] with ...`) `implements` its type. The
  `scala.given_definition.v1` fact carries `given_type` on every given.
- An `extension (s: Shape)` block is a `module` symbol named by the extended
  type (`Shape`) with `extendedType` metadata, as Swift names
  `extension Shape`. Its methods are its members.

## Type facts

- A `val` or `var` with no written type gets an inferred type fact from its
  initializer: `new T(..)`, `T(..)` for a same-file class `T`, or a call to or
  reference of a same-file `def` with a declared return type. The `def` can
  be local, top-level, a member of an enclosing class, object, or trait
  (`load()`, `this.load()`), or an object member (`Repo.create()`,
  `Repo.current`). The call must supply the def's explicit parameter lists,
  or all of them with its `implicit`/`using` list.
- `Name(..)` uses the `apply` methods of the same-file object `Name` when it
  declares any, and `Obj.apply(..)` follows the same rule. For a case class
  the result must be `Name`, because the synthetic `apply` returns `Name`. A
  case class declared in a nearer scope than a def or object of the same
  name makes `Name(..)` construct that class. A plain class or enum there
  records nothing, because Scala 2 and Scala 3 resolve the name differently.
- Same-named defs in the resolving scope must agree. A name bound by a
  parameter, pattern, `for` enumerator, `val`, or named `given` records no
  fact. Any trailing method other than a parameterless `.get` records no
  fact.
- `.get` removes one layer of the Scala `Option`, `Some`, `Try`, or
  `Success`. A qualified name must be `scala.Option`, `scala.Some`,
  `scala.util.Try`, or `scala.util.Success`, so `Parsed.Success[T].get`
  records nothing. An unqualified name records nothing when the file
  declares a type with that name, imports that name from another package
  (also by rename), or has a wildcard import from a package outside
  `scala`, `scala.util`, `scala.collection`, `scala.concurrent`,
  `scala.jdk`, `scala.annotation`, `scala.math`, and `java`. Unqualified
  `Try` and `Success` also need an import from `scala.util`.
- A type parameter or abstract type member records no fact, and neither
  does a `.get` that unwraps to one. An unqualified return type name inside
  a template that can inherit or export records no fact, because an
  inherited abstract type member can have that name. The exception is a
  name that the file declares as a class, trait, enum, or type alias and
  never as an abstract type member.
- A class, object, or trait that can inherit records no fact for that
  name, even when it declares its own defs of the name: an inherited
  overload can be the one the call selects. A template can inherit when it
  has an `extends` clause, a self type, an `export` clause, or a `case`
  modifier, or when it is an anonymous class or an enum or given body. The
  rule also applies to `Any` members such as `toString`, to a top-level
  `export`, and to the synthetic members of a same-file case class
  companion (`apply`, `unapply`, `fromProduct`, `tupled`, `curried`) or
  enum companion (`values`, `valueOf`, `fromOrdinal`). This covers
  `load()`, `this.load()`, `Repo.create()`, `Port.unapply(..)`,
  `Color.valueOf(..)`, and `Name(..)` through an inheriting companion
  object. Other files are out of scope because each file is extracted
  alone.

## Relationships and identifiers

- Inheritance edges come from the definition at the same span, so a
  companion object and its class keep their own supertypes. A qualified
  supertype splits into terminal name, receiver, and namespace:
  `play.api.libs.json.Reads[Money]` targets `Reads` with receiver `json`.
- `def this(...) = this(...)` calls the class, which is its primary
  constructor.
- An alphanumeric infix operator is a method call: `a plus b` calls
  `a.plus`, and `xs map f filter g` calls `xs.map` and `filter`. Symbolic
  operators (`+`, `::`, `!`) stay operators. FlatSpec `should`/`in` words do
  not call out of the test they declare.
- Package segments of a qualified type (`java.io.Serializable`), declared
  names of `val a, b`, enum case names, and package object names are not
  `variable_ref` rows. A qualified generic type records its type arguments.

## Literals

- An interpolated string decodes its holes as `{}`:
  `s"https://api.example.com/users/$id"` is
  `https://api.example.com/users/{}`. It is a `string_literal` source region.
- A custom interpolator is the literal's carrier: `sql"..."`, `fr"..."`, and
  `fr0"..."` classify as SQL, and `uri"..."` as a URL. `ws.url("...")` and
  `requests.patch(...)` arguments classify as URLs.

## Frameworks

- `akka_http.route.v1`: Akka HTTP and Apache Pekko HTTP directives. A route
  joins the static `path`/`pathPrefix` matchers with the method directive
  that governs them, in either nesting order. `LongNumber`, `Segment`, and
  the other segment matchers become `{LongNumber}`-style segments. A dynamic
  matcher silences the routes below it, and a method directive with no path
  stays silent.
- `http4s.route.v1`: `case GET -> Root / "users" / IntVar(id) =>` gives
  `GET /users/{id}`. A query matcher (`:?`) ends the path.
- `http.client_request.v1`: requests-scala `requests.get("...")`, sttp
  `basicRequest.post(uri"...")`, and Play WS `ws.url("...").get()` where `ws`
  is declared in the file as a `WSClient`. Only a static URL produces a fact.

Evidence: `fixtures/extraction/scala/members_and_literals/`,
`fixtures/extraction/scala/web_routes/`, and
`fixtures/extraction/scala/sbt_build/`.
