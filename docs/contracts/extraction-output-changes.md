# Declared extraction-output changes

Same-epoch compatibility is a gated claim, never an assumption (v4 contract §7/§16.8). Two
`julie-extract` binaries that report the same schema epoch must produce byte-equivalent extraction
output for the same source tree — otherwise a consumer that merges or trusts artifacts across
binaries reads a silently wrong index.

`cargo xtask compat-check` enforces that claim. It scans
`fixtures/extraction/resolution_contract/` with the previous published release binary and with the
current build, dumps every comparable extraction table from both artifacts, and byte-compares the
dumps.

**Gate invariant:** an extractor binary change that alters per-version extraction output cannot
merge silently. It either byte-matches the previous release on the fixture, or it names itself in
this ledger.

## What the gate compares

Tables are enumerated at run time from `sqlite_master`, so a table added or dropped by a schema
change is itself a reported difference that this ledger must declare.

Excluded from the comparison:

- `artifact_metadata`, `extraction_revisions`, `revision_file_changes` — per-scan identity and
  timestamps, so two runs of the *same* binary already differ there.
- `files.indexed_at` and `files.last_revision_id` — per-scan columns inside a compared table.
- `identifier_resolutions` and `pending_resolutions` — schema v7 removed these overlay tables.
  The previous release still writes them. Excluding them keeps the gate on fact-table identity
  and classifies the removal as intentional.
- `language_capability_gaps` — the previous release wrote `reference_resolution.*` gap rows.
  This binary does not. The remaining extractor capability-gap rows stay in the artifact;
  the gate excludes the table so that retired resolver snapshot is not an epoch bump.

The retired overlay tables are not a silent fact-table change. A v2.33.7 reader that joins
them will not find them on a v7 artifact. That break is recorded below.

Known blind spot: the enumeration filters `sqlite_master` to `type='table'`, so an index or trigger
added or dropped by a schema change is NOT independently visible to the gate — declare such changes
in this ledger on the strength of the DDL, not of a gate diff.

## Exit codes

| Code | Meaning |
| --- | --- |
| 0 | Dumps are identical. |
| 0 | Dumps differ and this ledger declares the current version. A `NOTICE:` line names the entry. |
| 1 | Dumps differ and this ledger does NOT declare the current version. The gate fails. |
| 2 | Usage or environment error (bad flag, missing binary, failed scan). Never a gate verdict. |

## How to declare a change

The version checked is the `version` field of `crates/julie-extract-cli/Cargo.toml` in the current
build. Add one `## <version>` section per version whose extraction output changes, before the
change merges. A section needs a `classification:` line and prose that a consumer can act on.

```md
## 2.31.0

classification: compatible

What changed, which tables and columns move, and what a consumer must do — or why nothing
breaks for a reader on the previous release.
```

The example above is inside a code fence deliberately. `find_ledger_entry` trims each line before
matching, so an *indented* example heading is indistinguishable from a real declaration and would
declare whichever version it names — shadowing the real entry below, because the first match wins.
Fenced blocks are skipped. Keep any future example fenced.

`classification: compatible` means a reader built for the previous release still reads the new
output correctly. `classification: incompatible` means it does not, and the change needs a
schema or contract version bump. The classification recorded here is the contract.

## How to run it

Locally, against any older `julie-extract` binary:

    cargo xtask compat-check --previous-binary /path/to/previous/julie-extract

Useful flags: `--current-binary <path>` (skip building the current release binary),
`--fixture <path>`, `--out-dir <path>` (defaults to `target/compat-check/`, which keeps both
artifacts and both dumps for inspection), `--max-diff-rows <n>`.

In CI, the `Extractor Compatibility` job downloads the latest published release for
`x86_64-unknown-linux-gnu` and runs the same command.

## Declared changes

Every release before 2.30.0 byte-matches its predecessor on the fixture.

## 3.9.1

classification: compatible

The SQLite schema remains 8, the report schema remains 3, and the extraction
identity epoch remains 10. `EXTRACTION_CONTRACT_VERSION` advances
`csharp-argument-types-v1` to `csharp-argument-types-v2`. No table or column
shape changes. A schema 8 artifact written by 3.9.0 has the old capability
snapshot fingerprint; refresh it with `julie-extract scan --root <project>
--db <artifact.db> --force` before incremental use with 3.9.1. A schema 7
artifact still requires extraction into a new database path, as described
below.

`identifiers`, C#: `arg_types` retains its existing shape, with corrected
values. `typeof(…)` gives `System.Type`. A name gives the declared type of the
nearest local or parameter declared before the call and visible at that site,
including C# block, loop, `catch`, `using`, and `fixed` scopes; an unknown or
out-of-scope binding stays `null` instead of borrowing an outer type. Integer
literals use their value, radix, and suffix to choose `int`, `uint`, `long`, or
`ulong`; values outside those ranges stay `null`. Other C# argument expressions
keep the rules recorded under 3.9.0.

## 3.9.0

classification: incompatible

The SQLite schema is 8 (`docs/contracts/sqlite-schema-v8.md`). The report schema remains 3, and
the extraction identity epoch remains 10. `EXTRACTION_CONTRACT_VERSION` adds
`javascript-assigned-properties-v1`, `csharp-argument-types-v1`, and
`csharp-preprocessor-conditions-v1`. Re-extract every existing artifact into a
new database path with `julie-extract scan --root <project> --db <new-v8.db>`.
A write into a schema 7 artifact, including `scan --force`, reports
`schema_migration_required` (exit code `3`) without changing rows. Close readers
before replacing the old artifact with the completed v8 artifact.

`symbols`, JavaScript: a value other than a function assigned on a named object
(`res.locals = res.locals || Object.create(null)`) writes a `property` symbol named after the
member, with `className` (the object) and `isAssignedProperty: true` in `metadata_json`. Only the
first such assignment of each `object.name` in a file writes a row. The owner is the top-level
function or class with the object's name when the file has one, else the enclosing symbol. An
object of `this`, `module`, or `exports`, a member chain (`res.locals.user = …`), and a prototype
(`Queue.prototype.size = 0`) write no row, and neither does a bare name as the value
(`api.render = render`, `app.route = route`): the binding it names is the definition. A chained
assignment (`req.get = req.header = function header()`) writes no row for the outer member, so the
inner method keeps its owner. The signature is the first line of the assignment. Such a property
owns no reference: the calls and reads in its value keep the function around it, or no owner at
module level, as before.

`symbols`, C#: a symbol in the kept branch of an `#if` group gets `preprocessorIf` in
`metadata_json`: the condition text after `#if`, without a trailing `//` comment, joined with
` && ` under an enclosing kept branch (`!HAVE_CONCURRENT && HAVE_BARRIER`). The kept branch is the
first branch of each group, as before; the other branches stay blanked and write no rows. `using`
rows count as symbols here, so a reader can pair a fallback type under `#if !X` with a `using`
under `#if X`.

`identifiers`, C#: a `call` identifier that has `arg_count` also gets `arg_types` at the
`references` and `full` levels: one entry per argument, the type name its expression states, or
`null`. A literal gives its keyword type (`string`, `char`, `bool`, `int`, `long`, `uint`,
`ulong`, `double`, `float`, `decimal`; `null` gives `null`); `new T(…)`, a cast `(T)e`, `e as T`,
and `default(T)` give `T`; `typeof(…)` gives `Type`; `nameof(…)` gives `string`; `this` gives the
enclosing type; a name gives the declared type of the nearest local or parameter declared before
the call, else of a field, property, or constant of the enclosing type. Whitespace inside a type
name is removed. A call with a named argument or a `ref`, `out`, or `in` argument gets no
`arg_types`, since those change which parameter an argument fills.

`identifiers` and `reference_sites`, `references` level only: an identifier whose site no written
relationship or pending relationship shares has a null `reference_site_id`, and no
`reference_sites` row is written for it. Its position stays in the identifier's own columns. The
`full` level is unchanged.

Consumer action for code-kb: read `arg_types` for C# overload binding and `preprocessorIf` for
conditional definitions; read nothing from `reference_sites`.

## 3.8.0

classification: compatible

The SQLite schema remains 7, the report schema remains 3, and the extraction
identity epoch remains 10. `EXTRACTION_CONTRACT_VERSION` adds
`rust-type-fact-base-names-v1` and `call-argument-counts-v1` because canonical
output changes. Re-extract every existing artifact with
`julie-extract scan --force`; incremental commands against an artifact from
another binary version report `fingerprint_mismatch` (exit code `3`) without
changing rows.

`type_facts`: the Rust extractor no longer writes rows cut out of signature
text. Before, a parameter, local, field, or `static mut` whose type has no base
name (a slice, array, closure, `impl Trait`, function pointer, or reference to
one of these) got a row holding the first word of its annotation:
`items: &[Box<dyn Shape>]` gave `&[Box<dyn`, `rows: &mut [Row]` gave `&mut`,
`pairs: &'a [(String, String)]` gave `&'a`, `visit: impl FnMut(&str)` gave
`impl`, and `bytes: [u8; 4]` gave `[u8;`. A slice without spaces, such as
`&[&str]`, was written whole. Every one of these values breaks the base-name
rule of the 2026-09-08 corrupt-type gate. Now these symbols have no
`type_facts` row, which matches the AST recorders: they already decline these
shapes.

Only Rust `type_facts` rows are removed. No row for a type with a base name
(`u32`, `Traversal`, `Vec`, `Option`) is added or changed; it keeps its
`resolved_type`, `is_inferred`, and metadata. The Rust `type_facts` counts in
3.7.2 and 3.8.0 scans of three repositories differ only by these removals:

| Repository | Rust `type_facts` rows, 3.7.2 | 3.8.0 | Corrupt-type gate, 3.7.2 | 3.8.0 |
| --- | --- | --- | --- | --- |
| code-kb | 3112 | 3050 | 60 | 0 |
| julie-extractors | 49419 | 48000 | 1361 | 0 |
| julie | 16425 | 16098 | 309 | 0 |

Consumer action for code-kb: none beyond the forced rebuild. A removed value
never equals the name of a type symbol, so no receiver-type join matched it.
Treat a missing row as "no base type name".

`identifiers`: every `call` identifier in Python, JavaScript, TypeScript, TSX,
Java, C#, Go, Rust, Kotlin, C and C++ whose call node has an argument list gets
two optional `metadata_json` keys at the `references` and `full` extraction levels. No column,
table or index changes; the keys merge into the same object that holds
`receiver` and `receiver_qualifier`.

- `arg_count` is an integer: the number of named children of the argument
  list, not counting comments. A keyword or named argument counts as one
  argument. A constructor call such as `new Foo(a)` or Python `Foo(a)` counts
  like any other call. Each call counts its own list: in `outer(inner(1, 2), 3)`
  `outer` has `2` and `inner` has `2`.
- `arg_spread` is `true`, and is absent otherwise, when one argument is a
  spread or splat: Python `*x` and `**x`, JavaScript and TypeScript `...x`, Go
  `x...`, Kotlin `*x`, and C++ pack expansion `x...`. When it is present,
  `arg_count` counts the spread as one argument, so the real count is unknown.
  Java and C# have no spread syntax.

