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
