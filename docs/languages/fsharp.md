# F# support

Julie registers `fsharp` for `.fs`, `.fsx`, and `.fsi` files. The source and
script extensions use `tree_sitter_fsharp::LANGUAGE_FSHARP`; signature files
use `tree_sitter_fsharp::LANGUAGE_SIGNATURE`. All three parser choices publish
the stable artifact language `fsharp`.

Run the language target when changing F# extraction:

```bash
cargo xtask test language fsharp
```

The command runs the F# unit-test modules and the golden extraction test with
`JULIE_GOLDEN_LANGUAGE=fsharp`. The canonical evidence families are:

- `fixtures/extraction/fsharp/basic` — modules, classes, records, unions,
  methods, properties, fields, annotations, calls, inheritance, types,
  literals, and complexity.
- `fixtures/extraction/fsharp/script` — top-level `.fsx` values and functions,
  `#r` and `#load` directives, and `open` imports.
- `fixtures/extraction/fsharp/signature` — `.fsi` namespaces, modules,
  signatures, record fields, and the signature parser.
- `fixtures/extraction/fsharp/test_roles` — xUnit `Fact` and `Theory`
  functions, including a qualified attribute and an ordinary control.
- `fixtures/extraction/fsharp/negative` — a bare value read, member access,
  and unresolved call control.
- `fixtures/extraction/fsharp/declaration_forms` — constructors, auto
  properties, `val` fields, operators, active patterns, interfaces, enums,
  `[<Struct>]` and `[<Literal>]` kinds, type extensions, tuple bindings,
  locals, chained calls, index access, and domain-native facts.
- `fixtures/extraction/fsharp/web_routes` — ASP.NET attribute routes, minimal
  API routes, Giraffe routes, and `HttpClient` requests.

## Recorded facts

F# emits symbol rows for declarations: modules, namespaces, classes,
interfaces, structs, enums and enum members, unions, union cases, methods,
constructors, properties, fields, functions, constants, variables, imports,
and the type declarations exposed by the grammar. A type with only abstract
members and no primary constructor, or with `[<Interface>]`, is an interface;
`[<Struct>]` makes a struct; a `let` whose value is `function` or `fun` is a
function; `[<Literal>]` makes a constant. Primary-constructor parameters,
`new(...)` constructors, `member val` and `static member val` auto
properties, abstract properties, `val` fields, operator members such as
`(+)`, active patterns such as `(|Even|Odd|)`, and every name bound by a
tuple or record pattern are symbols. Members of a type extension
(`type Shape with ...`) belong to the extended type when it is in the file;
otherwise they carry `extendedType` metadata that names it. Locals
(`let`, `use`) belong to their enclosing function and are private. Identifier rows are usage-only:
`call`, `member_access`, `type_usage`, and `variable_ref`. A declaration is
never represented by an identifier kind.

Relationships include exact local calls and inheritance, plus structured
pending calls and imports. `open A.B` is an import symbol named `B`;
`#load "file.fsx"` and `#r "nuget: Package, 1.0"` are import symbols named by
the file or package, with `directive`, `source`, and `version` metadata. Each
import symbol is the caller of its pending `imports` row. A chained call keeps
its receiver expression (`s.Trim()`), never a namespace path; `xs[0]`,
`xs[1..]`, and `struct (1, 2)` are not calls. Explicit
type annotations produce non-inferred type facts; scalar literal initializers
produce inferred type facts. Generic type arguments are retained in
`type_argument_usages` with nested argument positions.
Member return types (`: string option`) and signature-file `val` types are
declared type facts; untyped parameters get none. Suffixed numeric literals
(`10L`, `3u`) infer their exact types. A literal that is a call argument
carries the called function as its carrier and its tuple position. The F#
policy uses the C# URL and SQL carrier lists and retains every other literal
as `other`.