A call identifier gets the keys only when it is the terminal name of its
callee: the first call node above it must have an argument list, and only
whitespace, comments, one optional-call `?.` (JavaScript, TypeScript and TSX:
`f?.(x)`, `obj.m?.(x)`, `a?.b?.(1, 2)`, `f?.<T>(x)`), and one type-argument
list may stand between the name and that list. A Python backslash line
continuation (`f \` then a newline, LF or CRLF, then `(1)`) counts as
whitespace. The type-argument list is `<...>` in TypeScript, TSX, Rust (written
`::<...>`), C#, C++, Kotlin and Java, and `[...]` in Go. The check reads the
tokens of the syntax tree between the name and the list, not the source text,
and nests round, curly and square brackets. A string, character, regex, raw
string or template literal and a comment is one leaf or extra of the tree and
never opens or closes an angle, and the `>` of `->` and `=>` is another token.
So `useState<() => void>(null, 1)`, `f<'>' | 'x'>(1)`,
`f<() => void /* > */>(x)`, ``f<`a${`b${string}>`}>`>(x)`` and the C++ raw
string `f<R"x(>)x">(1)` keep their counts. In C++ `<<` is a shift inside the
list (`f<1 << 2>(1)`) and `'` after a digit is a digit separator
(`f<1'000>(1)`). An identifier that sits inside a larger callee
but is not its last name gets no key and never takes the count of the larger
call. In `(0 until n step 2).toList()` `toList` has `0` and `step` and `until`
have no key.

Both keys are absent for these call identifiers:

- A call whose call node holds a syntax error anywhere inside it, in the callee
  or in the argument list. tree-sitter recovers from an error by guessing, and
  the guessed argument list has the wrong length: the C and C++ macro call
  `N4_OP(1, ==, 3, 4)` gave `3`, the jq line
  `#define BINOP(name) CFUNC(f_ ## name, "_" #name, 3),` gave `64`, and the
  JavaScript call `f(1, @@@, 3)` gave `2`. The same rule covers the Go
  conversion form `F[int](x y)`. Calls with no error in their own node keep
  their keys even when the file has errors elsewhere.
- A TypeScript, TSX, C#, Kotlin, C or C++ call whose argument list is
  angle-ambiguous. These grammars read `f(g<A, B>(1) + 2, x)` as comparisons
  without an error node, so `f` would get `3` instead of `2`; Kotlin
  `f(HashMap<String, Int>(16))` gave `2` instead of `1`; tree-sitter-c-sharp
  reads `f(a < b, c > d)` as one declaration of `d`, so `f` would get `1`; and
  C++ `f(a < b, u<A>(x))` is a template call or two comparisons depending on
  what `a` means, which the parser cannot know. A C file has the same check,
  because a `.h` header that the language detector classes as C can hold C++.
  One rule decides, and it does not depend on what a name means. In a tree
  without errors a count is wrong only when one reading puts a comma inside
  `<...>` and the other reading puts it between arguments. In both readings
  that comma has a `<` token before it and a `>` token after it, at the top
  level of the argument list. Round, square and curly brackets are not
  ambiguous, so an angle list never crosses one.
  The rule walks the leaf tokens of the argument list in source order with a
  tree cursor and tracks two depths. The bracket depth uses `(`, `)`, `[`, `]`,
  `{`, `}`, the `${` of a template substitution, and the C# interpolation
  braces. The angle depth starts at 0 at the top level of the list. A `<` token
  adds 1. A `>` token subtracts 1, `>>` subtracts 2 and `>>>` subtracts 3, and
  the depth never goes below 0. TypeScript and TSX also add 2 for `<<`, because
  `f<<T>(x: T) => void>(y)` holds a type list inside a type list; in the other
  languages `<<` is a shift and adds nothing. A token with an `=` (`<=`, `<<=`,
  `>=`, `>>=`, `>>>=`) opens and closes nothing, because a type list cannot
  start or end with `=`. A comma at the top level of the list is a straddle
  when the angle depth is above 0 at that comma. A list is angle-ambiguous when
  it has a straddle and a `>` token (`>`, `>=`, `>>`, `>>=`, `>>>`, `>>>=`)
  comes after it at the top level. The call then has no key. `=>` and `->` are
  other tokens and never count. A `<` or `>` inside a string, character,
  regex, raw string, template text, JSX text or comment is part of a literal
  leaf of the tree and never counts.
  Why the depth is exact enough: every comma that the two readings put in
  different places has its `<` before it and its `>` after it at the top level.
  Between that `<` and the comma, every `>` closes a type list that opened
  after the `<`, because a type holds `<` and `>` only as list delimiters. So
  the closers never exceed the openers in that span, the depth is 1 or more at
  the comma, and the clamp at 0 drops only a stray `>` that comes before the
  `<`. A token counted as an opener when it opens nothing, such as a shift,
  only raises the depth and gives a key less, never a wrong one. The `>` after
  the comma meets the later `>` test, and `<=` and `>=` count in that test.
  So an ambiguous comma is never kept. A call whose type lists all close before
  the next comma, such as `g<A>(x), h<B>(y)`, has depth 0 at its comma and
  keeps its count in all five languages.
  C# has one more rule: the type after `new` in an object creation or an array
  creation is not a list of angle tokens. In `new Dictionary<A, B>()` the
  grammar reads the type as one node, so the argument list of the outer call
  has no straddle and keeps its key. A `new a < b, c > (d)` that the grammar
  reads as comparisons has no such type node and gives no key as before.
  TypeScript `new Map<A, B>()` keeps counting its angle tokens. Against the
  TypeScript API, `new a < b, c > + d, e` has 3 arguments, while
  tree-sitter-typescript reads it as a two-argument binary expression, and the
  same holds for `-` and `[d]` after the `>`. A skip would give 2 for 3.
  These have no key: `f(a < b, c > (d))`, `f(a < b, c || d > (e))`,
  `f(a < b, u<A>(x))`, `f(x < this.Max, y > (z))`, `f(g<A, B>(1), 2)`,
  `f(x as Map<K, V>, y)`, `f(R"(,)", a < b, c > (d), "x")`,
  `f(@"C:\dir\", a < b, c > (d))`, `f(a < b, c >= d)`, C#
  `f(new Dictionary<A, B>(), a < b, c > (d))` and TypeScript
  `f(a << b, c > d)`. These keep their key: `f(a, b)`, `f(x < y, z)`,
  `f(a > b, c < d)` (a `>` before a `<` is no straddle), `f(g<A>(x), y)` (no
  comma inside the angles), `f(g<A>(x), h<B>(y))` (the angles close before the
  comma), `f(a <= b, c > d)` (`<=` opens nothing), `f((a < b), c > d)` (the `<`
  sits inside parentheses), `f(x => x < 1, y)`, `f("<", ",", ">")`, the TSX call
  `f(<p>a</p>, <p>b</p>)`, and the C# and C++ call `f(a << b, c > d)`. In C#,
  `f(new Dictionary<A, B>(), x)` and
  `f(new KeyValuePair<string, string>(a, b))` keep their key. The cost is that
  a call with two or more type arguments inside another call's arguments, when
  it is not a C# object creation, leaves the outer call without a key:
  `f(g<A, B>(1), 2)`, a C# `out Dictionary<A, B> d`, TypeScript
  `f(new Map<string, number>())`, and a generic arrow function `<T, U>(x) => x`.
  TypeScript inside a Vue `<script lang="ts">` follows the rule. JavaScript has
  no type arguments and is not checked, so `f(a < b, c > (d))` has `2` in a
  JavaScript file and in an HTML `<script>` block.
  A second check covers the same languages. The same tokens give the
  number of non-empty comma-separated items at the top level of the list; a
  trailing comma after the last argument does not count. When that number
  differs from the number of argument nodes in the tree, the call has no key.
  This catches a comma that no bracket protects and that the tree does not
  split: the TypeScript `f(class implements B, C {})` and the Kotlin
  `f(object : A, B { })` hold one argument and one such comma. A Kotlin
  trailing lambda sits outside the argument list node and never enters the
  count, so `f(a < b, c) { e }` still has `3` and `f(a < b, c > (d)) { e }`
  has no key.
  The walk uses a cursor and no recursion. A group that opens and closes with
  brackets is one token and is not entered, so a call inside the arguments of
  another call costs the outer call one token. One call with 20,000
  comparisons scans in 0.9 to 1.3 s in each of the four languages, at most
  0.3 s more than the 3.7.2 build.
- A C# call whose argument list holds a preprocessor directive line (`#if`,
  `#elif`, `#else`, `#endif`, `#define`, `#undef`, `#region`, `#endregion`,
  `#pragma`, `#nullable`, `#line`, `#error`, `#warning`). The C# extractor
  blanks every directive line before it parses and keeps only the first branch
  of each `#if` group, so the tree has no directive node and the other
  branches vanish. The call `f(a,` then `#if false`, `c,`, `#endif`, `b)` gave
  `3` because julie kept the `c` branch, while Roslyn counts 2 with `false`.
  The branch that the compiler takes depends on build symbols, which the index
  does not know. The rule reads the original source: a line between the start
  of the call and the end of its argument list whose first non-space character
  is `#` and whose word is a directive name removes the key of that call and of
  every call whose list contains it. The blanking reads such a line as a
  directive even inside a verbatim or raw string or a comment, and blanking an
  `#else` branch there can merge two arguments, so the rule counts that line
  too and the call has no key. The Roslyn check of aspnetcore found one wrong count of this kind, in
  `AssemblyTestLog.cs`, before this rule.
- A C or C++ call whose argument is a macro that expands to several arguments.
  This is a stated limit: julie does not run the preprocessor, so
  `arg_count` counts the arguments as written. After `#define ARGS a, b`, the
  call `q3(ARGS)` has `1`, while a compiler sees 2. A macro argument that
  expands to nothing or to a comma list changes the real count in the same way.
  Treat `arg_count` on a C or C++ call that passes a macro name as the count of
  written arguments.
- A call whose argument list has a comma with no argument before it. JavaScript,
  TypeScript and TSX parse `f(,a)`, `f(a,, b)` and `f(,)` without an error node
  and counted only the arguments that were present (`1`, `2` and `0`); Python
  and Rust parse `f(,)` the same way and gave `0`. A single trailing comma
  after an argument (`f(a, b,)`) keeps its count.
- A Kotlin call whose trailing lambda the tree does not attach reliably: the
  call that wraps the lambda holds a syntax error (`f(a, b)) { c }`,
  `f(a, b) { @@@ }`), a `{` follows the call but the tree left it out (a stray
  character inside the braces turns the whole lambda into error text), or the
  call is a class delegation that took braces (`class A : I by f(1) { }`, where
  the braces may be the class body).
- A Rust macro call (`foo!(...)`) and any call inside macro arguments. This is
  a stated limit: julie does not parse macro token trees as expressions. It is
  common. In code-kb's own crates 4,321 of 29,430 Rust call identifiers (about
  15%) sit inside macro arguments, and 4,227 more (about 14%) are the macro
  invocations themselves; none of them carries a key. A missing key means
  "unknown", never zero.
- A JavaScript or TypeScript tagged template, `new Foo` without parentheses, a
  C# `new Foo { ... }` without parentheses, a Java method reference, and a JSX
  component.
- A TypeScript non-null call (`f!(1)`, `obj.m!(x)`): julie emits no call
  identifier for it. A non-null name that is not the callee (`a!.m(1)`) keeps
  its count.
- A call whose callee is the result of another call (`o->get()(1, 2)`,
  `f(1)(2, 3)`): the outer argument list belongs to no name, so its count is not
  recorded. The inner call keeps its own count, and the C and C++ identifier
  that names the whole callee text (`o->get()`) carries no key.
- An immediately invoked lambda in C++ (`[&]{ ... }()`,
  `[](int a, int b) { ... }(1, 2)`): the identifier that names the lambda
  carries no key. A call inside the lambda body keeps its own count.
- A Kotlin infix call (`a infixop b`), a Kotlin callable reference (`::ref`),
  and a name that is only part of the callee of a larger call, such as the
  infix function in `(a infixop b).after(1, 2)` or `(p infix3 q)(1, 2)` and the
  reference in `(::ref).invoke(1, 2, 3)`.
- A C name inside a parenthesized callee, because the bytes between the name and
  the argument list are `)`. In `(*callback)(1, 2)`, `(*(*cb))(1)` and
  `((a)->b)(7, 8)` julie emits the bare names `callback`, `cb` and `b`, and
  they carry no key. The name is the callee in meaning but not its terminal
  name in syntax, so the count stays unknown. C++ differs: julie names the
  call after the whole callee text, so the identifiers `(*callback)`,
  `(*(*cb))`, `((fn_t)ptr)` and `((a)->b)` carry the count of the call that
  follows them, and C gives `((fn_t)ptr)` its count too.
- Java `new Foo()` and C++ `new Foo()`: julie emits no `call` identifier for
  them, so there is no row to carry a key.

Embedded code gets keys by the same rules. JavaScript and TypeScript inside Vue
`<script>` blocks, inside Vue template expressions (`@click="save(1)"`,
`:title="titled(1, 2)"`, `{{ fmt(x, y) }}`), inside HTML `<script>` blocks and
inside HTML event-handler attributes (`onclick="save(1)"`) carry `arg_count`. A
handler attribute that names a function without calling it (`@click="save"`)
has no argument list, so its call identifier carries no key.

Kotlin counts a trailing lambda as one argument, so `f(1) { ... }` has `2` and
`f { ... }` has `1`. A lambda after a call that already ends in a lambda
belongs to the result of that call, so `f { a } { b }` gives `f` `1` and
`g(1) { a } { b }` gives `g` `2`. A Python call whose only argument is a generator
expression has `1`. A Go call such as `f(g())` has `1` even when `g` returns
several values. Go parses `F[T](x)` as a type conversion with one operand, so
the `F` identifier has `1`, and `Wrap[string](x).Unwrap()` gives `Wrap` `1` and
`Unwrap` `0`. A qualified C++ call of any depth, such as
`boost::asio::ip::tcp::resolver::query(1, 2)`, keeps its count.

For the call-argument change alone, only existing `call` identifier rows gain
these keys: no identifier row is added or removed, and no other column or
metadata key changes. Across the golden fixtures, this is the only difference
from the staged 3.7.3 build.
Consumer action for code-kb: read `arg_count` and `arg_spread` with `json_extract` on
`identifiers.metadata_json`, and treat a missing key as "unknown". A reader
built for 3.7.x ignores the keys and keeps working.

## 3.7.0

classification: compatible

The SQLite schema remains 7, the report schema remains 3, and the extraction
identity epoch remains 10. `EXTRACTION_CONTRACT_VERSION` adds
`reference-occurrence-identity-v2`, `canonical-receiver-facts-v1`,
`reference-integrity-v1` and `regex-branch-reset-v1`. Re-extract every existing
artifact with `julie-extract scan --force`.

Producer freshness: incremental `scan`, `update`, `delete` and `rebind` now
also require the artifact's `binary_version` to match the running binary, and
`capability_snapshot_fingerprint` now includes the semantic contract version.
An artifact built by 3.6.3 therefore fails the first incremental command with
`fingerprint_mismatch` (exit code `3`) before any row changes. A `--force` scan
that changes producer generation commits facts and producer metadata together;
source or discovery errors abort it without replacing the previous generation.
Consumer action for code-kb: run one `--force` scan after the upgrade and treat
`fingerprint_mismatch` as the rebuild signal.

`relationships` and `pending_relationships`: row identity now includes the
available source span, so separate occurrences on one line stay separate rows
instead of collapsing. A row with only a context span gets its own nonexact
`reference_sites` row with null span columns. Exact sites keep their
`(file_id, start_byte, end_byte)` identity. Consumer action for code-kb: expect
more rows for repeated same-line calls, and do not join nonexact sites by span.

`identifiers`: the language extractor, not the CLI, now produces `receiver`
and `receiver_qualifier` metadata from the syntax tree. For `a.b.run()`,
`receiver` is `b` and `receiver_qualifier` is `a`. Expression-rooted chains
such as `factory().b.run()` get no guessed receiver. HTML, regex and C#
identifier spans now select the target token instead of a wider node, and SQL
and Rust macro calls gain qualified receiver facts. Consumer action for
code-kb: read receivers from the artifact rather than parsing source text.

Rows that share an exact reference site now agree on its container. A Go call
that declares its own node, such as Ginkgo `Context("addition", ...)` or
`t.Run("case", ...)`, has the enclosing block as its identifier
`containing_symbol_id`, not the node it declares. A QML pending call inside a
property binding has that property as `caller_scope_symbol_id`, matching its
identifier row. Consumer action for code-kb: none beyond the forced rebuild.

The reference-integrity corrections add `reference-integrity-v1` to the
extraction contract. False local targets caused by shadowing, unrelated
scopes or incompatible Elixir arity become structured pending calls with
source context. ECMAScript ordinary functions no longer inherit an enclosing
class receiver. Calls requiring inheritance lookup also remain pending rather
than selecting a method by file-wide name uniqueness. Existing call sites
remain available for consumer resolution.

Structural facts now cover Actix resources, local client bindings, Kotest
checks, Swift Testing traits, Rust benchmarks, Phoenix sockets/channels and
regex conditionals. Cucumber step methods use the existing step-definition
role. The regex parser is the licensed `0.25.0-julie.1` vendored patch, reflected
in parser inventory and capability metadata. The SQLite schema is unchanged.
Re-extract existing artifacts with the changed binary and `--force` to replace
old facts; the extraction contract prevents mixing producer semantics.

Regex branch-reset groups `(?|...)` now parse and add
`regex-branch-reset-v1` to the extraction contract. One new pattern id,
`regex.branch_reset.v1`, records each group with `branch_count`. Captures
inside the group use PCRE2 numbering: each branch restarts at the same number,
and later captures continue from the widest branch. `capture_index` on capture
facts and `captureIndex` on capture symbols can therefore repeat within one
pattern. A backreference or condition to a shared number or name emits one
`references` row per group that holds it. Before this change the construct
produced parse diagnostics and sequential numbers. Consumer action for
code-kb: allow repeated capture numbers, and re-extract regex files with
`--force`.

## 3.7.1

classification: compatible

The SQLite schema remains 7, the report schema remains 3, and the extraction
identity epoch remains 10. The 3.7.0 markers `reference-occurrence-identity-v2`,
`canonical-receiver-facts-v1`, `reference-integrity-v1`, and
`regex-branch-reset-v1` remain present. `EXTRACTION_CONTRACT_VERSION` adds
`flask-route-decorator-span-v1`, `csharp-interpolated-verbatim-v1`, and
`python-inferred-return-type-v1`. The additional Python `super` receiver
recognition is covered by the existing `canonical-receiver-facts-v1` marker.
Existing artifact readers remain
compatible, but consumers must re-extract existing artifacts with
`julie-extract scan --force`; incremental commands against a different binary
version report `fingerprint_mismatch` (exit code `3`) without changing rows.

C# `@$` and `$@` interpolated-verbatim strings with doubled quotes around
interpolation expressions now parse without diagnostics. The fixture verifies
method symbols and interpolation-variable reference facts that the published
3.7.0 grammar missed. `parser_inventory` records the C# fork revision changing
from `688cf95ae4c984638557dab73253bd66719bdd5c` to
`388c0e74f41a3f074f4bbc0f62aaf4e6ca48fffe`; `language_capability_fixtures`
adds the `csharp`/`interpolated_verbatim` fixture row. Consumers must
re-extract affected C# source files.

Python `flask.route.v1` facts from decorators now use the route decorator's
source span and `decorator` node kind instead of the view's `def` line. The
facts remain bound to the decorated view symbol; `add_url_rule` facts remain
anchored at their registration call. Structural-fact identity includes
location, so decorator-anchored rows receive new IDs and affected artifacts
must be rescanned.

Python member reads through `super().member` and
`super(Type, self).member` now record `receiver: "super"` in identifier
metadata. Ordinary receivers and values named `super` are not treated as
implicit super receivers.

An unannotated synchronous Python function now gains optional
`inferredReturnType` metadata when all returns name the same local and its sole
dominating assignment constructs a same-file class or an explicitly imported
class-style name. The existing `returnType` remains the explicit annotation,
or the empty string. An imported factory with a class-style name can still be
classified as a class; ambiguous assignments, fallthrough, async/generator
functions, and unsupported control flow omit the inferred key.

## 3.6.3

classification: compatible

Five cross-language parity fixes across JavaScript, TypeScript, and C++, and
one Flask route change. No SQLite or report-schema column is added, removed, or
retyped: SQLite schema remains 7, report schema remains 3, and extraction
identity epoch remains 10. `EXTRACTION_CONTRACT_VERSION` adds
`ecmascript-instanceof-type-usage-v1`,
`typescript-decorator-signature-arguments-v1`,
`cpp-anonymous-union-naming-v1`,
`javascript-property-init-coexistence-v1`,
`typescript-constructor-assigned-properties-v1`, and
`flask-route-endpoint-v1` because canonical output changes.

- The right-hand side of ECMAScript and TypeScript `instanceof` expressions
  (`x instanceof MyClass`) is emitted as an `identifiers` row with kind `type_usage`
  instead of `variable_ref`, and is excluded from value-read expressions.
- Decorated TypeScript methods, classes, and properties retain decorator arguments
  in signatures (normalized to single-line and truncated at 100 chars with `…`),
  matching Python decorator signature formatting.
- Anonymous unions in C++ are named using 1-based row indexing (`anonymous_union_{row + 1}`)
  without enclosing angle brackets, aligning with anonymous structs and cross-language conventions.
- In JavaScript, constructor property initializations (`this.x = value`) coexist with
  getter/setter methods of the same name instead of being suppressed as duplicate declarations.
- In TypeScript, constructor-assigned instance properties (`this.x = value`) are extracted
  as property symbols with visibility and doc comments, mirroring JavaScript extractor behavior.
- A Flask route fact records a literal endpoint name in the optional
  `endpoint` metadata key: the `endpoint=` keyword of a route decorator or of
  `add_url_rule`, or the second positional `add_url_rule` argument.
  `app.add_url_rule("/", endpoint="index")` now says which endpoint the rule
  names. A route without a literal endpoint has no key.

## 3.6.2

classification: compatible

Nine Python changes. No SQLite or report-schema column is added, removed, or
retyped: SQLite schema remains 7, report schema remains 3, and extraction
identity epoch remains 10. `EXTRACTION_CONTRACT_VERSION` adds
`python-return-arrow-v1`, `flask-methods-tuple-v1`, `python-cls-binding-v1`,
`python-decorator-args-v1`, `python-annotation-only-v1`,
`python-isinstance-type-v1`, `python-lambda-line-v1`,
`python-attribute-docs-v1`, and `python-property-init-v1` because canonical
output changes.

- A Python function or method signature writes its return annotation with
  `->`: `def load(path: str) -> Config`, not `def load(path: str): Config`.
  The `returnType` metadata key holds the annotation text alone (`Config`, not
  `: Config`), as it does in PHP and Kotlin. A function without an annotation
  keeps `returnType` as the empty string. Names, kinds, spans, and ids do not
  change.
- A Flask route whose `methods=` value is a tuple or a set now gives one route
  fact for each listed method, as a list already did. Before, such a route got
  one fact with the default verb `GET`, so `methods=("POST",)` was recorded as
  `GET`. Rows are added and changed in `structural_facts` for these routes.
- `cls(...)` and `cls.m()` name the enclosing class only when the nearest
  function that binds `cls` takes it as a parameter, as a classmethod and its
  closures do. A function that assigns or imports `cls` binds something else,
  and an unbound `cls` names nothing. Before, flask's `Flask.test_client`
  (`cls = self.test_client_class` ... `return cls(...)`) recorded a pending
  call to `Flask`. Such a call now stays a pending call to `cls`, with no
  receiver type.
- A decorated Python function or class signature keeps each decorator's
  arguments: `@bp.route("/create", methods=("GET", "POST")) def create()`, not
  `@bp.route def create()`. Each decorator is written on one line, with runs of
  whitespace joined, and a decorator longer than 100 characters ends with `…`.
  The `decorators` metadata still holds the bare names. Only signatures change.
- An annotated assignment with no value (`default_config: dict[str, Any]` in a
  class body) no longer ends its signature with ` = `.
- The class argument of `isinstance(value, C)` or `issubclass(cls, C)`, alone or
  in a tuple, is an `identifiers` row of kind `type_usage` for `C`. Before, it was
  a `variable_ref`. Rows change kind in `identifiers` for these arguments.
- A Python lambda is named for its one-based line: `lambda_312` for a lambda on
  line 312. Before, the name used the zero-based row (`lambda_311`). Names, keys,
  and ids of lambda rows change.
- A `self.x = ...` member row takes the Sphinx `#:` comment lines right above it,
  or the attribute docstring right after it, as its `doc_comment`, as a class
  attribute already did.
- A `@property` method no longer hides the first `self.x = ...` assignment of
  the same name. `self.static_folder = static_folder` in `__init__` is a member
  row next to the `static_folder` getter and setter rows.

## 3.6.1

classification: compatible

The signature of a Python `self.x = ...` member row keeps the `self.`
receiver: `self.cli = cli.AppGroup()`, not `cli = cli.AppGroup()`. This
covers annotated targets (`self.debug: bool = False`) and tuple targets
(`self.a, self.b = pair`). The name, kind (`property`), parent, span, and id
do not change. No other table changes. SQLite schema remains 7, report schema
remains 3, and extraction identity epoch remains 10.
`EXTRACTION_CONTRACT_VERSION` adds `python-instance-attribute-signature-v1`
because canonical output changes. A reader that parses the signature text
must accept the receiver prefix.