A `let` or `use` with no written type gets an inferred type fact from a
same-file constructor call (`Workspace()`) or from a full application of a
same-file callable with a declared return type: a `let` function in scope at
the call (`load ()`, `make 1 2`, `x |> make 1`), a module function
(`Repo.load ()`), a static member on a same-file type (`Store.Create()`), or an
instance member through the enclosing member's self identifier
(`this.Load()`). Only a binding to a single name gets a fact: `let x`, `let (x)`,
`let mutable x`, or `use x`. Tuple, list, array, cons, union-case, and `as`
patterns record no fact, because each name binds only part of the value. A type
written on the pattern (`let (x: T) = ...`) wins like a type after the pattern.

A plain `let` function is in scope from the end of its definition to the end
of its module, type, or `let ... in` body, so a call to its own name inside its
body records no fact. A `let rec` function is in scope from its start. A
qualified call (`Repo.load ()`, `Store.Create()`) sees a module function only
after its definition, and sees a module or type only from its definition to the
end of the module or namespace that holds it. A call that needs an `open` to
see the module or type, such as a call into a sibling module, records no fact.
The qualifier must name the nearest same-file module or type with that name
that is visible at the call. A nearer type, module, or module abbreviation
(`module Repo = Helpers`) with the same name hides the outer one, so the call
records no fact when the nearer one does not declare the callee. A qualifier
that names both a visible module and a visible type records no fact. An
`open`, `open type`, or `[<AutoOpen>]` module that comes after the definition
and is in scope at the call can bring in a same-named function, module, or type
from anywhere, so that definition records no fact at that call. An `open`
before the definition does not hide it. Every candidate with the name must
agree on the return type.

A self call (`this.Load()`) sees only instance members declared directly in the
same type definition. A member of a same-named type elsewhere, an explicit
interface member (`interface ILoader with member this.Load()`), and a member
in a `type ... with` extension do not count. A self call or static call on a
type with an `inherit` clause records no fact, because the base type, possibly
in another file, takes part in overload resolution and can win. A call to a
member named like an `obj` member (`ToString`, `Equals`, `GetHashCode`,
`GetType`, `Finalize`, `MemberwiseClone`, `ReferenceEquals`) records no fact
for the same reason.

These calls record no fact: a partial application, a name that any pattern in
the file binds (parameter, lambda, match, or loop variable), a qualifier that
any pattern in the file binds, a self identifier that a pattern inside the same
member binds again (`fun this -> this.Load()`), a type parameter return (`'T`),
a flexible return (`#seq<int>`, a hidden type parameter the caller fixes),
a call through another receiver, a self call inside an object expression, and a
callee in another file. `let!` and `use!` remove one `Async` layer inside
`async { }` and one `Task`, `ValueTask`, or `Async` layer inside `task { }` or
`backgroundTask { }`; any other builder, and any other return type, records no
fact. Other files are out of scope because each file is extracted alone.

Comments, `///` XML documentation comments, and F# string forms publish exact
`source_regions` spans. Attribute nodes publish the registered
`fsharp.attribute.v1` structural fact with `metadata` query-family metadata and
the annotated declaration as `containing_symbol_id`. The attribute fact captures
the grammar's `attribute` node span (the attribute name, excluding `[<` and
`>]`). The owner is the declaration that holds the attribute list, so a
member attribute belongs to the member, not the type. An attribute with no
owning declaration, such as `[<assembly: ...>]`, keeps its lexical container
as `containing_symbol_id`.

Domain-native facts: `fsharp.computation_expression.v1` names the builder of
each computation expression (`task`, `async`, `seq`); `fsharp.active_pattern.v1`
records each active pattern definition with its cases and a `partial` flag;
`fsharp.quotation.v1` records each code quotation as `typed` or `untyped`.

Web facts use the shared .NET patterns: `aspnet.attribute_route.v1` for
`[<Route>]` and `[<Http*>]` on controllers and members,
`aspnet.minimal_api.route.v1` for `app.MapGet(...)`, and
`http.client_request.v1` for `HttpClient` calls with a literal URL. Giraffe
routes (`route`, `routef`, `routeStartsWith`, `subRoute`, and the `Ci` forms)
publish `giraffe.route.v1` with the verb from an enclosing `GET >=>` chain and
the joined `subRoute` prefix; `routef` format specifiers normalize to
`:argN` segments.