## 3.6.0

classification: compatible

This release infers the type of a binding whose value is a call to a same-file
callee with a declared return type. See
[2026-09-24-call-initializer-type-facts.md](../decisions/2026-09-24-call-initializer-type-facts.md).
No SQLite or report-schema column is added, removed, or retyped: SQLite schema
remains 7, report schema remains 3, and extraction identity epoch remains 10.
`EXTRACTION_CONTRACT_VERSION` adds `call-initializer-type-facts-v1` because
canonical output changes.

`type_facts` gains inferred rows (`is_inferred = 1`) in every general-purpose
language. Each `docs/languages/<lang>.md` page states which call forms infer a
type and when no row is recorded. The rules for every language:

- The callee must be in the same file, and every same-named candidate must
  agree on one return type.
- A generic or type-parameter return records nothing.
- A local, parameter, import, or inherited member that the call may bind
  instead records nothing.
- A chain that ends in an unknown method records nothing.
- Only the language's own unwrap layers are removed, such as `?`, `await`,
  `try`, `!!`, and `.unwrap()`. The row's `metadata.declared` keeps the full
  written return type.
- A written type always wins.

In most languages the rows also land on top-level variables, class fields, or
properties, not only on locals. Each language page names the declarations that
get a row. In the compat fixture this adds one TypeScript row
(`made` = `number`) and two `language_capability_fixtures` rows.

Other rows change where the old output was wrong or where the new scoping rules
apply. By language:

- C: the parser reads `auto w = make();` as a prototype of `make`. The old
  output was a function `make` with return type `w`. It is now a variable `w`
  with a `calls` edge and a `call` identifier. `auto t = make(1);` and
  `auto n = 5;` now give named variables, not empty names. Every `__auto_type`
  declared row is removed, together with the `uses` rows, type-usage
  identifiers, and complexity rows of the misread symbols.
- C++: a macro before the return type (`MACRO_ATTR static T f()`) is no longer
  recorded as the return type. `const auto& [p, q]` and `auto&& [l, r]` now emit
  symbols. `auto [y] = One();` loses its old inferred row.
- Zig: `Self` (an alias of `@This()`) resolves by scope. Declared rows on fields
  and parameters, and `receiver_type` on call identifiers and pending calls,
  change to match. An `init` that the type declares supplies its own return
  type (`declared: "!Real"`). An alias of another type, a `void` init, a type
  with `usingnamespace`, and destructuring record nothing.
- Go: a type-parameter result records nothing. A generic result records only
  its base name. If the enclosing function redeclares the callee name, the row
  is withheld.
- Rust: `Self::new()` and `Self {}` in an impl record the impl type, with
  `declared: "Self"`. `Type::new()` uses the same-file `new`, so
  `fn new() -> Result<Self, ()>` records `Result`. `Self` outside an impl and
  `T::new()` on a type parameter record nothing.
- Java: `try (var r = new Foo())` gets a row.
- Kotlin: some old constructor rows are gone (a constructor on an `object`, an
  imported name, a nested class that is not in scope). `(Repo())` and
  `Repo()!!` now get a row.
- Scala: with a companion `apply` that returns `Option[Foo]`, `Foo(1)` records
  `Option`. Some case-class rows, and rows where a parameter has the class
  name, are gone.
- Swift: tuple destructuring (`let (a, b) = Foo()`) and subscripts (`Foo[0]`)
  lose their wrong old rows. `try`, `await`, and `!` around a constructor keep
  the type.
- Dart: `Foo.create()` records the return type of the static method, not
  `Foo`. A parameter or member with the constructor's name blocks the row.
- Python: an annotation that wraps a union of two real types
  (`Union[int, str]`) records no row; it recorded the wrapper name. A `Foo()`
  row is gone when a parameter or assignment of the same name shadows `Foo`.
  Enum members get no inferred row.
- Ruby: `T.let`, `T.cast`, and trailing `#: T` comments give declared rows,
  also on constants. An `@ivar` used at more than one `self` level gets no row.
  Pattern variables and regex named groups are locals: their reads change from
  `call` to `variable_ref` identifiers, and their `calls` pending rows are gone.
- Lua: `---@return Foo, string` records `Foo`, not `Foo, string`. `Foo | Bar`
  records nothing. `Foo | nil` records `Foo` with the full text in
  `metadata.declared`. An explicit `---@return` on a constructor changes or
  turns off the old constructor rows.
- R: a same-file constructor row is gone when the file rebinds the class name,
  or when a `setGeneric`, a Reference Class, or a `with()` block can bind the
  call. Parenthesized values and `->` / `->>` assignments give constructor rows.
- JavaScript: a doc comment before `let a = 1, b = 2;` documents only `a`, so
  `b` loses its doc and its JSDoc type rows. Declared types come only from the
  last `/** */` block. `@overload`, and `@callback` or `@typedef` in the last
  block, give no declared row. A declared `@type` replaces an inferred `new`
  row.
- QML: a typed local such as `let n: int = 5` gets a declared row, which
  replaces an inferred `new Bar()` row. A bare call links only through the
  nearest object and the component root. A call to a name that is not unique
  in the file, or through an `id` that two objects in one component declare,
  becomes a pending row.
- Razor: constructor rows appear on `var` locals in `using (...)` and
  `for (...)` headers and in `@{ }` blocks.
- F#: curried static members get a declared return row. A type written on the
  pattern (`let (x: int64) = 5`) gives a declared row, not an inferred one.
  `as` patterns and `let!` constructor bindings lose their inferred rows. In
  `let rec ... and`, the first binding no longer takes a later binding's type.
  `use` bindings get literal and constructor rows. `this.X()` in an object
  expression inside a type loses `receiver_type`.
- Elixir: `{:ok, x} = ...` makes a local symbol `x`. An `alias` or nested
  `defmodule` applies only from its own position, so an earlier call becomes a
  pending row. A trailing `do` block counts as a call argument and comments do
  not, which moves call edges and the `arity` metadata of `@type` and
  `@callback`. A `@spec` after its `def` matches it; disagreeing specs give no
  row.
- Erlang: `{ok, X} = ...`, `X ?= ...`, and `{ok, X} ?= ...` make local symbols.
  Comments no longer count toward arity, which changes `name/N` signatures,
  `arity` metadata, export visibility, call edges, and `-spec` parameter rows.
  An inferred record row drops when another pattern in the function binds the
  same name.
- GDScript: a `Foo.new()` row is withheld when a parameter, local, or member
  named `Foo` hides the class, or when an `extends` cannot be resolved.
- PowerShell: cast and constructor rows are withheld for compound assignments,
  values wrapped by an operator or a leading comma, commands with a
  redirection, and generic types (`[Foo[int]]::new()`). Values in parentheses
  get a row.

C#, VB.NET, TypeScript, and PHP change nothing beyond the new inferred rows.

Consumer action: none for a reader built for 3.5.0. Receiver calls that had no
typed receiver now resolve through `type_facts`, so a consumer that resolves
calls at query time finds more callers. Replace the binary and rebuild every
artifact. No schema migration is required.

## 3.5.0

classification: compatible

This release adds the `step_definition` test role and the `gomod` and `gosum`
languages from the [gap follow-ups](../plans/2026-09-23-gap-followups.md). No
SQLite or report-schema column is added, removed, or retyped: SQLite schema
remains 7, report schema remains 3, and extraction identity epoch remains 10.
`EXTRACTION_CONTRACT_VERSION` adds `step-definition-role-v1`,
`go-module-manifest-v1`, and `go-sum-checksums-v1` because canonical output
changes.

`symbols.metadata_json.test_role` gains the value `step_definition`. `csharp`,
`vbnet`, `fsharp`, and `razor` methods of a `[Binding]` class with a `[Given]`,
`[When]`, `[Then]`, or `[StepDefinition]` attribute carry it. `php` methods of
a class that implements a Behat context interface (written qualified in the
`Behat\` namespace, or bound by a `use Behat\...` import) carry it when they
have a `Given`, `When`, or `Then` attribute or docblock tag. The same PHP class becomes
a `test_container`, and its `Before*` and `After*` hook methods become
`fixture_setup` and `fixture_teardown`. A step definition sets none of the
`is_test`, `test_container`, and `test_lifecycle` columns. See
[2026-09-23-step-definition-test-role.md](../decisions/2026-09-23-step-definition-test-role.md).

File selection changes. A file whose basename is `go.mod` or `go.sum`,
compared without case, is now the new `gomod` or `gosum` language instead of
unsupported, so a rebuild adds `files` rows. The `gomod` row publishes a
`module` symbol for the module path, `property` symbols for `go`, `toolchain`,
and each `godebug` key, and an `import` symbol for each `require` line and
`tool` package. Each required
module and tool is an `imports` relationship from the module symbol, and a
`replace` to a file path is a structured pending `imports` row to
`<path>/go.mod`. Each `require` line is a `manifest.dependency.v1` fact with
ecosystem `go`, group `require`, and a new optional boolean key `indirect`. The
new pattern ids are `gomod.module.v1`, `gomod.go.v1`, `gomod.toolchain.v1`,
`gomod.godebug.v1`, `gomod.replace.v1`, `gomod.exclude.v1`, `gomod.retract.v1`,
`gomod.tool.v1`, and `gomod.ignore.v1`. A `gomod.retract.v1` rationale keeps
at most 500 bytes and sets `rationale_truncated` when it was cut. The `gosum`
row publishes no symbols
or edges. Each checksum line is a `gosum.checksum.v1` fact with `module_path`,
`version`, `go_mod`, `hash_algorithm`, `hash`, `incompatible`, and
`pseudo_version`, plus `timestamp` and `revision` for a pseudo-version.
`julie-extract languages --json` lists 42 languages.

Rust API: the public `TestRole` enum gains `StepDefinition`, so an exhaustive
`match` on it needs a new arm.

Consumer action: accept the new `test_role` string and the new `gomod` and
`gosum` languages, replace the binary, and rebuild every affected artifact. No
schema migration is required.

## 3.4.0

classification: compatible

This release closes the gaps of the
[2026-09-22 language gap audit](../findings/2026-09-22-language-gap-audit.md)
across all 40 languages: the high-rated gaps in wave 1 and the medium and low
gaps in wave 2. No SQLite or report-schema column is added, removed,
or retyped: SQLite schema remains 7, report schema remains 3, and extraction
identity epoch remains 10. `EXTRACTION_CONTRACT_VERSION` adds
`language-gap-closure-v1` because canonical output changes for every language
family. Consumers must rebuild artifacts after replacing the binary.

File selection changes. `.bats` is a `bash` extension. An extensionless file is
`bash` when it is a shell startup file (`.bashrc`, `.bash_profile`,
`.bash_login`, `.bash_logout`, `.bash_aliases`, `.profile`, `.envrc`) or when
its first line is a `sh`, `bash`, or `bats` shebang. A rebuild therefore adds
`files` rows that earlier scans reported as unsupported. See
[2026-09-22-shell-script-selection.md](../decisions/2026-09-22-shell-script-selection.md).
Wave 2 adds these selections:

- Extensions: `ruby` gains `rake`, `gemspec`, `ru`, `jbuilder`, `builder`,
  and `thor`; `json` gains `ndjson`, `jsonld`, `geojson`, `webmanifest`, and
  `code-workspace`; `xml` gains `vcxproj`, `sqlproj`, `proj`, `runsettings`,
  `xaml`, `axaml`, `xsl`, `xslt`, `xlf`, and `xliff`; `scala` gains `sbt`;
  `erlang` gains `escript`; `regex` gains `regexp`.
- Exact base names: `Gemfile`, `Rakefile`, `Guardfile`, `Capfile`, and the
  other Ruby build files select `ruby`; `Pipfile` selects `toml`; an R package
  `NAMESPACE` selects `r`; `rebar.config`, `sys.config`, and `*.app.src`
  select `erlang`.
- A `.config` file whose first non-blank text is `<` selects `xml`; other
  `.config` files stay unsupported.

Capability flags change. `razor` now publishes `pending_relationships`; `css`,
`toml`, `yaml`, and `xml` publish `pending_relationships` and `xml` publishes
`relationships`. Wave 2 adds `pending_relationships` for `markdown`. Readers
of `language_capabilities` see the new flags; no column changes.

Embedded blocks change. HTML `<script>`/`<style>` and Vue script and style
sections run the native JavaScript, TypeScript, TSX, JSX, or CSS pipeline and
publish every row family remapped to host coordinates. Calls inside an embedded
function now come from that function, not the host element or component. An
inline HTML handler call (`onclick`, Alpine, htmx) resolves only to a top-level
function of a classic script; any other call stays pending. See
[2026-09-22-embedded-blocks-use-native-pipeline.md](../decisions/2026-09-22-embedded-blocks-use-native-pipeline.md).

Row families that move, by language family (wave 1):

- Symbols and ids. Many languages add symbol kinds they dropped before (Rust
  generic-impl methods, C and C++ pointer declarators and out-of-line
  members, Go grouped types and interface methods, JS/TS enums and
  destructured bindings, Razor `@code` members, and more) and remove false
  or duplicate rows (C type references, duplicate JS function values, Lua
  nested-local duplicates, PowerShell phantom functions, read-site variables,
  and reassignment duplicates, R `self$x` names). A PowerShell reassignment
  adds no symbol when the same scope or drive already declares the name. A changed span changes the location-derived
  symbol id, so consumers must not carry ids across the rebuild.
- Visibility. Java members with no modifier report `internal`; interface
  members report `public`. Swift defaults to `internal` and propagates
  `private` to `fileprivate` members. C# interface and enum members report
  `public`, namespace-level types `internal`, and constructors without a
  modifier `private`.
- Body spans and hashes. Rust, Kotlin, C#, VB.NET, F#, SQL, Swift, YAML,
  Markdown, and Regex take body spans from the syntax tree instead of the
  text heuristic; declarations without a body lose their body span and hash.
- Doc comments. JavaScript and TypeScript attach a doc comment to the
  declaration, not the `export` row. Elixir stores the `@doc` string content.
  Erlang orders `-doc`, EDoc, and `-moduledoc` sources. YAML treats a
  same-column comment block above a key as its doc comment, and source regions
  follow the same rule.
- Relationships and pending relationships. Many languages add call, extends,
  implements, imports, and references edges that were missing, and remove
  self-edges and pending rows to test-DSL words (`describe`, `it`, `test`,
  `context`). Elixir pending remote calls carry the full module name as one
  namespace segment. SQL built-in function calls become pending `calls` rows.
- Identifiers. Receiver metadata no longer treats `->` as a member separator
  outside C, C++, and PHP. PowerShell names drop the `$` sigil and scope
  qualifier. Several languages add `call`, `member_access`, and `type_usage`
  rows for positions they skipped.
- Type facts. Swift, Python, PHP, Zig, and QML add declared type facts and
  remove placeholder facts (`Void`, `Any`, symbol-kind names). Bash type facts
  are keyed by symbol id and now persist.
- Structural facts. New pattern ids: `css.import.v1`,
  `manifest.dependency.v1`, `openapi.route.v1`, `sinatra.route.v1`,
  `sinatra.filter.v1`, `xml.msbuild_property.v1`, `yaml.ci_job.v1`,
  `yaml.ci_trigger.v1`, `yaml.ci_uses.v1`, and `yaml.ref.v1`. Existing Go
  router, Express and Fastify, Django, Razor Pages, and SQL facts cover more
  source forms. `docs/contracts/structural-fact-patterns.json` lists every id
  and its metadata keys.
- Test roles. Several frameworks gain container and lifecycle roles (Scala
  suites, Python Django `TestCase` bases, F# .NET test attributes, QML
  `TestCase`, C Criterion, C++ Catch2, bats, ShellSpec).

Wave 2 row-family changes, by language family:

- Scripting. Python emits PEP 695 aliases, protocols, enum members, and match
  patterns with their own kinds, and decorator receivers drop the `@`. Ruby
  Rails routes come from a syntax walk (member, nested, scope, namespace,
  mount, root, match); Rake files give namespace and task symbols. PHP emits
  one symbol per `const` or property element and per `use` clause, `define()`
  constants, clean pending receiver paths, and Codeception and PHPSpec roles.
- Data and markup. JSON `$ref` pointers resolve to `references` edges or
  structured pending rows, and JSONC comments become doc comments. TOML, YAML,
  and XML comments before a key or element become doc comments. YAML
  flow-mapping pairs and anchored sequence items become symbols, leaf pairs
  carry their first line as signature, and aliases bind to the nearest earlier
  anchor. XML symbol kinds follow the declared vocabulary (XSD, WSDL, XSLT,
  XAML, MSBuild, Ant, Spring, MyBatis, TestNG, resx). Markdown heading names
  are plain text, anchors use GitHub slugs, and reference links and footnotes
  give `references` edges or structured pending rows.
- Dynamic. Lua class idioms become classes with `extends` rows and LuaLS
  annotations give type facts. Lua call-argument table fields are no longer
  symbols, and `setmetatable` instances are no longer classes. R emits S4, R6,
  RefClass, and S7 members and more import forms. Bash drops duplicate
  declaration rows and prefix assignments, honors declaration flags, and links
  wrapped commands and trap handlers.
- QML, SQL, and Regex. QML signal handlers own their calls, property and
  handler body spans cover the value, and declaration sites emit no
  `variable_ref` rows. SQL type facts come from grammar type nodes with
  `is_inferred` false, docs stop bleeding into columns, and unnamed constraints
  and top-level select aliases lose their symbols. Regex lookarounds and
  property escapes are nested symbols.
- .NET. C# adds event, positional record property, and pattern-variable
  symbols. Before the parse, C# blanks preprocessor directive lines and every
  `#elif`/`#else` branch, so only the first branch of an `#if` group yields
  rows and the parse diagnostics that split directives caused are gone. F#
  rewrites `static member val` and `default val` to `member val` at the same
  byte length before the parse. VB.NET adds typed locals and `Implements`
  links. F# adds constructors, properties, operators, active patterns,
  interfaces, and `open`/`#load`/`#r` imports. Razor emits one file class per
  `.razor` or `.cshtml` file that owns template calls, one row per directive,
  and private default visibility, and render-fragment tags such as `<Columns>`
  are no longer component references. PowerShell parameters parent to the right
  function and unexported `.psm1` functions are private.
- JVM. Java adds interface constants, annotation elements, and module
  namespaces. Kotlin accessors and operator functions own their calls, plain
  constructor parameters are private, and `null`, `true`, and `false` are no
  longer `variable_ref` rows. Scala member `val`/`var` are properties, and
  extensions and anonymous givens get stable names.
- Apple, Dart, and Godot. Swift adds operator and macro symbols. Dart adds
  extension types, mixin applications, libraries, parts, and typedefs, and
  drops false body spans. GDScript visibility follows the underscore rule.
- C family. C and C++ bodiless declarations lose body spans and hashes, return
  types become recorded type facts, and trailing and Doxygen `/*!` docs attach
  to their symbols. C++ `#include`/`#define` give import and constant rows.
- BEAM. Elixir special forms emit no calls, `@attr` becomes a constant, and
  Ecto schemas give struct and field symbols. Erlang pending calls carry
  `arity` in `metadata_json`, and application resource and rebar files
  extract.
- Web. CSS rule sets are property symbols and at-rules are namespaces. HTML
  drops the synthetic `url:`/`resource:`/`endpoint:`/`script:` relationships
  in favor of structured pending rows, and element body spans run between the
  tags. Vue components span the whole file, and `defineProps`,
  `defineEmits`, and `defineModel` members are symbols.
- ECMAScript. TypeScript and JavaScript emit one export row per exported name,
  and export rows no longer parent declarations. Visibility follows module
  exports. Calls inside field initializers, object methods, and initializer
  variables belong to that declaration. JavaScript data object literals in
  expressions no longer emit property symbols. The `string` keyword no longer
  yields string regions.
- Systems. Rust impl members parent to the implemented type and item macros
  yield real items. Go adds `extends` and `implements` rows and range and
  type-switch bindings. Zig declaration kinds follow the initializer node.
- Receivers. A `.` on the previous line never names a receiver. A leading `@`
  is part of a receiver only in Ruby and C#. PowerShell reads a `.` after
  whitespace as an argument. Rust `.await` is not a receiver.
- Structural facts. Wave 2 adds these pattern ids: `akka_http.route.v1`, `angular.route_definition.v1`, `aspnet.conventional_route.v1`, `chi.mount.v1`, `chi.route.v1`, `cowboy.route.v1`, `css.scope.v1`, `css.tailwind_apply.v1`, `css.tailwind_directive.v1`, `drf.api_view.v1`, `drf.router_registration.v1`, `drf.viewset_action.v1`, `efcore.db_set.v1`, `efcore.entity_configuration.v1`, `efcore.table_mapping.v1`, `fiber.route.v1`, `fsharp.active_pattern.v1`, `fsharp.computation_expression.v1`, `fsharp.quotation.v1`, `giraffe.route.v1`, `go_router.route_definition.v1`, `go_router.route_reference.v1`, `godot.node_path.v1`, `godot.resource_reference.v1`, `godot.rpc_annotation.v1`, `godot.signal_connection.v1`, `godot.signal_emission.v1`, `gorilla_mux.route.v1`, `hapi.route.v1`, `html.embed.v1`, `html.resource_link.v1`, `http4s.route.v1`, `httpz.route.v1`, `java.module_directive.v1`, `jaxrs.route.v1`, `json.schema_definition.v1`, `koa.route.v1`, `lapis.route.v1`, `lazy_nvim.plugin_spec.v1`, `love.callback.v1`, `manifest.script.v1`, `markdown.autolink.v1`, `markdown.definition_list_item.v1`, `markdown.footnote_definition.v1`, `markdown.footnote_reference.v1`, `markdown.reference_link.v1`, `markdown.task_list_item.v1`, `neovim.autocmd.v1`, `neovim.keymap.v1`, `neovim.user_command.v1`, `php.include_call.v1`, `plumber.route.v1`, `powershell.data_key.v1`, `powershell.dsc_resource.v1`, `powershell.module_manifest.v1`, `qml.component_url.v1`, `qmldir.static.v1`, `qmldir.system.v1`, `r.namespace_directive.v1`, `razor.layout_reference.v1`, `razor.model_binding.v1`, `razor.mvc_link.v1`, `razor.partial_reference.v1`, `razor.view_component_reference.v1`, `regex.backreference.v1`, `regex.inline_flags.v1`, `regex.quoted_literal.v1`, `rocket.mount.v1`, `rocket.route.v1`, `shelf_router.route.v1`, `shiny.app.v1`, `shiny.input.v1`, `shiny.module.v1`, `shiny.output.v1`, `shiny.reactive.v1`, `spring.functional_route.v1`, `sql.extension.v1`, `sql.policy_definition.v1`, `sql.role_definition.v1`, `sql.sequence_definition.v1`, `sql.type_definition.v1`, `swiftpm.package.v1`, `swiftpm.product.v1`, `swiftpm.target.v1`, `vapor.route.v1`, `xml.android_component.v1`, `xml.android_permission.v1`, `xml.config_entry.v1`, `xml.document_link.v1`, `xml.mybatis_statement.v1`, `xml.servlet_route.v1`, `xml.spring_bean.v1`, `xml.spring_component_scan.v1`, `xml.test_selection.v1`, `xml.xsd.schema.v1`, `yaml.ansible_task.v1`, `yaml.compose_service.v1`, `yaml.k8s_resource.v1`, `zig.build_artifact.v1`, `zig.build_dependency.v1`, `zig.build_module.v1`, `zig.build_module_import.v1`, `zig.build_step.v1`.
  Existing Rails, Phoenix, Spring, Laravel, and HTTP client facts cover more
  forms and metadata keys.

Per-language detail is in `docs/languages/*.md` and in the golden fixtures
under `fixtures/extraction/`.

Consumer action: replace the binary and rebuild every artifact. No schema
migration is required. A consumer that pinned the previous
`EXTRACTION_CONTRACT_VERSION` must accept the new suffix.

## 3.3.1

classification: compatible

This unreleased review patch corrects Qt/QML extraction rows without adding,
removing, or changing the type of a SQLite or report-schema column. SQLite
schema remains 7, report schema remains 3, and extraction identity epoch remains
10. `EXTRACTION_CONTRACT_VERSION` adds `qt-reference-corrections-v1` because canonical
output changes. Consumers must rebuild artifacts after replacing the binary.

`qml` rows change at inline-component scope boundaries. Calls and pending calls
now use the owning same-line inline component, respect local and parameter
shadowing, and keep inaccessible known targets as structured pending edges. The
same applies to handler calls. Nested-object `parent` and `this` binding `uses`
relationships now target the exact object row rather than a containing component,
and same-line inline-component `extends` relationships use byte-exact ownership.
Consumers must rebuild to replace previously mis-owned or falsely resolved
relationship rows.

`cpp.qt_property.v1` structural-fact metadata and the corresponding `property`
symbol metadata can now carry optional string keys `designable`, `scriptable`,
`stored`, `user`, and `revision`. These are additive metadata keys registered in
`structural-fact-patterns.json`; existing readers continue to parse the rows.

`javascript` QML-directive import symbols and pragma facts now end at directive
text rather than including a trailing line comment, so their source spans and
location-derived ids change for that input. C++ Qt preprocessing preserves
runtime calls for `Q_ASSERT`, `Q_ASSERT_X`, `Q_CHECK_PTR`, `Q_ASSUME`,
`Q_LIKELY`, `Q_UNLIKELY`, `Q_UNREACHABLE`, and `Q_UNUSED`; preserves `Q_NULLPTR`
unmodified in ordinary expressions; leaves ordinary `signals:` and `slots:`
labels outside a class body untouched; restores declaration-context macro
preprocessing; and parses numeric digit separators without hiding later macros.
Typed `Q_D`, `Q_Q`, and `Q_FOREACH` remain declaration-preprocessed. This does
not expand Qt macros or publish new typed-loop facts. Consumers must rebuild to
remove the old parse diagnostics and restore the corrected symbols, identifiers,
and facts.

Consumer action: replace the binary and rebuild every affected artifact. No
schema migration is required.

## 3.3.0

classification: compatible

Qt C++ support changes the `cpp` rows. A macro pre-pass blanks the Qt macros before the parser
reads a `.h` or `.cpp` file, so a Qt class parses for the first time, and four pre-existing C++
defects are fixed at the same time. The SQLite schema stays 7 and the report schema stays 3, and
every column keeps its type, so a reader built for 3.2.0 still parses the output. The row content
moves, so a consumer that matched on the old shapes must follow the notes below. Consumers that
already hold an artifact must rebuild it, because an unchanged file keeps its stored rows.

`symbols`, new cpp `property` rows: every `Q_PROPERTY(...)` line in a class or struct body is a
`property` symbol at the macro's own range, named by the property, parented to the innermost class
or struct, visibility public, with the whitespace-collapsed macro text as its signature. Its
`metadata_json` carries `property_type` and, when the macro names them, `read`, `write`, `notify`,
`member`, `reset` and `bindable` as strings, and `constant`, `final`, `required` as booleans.
Consumer action for code-kb: read Qt properties as `property` rows and their accessors from the
metadata; do not look for a symbol named `Q_PROPERTY`.

`symbols`, new cpp `event` rows: a method declared in a `signals:` or `Q_SIGNALS:` section, or
prefixed by `Q_SIGNAL`, is a `event` row instead of a `method` row. Visibility is public, as Qt
defines a signals section. Consumer action for code-kb: accept `event` as a cpp symbol kind in
skeletons and search, and expect a Qt signal not to appear in a `method` query.

`symbols.metadata_json`, new cpp keys: a method in a `slots:` or `Q_SLOTS:` section or prefixed by
`Q_SLOT` carries `qt_slot: true`; a method prefixed by `Q_INVOKABLE` carries `qt_invokable: true`.
A class or struct carries `qt_object: true` for `Q_OBJECT`, `qt_gadget: true` for `Q_GADGET`,
`qml_element` and `qml_singleton`, `qml_anonymous` and `qml_attached` for the matching `QML_*`
macros, and `qml_uncreatable` for `QML_UNCREATABLE`. A `QML_ELEMENT` with no argument names the
class itself. A macro outside a class body sets nothing. Consumer action for code-kb: none required,
the keys are additions; read them to tell a QML-exposed class from a plain one.

`symbols`, cpp rows that disappear: a row named `Q_PROPERTY`, and rows named after other Qt macros,
are gone, because the macro no longer parses as a declaration — 294 such rows on the Kirigami
corpus and 786 on plasma-workspace fall to 0. Empty-name rows for a section label are gone: 5 on
Kirigami and 28 on plasma fall to 0. A forward declaration such as `class ColumnView;` emits no
`class` row, because it names no new type: 57 one-line `class X` rows on Kirigami and 653 on plasma
fall to 0. A declared constructor or destructor emits one row instead of two; the duplicate at the
same path, name and line is gone: 43 on Kirigami and 493 on plasma fall to 0. A `Q_EMIT` or `emit`
line emits no `variable` row. Consumer action for code-kb: a symbol count for a C++ file drops, and
a stored id for one of these rows no longer resolves; rebuild the artifact.

`symbols.signature`, cpp methods: a method declared inside a class keeps its declared return type.
Before this release the declaration carried no type at all, and a modifier from a sibling member
leaked onto it, so `void setIndex(int index);` beside `void paint() override;` rendered as
`override setIndex(int index)`. The false leading `override` is gone from 412 signatures on
Kirigami and 2,365 on plasma-workspace — 0 signatures start with `override ` on either corpus now.
A leaked `explicit`, `static` or `virtual` from a sibling member is gone for the same reason. The
return type is now the whole declared type: the `const` qualifier, one `*`, `&` or `&&` per pointer
or reference wrapper, template arguments and namespace qualifiers are all kept, so a member reads
`QQuickItem *contentItem() const`, `const QString &name() const`, `QList<int> *items()` and
`static ColumnViewAttached *instance()`. `override` and `final` are trailing specifiers, written
after the parameters and the `const` qualifier the way C++ writes them, never leading modifiers.
`= 0`, `= default` and `= delete` on a method are not part of its signature; a constructor keeps
`= default` and `= delete`. Consumer action for code-kb: re-index, and match a return type at the
head of the signature rather than assuming a C++ member signature starts with its name.

`symbols`, cpp constructor and destructor rows: both now carry the visibility of their access
section. Before this release both were hardcoded `public`, so a `private:` constructor was reported
public. A destructor signature no longer ends in `;`: `~ColumnViewAttached() override;` is now
`~ColumnViewAttached() override`. Consumer action for code-kb: read the stored visibility as the
declared one, and render a destructor signature as-is.

`structural_facts`, one new pattern id: `cpp.qt_property.v1`, one fact per `Q_PROPERTY` site, with
`capture_name` `qt_property`, `node_kind` `macro`, `containing_symbol_id` set to the enclosing class
or struct symbol, and the same metadata keys as the property row (`property_type`, `read`, `write`,
`notify`, `member`, `reset`, `bindable`, `constant`, `final`, `required`). Facts are emitted at
`--level facts` and above; a `--level symbols` scan emits none. Consumer action for code-kb: none
required, the pattern is an addition registered in `structural-fact-patterns.json`.

`parse_diagnostics`, far fewer cpp rows: the Qt macros no longer produce diagnostics, because the
pre-pass blanks them before the parse. On Kirigami, files with diagnostics fall from 70 to 12 and
diagnostics from 981 to 31; on plasma-workspace, files fall from 649 to 77 and diagnostics from
4,627 to 298. `src/layouts/columnview.h` in Kirigami falls from 92 diagnostics to 0. The remaining
rows are tree-sitter-cpp grammar limits and project-local macros, not Qt vocabulary. Consumer action
for code-kb: none required; a Qt header that was previously skipped or half-extracted now yields its
whole symbol table, so its row count rises.

`language_capabilities`, one row: the `cpp` row adds `event` and `property` to
`kind_coverage.symbols.supported` and to `kind_coverage.body_spans.not_applicable`, and adds
`cpp.qt_property.v1` to its structural facts. Consumer action for code-kb: none, the snapshot
describes the row changes already listed above.

`language_capability_fixtures`, one new row: `cpp`/`qt_header`. The
`capability_snapshot_fingerprint` value in `artifact_metadata` changes with it. Consumer action for
code-kb: none, the table lists this repository's own fixtures, and the fingerprint is per-scan
identity a consumer does not join on.

Golden fixtures: `cpp/basic` and `cpp/test_roles` changed accordingly, in `signature` and in the
type-inference `resolved_type` that follows it. No symbol row was added or removed in either file.
`cpp/qt_header` is new. Consumer action for code-kb: none, the fixtures are this repository's own.

`EXTRACTION_CONTRACT_VERSION` gains the `.qt-cpp-v1` suffix. `EXTRACTION_IDENTITY_EPOCH` stays 10,
as in 3.2.0: the contract version is what consumers observe, the epoch keys input identity, and no
symbol id of an unchanged non-Qt file moves. Consumer action for code-kb: treat the new suffix as
the drift signal that the stored artifact must be rebuilt.

## 3.2.0

classification: compatible

Qt support changes the QML, qmldir, and JavaScript rows. The schema does not change, and every
column keeps its type, so a reader built for 3.1.3 still parses the output. The row content moves,
so a consumer that matched on the old shapes must follow the notes below. Consumers that already
hold an artifact must rebuild it, because an unchanged file keeps its stored rows.

`symbols`, QML plain property bindings: a binding such as `width: 100` is no longer a symbol. The
same evidence stays in `structural_facts` as `qml.binding.v1`. The `uses` relationships that pointed
at those rows go with them: on the `plasma-framework` corpus in `docs/languages/qml.md`, resolved QML
relationships fall from 1,112 to 235 and pending rows from 2,360 to 1,682. Consumer action for
code-kb: read bindings from the fact table, not from the symbol table.

`symbols`, QML grouped property blocks: a block such as `anchors { fill: parent }`,
`font { pixelSize: 12 }`, or `border { width: 1 }` parses as an object but binds a group of
properties, so it emits no `field` row, no `instantiates` relationship, no pending row, and no
`type_usage` identifier. A type name whose terminal segment starts with a lowercase letter is the
rule. Its inner bindings stay `qml.binding.v1` facts. Consumer action for code-kb: read a grouped
property block from the fact table, and expect no symbol row named `anchors`, `font`, or `border`.

`symbols`, QML nested objects: a nested object is now a `field` row. Its name is the object's `id`
when it has one, else its type name; its signature is `id: Type` or the bare type; its metadata
carries `object_type`, `binding_kind: "object"`, and `value_source_property` for a `Behavior on`
value source. Consumer action for code-kb: accept `field` as a QML symbol kind in skeletons and
search.

`symbols`, QML nested `id:` bindings: a nested object no longer emits a separate `property` row
for its `id:` binding. The id names the object's own `field` row instead, so the evidence moves
rather than disappears. The file root and an inline component's body still emit their `id:`
property rows, because those objects carry a `class` row that the id cannot name. Consumer action
for code-kb: resolve a nested id against `field` rows, not against `property` rows with an
`id: ` signature.

`symbols`, QML nested-object members: a property, function, signal, or nested object declared
inside a nested object now has that object's `field` row as its `parent_symbol_id`. Before this
release every such member parented to the enclosing `class` row. Consumer action for code-kb:
walk the parent chain to reach the owning component instead of reading `parent_symbol_id` as the
component directly.

`symbols`, QML multi-line property signatures: a `property` row whose value spans more than one
line keeps only the declaration head in `signature`, with the trailing colon removed, instead of
the whole declaration text. A single-line declaration is unchanged. Consumer action for code-kb:
render the stored signature as-is; it is already the one-line form a skeleton needs.

`symbols`, JavaScript `.import` directive rows: an `.import` line at the head of a `.js` file now
emits an `import` symbol. Its name is the imported source, its signature is the directive text, and
its metadata carries `source`, `source_kind` (`uri` or `quoted`), `import_kind`, `alias`,
`local_name`, `imported_name`, `is_namespace` (always true), and `version` for a module import.
Consumer action for code-kb: read JavaScript QML imports from these rows the same way it reads QML
`import` rows.

`symbols`, qmldir type rows: a type row's kind changes from `class` to `export`. Consumer action
for code-kb: query qmldir types by kind `export`.

`symbols`, new QML rows: inline components are `class` rows with the signature `component Name: Base`;
signals carry the signature `signal name(type arg, ...)` and a `parameters` metadata list; a bare
`required property` row carries `required: true`; a `pragma Singleton` file marks its root
`singleton: true`. Consumer action for code-kb: none required, the rows are additions.

`pending_relationships` and `relationships`, new kind: a QML root object emits an `extends`
relationship to its base type, concrete when the base type is a symbol in the same file and a
structured pending row otherwise. Consumer action for code-kb: include `extends` where it reads
QML inheritance.

`relationships` and `pending_relationships`, inline component bodies: the object directly under a
`component Name: Base` header is that inline class's body, so it emits `extends` from the inline
`class` row instead of `instantiates` from the outer class. A qualified base type such as
`QQC2.Button` never resolves to a same-file class; it always takes the structured pending row that
carries its terminal name and receiver. Consumer action for code-kb: read an inline component's
base type from its `extends` row, not from an `instantiates` row on the enclosing component.

`identifiers`, QML type usages: a qualified type name such as `QQC2.Button` is now recorded by its
terminal segment (`Button`) with the qualifier in the new `receiver` metadata key. Before this
release the whole dotted text was the identifier name. Consumer action for code-kb: match the
terminal name and read the qualifier from the metadata.

`identifiers.metadata_json`, new keys: `role` (`base_type`, `attached_type`, `signal_handler`),
`receiver`, and `change_handler` (true for an `onXChanged` handler). A `.qmltypes` root is a module
descriptor, so it records no `base_type` identifier. Consumer action for code-kb: read `role` when
it separates base types from ordinary type usages; the keys are optional and absent rows keep their
old meaning.

`structural_facts`, two new pattern ids: `qml.pragma.v1` (metadata `name`, `value`) and
`javascript.qml_directive.v1` (metadata `directive`, `name`) for a `.pragma` line at the top of a
JavaScript file. Consumer action for code-kb: none required, both are additions registered in
`structural-fact-patterns.json`.

`language_capabilities`, three rows: the `qml` row adds `field` to its symbol kinds, body spans,
annotation exclusions, and doc-comment exclusions, adds `extends` to its relationship kinds, and adds
`qml.pragma.v1` to its structural facts. The `qmldir` row replaces `class` with `export` in the same
four lists. The `javascript` row adds `javascript.qml_directive.v1` to its structural facts. Consumer
action for code-kb: none, the snapshot describes the row changes already listed above.

`language_capability_fixtures`, two new rows: `qml`/`qt_symbols` and `javascript`/`qml_directives`.
Consumer action for code-kb: none, the table lists this repository's own fixtures.

Public extractor API: `Identifier` gains an optional `metadata` field. A crate that builds an
`Identifier` literal must add the field; a crate that reads one is unaffected. Consumer action for
code-kb: none, it reads identifiers from SQLite.

## 3.1.1

classification: compatible

Two tables change: `pending_relationships` and `relationships`. A qualified call chain `Q1.Q2 ... Qn.t(...)` now
records `target_terminal_name = t`, `target_receiver = Qn`, and `target_namespace_json =
[Q1 .. Qn-1]`, with `target_display_name` joined by `.`. Before this release each language chose
its own split: Ruby kept `Net::HTTP` whole in the receiver, C++ kept `->` in the display name, Java,
C++, PowerShell, and Zig dropped the qualifiers or emitted no row, and C# recorded the receiver as
the terminal name. Eighteen languages change (C#, Java, Kotlin, Go, PowerShell, C++, Zig,
JavaScript, TypeScript, Python, Swift, Dart, Lua, GDScript, C, PHP, Ruby, R); Scala and VB.NET already
matched. Rust, Elixir, F#, and Erlang are unchanged: they record a path-style call with every
qualifier in `target_namespace_json` and an empty `target_receiver`. The Java fixture also loses two resolved `relationships`
rows for `fixture.Worker.evaluate` and `fixture.Worker.observeRun`, which now surface as pending
rows with receiver `Worker` and namespace `["fixture"]`.
Four Java fixture outputs also remove duplicate pending rows after the extractor stopped traversing
the same call site repeatedly; no distinct Java relationship is removed.
Ruby `RSpec.describe` pending rows now retain `RSpec.describe` as their terminal and display names
instead of including the enclosing block text.

Nothing in the schema changes and no reader breaks: the same columns carry the same kinds of
values, and a reader built for 3.1.0 reads the new output correctly. A reader that matched
`target_receiver` against a whole chain must match the last qualifier and read the rest from
`target_namespace_json`. Consumers that already hold an artifact must rebuild it rather than
rescan, because an unchanged file keeps its stored rows.

## 3.1.0

classification: compatible

Three tables change. First, `symbols`: a fenced code block in a standalone markdown file no longer
carries the rustdoc test rule. Blocks with no info string, an info string whose first token is
`rust` (such as `rust,no_run` or `rust,should_panic`), or an info string made only of `no_run` and
`compile_fail` previously set `is_test = 1` and `metadata_json.test_role = "test_case"`. They now carry neither, and markdown emits no test roles
at all. Second, `language_capabilities`: the markdown row moves `test_case` out of
`kind_coverage.test_detection.supported` and into `not_applicable`, which is the same change
stated as a capability claim. Third,
`symbol_annotations` and `complexity_metrics`: both tables are written only at `--level full`. A
`symbols` or `facts` scan leaves them empty; a `full` scan of non-markdown source is otherwise
byte-identical to 3.0.0.

Nothing in the schema changes and no reader breaks: a reader that counted markdown code blocks as
tests now sees fewer test symbols, and a reader that needs annotations or complexity metrics scans
at `full`. Consumers that already hold an artifact must rebuild it rather than rescan, because an
unchanged file keeps its stored rows.

## 3.0.0

classification: compatible

One table moves: `language_capabilities`. The `capability_gaps` JSON column carries prose that
named Miller as the consumer responsible for cross-file route-prefix joins (Phoenix, Laravel,
NestJS, and the `.NET` service lanes). That prose now names `code-kb`. No key, kind, or
structural value changes; only the human-readable `reason` and `required_closure` text differs.
Every other compared table is byte-identical to 2.43.0. A reader built for 2.43.0 reads the new
output correctly; nothing to do.

## 2.41.1

classification: compatible

Agent-usefulness extraction repairs add facts without changing tables, columns,
or JSONL fields. Named return types now emit exact `type_usage` identifiers in
all 25 languages with native return-type syntax. ASP.NET minimal APIs cover
`MapHead`, `MapOptions`, literal `MapMethods` verbs, route groups, and handler
metadata. htmx and component attributes preserve normalized template evidence
and explicit uncertainty. Java single-segment package declarations now emit
their namespace symbol.

Artifact schema remains 7, JSONL remains contract v5, and family stores remain
store schema 2. Extraction identity advances from 9 to 10 so unchanged files
are re-extracted instead of reusing epoch-9 rows that predate these facts.

Consumer action: replace the binary and re-extract, or let epoch-10 file
versions populate through the family-store writer. Existing artifacts remain
readable. Existing families preserve their stored reader floor; newly created
2.41.1 families stamp `min_reader_version = 2.41.1`, so consumers adopting the
binary must advertise 2.41.1 reader capability. Do not delete prior file
versions.

## 2.41.0

classification: compatible

Optional Rust host syntax API (`syntax-api` Cargo feature) and public crate-root
export of existing relationship fact types (`PendingSpan`, `UnresolvedTarget`).

Extraction output, database schemas, and CLI contracts remain completely unchanged:
- Artifact schema remains 7.
- JSONL export contract remains v5.
- Extraction identity epoch remains 9.
- SQLite/JSONL fact table outputs and CLI behavior remain 100% byte-identical.
- Default features and dependencies in `Cargo.toml` are unchanged.
- Primary CLI and artifact interfaces remain unchanged.

## 2.39.0

classification: compatible

Receiver-type facts waves 1 and 2 change extraction output for every
general-purpose language (wave 1: csharp, typescript, javascript, python, rust,
go, java; wave 2: the remaining twenty plus wave-1 refinements), and the
`RAZORBACK` marker token widens `code.marker.v1`, without touching any table,
column, or JSONL field. Artifacts stay schema 7, JSONL stays contract v5, and
family stores stay store schema 2. The extraction identity epoch advances from
8 to 9, so family-store import writes fresh epoch-9 file versions instead of
reusing epoch-8 identities that predate these rows.

- New `variable` symbol rows for parameters (`metadata_json.role = "parameter"`)
  and function-local declarations, parented to the enclosing callable.
- New `structural_facts` rows for `RAZORBACK` comments under `code.marker.v1`.
- Wave 1 dropped legacy `types` values that can never match a type symbol
  (whitespace, commas, dangling `<` or `>`) and the TypeScript `unique symbol`
  annotation row; 66 rows left the goldens.
- New `type_facts` rows for declared and same-file inferred types; every
  `resolved_type` is a base type name, with the full declared text in
  `metadata_json.declared` when it differs.
- `receiver_type` keys in `identifiers.metadata_json` and
  `pending_relationships.metadata_json` for self-style call sites.
- Kind changes on existing rows: function-local `val`/`let`/`const`/`final`
  rows become `variable` (kotlin, swift, gdscript, scala, zig, dart, ruby);
  ruby `@x`/`@@x` and razor `@code` fields become `field`; powershell class
  constructors become `constructor`; kotlin and scala primary-constructor
  parameters are `property` rows under the class.
- Span changes on existing rows: dart callable symbols span the whole
  declaration; R6 and RefClass members span their own argument. Containing
  and caller-scope keys inside those bodies move with them.
- Legacy inference rows that carried non-base-name text (`[Foo]`,
  `List<String>`, `void Function()`, `*Worker`, `final`, `inferred`) are gone.
- `language_capabilities` rows change for the nineteen closed `open_gaps`
  entries; lua and r claim `types`.

`EXTRACTION_CONTRACT_VERSION` gains the suffixes `marker-razorback-v1`,
`receiver-type-facts-v1`, and `receiver-type-facts-v2`.

Consumer action: replace the binary and re-extract. An existing artifact keeps
reading unchanged, but an index that merges 2.38.x and 2.39.0 output disagrees
on the kinds and spans above and misses the new fact rows until re-extracted.

## Rust API

Version 2.39.0 narrows the `julie-extractors` crate root to canonical extraction entrypoints, fact row types, enums, capability snapshot types, and language detection utilities:

- **Internal modules made `pub(crate)`:**
  - Infrastructure modules: `base`, `registry`, `pipeline`, `test_detection`, `test_calls`, `utils`, `language`.
  - All 38 language extractor modules: `bash`, `c`, `cpp`, `csharp`, `css`, `dart`, `elixir`, `erlang`, `fsharp`, `gdscript`, `go`, `html`, `java`, `javascript`, `json`, `kotlin`, `lua`, `markdown`, `php`, `powershell`, `python`, `qml`, `qmldir`, `r`, `razor`, `regex`, `ruby`, `rust`, `scala`, `sql`, `swift`, `toml`, `typescript`, `vbnet`, `vue`, `xml`, `yaml`, `zig`.
- **Removed exports from crate root:**
  - `BaseExtractor` (language implementations and base extractor state are crate-internal)
  - `is_test_symbol` (internal test detection logic)
  - `detect_language_from_extension` (internal extension mapping; consumers use `detect_language_for_path` or `detect_language_for_source`)
  - `get_tree_sitter_language` (internal tree-sitter language loader)
  - `LanguageRegistryEntry` (internal registry record; consumers query `supported_languages` or `capability_snapshot`)
- **New crate-root re-exports:**
  - `NormalizedSpan`, `StructuredPendingRelationship`
  - `supported_languages`
  - `classify_literals_by_carrier`
  - `structural_fact_patterns_json`

## 2.38.2

classification: compatible

Extraction identity epoch advances from 7 to 8. v2.38.0 already maps a C#
declaration marked `internal` to visibility `internal` instead of `private`,
but it left the identity epoch at 7. Family-store import reuses a completed
`(path, content_hash, extraction_epoch)` identity, so an unchanged C# file
kept its epoch-7 rows and never rewrote L1.

No table, column, or JSONL field is added, removed, or renamed. Artifact
dumps stay byte-identical to v2.38.1. A reader built for v2.38.1 still reads
schema 7 / JSONL v5 / store schema 2. The capability snapshot is keyed to
epoch 8 so a live store does not collide with the epoch-7 snapshot (39
languages vs 40 after F#).

Consumer action: replace the binary and let epoch-8 family-store file versions
populate on the next import or update. Epoch 7 rows stay immutable. Do not
delete live `file_versions` to force a rewrite.

## 2.38.0

classification: compatible

F# support and three extractor corrections change extraction output without
touching any table, column, or JSONL field. Artifacts stay schema 7, JSONL
stays contract v5, family stores stay store schema 2, and the extraction
identity epoch stays 7.

- New language `fsharp` (`.fs`, `.fsx`, `.fsi`): new rows in every fact table
  for F# sources, and a new `fsharp` row in the grammar inventory, which
  shifts the dump order of every inventory row that sorts after it.
- `language_capabilities.kind_coverage_json` widens with the pattern ids
  `rust.doc_test.v1` and `fsharp.attribute.v1`.
- Rust: `rust.doc_test.v1` structural facts record rustdoc fences in `///`
  and `//!` line comments and in `/** ... */` and `/*! ... */` block doc
  comments — new rows for rust sources that carry doc tests.
- C#: a declaration marked `internal` now reports visibility `internal`
  instead of `private`, so existing C# symbol rows change value on
  re-extract.
- Go: literal `t.Run` subtests emit test rows that did not exist before.

`EXTRACTION_CONTRACT_VERSION` gains the suffixes `csharp-visibility-v2`,
`go-subtests-v1`, `rust-doc-test-facts-v1`, and `fsharp-v1`.

Consumer action: replace the binary and re-extract. An existing artifact keeps
reading unchanged, but an index that merges 2.37.x and 2.38.0 output disagrees
on C# visibility values and misses the new fact rows until re-extracted.

## 2.37.1

classification: compatible

The xml language spec now claims the MSBuild and .NET project XML extensions:
`csproj`, `props`, `targets`, `vbproj`, `fsproj`, `slnx`, `nuspec`, and `resx`.
`sln` stays unclaimed because it is not XML. Files with these extensions were
previously dropped by discovery as `unsupported`; they now parse through the
xml extractor and publish the same data-language facts as `.xml` files, in the
existing schema 7 tables. No table, column, or JSONL field is added, removed,
or renamed. A reader built for v2.37.0 still reads schema 7 / JSONL v5 / store
schema 2.

Extraction identity epoch is 7. Family-store file versions re-extract because
identity is `(path, content_hash, extraction_epoch)`, and paths recorded as
`unsupported` in earlier scans re-enter extraction.

Consumer action: replace the binary and re-extract or rebuild standalone
artifacts, or let epoch-7 family-store file versions populate on the next
import or update. The compat fixture contains none of the new extensions, so
the gate byte-matches; this entry declares the change on the strength of the
spec diff.

## 2.36.0

classification: compatible

Test-role contract expansion: `test_role` string, lifecycle direction, per-language
role corrections.

Every symbol that the shared test-detection writer flags now carries a `test_role`
string in `symbols.metadata_json`, next to the existing `is_test`,
`test_container`, and `test_lifecycle` booleans. The value is one of `test_case`,
`parameterized_test`, `fixture_setup`, `fixture_teardown`, or `test_container`.
One helper writes the booleans and the string together, so the two can never
disagree. The lifecycle arms now report a direction — setup, teardown, ambiguous,
or none — instead of a bare "is a lifecycle hook" answer. A hook that wraps a test
case on both sides (an `around`-style hook) reports `Ambiguous` and takes the
`fixture_setup` role, because a wrapping hook always runs its setup half first.
Later work on this branch corrects per-language role classification for ten
languages; this one entry covers that whole branch.

The typed `symbols.is_test`, `symbols.test_container`, and `symbols.test_lifecycle`
columns keep their current values. No table, column, or JSONL field is added,
removed, or renamed. A reader built for v2.35.1 ignores the new metadata key and
still reads schema 7 / JSONL v5 / store schema 2.

Extraction identity epoch is 6. Family-store file versions re-extract because
identity is `(path, content_hash, extraction_epoch)`.

Consumer action: to read `test_role`, replace the binary and re-extract or rebuild
standalone artifacts, or let epoch-6 family-store file versions populate on the
next import or update. A consumer that reads only the booleans needs no action.

The same branch adds change-journal coverage for unsupported files. A scan now
writes one `files` row per path the discovery walk reached and dropped for an
unsupported extension, with `status = 'unsupported'`, `language = 'unsupported'`,
a content hash, a byte count, and a null `line_count`. Those paths are read once
for the hash and are never parsed, so they add no symbol or other fact rows. The
`files` table therefore gains rows on the compat fixture, and
`revision_file_changes` (already excluded from the gate) gains `unsupported` and
`deleted` entries for them. Ignored paths, hard-excluded paths, oversized source
files, and the artifact's own `-wal`/`-shm`/`-journal` companions stay out.

A store view built from such an artifact carries one manifest entry per
unsupported path, with `status = 'failed'` and `error_class = 'unsupported'` and
no file version — the existing from-artifact mapping, now reachable.

Consumer action: a reader that lists `files` must filter on
`status = 'indexed'` if it wants only parsed files. A reader that already
filters by status, or that reads the change journal, needs no action.

## 2.35.1

classification: compatible

The v2.35.1 release makes QML a first-class extraction family. QML source now
publishes normalized imports, type facts, object-instantiation relationships,
Qt Quick Test roles, and source evidence. `qmldir` files publish module,
component, import, plugin, typeinfo, and related manifest facts. `.qmltypes`
files publish tooling module, type, member, revision, and export evidence.
The existing SQLite and JSONL tables remain schema-compatible; the new rows and
the QML capability snapshot are the declared output change.

Extraction identity epoch is 5. A reader built for v2.35.0 still reads schema
7 / JSONL v5 / store schema 2. Family-store file versions re-extract because
identity is `(path, content_hash, extraction_epoch)`.

Consumer action: replace the binary and re-extract or rebuild standalone
artifacts, or let epoch-5 family-store file versions populate on the next
import or update.

## 2.34.4

classification: compatible

The v2.34.4 release expands `is_test` facts across the supported language
extractors. Test-role closure records 21 supported capability cells and 7
source-backed `not_applicable` entries across C, C++, Rust, Zig, HTML, SQL,
Markdown, JSON, TOML, YAML, and XML. Additional framework, annotation,
naming, and test-lifecycle evidence is emitted where the grammar and language
conventions support it; existing test-role facts remain schema-compatible.
The changed facts are otherwise within the existing schema 7 extraction
tables.

Extraction identity epoch is 4. A reader built for v2.34.3 still reads schema
7 / JSONL v5 / store schema 2. Family-store file versions re-extract because
identity is `(path, content_hash, extraction_epoch)`.

Consumer action: replace the binary and re-extract or rebuild standalone
artifacts, or let epoch-4 family-store file versions populate on the next
import or update.

## 2.34.3

classification: compatible

The v2.34.3 release narrows `is_test` facts. Python decorator evidence is
limited to `pytest.mark.*` and exact `unittest.skip`, `unittest.skipIf`,
`unittest.skipUnless`, and `unittest.expectedFailure`; `pytest.fixture` and
`unittest.mock.*` are not test evidence. Bare `test_` names still require
test-path evidence. Scala and Elixir no longer treat a test path alone as
callable test evidence; their test-name conventions and supported annotations
remain active. The changed facts are otherwise within the existing schema 7
extraction tables.

Extraction identity epoch is 3. A reader built for v2.34.2 still reads schema
7 / JSONL v5 / store schema 2. Family-store file versions re-extract because
identity is `(path, content_hash, extraction_epoch)`.

Consumer action: replace the binary and re-extract or rebuild standalone
artifacts, or let epoch-3 family-store file versions populate on the next
import or update.

## 2.34.2

classification: compatible

QML, GDScript, Bash, and Scala test-role flags are now emitted, and R
`test_lifecycle` is recorded as `not_applicable`. The `language_capabilities`
snapshot JSON changes for those languages. Fact tables on the compat fixture
are otherwise unchanged.

Extraction identity epoch is 2. A reader built for v2.34.1 still reads schema
7 / JSONL v5 / store schema 2. Family-store file versions re-extract because
identity is `(path, content_hash, extraction_epoch)`.

Consumer action: replace the binary. Rebuild standalone artifacts or let the
next extract rewrite them. Family stores write new epoch-2 file versions on
the next import or update.

## NEXT (unreleased)

classification: incompatible

The resolution write path is retired. Schema v7 removes `identifier_resolutions`
and `pending_resolutions`. JSONL v5 drops the overlay keys. `store resolve` is
gone. Family stores stay schema v2 and drop leftover resolution objects on
writer open.

The compat dump excludes the two overlay tables and `language_capability_gaps`
so fact-table identity remains the gate against v2.33.7. Their absence is this
classified break, not an undeclared table drop.

Consumer action: rebuild standalone artifacts. Family stores migrate in place.
code-kb must use query-time resolution before pinning this binary.

See [2026-08-18-resolution-write-path-retirement.md](../decisions/2026-08-18-resolution-write-path-retirement.md).

## 2.30.0

classification: incompatible

Two independent output changes ship in this release. The section-level classification above is the
stronger of the two — the schema v6 identifiers shape. The `metadata_json` canonicalization is
compatible on its own; it is folded into this entry because the gate reports one verdict per
version, and a reader that survives the key reordering still cannot read a v6 artifact.

**1. Canonical `metadata_json` serialization — compatible on its own.**

Every `metadata_json` value is now serialized through `serde_json::Value` at the CLI's single
serialization chokepoint, so keys are emitted in sorted order rather than in extractor insertion
order. The key SET is unchanged and every value is unchanged, so any JSON reader is unaffected.
Only a consumer byte-comparing the stored strings sees a difference, and it sees it once: rows
whose metadata carried 2+ keys in non-canonical order are rewritten in canonical order on the first
scan with this binary, after which the output is already canonical.

Tables affected: every metadata-carrying table. Measured across two scan processes on the
determinism gate's fixture before the fix, 90 of 210 metadata-carrying rows differed — `symbols` 73
of 192 and `structural_facts` 17 of 18 — with zero rows present in only one artifact. Against the
previous release binary on the compat fixture, the difference is confined to `symbols.metadata_json`
key ordering.

Consumer action: none, unless the consumer persists or diffs raw `metadata_json` bytes. Such a
consumer must expect one rewrite of the affected rows and should compare parsed objects rather than
text.

**2. Schema v6 — `identifiers` loses `target_symbol_id` — incompatible.**

The `identifiers` table drops the denormalized `target_symbol_id` column, its
`FOREIGN KEY (target_symbol_id) REFERENCES symbols(symbol_id) ON DELETE SET NULL`, and the
`idx_identifiers_target` index. `identifier_resolutions` becomes the sole source of identifier
resolution outcomes; the resolution store's lockstep writes into the column are deleted.

The gate reports this as the `identifiers` dump's `#columns` header losing a column, which this
entry declares. The dropped `idx_identifiers_target` index is declared here on the strength of the
DDL alone: the gate's enumeration compares tables only, so index drops are not independently
visible to it (see the blind-spot note under "What the gate compares").

Consumer action: **rebuild via a full rescan.** A v6 binary refuses a v5 artifact with exit code 3
and `schema_migration_required`; no migration engine exists. Any consumer SQL selecting
`identifiers.target_symbol_id` must read `identifier_resolutions.target_symbol_id` through a join
instead. This is why the classification is `incompatible`: a reader built for 2.29.0 cannot read a
v6 artifact at all.

Not a difference the gate reports, but worth recording beside the shape change: the JSONL export
contract is **unbumped at 4**. The identifier record keeps its `target_symbol_id` key, now sourced
through a `LEFT JOIN identifier_resolutions`, and is byte-identical to 2.29.0's output.

**Also in this release, and deliberately absent from the diff.** The extraction pass stopped
resolving symbol references across file boundaries (`SymbolLookup` is now per-file). That is a
producer behavior change, but it alters no output on any real corpus — a per-file extractor cannot
mint another file's stable symbol id, and the corpus survey behind the change found 0 cross-file
links over 703k rows. The compat harness re-ran after the narrowing and attributed no extraction-
output difference to it. It is named here so a future reader does not mistake its absence from the
dumps for an omission.