F# tests use the .NET attribute contract shared with C#: xUnit `[<Fact>]`,
NUnit `[<Test>]`, and MSTest `[<TestMethod>]` publish `test_case`;
`[<Theory>]`, `[<TestCase>]`, `[<TestCaseSource>]`, and `[<DataTestMethod>]`
publish `parameterized_test`; NUnit and MSTest setup and teardown attributes
publish `fixture_setup` and `fixture_teardown`. Qualified attributes such as
`[<Xunit.Fact>]` match by their last segment. A class with a container
attribute (`[<TestFixture>]`, `[<TestClass>]`) or a test member is a
`test_container`. An Expecto `[<Tests>]` value publishes `test_case`, so test
impact reaches the test list. The `testCase` entries inside the list are
expressions, not declarations, so they carry no role of their own. Similar names and unannotated functions remain
ordinary symbols.

Calls include pipelines and operators: `x |> f`, `f <| x`, and `a + f x` all
emit a call to `f`, and a generic call (`f<T>()`) is a `call` whose type
arguments ride on the call identifier. A call belongs to the smallest
enclosing declaration: a method, property, module value, or nested module, not
the surrounding namespace. A call to a class in the same file is an
`instantiates` edge. `type ... and ...` and `let rec ... and ...` groups emit
one symbol per declaration. `exception` declarations are classes; an unnamed
union or exception field (`Cash of decimal`) is only a type, not a field
symbol. `private` and `internal` on `let` bindings and types set visibility,
and a `let` inside a class is private.

Body spans come from the syntax tree: the expression after `=` for bindings
and members, the member blocks of a type, and the declarations after a module
or namespace header.

## Recorded gaps

The capability row in `fixtures/extraction/capabilities.json` is the source of
truth for F# gaps. It records no open F# gaps at present.

The pinned grammar cannot parse `static member val` or `default val`, and
either form breaks the parse of every member after it. The extractor rewrites
them to `member val` at the same byte length before the parse, so both
publish auto-property symbols with exact spans. The signature of a
`static member val` starts at `member`.

The pinned Expecto evidence scan also shows the current boundary: its F# files
produce symbols, relationships, identifiers, types, type-argument usages,
complexity metrics, annotations, scalar literals, source regions, and attribute
structural facts, while xUnit roles remain absent because the corpus does not
use xUnit attributes. Parse diagnostics are reported rather than hidden; the
121 `error` and 8 `missing` rows in the pinned scan are all upstream grammar
limitations observed at concrete source forms, not extractor failures. They
cover semicolonless multiline record fields (`Expecto.Sample`), qualified
union-case patterns in record patterns (`Expecto.Tests/Prelude.fs` and
`Expecto.Hopac.Tests/Tests.fs`), multiline ordinary strings with embedded
escaped quotes (`Expecto.Tests/FsCheckTests.fs`), compact `function |` and
no-space `->` forms (`Build/Program.fs` and `Expecto/Expecto.Impl.fs`),
file-level `module internal` declarations (`Expecto/Async.fs`), numeric
unit-of-measure aliases (`Expecto/Performance.fs`), pointer/flexible externs
and type-extension/task-builder syntax (`Expecto/Logging.fs` and
`Expecto/Expecto.fs`), and layout-heavy computation-expression/test-list
forms (`Expecto/Progress.fs` and `Expecto.Tests/Tests.fs`).

## Grammar freshness

`tree-sitter-fsharp` is pinned exactly to version `0.3.0` from crates.io in
`Cargo.lock` (checksum
`054fba748f8bf3604fc14191b4e7da66d1b887de0e285e32cf6dbd2a3db3fc42`). Run:

```bash
node scripts/grammar-freshness-report.mjs --format json
```
