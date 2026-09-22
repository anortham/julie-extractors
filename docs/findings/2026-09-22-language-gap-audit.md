# Language gap audit, 2026-09-22

An audit of all 40 languages at `ea0bd76a` (v3.3.1). Two agents probed each language unit with a release build: one walked the pinned grammar, one scanned real code from local repositories. The probe scanned each snippet into a fresh artifact and printed every row family.

Verification status per unit:

- `verified`: an adversarial verifier re-ran every probe, merged duplicates, and refuted gaps that did not reproduce or that a recorded contract intends.
- `unverified`: the verifier did not run. Duplicate reports from the two lenses are merged by id only. The fix wave reproduces each gap with a failing test before it changes code, and drops gaps that do not reproduce.

Totals after id merge: 1102 gaps, 347 rated high. Wave 1 closes the high-rated gaps; see [the closure plan](../plans/2026-09-22-language-gap-closure.md). Wave 2 of the same plan takes the medium and low gaps.

The full evidence (snippet, observed rows, expected rows, fix location) was kept in the session scratchpad and handed to the fix agents; this file keeps the inventory.

## bash (unverified)

| Impact | Domain | Gap | Summary |
| --- | --- | --- | --- |
| high | identifiers | `bash.arithmetic-variable-refs-missing` | Variable reads inside $(( )), (( )), for (( )) and array indexes emit no variable_ref identifiers |
| high | test_detection | `bash.bats-extension-unsupported` | .bats test files are not scanned, so every bats test is missing from the artifact |
| high | test_detection | `bash.bats-shellspec-blocks-have-no-extent` | bats @test and ShellSpec Describe/It symbols cover only their header line, so test bodies and nesting are lost |
| high | other | `bash.extensionless-and-bats-files-not-discovered` | Extensionless shebang scripts, .bats files and shell dotfiles are never indexed |
| high | body_spans | `bash.shebang-symbol-spans-whole-file` | The shebang pseudo-symbol spans the whole file and owns every top-level identifier |
| high | test_detection | `bash.test-block-body-and-nesting` | bats @test and ShellSpec It/Describe symbols cover only the header line: no body span, no calls from tests, no container nesting |
| high | types | `bash.type-facts-never-persisted` | Bash type facts are keyed by symbol name, so zero type_facts rows reach SQLite; declared types from declare -i/-a/-A are ignored |
| medium | symbols | `bash.bare-declarations-and-subscript-symbols` | Declarations without a value (declare -A MAP, local result, readonly X) emit no symbol; array element assignments emit symbols named MAP[key] |
| medium | symbols | `bash.declaration-duplicates-and-flags` | declare/local/export emit two conflicting symbols per assignment, ignore flags, and drop bare declarations |
| medium | symbols | `bash.declaration-flags-kind-visibility` | local -r, declare -rx and declare -x get the wrong kind and visibility; export facts miss declare -x and extra names |
| medium | doc_comments | `bash.doc-comment-shebang-and-detached-comments` | Shebang, license header and blank-line-separated section comments become function doc comments |
| medium | symbols | `bash.framework-import-commands` | bats `load` / `bats_load_library` and ShellSpec `Include` are not recognized as imports |
| medium | structural_facts | `bash.http-client-facts-missing` | curl/wget requests with a static URL emit no http.client_request.v1 fact; curl header strings become url literals |
| medium | symbols | `bash.positional-parameter-forms` | Positional parameters miss ${1:-default}/${2}, treat $0 as a parameter, and nested-function parameters go to the outer function |
| medium | symbols | `bash.prefix-env-assignments-become-constants` | Per-command environment prefixes (IFS= read, GOOS=linux go build) are emitted as Constant symbols |
| medium | doc_comments | `bash.shebang-and-header-doc-bleed` | The shebang and blank-line-separated file headers become doc comments; comments inside bodies become doc_comment regions of later symbols |
| medium | symbols | `bash.shebang-whole-file-variable-symbol` | The shebang becomes a Variable named after the interpreter that spans the whole file and owns all top-level identifiers |
| medium | symbols | `bash.source-dynamic-path-garbage-import` | source/. with command substitution or process substitution produces garbage import names |
| medium | test_detection | `bash.test-framework-conventions-missed` | shunit2/bashunit test names, bats/shunit2/ShellSpec lifecycle hooks and *_test.sh files are not recognized |
| medium | test_detection | `bash.test-framework-vocabulary-gaps` | shunit2 testFoo and oneTimeSetUp, bats setup_file/teardown_file, and ShellSpec fIt/xIt/fDescribe and hooks are not classified |
| medium | types | `bash.type-facts-never-bind` | Bash type facts are keyed by name, so no row reaches the artifact while capabilities.types is true |
| medium | relationships | `bash.wrapped-command-and-trap-callees-lost` | Functions run through trap, sudo, exec, time, timeout, nohup, xargs, env or command get no call edge or identifier |
| medium | identifiers | `bash.wrapper-commands-hide-callee` | Callees behind trap, command, sudo, exec, bats `run` and ShellSpec `When call` get no identifier or edge |
| low | pending_relationships | `bash.builtin-and-dynamic-pending-garbage` | Pending calls emitted for shell builtins and for dynamic command names |
| low | pending_relationships | `bash.builtin-and-dynamic-pending-noise` | Pending calls are emitted for shell builtins (trap, shift, getopts, let, time, wait) and for dynamic command names |
| low | symbols | `bash.declaration-duplicate-self-parent` | Each assignment in declaration_command is emitted twice; the duplicate is its own parent and has a different kind |
| low | symbols | `bash.env-prefix-and-array-element-garbage-symbols` | Command-prefix env assignments and array element writes become variable/constant symbols |
| low | literals | `bash.heredoc-sql-literals-missed` | SQL sent to psql or mysql through a heredoc or herestring is not captured; the connection-string argument becomes a sql literal "{}" |
| low | literals | `bash.heredoc-sql-not-captured-as-literal` | SQL passed to psql/mysql/sqlite3 through a heredoc or herestring gives no literal |
| low | body_spans | `bash.non-function-body-spans-garbage` | Variables and imports get body spans from the first bracket in their text, such as {1:-world} |

## c (verified)

| Impact | Domain | Gap | Summary |
| --- | --- | --- | --- |
| high | test_detection | `c.criterion-test-body-detached` | Criterion Test symbol does not contain its body, so calls in the test have no test caller |
| high | symbols | `c.function-pointer-declarator-names` | Function-pointer variables become function symbols named "(*name)"; a function returning a function pointer is named "(*f(void))" |
| high | symbols | `c.pointer-return-functions-missing` | Prototypes that return a pointer, and definitions that return `**`, produce no function symbol |
| high | symbols | `c.pointer-variables-missing` | Pointer variables without an initializer are dropped; multi-declarator signatures and types are wrong |
| high | symbols | `c.struct-reference-symbols` | Every struct/union/enum type reference becomes a struct/union/enum symbol row |
| high | symbols | `c.typedef-name-and-kind` | Typedef name is the last identifier in the text: named-parameter function-pointer typedefs get a parameter's name, multi-name typedefs lose names, signature ... |
| medium | symbols | `c.anonymous-member-fields` | Members of anonymous unions/structs, and fields inside an anonymous struct-typed field, are not emitted |
| medium | doc_comments | `c.doxygen-doc-comment-forms` | Trailing /**< member docs attach to the next member; /*! and //! Doxygen docs are ignored |
| medium | identifiers | `c.member-call-identifier-names` | Calls through struct fields use the raw expression text as the call identifier name |
| medium | body_spans | `c.prototype-body-spans` | Prototypes, typedefs, function-pointer fields and variables get garbage body spans and colliding body hashes from the text fallback |
| medium | types | `c.return-type-enum-union-and-facts` | enum/union return types are reported as void; static, multi-word and struct return types get no type fact |
| medium | symbols | `c.typedef-enum-kind-and-members` | Typedef kind comes from a subtree search: typedef enum is kind type with orphan enumerators, a struct that contains a union is kind union |
| medium | test_detection | `c.unity-cmocka-lifecycle` | Unity setUp/tearDown and CMocka registered tests and hooks get no test roles |
| low | symbols | `c.expression-statement-false-struct` | Bare identifier statements become struct symbols when any sibling contains "typedef" |
| low | pending_relationships | `c.keyword-pseudo-calls` | static_assert and the _Generic default emit fake calls/uses; a top-level call right after #include is owned by the import symbol |
| low | annotations | `c.struct-variable-attributes` | Attributes on structs, fields and variables emit no annotations, and attribute arguments become fake call rows |

## cpp (verified)

| Impact | Domain | Gap | Summary |
| --- | --- | --- | --- |
| high | symbols | `cpp.alias-declaration-missing` | 'using X = Y;' aliases and alias templates emit no symbol and emit a pending use of their own name |
| high | symbols | `cpp.block-scope-direct-init-as-function` | Direct-initialized locals with identifier arguments become local function symbols |
| high | test_detection | `cpp.catch2-doctest-test-body-detached` | Catch2/doctest test symbols cover only the macro call, so calls and locals in the test body attach to nothing |
| high | symbols | `cpp.export-macro-classes-lost` | Class visibility macros other than *_EXPORT flatten the class into top-level functions and variables |
| high | relationships | `cpp.inheritance-extends-edges` | Base classes in other files, qualified bases, and template bases produce 'uses' instead of 'extends' |
| high | symbols | `cpp.out-of-line-member-definitions` | Out-of-line member definitions are qualified-name 'function' rows with no class link, wrong visibility, and no row for static data members or conversion oper... |
| high | symbols | `cpp.qualified-definition-heads` | Nested namespace definitions, qualified class definitions, and struct specializations emit no container row and orphan their members |
| high | symbols | `cpp.qualified-return-declaration-misnamed` | Function and method prototypes with a namespace-qualified return type are named after the return type |
| high | symbols | `cpp.wrapped-declarators` | Pointer, reference, array, and function-pointer variables and fields are dropped or named after their initializer |
| medium | body_spans | `cpp.declaration-body-spans` | Bodiless declarations get the parameter list as body span and share one body hash |
| medium | identifiers | `cpp.destructor-operator-struct-containment` | Calls in in-class destructors produce no call edges, and identifiers in operator, destructor, and struct bodies attach to the enclosing class or namespace |
| medium | doc_comments | `cpp.doxygen-doc-comment-forms` | Doxygen '//!' and '/*!' docs are ignored, '////' banners count as docs, and trailing '///<' docs attach to the next symbol |
| medium | symbols | `cpp.member-function-forms` | Inline conversion operators are missing, member templates are kind function, and friend declarations emit a self-parented duplicate |
| medium | symbols | `cpp.preprocessor-include-and-define-rows` | #include and #define produce no rows in C++ although the C extractor emits import and macro rows |
| medium | types | `cpp.return-type-facts` | No return type facts for pointer/reference returns or qualified definitions; trailing returns record 'auto' |
| medium | relationships | `cpp.template-params-and-qualified-type-refs` | Template type parameters are treated as real types, and qualified type references drop their namespace |
| medium | test_detection | `cpp.test-framework-roles` | Boost.Test cases, doctest suites/subcases/fixtures, and QtTest slots get no test roles or wrong ones |
| low | annotations | `cpp.attributes-on-declarations` | [[nodiscard]] and [[deprecated]] on declarations and classes produce no annotation rows |
| low | complexity_metrics | `cpp.complexity-cpp-node-kinds` | Complexity ignores range-for loops, catch clauses, and variadic parameters |
| low | symbols | `cpp.internal-linkage-visibility` | Namespace-scope static symbols and anonymous-namespace members are reported public |

Recorded open gaps assessed:

- `(none recorded)`: blocked (small). The cpp row in fixtures/extraction/capabilities.json has an empty capability_gaps list and no open_gaps in any kind_coverage domain, so there is nothing to assess. Both auditors reported no recorded gaps. Note: the le...

## csharp (unverified)

| Impact | Domain | Gap | Summary |
| --- | --- | --- | --- |
| high | relationships | `csharp.base-call-self-recursion` | base.M() in an override resolves to the override itself (false recursion) |
| high | pending_relationships | `csharp.base-list-raw-text-targets` | extends/implements targets use raw base-list text: type args and primary-ctor args break them |
| high | body_spans | `csharp.bodyless-declaration-body-spans` | Bodyless declarations get a parameter or argument list as body span and hash; expression-bodied properties get none or a call's argument list |
| high | symbols | `csharp.default-accessibility` | Default accessibility is wrong for interface members, namespace-level types, and constructors |
| high | symbols | `csharp.default-visibility-rules` | Default visibility is wrong for interface members, top-level types, and instance constructors |
| high | pending_relationships | `csharp.generic-method-call-edges` | Generic method invocations emit no call edge, keep type arguments in the call name, and emit the method name as a type_usage |
| high | relationships | `csharp.inheritance-edge-resolution` | Base-list edges pick the wrong source symbol, bind to properties and methods, and keep generic, qualified, and primary-constructor text in the target name |
| high | relationships | `csharp.null-conditional-and-nameof-call-edges` | ?. calls produce no call edge; nameof(...) produces a fake call |
| high | relationships | `csharp.null-conditional-call-edges` | Null-conditional calls (x?.M()) emit no call relationship or pending row |
| medium | structural_facts | `csharp.aspnet-route-coverage` | [HttpGet][Route("x")] loses the action template, and MapHub, MapControllerRoute, Map, and MapHealthChecks emit no route facts |
| medium | identifiers | `csharp.attribute-usage-identifiers` | Attribute usages emit no type_usage identifier for the attribute class |
| medium | body_spans | `csharp.body-span-text-fallback` | Body spans fall back to text heuristics: bodiless members, locals, properties, and foreach variables get wrong bodies |
| medium | structural_facts | `csharp.efcore-model-facts` | EF Core DbContext entity sets and model configuration emit no framework facts |
| medium | symbols | `csharp.event-declaration-missing` | Events with add/remove accessors (event_declaration) produce no symbol |
| medium | types | `csharp.implicit-lambda-param-self-type` | Implicitly typed parenthesized lambda parameters get their own name as their type |
| medium | structural_facts | `csharp.minimal-api-nested-groups-and-hubs` | Nested MapGroup prefixes do not compose; MapHub, MapControllerRoute, and MapHealthChecks emit no route facts |
| medium | symbols | `csharp.pattern-variable-bindings` | Pattern-matching designations (is T x, case T x) emit no variable symbol or type fact |
| medium | symbols | `csharp.positional-record-properties` | Positional record parameters are private variables, not the public properties the compiler generates |
| medium | crash_or_parse | `csharp.preprocessor-branch-cascade` | #if/#else inside if-else chains, switch sections, and base lists drops and misparents symbols |
| medium | relationships | `csharp.property-accessor-caller-scope` | Calls, instantiations, identifiers, and complexity inside property and indexer accessors are attributed to the enclosing class |
| medium | symbols | `csharp.record-positional-properties` | Positional record parameters are private parameter variables, not public properties |
| medium | relationships | `csharp.target-typed-new-instantiation` | Target-typed new(...) emits no instantiates edge or call identifier |
| medium | pending_relationships | `csharp.top-level-instantiations-dropped` | Object creation in top-level statements emits no instantiates row |
| low | annotations | `csharp.attribute-target-annotations` | Target-specified attributes publish 'return:' as the annotation, and parameter attributes are not captured |
| low | pending_relationships | `csharp.bare-call-enclosing-type` | Bare calls to members of the enclosing type stay ambiguous when another class in the file has the same method name |
| low | symbols | `csharp.extension-block-receiver` | C# 14 extension blocks drop the receiver parameter and do not mark members with the extended type |
| low | test_detection | `csharp.mspec-delegate-field-roles` | Machine.Specifications contexts and It/Establish/Because/Cleanup fields are unclassified |
| low | pending_relationships | `csharp.nameof-emitted-as-call` | nameof(x) is emitted as a call to a method named nameof |
| low | symbols | `csharp.params-parameters-missing` | params parameters produce no parameter symbol or type fact |
| low | test_detection | `csharp.specflow-step-bindings` | SpecFlow/Reqnroll [Binding] classes, step methods, and scenario hooks are unclassified |
| low | literals | `csharp.sql-literal-carriers` | EF Core 7+ raw-SQL APIs and ADO.NET SqlCommand SQL strings are not captured |
| low | test_detection | `csharp.tunit-test-roles` | TUnit data-driven tests and Before/After hooks are unclassified |
| low | identifiers | `csharp.type-position-identifier-kinds` | Types in as-expressions and constraint subjects are variable_ref; new T() emits an instantiates row for a type parameter |
| low | types | `csharp.type-text-fallbacks` | String-based type fallbacks publish garbage type facts and wrong signature types |
| low | symbols | `csharp.unsigned-shift-operator-missing` | operator >>> declarations produce no symbol; their parameters are reparented to the type |
| low | symbols | `csharp.using-alias-target-dropped` | using-alias directives drop the aliased type; global using loses its global marker |

Recorded open gaps assessed:

- `specflow.step_binding_test_roles`: closable now (medium). Partly closable now with the existing role vocabulary: [Binding] becomes test_container, [BeforeScenario]/[BeforeFeature]/[BeforeTestRun] become fixture_setup, and the After* hooks become fixture_teardown. The step me...
- `mspec.delegate_field_test_cases`: closable now (small). The ledger reason is factually outdated. It says roles are written only for callable symbols, but the probe shows the extractor already emits a callable `function` symbol (`should_have_one_item$lambda`) for each deleg...

## css (verified)

| Impact | Domain | Gap | Summary |
| --- | --- | --- | --- |
| high | identifiers | `css.compound-and-escaped-selector-identifiers` | Class and ID identifiers use the whole selector text, so compound and escaped selectors give wrong or missing names |
| high | relationships | `css.custom-property-reference-attribution` | var() and animation-name edges come from a line regex: wrong source rule when rules share a line, and only the last same-name declaration is a target |
| high | symbols | `css.custom-property-value-lost` | Custom property symbols lose their value (empty or first token only) and their span covers only the name |
| high | pending_relationships | `css.import-target-not-extracted` | @import emits no imports edge, no pending row, no fact, and every import symbol is named '@import' |
| medium | symbols | `css.at-rule-names-from-first-token` | At-rule symbol names are cut at the first whitespace token or drop the name they declare |
| medium | relationships | `css.keyframes-animation-shorthand-references` | The animation shorthand never links to @keyframes, and animation names get no identifiers |
| medium | literals | `css.literals-dropped-without-language-policy` | The artifact drops every CSS url() literal because CSS has no literal carrier policy |
| medium | structural_facts | `css.rule-kinds-break-fact-containment` | Rule kind depends on the first selector character, so tag, ID, '&' and at-rule facts lose or get the wrong containing symbol |
| medium | identifiers | `css.tailwind-apply-and-directives-ignored` | Tailwind @apply classes produce no identifiers, and @tailwind/@apply produce no framework facts |
| medium | doc_comments | `css.trailing-comment-becomes-next-doc` | A comment that trails a declaration or rule on the same line becomes the doc comment of the next symbol |
| low | relationships | `css.css-modules-composes-not-linked` | CSS Modules composes emits no reference to the composed class and no import for the 'from' file |
| low | identifiers | `css.pseudo-class-duplicate-identifiers` | Functional pseudo-classes emit duplicate call identifiers, some spanning the whole selector |
| low | symbols | `css.scope-statement-ignored` | @scope gets no symbol or fact, and its inner rules and prelude selectors lose their owner |
| low | symbols | `css.vendor-prefixed-keyframes-missing` | @-webkit-keyframes gets no symbol, and its fact has no animation name |

Recorded open gaps assessed:

- `capability_gaps.pending_relationships (status exception)`: closable now (medium). The exception is factually wrong. It says '@import directives resolve at extraction time and current relationship extraction emits direct edges', but the probe shows no relationship, pending row or fact for any @impor...
- `capability_gaps.types (status exception)`: blocked (small). Substantially right for consumers: CSS has no declared types that feed receiver-typed resolution. The wording 'there is no type metadata to extract' is not fully accurate, because '@property --x { syntax: "<color>" }'...

## dart (unverified)

| Impact | Domain | Gap | Summary |
| --- | --- | --- | --- |
| high | body_spans | `dart.accessor-constructor-span-signature-only` | Constructor, factory, getter, and setter symbols cover only the signature, so their bodies fall outside the symbol |
| high | identifiers | `dart.arrow-closure-calls-lost` | Calls inside arrow closures become variable_ref rows, with no call edge, and some produce junk pending targets |
| high | relationships | `dart.caller-lookup-requires-unique-name` | Calls inside any callable whose name repeats in the file get no relationships and no pending rows |
| high | pending_relationships | `dart.const-new-instantiation-no-call` | `const Foo(...)` and `new Foo(...)` instantiations emit no call identifier, relationship, or pending row |
| high | body_spans | `dart.constructor-accessor-spans-signature-only` | Constructor, factory, getter and setter symbols span only the signature; the body span points at the parameter list |
| high | doc_comments | `dart.doc-comment-bleeds-from-container` | Undocumented members, enum values, and parameters inherit the doc comment of the enclosing class or function |
| high | doc_comments | `dart.doc-comment-leaks-from-container` | Members and parameters inherit the enclosing class, enum or function doc comment |
| high | pending_relationships | `dart.member-call-dropped-on-local-name` | A member call is dropped with no edge and no pending row when its method name matches any symbol in the file |
| high | pending_relationships | `dart.pending-caller-lookup-by-unique-name` | Calls lose their caller in duplicate-named methods, constructors, getters, setters and test closures |
| high | pending_relationships | `dart.same-name-symbol-suppresses-calls` | A same-file symbol with the same name silently drops a receiver call or binds it to the wrong target |
| high | relationships | `dart.test-body-calls-attributed-to-main` | Calls in test() and group() callbacks are attributed to main, and synthetic test symbols become call targets |
| high | symbols | `dart.top-level-and-untyped-fields-missing` | Top-level variables and constants, static const/final fields, untyped fields, and extra declarators get no symbol |
| high | symbols | `dart.top-level-variables-and-untyped-fields-missing` | All top-level variables, untyped fields and static const fields produce no symbol |
| high | types | `dart.type-facts-signature-regex` | Type facts come from a regex over the signature: junk types on classes and accessors, and return types lost for generic, nullable, or annotated returns |
| medium | symbols | `dart.async-static-flags-from-substring` | isAsync and isStatic come from substring matches over the whole node text, and async*/sync* modifiers get no fact |
| medium | test_detection | `dart.bloc-test-not-detected` | blocTest(...) cases from package:bloc_test are not detected as tests |
| medium | identifiers | `dart.cascade-and-generic-call-sites` | Cascade sections emit no identifiers and receiverless pending rows, and generic calls lose their receiver and type arguments |
| medium | identifiers | `dart.const-new-constructor-invocations-not-calls` | const/new constructor invocations are recorded only as type_usage, with no call and no pending row |
| medium | symbols | `dart.deferred-import-and-part-directives-missing` | Deferred imports, part and part-of directives produce no symbol |
| medium | symbols | `dart.extension-type-declarations-missing` | Dart 3 extension types produce no symbol, and their members have no parent |
| medium | symbols | `dart.extension-type-mixin-application-missing` | Extension types and mixin application classes get no symbol, and extension type members have no parent |
| medium | structural_facts | `dart.framework-facts-missing` | No framework structural facts for Dart: HTTP client requests, routes, or DI registrations, and Uri.parse URL literals are dropped |
| medium | types | `dart.garbage-type-facts` | Type facts record keywords ('get', 'set', 'sealed', 'class', 'factory', 'required') as types |
| medium | symbols | `dart.import-directive-gaps` | Deferred imports, part, part of, and library directives are dropped, and import rows carry no alias, show, or hide data |
| medium | relationships | `dart.mixin-enum-inheritance-edges-missing` | Mixin on/implements clauses and enum with/implements clauses produce no inheritance edges |
| medium | structural_facts | `dart.no-http-client-facts` | Dart emits no http.client_request.v1 facts for package:http or dio calls, and the URL literal policy works only by variable-name luck |
| medium | structural_facts | `dart.no-route-facts` | go_router route definitions and navigation calls, and shelf_router routes, produce no facts |
| medium | symbols | `dart.operator-declarations-missing` | Operator overloads (==, +, [], []=) produce no symbol |
| medium | symbols | `dart.operator-redirecting-factory-missing` | Operator declarations and redirecting factory constructors (the freezed pattern) are dropped |
| medium | symbols | `dart.private-visibility-ignored` | Library-private `_` classes, enums, mixins, extensions, and constructors are reported as public |
| medium | test_detection | `dart.test-vocab-gaps` | blocTest cases are not detected, and group() in lib/ code creates a false test container |
| medium | symbols | `dart.underscore-private-types-public` | Library-private classes, enums, mixins and extensions (_Name) have visibility public |
| low | identifiers | `dart.declaration-names-as-references` | Constructor and field declaration names are emitted as variable_ref or type_usage reference rows |
| low | relationships | `dart.enum-mixin-supertypes-no-relationships` | Enum with/implements clauses and mixin on-constraints produce no relationships |
| low | symbols | `dart.pattern-variable-locals-missing` | Dart 3 pattern declarations (records, maps, if-case, for-in) produce no locals or type facts |
| low | symbols | `dart.signature-content-loss` | Constructor signatures drop parameters and the constructor name, getters drop the return type, and old-style typedefs lose their type |
| low | symbols | `dart.text-based-async-static-flags` | isAsync/isStatic and the signature prefix come from substring matches on the whole body |

## elixir (unverified)

| Impact | Domain | Gap | Summary |
| --- | --- | --- | --- |
| high | symbols | `elixir.alias-directives-not-modeled` | alias Foo.{A, B} becomes one garbage import, alias X, as: Y loses Y, and aliased calls are never expanded |
| high | pending_relationships | `elixir.alias-expansion-module-identity` | Pending targets keep the alias short name, multi-alias and `as:` aliases are not modelled, nested modules keep short names |
| high | relationships | `elixir.caller-lookup-drops-call-edges` | Calls inside multi-clause functions, name-colliding functions, ExUnit test/setup bodies, and defmacro bodies emit no relationship and no pending row |
| high | relationships | `elixir.calls-outside-def-dropped` | Calls inside ExUnit test/setup/describe blocks, defmacro/defmacrop and defguard bodies produce no relationship or pending row |
| high | relationships | `elixir.def-head-self-call` | Every function head is recorded as a call to itself (self-loop calls edge plus a Call identifier) |
| high | symbols | `elixir.defstruct-keyword-fields` | Keyword-form defstruct/defexception emits atom values as fields and drops the real field names |
| high | body_spans | `elixir.do-keyword-body-spans` | Body spans for `do:` one-liners, bodyless heads, guards, types, imports and delegates come from a brace/paren heuristic |
| high | doc_comments | `elixir.doc-comment-attachment` | @doc is lost when @spec/@impl sits between it and def; heredoc @moduledoc is truncated; an outer module takes the inner module's moduledoc; @typedoc is ignored |
| high | doc_comments | `elixir.doc-comments-lost` | Heredoc @moduledoc truncated to its first line, nested module docs bleed outward, @doc before @spec/@impl dropped, @typedoc ignored |
| high | relationships | `elixir.duplicate-name-drops-calls` | A function whose name is not unique in the file loses all outgoing calls, and calls to it do not resolve locally |
| high | pending_relationships | `elixir.remote-call-namespace-mismatch` | Pending remote calls split the module into segments, but module symbols carry the full dotted name, so consumers cannot join them; __MODULE__ is not substituted |
| medium | identifiers | `elixir.alias-args-type-usage-suppressed` | Module aliases passed directly as arguments to any local call emit no type_usage identifier |
| medium | test_detection | `elixir.def-test-prefix-false-test-role` | Any `def test_*` in production code is flagged as a test case; property and doctest tests are not detected |
| medium | pending_relationships | `elixir.defdelegate-no-target-edge` | defdelegate records no edge to its to: module/function (including as: renames) |
| medium | relationships | `elixir.defimpl-implements` | defimpl without for: is misnamed with a self-loop implements edge; external protocols get no pending implements; for: lists create garbage names |
| medium | symbols | `elixir.ecto-schema-fields` | Ecto schema/embedded_schema blocks emit no struct or field symbols |
| medium | pending_relationships | `elixir.field-access-as-remote-calls` | Map field access and anonymous-function calls become pending calls; variable/atom receivers go into namespace_path and get no member_access identifier |
| medium | relationships | `elixir.function-captures` | Local function captures `&name/arity` are recorded as variable reads with no call edge |
| medium | relationships | `elixir.local-capture-no-call-edge` | A local function capture &fun/arity is recorded as a variable read, with no call edge to the function |
| medium | symbols | `elixir.module-attribute-constants` | Module attribute constants (@timeout 5_000) get no symbol; the definition is a Call identifier and uses are unqualified variable_refs or fake parameters |
| medium | structural_facts | `elixir.phoenix-live-routes` | Phoenix `live` routes emit no route fact |
| medium | structural_facts | `elixir.phoenix-nested-resources` | Nested Phoenix resources and routes inside a resources do-block are dropped; the scope alias is not recorded for controller joins |
| medium | structural_facts | `elixir.phoenix-router-nesting-and-scope-options` | Nested resource blocks are dropped, `scope path:` prefix is ignored, controller is not joined with the scope alias |
| medium | types | `elixir.spec-return-type-leak` | @spec return types are keyed by bare name: they leak onto same-named variables, the wrong arity, and zero-arity specs record nothing |
| medium | types | `elixir.spec-type-facts-unscoped` | @spec return types are keyed by bare function name across the whole file |
| medium | pending_relationships | `elixir.special-forms-and-field-access-as-calls` | Special forms, map field access, and anonymous-function invocation are emitted as pending calls |
| medium | pending_relationships | `elixir.special-forms-as-calls` | Control-flow special forms and module-attribute names are emitted as calls |
| medium | identifiers | `elixir.stab-arrow-bogus-receiver` | Calls right after an Elixir `->` clause arrow get the clause pattern as their receiver |
| medium | identifiers | `elixir.stab-arrow-false-receiver` | Calls after a stab-clause `->` get a false receiver from the pattern token |
| medium | test_detection | `elixir.test-roles-precision` | `def test_*` in lib code is marked test_case; `property` tests and ExUnit case modules get no role |
| low | complexity_metrics | `elixir.function-head-arity-and-defaults` | Parameter count ignores pattern and default parameters; default values are bound as parameters |
| low | structural_facts | `elixir.http-client-alias-resolution` | HTTP client matching uses literal module names, so `alias ..., as:` gives false and missed facts |
| low | source_regions | `elixir.sigil-and-doc-regions` | Sigils (~H HEEx, ~r regex, ~p paths) produce no region; @doc heredocs are string_literal, not doc_comment |
| low | structural_facts | `elixir.tesla-module-client` | Module-style Tesla clients (`use Tesla` + bare verbs) emit no client fact; BaseUrl never joined |
| low | symbols | `elixir.typespec-names-and-remote-generics` | Parameterized @type names include their parameters, @macrocallback is ignored, and remote generic types record no type arguments |
| low | symbols | `elixir.typespec-symbol-names` | Parameterized @type names include their parameter list; @macrocallback is not extracted |

Recorded open gaps assessed:

- `phoenix.non_route_macros`: closable now (small). The reason is wrong for `live`. `live "/users", UserLive.Index, :index` has the same shape as `get` (path literal, module, action atom) and serves an HTTP GET, so the existing route collector can emit it now. `socket`...
- `phoenix.cross_file_scope_prefix`: blocked (large). A prefix from another router or forward is a cross-file join, which belongs to code-kb (decision 0004, plan section 2c). A related in-file defect exists and is reported separately: `scope path: "/beta"` keyword prefix...
- `elixir.http_client.tesla_middleware_base_url`: closable now (medium). Reproduced. The reason understates the gap: module-style `use Tesla` clients with bare `get("/path")` calls emit nothing, not just a path. Both BaseUrl forms are in the same file: a `plug Tesla.Middleware.BaseUrl, "li...
- `elixir.http_client.alias_resolution`: closable now (small). Reproduced. `alias Req, as: R` makes a real call silent, and `alias MyApp.Fake, as: Req` gives a false fact. An in-file alias map from the `alias` directives (with `as:` and multi-alias) fixes it. The same map closes ...

## erlang (unverified)

| Impact | Domain | Gap | Summary |
| --- | --- | --- | --- |
| high | body_spans | `erlang.clause-run-broken-by-comment` | A comment between function clauses cuts the function symbol to its first clause |
| high | doc_comments | `erlang.comment-noise-becomes-doc` | Section banners, separator lines and license headers become doc comments, even across blank lines, and a license header hides -moduledoc |
| high | test_detection | `erlang.ct-export-all-false-test-cases` | In a Common Test suite with -compile(export_all), every arity-1 helper becomes a test_case |
| high | test_detection | `erlang.ct-false-test-cases` | Common Test marks group/1 and every arity-1 helper under export_all as test cases |
| high | doc_comments | `erlang.doc-comment-placement` | The EDoc block above -spec is lost, parenthesised -doc is ignored, and a license header beats -moduledoc |
| high | doc_comments | `erlang.edoc-above-spec-doc-missing` | An EDoc %% @doc block above a -spec (the rebar3 template layout) gives the function no doc_comment |
| high | identifiers | `erlang.macro-call-sites` | Macros with lowercase names (?assertEqual, ?assert, ?config, ?is_user) emit no identifiers, and a macro call site has no edge to its -define |
| high | relationships | `erlang.module-macro-remote-call` | ?MODULE:f(...) calls emit no call edge at all |
| high | relationships | `erlang.module-macro-self-calls-dropped` | ?MODULE:f(...) calls and fun ?MODULE:f/N references make no relationship and no pending row |
| high | relationships | `erlang.recovery-loses-edges-and-misspans` | After a macro parse failure, recovered functions get symbols back but lose their calls, pending rows and identifiers, and the damaged function's span covers ... |
| high | identifiers | `erlang.remote-call-identifiers-lack-module` | Remote-call and record-field identifiers carry no module or record qualifier, so maps:get and kv_store_server:get give identical rows |
| medium | structural_facts | `erlang.cowboy-routes-no-facts` | Cowboy router dispatch tables make no route structural facts |
| medium | doc_comments | `erlang.doc-macro-and-paren-attribute-ignored` | ?DOC("...") and ?MODULEDOC("...") macros (telemetry style) and parenthesized -doc("...") and -moduledoc("...") give no doc |
| medium | pending_relationships | `erlang.fun-references-no-edges` | fun f/N and fun M:f/N references make no relationship or pending row, which breaks call graphs and EUnit test impact |
| medium | literals | `erlang.literals-dropped` | The artifact drops every Erlang literal: there is no carrier policy, and binary strings are never captured |
| medium | relationships | `erlang.mfa-calls` | Calls in MFA form (spawn/apply with literal module, function and args) name no target |
| medium | relationships | `erlang.mfa-dynamic-calls-no-edges` | MFA-tuple calls (spawn/3, apply/3, rpc:call/4, timer:apply_after/4, supervisor child specs) make no edge to the named function |
| medium | literals | `erlang.no-literal-carrier-policy` | Erlang has no literal-carrier policy, so no literal row reaches the artifact. Binary <<"...">> arguments are never captured, and there is no http.client_requ... |
| medium | symbols | `erlang.nominal-and-native-records` | OTP 28/29 forms the grammar parses are ignored: -nominal types, qualified records, -export_record and -import_record |
| medium | pending_relationships | `erlang.remote-call-arity` | Cross-file call rows drop the arity and the call identifier drops the module |
| medium | symbols | `erlang.shared-declarations-marked-private` | Records, types and macros in .hrl headers, and OTP 29 -export_record records, are always private |
| medium | other | `erlang.term-config-files-unsupported` | .app.src, rebar.config and sys.config term files are not extracted, although the pinned grammar parses them cleanly |
| medium | symbols | `erlang.test-conditional-blocks` | -ifdef(TEST) blocks count as unconditional: export_all makes private functions public, and the eunit include makes the production module a test container |
| medium | test_detection | `erlang.test-roles-eunit-fixtures-and-proper` | EUnit fixture setup, cleanup and instantiator functions, and PropEr prop_* properties, get no test roles |
| medium | identifiers | `erlang.type-position-identifiers` | -spec type references have no containing symbol, record field types and defaults are not walked, and remote types lose the module |
| medium | identifiers | `erlang.type-usages-missing-in-records-and-specs` | Record field type annotations and OTP 29 qualified records emit no type_usage identifiers, and -spec type usages have no containing symbol |
| medium | structural_facts | `erlang.web-framework-facts` | No Cowboy route facts and no http.client_request.v1 facts for httpc, hackney or gun |
| low | source_regions | `erlang.body-comments-classified-as-doc` | %% comments inside function bodies become doc_comment regions owned by the next documented function, so TODO markers point to the wrong symbol |
| low | types | `erlang.declared-type-facts` | Declared record field types and -spec parameter types produce no type facts |
| low | other | `erlang.escript-extension` | .escript files are unsupported, although the grammar parses them cleanly |
| low | symbols | `erlang.nominal-type-missing` | OTP 28 -nominal type declarations produce no type symbol |
| low | test_detection | `erlang.proper-and-eunit-fixture-roles` | PropEr properties get no test role, and EUnit setup/cleanup funs get no lifecycle role |
| low | type_argument_usages | `erlang.type-argument-usages` | Parameterized type applications emit no type_argument_usages |

## fsharp (unverified)

| Impact | Domain | Gap | Summary |
| --- | --- | --- | --- |
| high | symbols | `fsharp.and-mutual-declarations-dropped` | Second and later declarations in `type ... and ...` and `let rec ... and ...` groups are dropped and their members misparented |
| high | symbols | `fsharp.and-recursive-declarations-dropped` | Types and functions after `and` are dropped and their members, cases and calls move to the first declaration |
| high | identifiers | `fsharp.arrow-receiver-metadata` | Calls after `->` in match arms and lambdas get a bogus receiver taken from the pattern or lambda parameter |
| high | relationships | `fsharp.caller-attribution-skips-values-properties` | Calls, imports and identifiers in properties, module-level values and nested modules are attributed to the namespace or module |
| high | symbols | `fsharp.exceptions-anonymous-union-fields` | `exception` declarations emit no symbol; anonymous union/exception fields become fields named after their type and draw bogus `uses` edges |
| high | relationships | `fsharp.generic-method-calls-lost` | Generic method and constructor calls (GetService<T>(), Dictionary<K,V>()) emit no call rows and mis-kind the method as a type |
| high | symbols | `fsharp.let-and-type-visibility-ignored` | `private`/`internal` on let bindings and type definitions is ignored; class let bindings are marked public |
| high | identifiers | `fsharp.let-parameter-type-usages-missing` | Type annotations on let-function parameters emit no type_usage identifiers |
| high | identifiers | `fsharp.match-expression-identifiers-dropped` | Every variable_ref inside a match expression (scrutinee and arm bodies) and union-case pattern references are dropped |
| high | body_spans | `fsharp.member-and-class-body-spans-wrong` | Method, property and class body spans cover the parameter list or constructor parens instead of the body |
| high | body_spans | `fsharp.method-property-body-spans` | Method, property, module and type body spans come from brace/paren text heuristics; body hashes miss real edits |
| high | test_detection | `fsharp.nunit-mstest-expecto-test-cases` | NUnit, MSTest and Expecto tests get no test_case role (only xUnit Fact/Theory is detected) |
| high | relationships | `fsharp.pipeline-infix-calls-dropped` | Calls in pipelines and to the right of any infix operator are lost (no call edge, no pending row, no call identifier) |
| high | relationships | `fsharp.pipeline-infix-calls-lost` | Calls through \|>, <\| and infix operators get no call edge, pending row, or call identifier |
| high | symbols | `fsharp.unnamed-union-fields-garbage-symbols` | Unnamed union-case and exception fields become field symbols named after their type, with garbage uses edges |
| medium | symbols | `fsharp.active-patterns-operators-missing` | Active patterns and custom operators (let and static member) emit no symbols; their bodies are attributed to the module |
| medium | structural_facts | `fsharp.aspnet-giraffe-route-facts` | F# web routes (Giraffe, ASP.NET Core minimal API, attribute routes) emit no framework structural facts |
| medium | annotations | `fsharp.attribute-fact-and-annotation-attachment` | Attribute facts on members of an attributed type attach to the type; inline `let [<Attr>] x` attributes are lost |
| medium | pending_relationships | `fsharp.call-target-garbage` | Chained calls get namespace paths like ["s","Trim()"], and index syntax `xs[0]` and `struct (1, 2)` become false calls |
| medium | symbols | `fsharp.class-member-forms-missing` | Constructors, auto-properties, val fields, abstract properties and operator members are missing or garbage |
| medium | identifiers | `fsharp.declaration-names-emitted-as-variable-ref` | Abstract member names, their parameter names, lambda parameters and `use` bindings are emitted as variable_ref usages |
| medium | structural_facts | `fsharp.framework-route-facts-missing` | No route or HTTP facts for ASP.NET Core attribute routes, minimal APIs or Giraffe in F# |
| medium | pending_relationships | `fsharp.generic-application-calls-dropped` | Generic constructor and function calls with explicit type arguments emit no call, no pending row and no type-argument usage |
| medium | identifiers | `fsharp.identifier-context-match-declarations` | Identifiers inside match expressions are dropped, union-case pattern refs are never emitted, and declarations or chain segments are mis-kinded |
| medium | symbols | `fsharp.interface-enum-extension-type-forms` | Abstract-only and [<Interface>] types are classes, enum cases are missing, and type-extension members have no owner |
| medium | symbols | `fsharp.let-type-access-modifiers-ignored` | `let private`, `let internal`, `type private`, `val internal` and class-level `let` bindings report public visibility |
| medium | symbols | `fsharp.local-binding-and-property-containment` | Sequential local lets nest under each other, locals are marked public, and calls in computed properties are attributed to the type |
| medium | symbols | `fsharp.local-let-parent-chain` | Each local `let` is parented to the previous local `let` instead of the enclosing function |
| medium | symbols | `fsharp.member-val-constructors-primary-params` | `member val` auto-properties, `new(...)` constructors and primary-constructor parameters emit no symbols or type facts |
| medium | types | `fsharp.method-return-type-facts-leak` | Method return-type annotations are not recorded and leak onto parameters as wrong declared types |
| medium | symbols | `fsharp.module-declaration-forms-missing` | Exceptions, active patterns, operator definitions, measure types, externs and destructured lets are missing |
| medium | symbols | `fsharp.namespace-rec-named-rec` | `namespace rec X` produces a namespace named `rec` |
| medium | relationships | `fsharp.script-imports-and-directives` | Top-level `open` in .fsx and `#load`/`#r` directives emit no import rows of any kind |
| medium | symbols | `fsharp.symbol-kind-misclassification` | Interfaces and [<Struct>] types kinded class, function-valued lets kinded variable, and type signatures that show only the attribute line |
| medium | test_detection | `fsharp.test-containers-lifecycle` | No test_container or test_lifecycle roles for NUnit, MSTest, xUnit or Expecto |
| medium | test_detection | `fsharp.test-roles-nunit-mstest-missing` | Only xUnit Fact/Theory produce test roles; NUnit Test/TestCase and MSTest TestMethod are ordinary symbols |
| medium | symbols | `fsharp.type-extension-members-orphaned` | Members in `type X with` augmentations are parented to the module or namespace, with no type symbol |
| medium | types | `fsharp.type-facts-missing-common-type-forms` | No type facts for member return types, array/function/tuple types, .fsi val declarations or suffixed numeric literals, and no type-argument rows for postfix ... |
| low | structural_facts | `fsharp.attribute-fact-annotated-class` | Member attribute facts attach to the class when the class itself has an attribute |
| low | structural_facts | `fsharp.domain-native-facts` | Computation expressions, active patterns and quotations emit no structural facts |

Recorded open gaps assessed:

- `imports`: closable now (medium). Reproduced: top-level .fsx `open`, `#load` and `#r` emit nothing. The ledger says the gap waits for a caller identity, but Python, JavaScript, Ruby and C# already solve this with `import` kind symbols, so F# can emit ...
- `fsharp.domain_native_facts`: closable now (medium). Reproduced: no CE, active-pattern or quotation facts. The pinned grammar exposes all three (ce_expression with the builder expression, active_pattern with active_pattern_op_name, literal_expression for <@ @>). Closure...
- `test_container`: closable now (medium). Reproduced on NUnit [<TestFixture>], MSTest [<TestClass>] and an xUnit class with [<Fact>] members. F# symbols already carry normalized annotation keys, and the shared is_dotnet_container_annotation classifier already...
- `test_lifecycle`: closable now (medium). Reproduced on NUnit [<SetUp>]/[<TearDown>] and MSTest [<TestInitialize>]. The shared dotnet_test_lifecycle_direction already maps setup/teardown/onetimesetup/testinitialize/testcleanup keys for C#; F# can reuse it dir...

## gdscript (unverified)

| Impact | Domain | Gap | Summary |
| --- | --- | --- | --- |
| high | identifiers | `gdscript.chained-call-segments-lost` | Flat attribute chains keep only one segment: head calls, later method calls and interior member names are dropped |
| high | relationships | `gdscript.class-name-extends-one-line` | `class_name X extends Y` on one line records no base class, no extends edge and no GutTest container |
| high | symbols | `gdscript.class-name-script-split-into-two-classes` | class_name + extends scripts emit two class symbols and split members between them; single-line form orphans members |
| high | relationships | `gdscript.constructor-initializer-accessor-calls-dropped` | Calls in `_init`, in member initializers and in property set/get bodies emit no relationships or pending rows |
| high | relationships | `gdscript.extends-field-ignored` | extends inside class_name_statement or class_definition is ignored: no base type, no extends edge, missed GutTest containers |
| high | symbols | `gdscript.inner-class-containment` | Inner class symbol is anchored on the `class` keyword, so its members, nested classes, body span and extends are lost |
| high | symbols | `gdscript.inner-class-member-containment` | Inner-class fields and nested classes get the outer parent; functions after an inner class drop out of the script class |
| high | doc_comments | `gdscript.member-doc-comments` | `##` docs are lost on var/const/export/script class/enum members, and a signal absorbs the script doc block |
| high | identifiers | `gdscript.method-chain-calls-dropped` | Chained calls and interior members in attribute chains lose their call/member rows (identifiers and pending) |
| high | pending_relationships | `gdscript.pending-receiver-from-call-text` | The pending receiver comes from splitting the call text at its last '.', so a dot in any argument corrupts it |
| high | pending_relationships | `gdscript.pending-receiver-from-text` | Pending call target_receiver is cut from source text at the last '.', so arguments and lambdas leak into the receiver |
| high | relationships | `gdscript.property-accessor-bodies` | Property set/get bodies are not callers: their calls have no containing symbol, relationship, or pending row |
| high | symbols | `gdscript.script-class-duplicated-and-split` | A `class_name` + `extends` script produces two class symbols, and top-level members are split between them |
| medium | annotations | `gdscript.annotation-attachment` | Script-level @tool and standalone annotations leak onto later fields/consts; function annotations are dropped |
| medium | identifiers | `gdscript.cast-type-usage` | The type operands of `as` and `is` are variable_ref, and `:= expr as T` records no type fact |
| medium | types | `gdscript.cast-type-usage-and-facts` | as/is type operands are variable_ref, and `var x := expr as T` records no type fact |
| medium | body_spans | `gdscript.declaration-spans-keyword-only` | Class, field, and constant symbols span only their keyword token, so bodies and containing-symbol lookup fail |
| medium | symbols | `gdscript.enum-member-extraction` | Anonymous enum members are missing, and names in enum value expressions become fake enum members |
| medium | symbols | `gdscript.enum-members-anonymous-and-dupes` | Anonymous enum members are dropped, value expressions create duplicate enum_member rows, and lowercase members are skipped |
| medium | structural_facts | `gdscript.export-variant-facts` | Only bare `@export` produces export facts; @export_* variants and Godot 3 `export`/`onready` are missed |
| medium | annotations | `gdscript.function-class-annotations` | Annotations on functions and classes (@rpc, @warning_ignore, @abstract, @tool, @icon) are not recorded |
| medium | test_detection | `gdscript.gdunit4-test-roles` | gdUnit4 suites and lifecycle hooks, GUT inner test classes and Godot 3 GUT bases are not detected |
| medium | structural_facts | `gdscript.godot-framework-facts` | No structural facts for signal connections/emits, resource loads (preload/load), node paths, or @rpc |
| medium | symbols | `gdscript.member-visibility` | Field visibility follows @export instead of the `_` convention, and `_`-prefixed consts/signals/enums are public |
| medium | structural_facts | `gdscript.preload-load-imports` | `preload`/`load` resource dependencies emit no import symbol or import structural fact |
| medium | types | `gdscript.qualified-type-annotations` | Qualified type annotations (`Outer.Inner`, `Enemy.Kind`) become member_access, and the return type fact is truncated |
| medium | symbols | `gdscript.script-functions-kind-function` | In extends-only scripts, non-lifecycle funcs are kind `function`, and scripts with no header get no class |
| medium | symbols | `gdscript.script-methods-kinded-function` | In scripts without class_name, only lifecycle-named functions are methods; all other members are kind function |
| medium | structural_facts | `gdscript.signal-connection-facts` | Signal connections and emissions produce no structural facts, so string-named handlers are invisible |
| medium | body_spans | `gdscript.var-const-span-keyword-only` | var/const symbols span only the keyword, so accessor bodies and initializers have no symbol body or containment |
| low | relationships | `gdscript.bare-call-implicit-self` | Bare calls to a method name also defined in an inner class stay pending instead of resolving to the enclosing class |
| low | symbols | `gdscript.field-visibility` | Non-exported fields are always private, ignoring the underscore convention used for functions; locals get private |
| low | symbols | `gdscript.named-lambda-misparented` | A named lambda becomes a class method with no body span |

## go (unverified)

| Impact | Domain | Gap | Summary |
| --- | --- | --- | --- |
| high | relationships | `go.caller-attribution-name-lookup` | Call edges are lost for calls in package-level var initializers (cobra commands, Ginkgo specs) and in any function or method whose name is not unique in the ... |
| high | relationships | `go.chained-call-false-local-edge` | Calls on a call/index result (`x.F().Get()`, `xs[0].Get()`) resolve to any same-named local symbol by terminal name |
| high | relationships | `go.duplicate-name-caller-drops-edges` | Methods or functions whose name is shared with any other symbol in the file lose all outgoing calls, pending calls, and receiver `uses` edges |
| high | symbols | `go.grouped-type-declaration-drops-specs` | Grouped `type ( ... )` declarations emit only the first spec; later types vanish and their fields attach to the first type |
| high | symbols | `go.grouped-type-declarations` | A grouped `type ( ... )` block yields only its first type, and later fields are parented to that first type |
| high | symbols | `go.interface-method-elements` | Interface method elements are not extracted, and the interface symbol span covers only its name |
| high | symbols | `go.interface-method-elements-missing` | Interface method elements (`method_elem`) emit no symbols, and the interface signature leaves them out |
| high | symbols | `go.method-receiver-type-link` | Methods carry no parent and no receiver-type metadata, so nothing links a method to its type |
| high | symbols | `go.multi-result-function-signature-wrong` | Top-level functions with a parenthesized result list show the results as parameters and lose the real parameters |
| high | symbols | `go.phantom-functions-from-strings-and-comments` | Regex recovery pass invents function symbols from `func name(` lines inside raw strings and block comments |
| high | structural_facts | `go.route-receivers-by-declared-type` | gin, echo, and net/http routes on parameters, fields, or groups typed *gin.Engine, *gin.RouterGroup, *echo.Echo, or *http.ServeMux emit no route facts |
| high | structural_facts | `go.typed-router-receivers-emit-no-routes` | gin/echo/net/http routes on typed parameters or struct-field routers emit no route facts |
| high | symbols | `go.versioned-import-package-names` | Import symbols take the last path segment literally: /vN, gopkg.in .vN, and hyphenated paths get unusable names; blank and dot imports are mishandled |
| medium | relationships | `go.calls-outside-func-declarations-dropped` | Calls in package-level var initializers and in func literals assigned to vars emit no calls or pending rows |
| medium | structural_facts | `go.chi-gorilla-fiber-routes-unsupported` | chi, gorilla/mux, and fiber route registrations emit no structural facts |
| medium | structural_facts | `go.chi-gorilla-routes` | The extractor emits no route or mount facts for go-chi/chi or gorilla/mux |
| medium | doc_comments | `go.doc-comment-attachment` | Doc comments: missing on ungrouped var/const, detached comments get merged in, and //go: directives leak into type docs |
| medium | relationships | `go.embedding-and-implements-edges` | Struct and interface embedding and `var _ I = (*T)(nil)` assertions produce no extends, implements, or composition edges, and `_` becomes a symbol |
| medium | relationships | `go.embedding-emits-no-extends` | Struct and interface embedding emit no extends/composition edges (local or pending) |
| medium | complexity_metrics | `go.expression-switch-not-counted-in-complexity` | GO_CONFIG lists `switch_statement`, which the Go grammar does not have, so expression switches add no decisions or nesting |
| medium | relationships | `go.generic-instantiation-calls` | Calls with explicit type arguments (F[T](x)) produce no call edge and are recorded as type usages |
| medium | structural_facts | `go.http-client-method-constants` | Client requests that use http.Method* constants or *http.Client receivers emit no http.client_request facts |
| medium | symbols | `go.import-binding-names` | Import symbols are misnamed for versioned module paths, and blank or dot imports are not marked |
| medium | body_spans | `go.interface-and-type-definition-name-only-span` | Interface and non-struct type definitions span only the name, so they have no body and their inner identifiers have no containing symbol |
| medium | relationships | `go.interface-assertion-no-implements` | Compile-time interface assertions `var _ I = T{}` / `(*T)(nil)` give garbage `_` symbols instead of implements edges |
| medium | pending_relationships | `go.method-receiver-link-lost-cross-file` | A method whose receiver type is declared in another file gets no link to that type (no pending uses, no receiver metadata) |
| medium | structural_facts | `go.module-manifest-unsupported` | go.mod and go.sum are scanned as unsupported (recorded gap, reproduced) |
| medium | test_detection | `go.qualified-ginkgo-calls` | Package-qualified Ginkgo calls (ginkgo.Describe, ginkgo.It) get no symbols or test roles |
| medium | types | `go.range-and-typeswitch-bindings` | Range-clause and type-switch bindings are not emitted as symbols and get no type facts |
| medium | types | `go.return-and-inferred-type-facts` | Return-type facts come from regex over the signature: missing for `(T, error)` results, wrong for func-typed results, pointer not normalized; `u, err := f()`... |
| medium | types | `go.return-type-facts-from-text` | Return-type facts parsed from signature text are wrong for named and function results and keep pointer decorations |
| low | pending_relationships | `go.builtin-and-conversion-call-edges` | Builtin calls and type conversions produce pending `calls` rows, and local conversions produce `calls` edges to type symbols |
| low | relationships | `go.builtin-and-conversion-calls` | Builtins become pending calls and type conversions become calls edges to type symbols |
| low | complexity_metrics | `go.complexity-switch-and-method-params` | Expression switches and their cases count zero decisions, and a method's parameter count is its receiver count |
| low | test_detection | `go.gocheck-suite-registration` | A gocheck suite registered with var _ = Suite(&T{}) is not marked as a test_container (recorded gap, reproduced) |
| low | structural_facts | `go.http-client-method-constant-verbs` | `http.NewRequest(http.MethodPost, "url", ...)` and the context form emit no client-request fact when the verb is an http.Method* constant |
| low | symbols | `go.table-test-anonymous-struct-fields` | Fields of anonymous structs in table-driven tests are emitted as field symbols parented to the test function |
| low | symbols | `go.type-alias-visibility-always-public` | `type_alias` symbols are always public (lowercase aliases included) and the generic alias signature drops its type parameters |

Recorded open gaps assessed:

- `go.module_manifest_language`: closable now (medium). This is pure extractor work with no dependency on code-kb or cross-file joins. It needs: a language row keyed on the exact basenames go.mod and go.sum (qmldir precedent in language.rs, language_policy.rs, and registry...
- `gocheck.suite_registration`: closable now (small). The fix is local to the extractor. In _test.go files, collect the type named in `Suite(&T{})`, `check.Suite(&T{})`, or `Suite(new(T))` inside a `var _ = ...` spec, and pass those names to mark_go_test_containers in te...

## html (verified)

| Impact | Domain | Gap | Summary |
| --- | --- | --- | --- |
| high | identifiers | `html.attribute-expression-call-identifiers` | Event-handler attributes give one call identifier named with the whole expression; Alpine, hx-on, and Stimulus handlers give garbage or nothing |
| high | body_spans | `html.embedded-body-span-not-remapped` | JS and CSS symbols from <script> and <style> keep body spans in embedded-text coordinates |
| high | relationships | `html.inline-script-js-facts-missing` | Inline <script> and <style> blocks give symbols only: no calls, pending rows, identifiers, literals, complexity, or JS facts |
| medium | identifiers | `html.angular-template-bindings` | Angular component templates give no identifiers for event bindings, property bindings, structural directives, or interpolation |
| medium | body_spans | `html.element-body-span-brace-heuristic` | HTML element body spans run from the first '{' to the last '}' (or a parenthesized group), not over the element content |
| medium | identifiers | `html.id-reference-identifiers-missing` | Attributes that reference an element id (href="#x", for, list, aria-*, popovertarget, hx-target, <use href>) give no rows |
| medium | literals | `html.literals-dropped-by-missing-policy` | Every HTML literal is dropped from the artifact because html has no literal carrier policy |
| medium | pending_relationships | `html.pending-caller-previous-element` | Pending imports from adjacent <script src> or <link href> elements name the previous element as caller |
| medium | relationships | `html.relationships-never-reach-artifact` | All HTML relationships target synthetic ids and the writer drops them; anchors, iframes, objects, and form actions give no pending rows |
| medium | pending_relationships | `html.server-template-tags-ignored` | Jinja/Django/Tera template inheritance ({% extends %}, {% include %}, {% block %}) in .html gives no rows |
| medium | symbols | `html.uppercase-markup-ignored` | Uppercase tag and attribute names give no element symbols, no pending imports, and no id/class identifiers; <SCRIPT SRC> is marked inline |
| low | symbols | `html.attribute-value-quote-trim` | Attribute values that end in a quote character lose it |
| low | symbols | `html.babel-and-template-scripts-opaque` | <script type="text/babel"> JSX is not extracted |
| low | doc_comments | `html.comment-doc-misattribution` | Trailing close-tag comments, commented-out markup, and IE conditional comments become doc comments; distant doc_comment regions attach to a later symbol |
| low | symbols | `html.duplicate-doctype-symbol` | Each DOCTYPE gives two symbols and two type facts; the second comes from the anonymous 'doctype' keyword token |
| low | identifiers | `html.form-control-wrong-container` | Identifiers on <button>, <input>, <img>, and other field or variable elements are contained by the enclosing class-kind element |
| low | structural_facts | `html.link-rel-and-embed-assets` | <link> ignores rel: canonical and preconnect URLs become import rows; <link>, iframe, object, and embed get no asset fact |
| low | identifiers | `html.template-syntax-garbage-identifiers` | Server-template syntax inside class and id values becomes garbage member_access identifiers |

## java (verified)

| Impact | Domain | Gap | Summary |
| --- | --- | --- | --- |
| high | relationships | `java.constructor-call-edges` | Constructor calls: 'new Foo<>()' and 'new Foo<T>()' are dropped, this()/super() emit nothing, and qualified creations keep a dotted terminal name |
| high | symbols | `java.default-access-visibility` | Interface members report 'private', and package-private declarations report 'private' |
| high | pending_relationships | `java.initializer-calls-dropped` | Calls in field initializers, static/instance initializer blocks and enum constant arguments produce no relationship |
| high | relationships | `java.interface-extends-edges` | Interface 'extends' clauses emit no relationship, no pending row and no base_types |
| high | relationships | `java.method-reference-calls` | Method references (this::m, Type::m, Type::new) emit no call edge, no pending row and no call identifier |
| high | relationships | `java.supertype-generic-and-qualified` | Generic supertypes keep type arguments in the edge target, and qualified supertypes are dropped |
| medium | identifiers | `java.annotation-usage-identifiers` | Annotation usages emit no type_usage identifier, so annotation types have no references |
| medium | identifiers | `java.arrow-receiver-metadata` | Lambda and switch-rule arrows '->' are read as member access, so identifiers get a false receiver and receiver_qualifier |
| medium | structural_facts | `java.jaxrs-routes` | JAX-RS / Jakarta REST routes (@Path + @GET/@POST) emit no route facts |
| medium | literals | `java.jpa-query-literals` | JPA query strings (EntityManager.createQuery/createNativeQuery and Spring Data @Query) are not captured as query literals |
| medium | symbols | `java.missing-member-symbols` | Interface constants, annotation-type elements and record compact constructors emit no symbol, and compact-constructor calls have no caller |
| medium | types | `java.qualified-type-handling` | Qualified types: signatures show 'Object'/'void', package segments and 'var' become type_usage rows, type arguments are lost, and the regex fallback writes g... |
| medium | relationships | `java.same-named-type-edge-owner` | Supertype edges of a nested type attach to the first type with the same name in the file |
| medium | structural_facts | `java.spring-http-clients-missing` | No http.client_request facts for Spring RestTemplate, WebClient, RestClient or OpenFeign in Java |
| medium | structural_facts | `java.spring-route-scope-bugs` | Spring routes lose the class prefix after a nested class, miss same-line annotations, add a trailing slash, skip FQ annotations, and treat @FeignClient metho... |
| low | test_detection | `java.cucumber-step-roles` | Cucumber-JVM step methods get no role and a glue class without hooks is not a container |
| low | symbols | `java.module-info` | module-info.java extracts nothing |
| low | symbols | `java.pattern-bindings` | Switch type patterns and record patterns create no binding symbols, and record-pattern types are variable_ref reads |
| low | test_detection | `java.test-container-gaps` | @Suite classes, JUnit 5 test interfaces and the classes that implement them are not test containers |
| low | annotations | `java.type-annotations-non-class` | symbol_annotations has no rows for annotations on interfaces, enums, records, annotation types or record components |

Recorded open gaps assessed:

- `junit_platform.suite_container_roles`: closable now (small). Reproduced: an @Suite class stays 000. The container role is local: add 'suite' to is_java_container_annotation (shared with Kotlin, where @Suite is also JUnit Platform). The @SelectClasses links can be pending 'refer...
- `cucumber.step_binding_test_roles`: blocked (medium). Reproduced for step methods and hook-less glue classes. The grammar supports it, but the required closure asks for step annotations 'as their own role', and that role value is not in the test_role vocabulary. The same...

## javascript (verified)

| Impact | Domain | Gap | Summary |
| --- | --- | --- | --- |
| high | relationships | `javascript.bare-call-binds-to-method` | A bare call resolves to a same-named class method with confidence 1.0, although methods are not in lexical scope |
| high | symbols | `javascript.destructured-bindings-and-require-imports` | Destructured require/import bindings are plain variables, and renamed, defaulted, nested and rest bindings and destructured parameters emit no symbols |
| high | doc_comments | `javascript.doc-comments-lost-on-wrapped-declarations` | JSDoc before export statements and member assignments never reaches the declared function, class, variable or method |
| high | symbols | `javascript.duplicate-function-symbols` | Function-valued declarators, object pairs and member assignments emit two function symbols, which blocks same-file call edges |
| high | structural_facts | `javascript.express-mounts-and-router-shapes` | Express facts miss cross-file router mounts, mounts with middleware, destructured Router() routers and newline-led route chains |
| high | pending_relationships | `javascript.member-call-pending-suppressed-by-same-name-local` | A call on an imported namespace is dropped when any symbol in the file has the same terminal name |
| high | pending_relationships | `javascript.pending-scope-misses-function-expressions` | Calls inside function expressions and generator functions emit no pending rows, and IIFE calls go to the outer function |
| high | test_detection | `javascript.test-dsl-false-lifecycle-roles` | Calls to a locally required helper named after/before become fixture_teardown/fixture_setup symbols |
| high | identifiers | `jsx.component-usage-not-extracted` | JSX component elements emit no call identifiers, relationships or pending rows in .jsx and .js files |
| medium | symbols | `javascript.class-expressions-ignored` | Class expressions give no class symbol, no extends edge, and a nested one gives a false extends edge to the outer class |
| medium | symbols | `javascript.export-clause-first-specifier-only` | Export lists keep only the first specifier; star re-exports, export function* and default re-exports emit nothing |
| medium | structural_facts | `javascript.koa-hapi-routes-missing` | Koa (@koa/router) and Hapi server.route registrations emit no route facts |
| medium | symbols | `javascript.member-assigned-functions-mis-modeled` | Prototype, static and module.exports-assigned functions have no owner, whole-body signatures, fake class names and no this-call edges |
| medium | symbols | `javascript.object-literal-pair-noise` | Every key of every object literal becomes a property symbol, including call arguments, JSX attributes, decorator arguments and import attributes |
| medium | types | `javascript.param-and-member-type-facts-missing` | JSDoc @param and field types, new-initialized fields and constructor this.x members give no type facts, and JSDoc types are not normalized |
| medium | relationships | `javascript.test-dsl-symbols-resolve-as-call-targets` | Synthetic test DSL symbols become call targets: calls resolve to describe blocks and hooks call themselves |
| medium | symbols | `javascript.visibility-not-derived-from-exports` | Class visibility ignores CommonJS, export-list and default exports, and function/variable visibility ignores export status |
| low | annotations | `javascript.field-decorator-annotations-missing` | Decorators on class fields produce no annotation rows |
| low | pending_relationships | `javascript.garbage-pending-targets` | Pending calls are emitted for super, require, and the raw text of subscript, parenthesized and curried callees |
| low | relationships | `jsx.wrapped-components-have-no-callable` | Components wrapped in forwardRef/memo/HOC calls get no callable symbol, so their calls have no caller edge |

Recorded open gaps assessed:

- `nextjs.signal_free_pages_router_files`: blocked (large). Reproduced: pages/about.jsx gives no nextjs.file_route.v1 although next.config.js and package.json with a next dependency are in the same scan; pages/blog/[slug].jsx (imports next/link) gets /blog/:slug. It is not clo...
- `nextjs.signal_free_pages_router_files`: blocked (large). Same producer, gate and product decision as javascript; reproduced with pages/about.jsx. Closing it for javascript closes it for jsx with one more fixture, but only after the product decision and the scan-level signal...

## json (unverified)

| Impact | Domain | Gap | Summary |
| --- | --- | --- | --- |
| high | symbols | `json.array-object-elements-flattened` | Object elements of arrays get no container symbol, so their keys flatten into the array's symbol |
| high | body_spans | `json.body-span-text-heuristic` | Body spans come from a text search for braces/parens, so string values get bogus bodies, keys with { break object bodies, and scalar arrays get none |
| high | literals | `json.literals-dropped-by-cli-policy` | The CLI artifact drops every JSON literal row, but the capability row claims literals are supported |
| high | relationships | `json.ref-in-array-or-root-dropped` | A $ref inside an array (allOf/oneOf/anyOf/prefixItems/parameters) or at the document root emits no relationship and no pending row |
| high | relationships | `json.ref-in-array-or-root-emits-no-edge` | A $ref inside an array element (allOf/oneOf/anyOf/parameters) or at the document root emits no relationship and no pending row |
| medium | doc_comments | `json.doc-comments-ignored-for-jsonc-and-schema-description` | JSONC comments above a key and JSON Schema/OpenAPI description or title never become doc comments; module symbols never carry a doc |
| medium | body_spans | `json.heuristic-body-spans-on-values` | Text heuristics produce body spans: scalar strings get fragment bodies, arrays get none or partial bodies |
| medium | body_spans | `json.jsonl-body-span-not-offset` | JSONL body spans are not shifted by the record offset, so every record after line 1 points into line 1 |
| medium | symbols | `json.key-and-string-escapes-not-decoded` | Symbol names, fact keys and doc text keep raw escape sequences; an escaped quote at the end of a key is mangled |
| medium | literals | `json.literals-dropped-from-artifact` | The extractor records config string literals, but the CLI drops every JSON literal because json has no literal-carrier policy |
| medium | structural_facts | `json.manifest-dependencies-and-file-links-missing` | package.json dependencies/scripts and tsconfig extends/references emit no dependency facts and no file-link pending rows |
| medium | relationships | `json.manifest-dependency-and-extends-edges` | package.json/composer.json dependencies and tsconfig extends/references emit no edges, while Cargo.toml dependencies do |
| medium | structural_facts | `json.openapi-operations-no-route-facts` | OpenAPI/Swagger JSON paths and operations emit only generic json.property facts, no route facts |
| medium | pending_relationships | `json.ref-file-level-external-no-pending` | A whole-document external $ref (no fragment, or an empty fragment) emits no pending relationship |
| medium | pending_relationships | `json.ref-file-only-and-anchor-no-edge` | A $ref to a whole file (no '#') or to a $anchor emits no pending row and no relationship |
| medium | relationships | `json.ref-pointer-not-decoded-or-root-anchored` | Local $ref pointers are matched on raw source text and are not anchored at the root, so escaped pointers get no edge and ambiguous pointers get the wrong target |
| medium | relationships | `json.ref-pointer-resolution-wrong-or-missing` | Local $ref pointers resolve to the wrong target or not at all: not anchored at the root, no ~1/~0 unescape, no JSON string decode |
| medium | structural_facts | `json.schema-definitions-no-model-facts` | JSON Schema/OpenAPI definitions ($defs, definitions, components.schemas) emit no definition facts |
| medium | doc_comments | `json.schema-description-not-doc` | The JSON Schema/OpenAPI description, title and summary keywords are not the doc comment of the symbol they describe |
| medium | symbols | `json.string-escapes-not-decoded` | Symbol names, doc comments, fact keys and literal text keep raw JSON escapes |
| low | structural_facts | `json.dotted-path-ambiguous` | Fact path metadata joins keys with '.' without quoting, so keys that contain dots collide with nested paths |
| low | structural_facts | `json.fact-binding-wrong-on-single-line-documents` | In minified JSON and JSONL, top-level property facts bind to an unrelated module on the same line |
| low | structural_facts | `json.json5-unsupported` | JSON5 files are not scanned at all; unquoted keys in .json produce errors and a garbage symbol |
| low | doc_comments | `json.jsonc-comments-not-docs` | JSONC comments on keys are never attached as doc comments |
| low | other | `json.ndjson-and-json-family-extensions-unsupported` | Common JSON-family extensions (.ndjson, .code-workspace, .jsonld, .geojson) are unsupported |
| low | symbols | `json.scalar-values-invisible` | Symbols never get a signature, so number, boolean and null values appear in no symbol row |
| low | symbols | `json.scalar-values-no-signature` | Scalar JSON keys carry no signature, so number/boolean/null values are absent from the symbol row |

Recorded open gaps assessed:

- `json5_unquoted_keys`: closable now (medium). The work stays inside this repo and needs nothing from code-kb and no cross-file joins. There are two closure paths. (1) The cheap path: write a decision doc that marks JSON5 out of scope, then remove the open gap. (2...

## kotlin (unverified)

| Impact | Domain | Gap | Summary |
| --- | --- | --- | --- |
| high | symbols | `kotlin.annotated-declaration-misparse` | Top-level declarations with an argument-bearing annotation vanish or lose their annotations and KDoc |
| high | relationships | `kotlin.chained-receiver-false-call-edges` | Calls on a complex receiver drop the receiver and resolve by bare name to unrelated same-file methods |
| high | pending_relationships | `kotlin.extension-receiver-calls-unjoinable` | Receiver calls to extension functions stay pending with a receiver that cannot match a top-level target, and extensions carry no receiver type |
| high | body_spans | `kotlin.function-body-span-text-heuristic` | Function body spans come from a brace/paren text guess, so expression bodies and annotated methods get garbage bodies |
| high | relationships | `kotlin.generic-base-type-targets` | Supertypes with type arguments or qualifiers keep the raw text as the target name, so extends/implements never join |
| high | relationships | `kotlin.generic-supertype-names` | Generic supertypes keep type arguments in the target name, so extends/implements never resolve |
| high | relationships | `kotlin.infix-and-function-reference-calls` | Infix function calls and `::function` references produce no call identifier, relationship, or pending row |
| high | identifiers | `kotlin.infix-calls-invisible` | Infix function calls emit no call identifier, relationship or pending row |
| high | identifiers | `kotlin.qualified-type-usage-first-segment` | Qualified and nested type references emit only the first segment (ApiResult.Success -> ApiResult, java.io.IOException -> java) |
| high | relationships | `kotlin.same-name-nested-class-inheritance-source` | Inheritance edges for same-named nested classes attach to the first class with that name |
| high | annotations | `kotlin.top-level-annotated-declaration-misparse` | Top-level annotated declarations lose annotations and KDoc, and some vanish (class members become top-level functions) |
| high | relationships | `kotlin.top-level-property-initializer-calls` | Calls inside top-level property initializers, delegates, lambdas, and getters produce no relationships or pending rows |
| medium | identifiers | `kotlin.arrow-false-receiver` | A call after a lambda or `when` arrow gets a false receiver and qualifier |
| medium | relationships | `kotlin.class-delegation-implements-missing` | `: Interface by delegate` supertypes emit no implements relationship |
| medium | symbols | `kotlin.declaration-facts-dropped` | Import aliases and wildcards, expect/actual, extension-property receivers, and function-typed property types are dropped |
| medium | relationships | `kotlin.delegated-supertype-dropped` | Interface delegation (`: Handler by inner`) emits no implements edge or pending row |
| medium | symbols | `kotlin.enum-kind-and-local-declaration-shape` | Enum classes with any modifier or annotation become kind class; local functions become methods; enum-entry methods attach to the enum |
| medium | identifiers | `kotlin.extension-this-receiver-type` | `this.m()` inside an extension function records the enclosing class (or nothing) instead of the extension receiver type |
| medium | literals | `kotlin.jvm-literal-carriers` | Kotlin literal carrier lists omit the JVM SQL/URL APIs that Java classifies |
| medium | test_detection | `kotlin.kotest-behaviorspec-vocabulary` | Kotest `when`/`and` (backticked), Given/Then/And, and several disabled spellings are unclassified |
| medium | test_detection | `kotlin.kotest-spek-vocabulary-gaps` | Capitalized BehaviorSpec/Gherkin steps and Kotest spec-level lifecycle hooks get no test role |
| medium | structural_facts | `kotlin.ktor-pathless-verb-routes` | Ktor verb handlers without a path argument inside route("/x") { } emit no route fact |
| medium | identifiers | `kotlin.literal-keywords-as-variable-refs` | null, true and false are published as variable_ref identifiers |
| medium | relationships | `kotlin.operator-fun-body-attribution` | Calls and identifiers inside `operator fun` bodies are attributed to the enclosing class |
| medium | symbols | `kotlin.plain-constructor-param-as-property` | Primary-constructor parameters without val/var become properties with a fabricated `val` signature |
| medium | relationships | `kotlin.property-accessor-containment` | Calls and references inside member property getters, setters and delegates are attributed to the enclosing class |
| medium | structural_facts | `kotlin.spring-functional-routes` | Spring coRouter/router functional routes emit no route facts |
| medium | literals | `kotlin.sql-url-literal-carriers` | Kotlin literal carrier policy omits the JVM SQL/URL APIs Java has, so JdbcTemplate/Android SQL strings are dropped |
| medium | symbols | `kotlin.visibility-internal-and-locals` | Kotlin `internal` maps to private (or public for constructor properties), and local vals are public |
| medium | symbols | `kotlin.visibility-mapping` | `internal` maps to private, internal/protected-less constructor params and private companions map to public |
| low | symbols | `kotlin.dsl-phantom-symbols-outside-specs` | Test-DSL words outside any spec create phantom function symbols that steal call attribution |
| low | structural_facts | `kotlin.http-client-typed-body-property` | RestTemplate/WebClient receivers declared as typed class-body properties give no client_request fact |
| low | symbols | `kotlin.import-alias-and-wildcard` | Import aliases (`as`) and wildcards (`.*`) are dropped from import symbols |
| low | identifiers | `kotlin.keyword-and-class-literal-identifier-noise` | null/true/false emit variable_ref rows and `Foo::class` emits a variable_ref plus a member_access named `class` |
| low | test_detection | `kotlin.kotest-withdata-roles` | Kotest withData blocks, which generate one test per row, get no test role |
| low | literals | `kotlin.spring-data-query-strings` | Spring Data @Query JPQL/SQL strings are not captured |

Recorded open gaps assessed:

- `kotlin.functional_routing`: closable now (medium). Reproduced: coRouter/router routes produce no facts (gaps/corouter.kt). The recorded reason says 'nest()/path() nesting is not single-assignment same-file'. That reasoning is weak. `"/api".nest { GET(...) }` inside `c...
- `kotlin.http_client.instance_receiver`: closable now (small). Partly stale. Ktor class properties initialized with HttpClient(...) and primary-constructor RestTemplate/WebClient properties already emit facts (probed). What stays silent is a typed class-body property (`@Autowired...
- `kotest.data_driven_test_roles`: closable now (small). The gap targets the wrong construct. `io.kotest.data.forAll(row(...))` runs table assertions inside an existing test(...) step, and Kotest reports it as that one test, so the enclosing case already carries the correct...
- `kotest.property_test_roles`: closable now (small). This only needs the contract decision the entry defers. checkAll/forAll over generators can only run inside a suspend test scope, and Kotest reports the iterations as the enclosing test. The block is therefore a case ...

## lua (unverified)

| Impact | Domain | Gap | Summary |
| --- | --- | --- | --- |
| high | relationships | `lua.anonymous-function-calls-dropped` | Calls inside function-valued assignments, table-field functions, and busted test blocks emit no calls or pending rows |
| high | relationships | `lua.calls-in-function-values-and-callbacks-dropped` | Calls inside anonymous function bodies (function values, table-field functions, busted it/describe callbacks, lazy.nvim config) produce no relationship or pe... |
| high | symbols | `lua.dot-assignment-parent-by-first-name` | `self.x = v` and `obj.p = v` fields attach to the first symbol with that name anywhere in the file |
| high | relationships | `lua.duplicate-name-caller-drops-calls` | A function or method whose name appears twice in a file emits no call edges at all |
| high | relationships | `lua.enclosing-caller-resolved-by-unique-name` | Calls are dropped (no relationship, no pending row) from any function whose name is also used by another symbol in the file |
| high | body_spans | `lua.function-value-symbol-spans` | Functions bound by assignment or table field span only their name: no body, no parameters, zero complexity |
| high | body_spans | `lua.function-value-symbols-name-only-span` | Functions assigned as values get a name-only span, no body hash, no parameters, and empty complexity |
| high | pending_relationships | `lua.local-require-no-import-edge` | `local x = require("m")` emits no imports pending row and no source metadata; calls through x carry no import context |
| high | symbols | `lua.multi-segment-dotted-names` | `function M.a.b()` is dropped, and `M.a.b = v` becomes a symbol literally named `M.a.b` |
| high | symbols | `lua.nested-dotted-names-dropped-or-garbled` | `function a.b.c()` is dropped entirely and `a.b.c = v` becomes a variable literally named `a.b.c` |
| high | symbols | `lua.reassignment-phantom-globals` | Reassigning a local or parameter inside a function creates a new public top-level variable symbol |
| high | symbols | `lua.reassignments-emit-parentless-public-variables` | Reassigning a local, parameter, or upvalue creates a new public top-level variable; `local x` without initializer emits nothing |
| medium | test_detection | `lua.busted-dsl-ungated` | Busted DSL calls get test roles in production files |
| medium | test_detection | `lua.busted-luaunit-vocabulary` | Busted aliases (test, spec, insulate, expose, pending, strict_*) and luaunit fixtures are not recognized |
| medium | test_detection | `lua.busted-luaunit-vocabulary-incomplete` | busted aliases (spec, test, pending, insulate, expose, strict_setup/teardown, finally) and luaunit setUp/tearDown/Test* tables are not recognized |
| medium | symbols | `lua.call-argument-table-fields-emitted-as-symbols` | Tables passed as call arguments become field symbols, while tables nested in declared values are never walked |
| medium | symbols | `lua.class-patterns-missed` | PIL `X:new` classes and middleclass `class()`/`:subclass()` are not classes, and `Foo:new()` records no type fact |
| medium | identifiers | `lua.colon-call-identifiers-lack-receiver` | `obj:method()` call identifiers carry no receiver metadata, unlike `obj.fn()` |
| medium | identifiers | `lua.colon-call-receiver-and-chain` | `obj:m()` call identifiers lack a receiver, and `a.b:m()` drops the `b` member_access |
| medium | identifiers | `lua.declaration-name-emitted-as-call-identifier` | Method declarations emit a `call` identifier (colon form) or `member_access` identifier (dot form) for their own name |
| medium | identifiers | `lua.declaration-names-emitted-as-references` | Method declaration names are emitted as call or member_access identifiers |
| medium | symbols | `lua.false-class-from-setmetatable-instances` | Constructor instances (`local self = setmetatable({}, Foo)`) and weak/proxy tables are classified as classes |
| medium | structural_facts | `lua.framework-structural-facts` | No framework facts for Lapis routes, Neovim commands/autocmds/keymaps, or LÖVE callbacks |
| medium | types | `lua.luacats-annotations-no-type-facts` | LuaCATS/EmmyLua `---@param`, `---@type`, `---@return`, `---@class` annotations produce no type facts |
| medium | types | `lua.luals-annotation-types` | LuaLS `---@type/@param/@return/@class A : B` annotations produce no type facts or inheritance |
| medium | relationships | `lua.no-extends-relationships` | Inheritance is only baseClass metadata: no extends edges, and the `{__index = Base}` form is missed |
| medium | structural_facts | `lua.no-framework-structural-facts` | No framework facts for Neovim commands/keymaps/autocmds, lazy.nvim plugin specs, or Lapis routes |
| medium | relationships | `lua.oop-class-idioms-and-extends-edges-missing` | middleclass/`---@class`/`{__index = Base}` classes are missed and no inheritance produces an extends relationship |
| medium | pending_relationships | `lua.require-alias-import-linkage` | `local x = require("m")` has no source metadata or imports edge, and calls through x have no import_context |
| medium | symbols | `lua.self-and-dotted-field-writes-misparented` | `self.x = v` in methods creates one field per write, parented to the first symbol named `self` anywhere in the file |
| medium | symbols | `lua.setmetatable-instance-as-class` | `local self = setmetatable({}, Foo)` in every constructor becomes a class named `self` |
| medium | symbols | `lua.table-field-parents-noise-docs` | Table-constructor fields: orphaned under `X = {}`/`M.k = {}`, noise from argument tables, and no doc comments |
| low | symbols | `lua.declaration-forms` | `local x` without initializer emits nothing, forward-declared local functions become public, and Lua 5.5 `global x = 1` is private |
| low | doc_comments | `lua.doc-comment-attaches-across-blank-line` | A `---` comment block separated by a blank line (file `---@brief` header) becomes the doc of the next declaration |
| low | pending_relationships | `lua.expression-receivers-garbled-in-pending` | Pending targets use raw expression text as receiver and rewrite ':' inside string literals |
| low | relationships | `lua.same-file-qualified-calls-pending` | Calls to same-file module or class functions via `M.f()` / `Foo.new()` stay pending |
| low | literals | `lua.sql-literal-carriers` | `os.execute("rm ...")` is captured as an SQL literal, and `db:query("SELECT ...")` is missed |

## markdown (unverified)

| Impact | Domain | Gap | Summary |
| --- | --- | --- | --- |
| high | body_spans | `markdown.body-span-brace-heuristic` | Code block and heading body spans come from the first '{'..'}' or '('..')' pair, not from the code content |
| high | body_spans | `markdown.crlf-link-byte-drift` | Link and footnote symbols in CRLF files get byte offsets that drift by one byte per preceding line |
| high | symbols | `markdown.inline-link-regex-misparses` | Inline links come from a regex instead of the inline grammar: titles leak into destinations, parenthesized URLs truncate, badge links produce garbage, images... |
| high | symbols | `markdown.link-rows-inside-code-and-comments` | The regex line scanner emits link and footnote symbols from fenced code, indented code, inline code, HTML comments, and escaped brackets |
| high | symbols | `markdown.links-and-footnotes-extracted-from-code` | Link and footnote regex runs inside fenced code, indented code, and inline code spans, so it emits garbage import and footnote symbols |
| high | symbols | `markdown.setext-headings-missing` | Setext headings (=== / --- underlines) produce no heading symbols, and the line-scanner setext facts fire on lists, tables, quotes, code fences, and empty fr... |
| high | symbols | `markdown.setext-headings-missing-symbols` | Setext headings (Title / ===, Title / ---) emit no heading symbols, so the content under them gets the wrong parent |
| medium | relationships | `markdown.anchor-slug-non-ascii` | Anchor slugs drop all non-ASCII letters, so CJK and similar headings share an empty slug and links resolve to the wrong heading; GitHub duplicate suffixes ne... |
| medium | body_spans | `markdown.code-block-body-span-heuristic` | Code block and heading body spans come from brace/paren heuristics on the node text, not from the fence content or section content |
| medium | source_regions | `markdown.fence-language-ignores-grammar-token` | Code fence language uses the first whitespace token of the info string instead of the grammar's language node, so rustdoc and Quarto/R Markdown fences get ba... |
| medium | structural_facts | `markdown.frontmatter-keys-not-extracted` | YAML/TOML frontmatter gives only one opaque 'frontmatter' property, with no key rows and no embedded region |
| medium | source_regions | `markdown.frontmatter-keys-opaque` | Frontmatter is one opaque property: no per-key rows (title, slug, tags) and no embedded YAML/TOML source region |
| medium | symbols | `markdown.heading-name-markup` | Heading names keep ATX closing hashes, attribute blocks, and link syntax; an empty heading yields an empty-named symbol |
| medium | relationships | `markdown.heading-text-and-anchor-ids` | Heading names keep raw markup, closing '#'s, and {#id} attributes, so anchor links to those headings do not resolve |
| medium | structural_facts | `markdown.inline-link-parse-inaccurate` | Inline link parsing cuts destinations at ')', keeps titles, misses wrapped labels, garbles badge links, and drops images |
| medium | symbols | `markdown.line-pass-wrong-parent` | Links and footnotes on the line after a code block or link definition get that block as parent |
| medium | literals | `markdown.literals-dropped-in-artifact` | Markdown link literals never reach the SQLite artifact because markdown has no literal-carrier policy |
| medium | symbols | `markdown.parent-off-by-one-after-block` | A link or footnote on the line right after a code block or link definition gets that block as its parent |
| medium | doc_comments | `markdown.pipe-table-missing-from-section-doc` | Section doc_comment omits GFM tables because is_content_node checks 'table', not 'pipe_table' |
| medium | relationships | `markdown.reference-link-usages-and-footnote-edges` | Reference-style link usages and footnote references have no rows and no references edge to their definitions |
| medium | relationships | `markdown.reference-links-no-edges` | Reference-style link usages emit nothing, and footnote references are not linked to their definitions |
| medium | structural_facts | `markdown.task-lists-autolinks-footnote-facts` | Task-list items, autolinks, and footnotes emit no structural facts (recorded open gap, reproduced) |
| low | pending_relationships | `markdown.cross-file-anchor-links-no-pending` | Links to a heading in another document (other.md#anchor) emit no pending relationship; the capability exception says such links are not symbol references, wh... |
| low | symbols | `markdown.duplicate-footnote-definition` | A footnote definition with a single-token body is emitted twice, once by the tree pass and once by the line regex |
| low | source_regions | `markdown.fence-language-not-normalized` | The fenced code language keeps rustdoc attributes and Pandoc braces ('rust,no_run', '{.python}') |
| low | source_regions | `markdown.html-block-not-embedded` | Non-comment HTML blocks produce no embedded HTML region and their links and images are lost |

Recorded open gaps assessed:

- `markdown.extended_markdown_blocks_and_references`: closable now (medium). Most of this gap can be closed inside the extractor now. Task lists come from the block grammar (task_list_marker_checked/unchecked). Reference-link usages and angle-bracket autolinks come from tree_sitter_md::INLINE_...

## php (unverified)

| Impact | Domain | Gap | Summary |
| --- | --- | --- | --- |
| high | symbols | `php.assignment-lhs-garbage-variables` | Assignments to properties, array elements, or list() create a bogus variable named after the right-hand variable |
| high | identifiers | `php.class-constant-access-identifiers` | Foo::class gives variable_ref instead of type_usage, and Enum::Case, self::CONST and static::$prop give no member_access |
| high | test_detection | `php.docblock-test-substring-false-roles` | Any docblock that contains the text "@test" makes a production method a test_case and its class a test_container |
| high | types | `php.garbage-type-facts` | infer_types publishes declaration-kind labels as type facts and marks declared return types as inferred |
| high | pending_relationships | `php.nullsafe-calls-dropped` | Nullsafe calls $x?->m() emit no call identifier and no relationship, and $x?->prop emits no member_access |
| high | identifiers | `php.nullsafe-calls-ignored` | Nullsafe ?-> calls and accesses emit no identifiers, relationships, pending rows, or literals |
| high | symbols | `php.promoted-property-dropped` | Constructor-promoted properties never reach the artifact: the parameter symbol has the same id and wins the dedup |
| high | annotations | `php.stacked-attribute-groups` | Stacked #[..] attribute groups are parsed as one blob: annotations are garbled and PHPUnit test roles are lost |
| high | annotations | `php.stacked-attribute-groups-garbled` | Stacked #[A] #[B] attribute groups parse into one garbage annotation, which removes PHPUnit test roles and garbles Doctrine and Symfony annotations |
| high | relationships | `php.trait-use-not-modeled` | Trait use inside a class emits no uses edge; qualified trait names become import symbols and bare names vanish |
| medium | symbols | `php.assignment-phantom-locals` | $this->x = $x, $arr['k'] = $v and static::$p[] = $v create phantom local variable symbols named after the right-hand side |
| medium | pending_relationships | `php.chained-call-pending-garbage` | Chained method calls put raw source text (arguments, newlines, closures) into target_display_name and target_receiver of pending rows |
| medium | pending_relationships | `php.chained-call-raw-receivers` | Pending call rows copy raw multi-line source into receiver and display name for fluent chains and dynamic callees |
| medium | identifiers | `php.class-constant-and-static-property-identifiers` | Enum case, class constant, and static property accesses emit no member identifiers |
| medium | relationships | `php.enum-anon-class-heritage` | Enum implements and anonymous-class extends/implements emit no edges; new class {...} yields a garbage instantiates target |
| medium | literals | `php.heredoc-sql-literals` | Heredoc and nowdoc call arguments are never captured as literals, and DBAL and Laravel DB carriers are missing |
| medium | structural_facts | `php.http-client-property-receiver` | Guzzle and Symfony HttpClient calls through $this->prop emit no http.client_request.v1, although the property type is declared in the same class |
| medium | structural_facts | `php.laravel-route-handler-binding` | Laravel route facts lose the handler for invokable controllers and Route::controller() groups, and no route names are captured (Laravel ->name(), Symfony name:) |
| medium | symbols | `php.multi-element-declarations` | Only the first element of const A=1, B=2; and public $a, $b; becomes a symbol |
| medium | identifiers | `php.new-and-qualified-type-identifiers` | new Foo() emits no identifier, and qualified names stay raw (\App\Support\audit, \Illuminate\Http\Request) |
| medium | identifiers | `php.qualified-type-usage-identifiers` | A type_usage identifier keeps the full qualified text with a leading backslash, so find-references by class name misses it |
| medium | structural_facts | `php.route-service-provider-prefix-site` | Route::prefix('api')->group(base_path('routes/api.php')) emits no laravel.route_prefix.v1 at the definition site, so code-kb has nothing to join |
| medium | symbols | `php.single-segment-and-grouped-imports` | use Exception; (a single-segment name) emits no import symbol; grouped-use facts keep only the first short name |
| medium | literals | `php.sql-literal-carriers` | The SQL literal carriers give a false positive on Laravel $request->query() and miss DB::select, whereRaw and Doctrine createQuery |
| medium | relationships | `php.static-self-local-resolution` | static::m(), ClassName::m(), new self and new static do not resolve to same-file symbols |
| medium | source_regions | `php.string-type-keyword-regions` | The string type keyword is emitted as a string_literal source region |
| medium | test_detection | `php.test-frameworks-codeception-behat-phpspec` | Codeception Cest, Behat contexts and PHPSpec specs publish no test roles |
| medium | types | `php.type-facts-quality` | Type facts contain garbage for classes, imports, namespaces and locals, do not normalize self, static or ?T, and drop union, intersection and never types |
| medium | relationships | `php.type-hierarchy-edges-missing` | Trait use, enum implements and anonymous-class extends/implements emit no edges, and new class {...} emits a pending row whose target is the class body |
| medium | symbols | `php.union-intersection-never-types-dropped` | Union, intersection, and DNF property types and never/intersection/DNF return types are dropped from signatures and metadata |
| medium | symbols | `php.use-imports-incomplete` | Imports lose single-segment names, second clauses and grouped facts, and a qualified trait `use` becomes an import |
| low | body_spans | `php.bodyless-body-spans` | Abstract and interface methods get their parameter list as body span and hash, and imports and properties get fallback body spans |
| low | symbols | `php.define-constants-missing` | define('NAME', value) global constants produce no constant symbol |
| low | symbols | `php.function-kinded-method` | Functions inside a braced namespace or a closure are emitted as kind method |
| low | structural_facts | `php.include-require-define` | require/include produce no import facts, and define() constants produce no symbols |
| low | structural_facts | `php.laravel-controller-group-action` | Routes inside Route::controller(X::class)->group() lose the controller in controller_action |
| low | relationships | `php.pest-dsl-self-calls` | Each Pest DSL symbol records a call to its own it/test/describe/beforeEach function |
| low | relationships | `php.pest-self-call-edges` | A Pest test or hook symbol records a call edge to its own DSL invocation (a self-loop or a pending 'it') |
| low | source_regions | `php.string-keyword-regions` | Every `string` type keyword becomes a string_literal source region |

Recorded open gaps assessed:

- `laravel.route_service_provider_prefix`: blocked (small). The full gap needs a cross-file join, which decision 0004 gives to code-kb: the prefix in the provider must reach effective_route_template in routes/api.php. The extractor half is missing today and can be fixed now. t...
- `php.http_client.instance_receiver`: closable now (small). The recorded reason makes the fix depend on 'in-file type resolution', but that information already exists. The extractor records declared property types (propertyType metadata and type facts) and promoted-parameter t...
- `codeception.cest_and_actor_roles`: closable now (small). All the evidence is in the file: the *Cest.php file name or class name suffix, public methods with an actor argument, and the _before/_after names. The fix is a rule in test_detection.rs (detect_php and mark_php_test_...
- `behat.step_definition_roles`: closable now (medium). The context class (implements Context) and the #[Given]/#[When]/#[Then] attributes or @Given/@When/@Then docblock tags are all in the file. First fix the multi-attribute annotation bug (gap php.stacked-attribute-group...
- `phpspec.example_roles`: closable now (small). This is a pure in-file rule: a class that extends ObjectBehavior (compared on the last segment, as TestCase is) is a container, it_/its_ methods are test_case, and let/letGo are fixture hooks. It needs a golden fixtur...

## powershell (unverified)

| Impact | Domain | Gap | Summary |
| --- | --- | --- | --- |
| high | pending_relationships | `powershell.calls-outside-function-statement-dropped` | Command calls in class methods, constructors and Pester blocks emit no call or pending rows |
| high | relationships | `powershell.calls-outside-functions-dropped` | Command calls inside class methods, Pester blocks, and other non-function scopes produce no call relationship or pending row |
| high | relationships | `powershell.class-base-list` | The base class comes from a regex over the class body, so bases are wrong, interfaces are dropped and cross-file bases get no pending row |
| high | doc_comments | `powershell.doc-comment-header-leak` | A file-leading or class-leading comment becomes the doc comment of every later symbol |
| high | doc_comments | `powershell.doc-comment-misattribution` | In-body comment-based help is ignored while file headers and class help bleed onto unrelated symbols |
| high | symbols | `powershell.garbage-variable-symbols` | Variable reads, member/index/static assignments, and env assignments create garbage or duplicate Variable symbols |
| high | symbols | `powershell.phantom-functions-from-error-text` | Any program-level parse error containing 'function Name' creates phantom Function symbols inside every advanced function |
| high | pending_relationships | `powershell.receiver-keeps-dollar-sigil` | Pending call receivers keep the $ sigil, so they never join to the typed variable symbol |
| high | pending_relationships | `powershell.receiver-sigil-breaks-typed-calls` | Call receivers keep the $ sigil, so receiver-typed resolution never joins to variable type facts |
| high | identifiers | `powershell.type-literal-usages-missing` | Type references in [Type] literals, casts, static calls, base lists, New-Object, and OutputType emit no type_usage identifiers |
| high | identifiers | `powershell.type-usage-identifiers-missing` | type_usage identifiers are emitted only for method return types and generic names |
| high | symbols | `powershell.variable-symbol-garbage` | Variable reads, $_, $this and member or index assignments create Variable symbols |
| medium | identifiers | `powershell.call-operator-commands` | Commands run with the & call operator emit no call identifiers or pending rows |
| medium | relationships | `powershell.case-insensitive-command-resolution` | Same-file command calls resolve case-sensitively, but PowerShell command names are case-insensitive |
| medium | literals | `powershell.cmdlet-literal-arg-binding` | Every string argument of a URL or SQL cmdlet is classified as a URL or SQL literal, and no http.client_request facts exist |
| medium | symbols | `powershell.export-module-member` | Export-ModuleMember yields an export named after the flag, drops exported names, and never narrows visibility |
| medium | symbols | `powershell.hidden-static-text-match` | hidden and static come from a substring search over the whole member text |
| medium | structural_facts | `powershell.http-client-facts-missing` | Invoke-RestMethod/Invoke-WebRequest emit no http.client_request facts and literals are tagged by carrier, not by parameter |
| medium | symbols | `powershell.import-forms` | Path-based and #Requires imports are missed or get garbage names |
| medium | symbols | `powershell.import-forms-missed` | Import-Module with a quoted path and Import-DscResource produce no import; using statements emit 'using' call identifiers |
| medium | doc_comments | `powershell.in-body-and-parameter-help` | Comment-based help inside the function body and parameter comments are not captured |
| medium | relationships | `powershell.inheritance-regex` | Class bases are parsed by regex: no pending extends for cross-file bases, interfaces dropped, bogus base from '::' in the body |
| medium | symbols | `powershell.module-exports-and-aliases` | Export-ModuleMember rows are named after the switch, and aliases emit no symbols |
| medium | symbols | `powershell.nested-param-attribution` | Parameters of nested functions and script blocks are attached to the outer function |
| medium | symbols | `powershell.parameter-mandatory-and-signature` | Parameter signatures show $true instead of the name and Mandatory parameters are labeled optional |
| medium | symbols | `powershell.parameter-signature-mandatory` | The parameter signature ends with $true instead of the name, and common Mandatory forms read as optional |
| medium | structural_facts | `powershell.psd1-manifest-empty` | Module manifests and settings files (.psd1) produce no rows beyond string regions |
| medium | structural_facts | `powershell.psd1-manifest-semantics` | Module manifests (.psd1) produce no rows |
| medium | types | `powershell.return-and-cast-type-facts` | Method return types, [OutputType()] and cast assignments record no type facts |
| medium | symbols | `powershell.scope-modifier-normalization` | Lowercase scope modifiers ($script:, $global:, $using:) and scoped function names are not normalized |
| medium | symbols | `powershell.scope-qualifier-names` | Lowercase scope qualifiers, splats and function scope prefixes stay in names |
| medium | symbols | `powershell.script-param-block` | Script-level param() parameters are not extracted |
| medium | symbols | `powershell.script-param-block-missing` | Script-level param() blocks produce no parameter symbols |
| low | identifiers | `powershell.bogus-call-receiver-metadata` | Command call identifiers get a receiver from the previous line when that line ends with a '.' argument |
| low | relationships | `powershell.case-insensitive-commands` | Command and Pester keyword matching is case-sensitive |
| low | annotations | `powershell.class-member-attributes-no-annotations` | Attributes on class properties and methods (DscProperty, Validate*) emit no annotation rows |
| low | structural_facts | `powershell.dsc-configuration-internals` | DSC Configuration bodies (Node blocks, resource instances, DependsOn) produce no rows |
| low | annotations | `powershell.dsc-resources-and-property-attributes` | DSC resource blocks and class property attributes are not extracted |
| low | structural_facts | `powershell.pipeline-fact-duplicates` | pipeline_expression facts duplicate on assignments and fire on '\|' inside strings and regexes |
| low | structural_facts | `powershell.psake-tasks-not-symbols` | psake/Invoke-Build task definitions produce no symbols or facts, so calls inside tasks have no caller |

## python (unverified)

| Impact | Domain | Gap | Summary |
| --- | --- | --- | --- |
| high | relationships | `python.extends-imported-bases` | Imported bases yield edges to Import symbols; dotted bases yield nothing |
| high | structural_facts | `python.flask-route-imported-receivers` | Flask routes on imported blueprints, flask.Flask and add_url_rule emit no facts |
| high | body_spans | `python.garbage-body-spans-non-callables` | Import, variable, constant, and attribute symbols get body spans from a text heuristic, even inside string literals |
| high | pending_relationships | `python.import-structured-metadata` | Import rows have no source-module or imported-name metadata, and pending calls through an import alias have no import context |
| high | symbols | `python.lambda-duplicate-orphan-symbols` | Every lambda emits two function symbols (one garbage), both with no parent, so calls inside lambdas break the call graph |
| high | symbols | `python.nested-callable-parent` | A nested def or class loses its enclosing function as parent; a def nested in a method becomes a method of the class |
| high | symbols | `python.nested-definition-parent` | Nested defs and classes get no parent or the outer class as parent |
| high | types | `python.optional-union-type-facts` | Optional[X] records the fact type 'Optional', X \| None records nothing, and a string annotation records a quoted name |
| high | symbols | `python.self-attribute-duplicate-properties` | Each self.x assignment creates a new property row, and a class-level x: T is a separate variable row of a different kind |
| high | types | `python.signature-regex-type-garbage` | Regex fallback turns signatures into false type facts |
| high | test_detection | `python.test-roles-django-testcase-variants` | Django tests.py, *TestCase bases, setUpTestData, pytest_asyncio fixtures get no roles |
| high | types | `python.type-facts-optional-annotated-property` | Optional, X \| None, Annotated, Mapped, forward refs and @property types unresolved |
| medium | structural_facts | `python.annotated-framework-receiver` | An annotated Flask/FastAPI receiver (app: Flask = Flask(...)) loses all its route facts |
| medium | type_argument_usages | `python.builtin-generic-type-args` | PEP 585 builtin generics (list[User], dict[str, User], tuple[...]) record no type-argument usages |
| medium | symbols | `python.class-kind-substring-match` | A class is kinded interface or enum when any base name merely contains 'Protocol' or 'Enum' |
| medium | pending_relationships | `python.decorator-call-attribution` | Decorator calls have no caller or are attributed to the enclosing class |
| medium | doc_comments | `python.docstring-from-later-statement` | Docstring taken from any bare string in the body, not the first statement |
| medium | doc_comments | `python.docstring-selection` | The docstring is any bare string in the body, not the first statement; string prefixes are not stripped; f-strings are accepted |
| medium | structural_facts | `python.drf-router-register-facts` | Django REST Framework routers, @action and @api_view emit no route facts |
| medium | symbols | `python.enum-member-casing` | Enum members that are not all-caps (or are one letter) are kinded variable, not enum_member |
| medium | relationships | `python.generic-base-extends` | A subscripted base class in the same file (Base[T]) gets no extends edge |
| medium | structural_facts | `python.http-client-instances` | http.client_request facts only fire for module-qualified requests.x/httpx.x; Session and Client instances, from-imports, and url= are missed |
| medium | identifiers | `python.match-pattern-references` | Class patterns and dotted value patterns in match/case emit no identifiers |
| medium | symbols | `python.protocol-enum-kinding` | Substring rules mis-kind asyncio.Protocol; lowercase enum members stay variables |
| medium | identifiers | `python.string-annotation-type-usage` | Forward-reference string annotations ("Repo") emit no type_usage identifiers |
| medium | identifiers | `python.string-forward-ref-type-usage` | String forward-reference annotations produce no type_usage |
| medium | pending_relationships | `python.super-call-receiver` | super().m() calls have receiver 'super()', no receiver_type, and an extra pending call to the super builtin; cls(...) is pending 'cls' |
| medium | symbols | `python.type-alias-statement` | PEP 695 `type X = ...` emits no symbol, and the alias name is recorded as a type_usage |
| low | doc_comments | `python.attribute-doc-comments` | Constants and fields never get #: comments or attribute docstrings |
| low | identifiers | `python.decorator-receiver-at-prefix` | Decorator member calls get receiver '@bp' with the at-sign |
| low | identifiers | `python.decorator-receiver-at-sign` | Call and member identifiers in decorators carry receiver '@app' / '@pytest' (the @ is kept) |
| low | structural_facts | `python.django-re-path-optional-slash` | re_path with optional trailing slash or unnamed groups gets no join key |
| low | structural_facts | `python.django-url-raw-string-and-include` | re_path raw strings lose backslashes; tuple includes store the whole call |
| low | symbols | `python.pep695-type-alias-symbols` | PEP 695 type aliases produce no symbols; type parameters dropped |
| low | symbols | `python.private-class-visibility` | Classes are always public, even with a leading underscore |
| low | symbols | `python.signature-splat-params` | Signatures drop *args, **kwargs, and the / and * separators, and lose the names of typed splat parameters |

Recorded open gaps assessed:

- `extends`: closable now (small). This does not depend on code-kb. The same file already emits structured pending rows for calls (relationships.rs, the LocalTargetResolution::Import arm). To close it: in extract_class_relationships, turn Import-symbol...
- `django.re_path_non_template_regex_normalization`: closable now (small). The recorded reason is accurate for alternation, lookaround and optional groups. Two common forms can be closed now in normalize_django_regex_route (python_web.rs:804). The first is an optional trailing slash '/?$' (p...

## qml (unverified)

| Impact | Domain | Gap | Summary |
| --- | --- | --- | --- |
| high | types | `qml.declared-object-and-parameter-types-missing` | id'd objects, id properties and typed function parameters get no declared type facts |
| high | relationships | `qml.false-uses-edges` | Property `uses` edges resolve chained or foreign member accesses to unrelated same-named properties of the current component |
| high | relationships | `qml.id-receiver-scope-resolution` | Calls through a component-scoped id from a nested object lose receiver_type, and a same-named nested function blocks same-file resolution |
| high | test_detection | `qml.nested-testcase-no-test-roles` | A TestCase nested inside the root object gets no test roles |
| high | pending_relationships | `qml.property-initializer-calls-dropped` | Calls inside property declaration initializers emit no calls relationship and no pending relationship |
| high | symbols | `qml.qmltypes-one-line-descriptor-names` | .qmltypes one-line descriptors produce quoted, semicolon-suffixed symbol names |
| high | types | `qml.typed-parameter-becomes-return-type` | A function with typed parameters and no return type gets a garbage return-type fact |
| high | symbols | `qml.ui-qml-form-component-name` | A .ui.qml file's component is named 'X.ui' instead of 'X' |
| medium | pending_relationships | `qml.alias-call-pending-no-import-context` | Pending calls through an import alias carry no import_context |
| medium | structural_facts | `qml.component-url-references` | Components loaded by URL (Loader.source, Qt.resolvedUrl, Qt.createComponent, StackView/pageStack push) leave no fact or reference |
| medium | structural_facts | `qml.component-url-references-no-facts` | Component references by URL (Loader, StackView, Qt.createComponent) produce no facts |
| medium | identifiers | `qml.declaration-sites-as-variable-refs` | Declaration names, import names, the `id` keyword, and handler names are emitted as variable_ref references |
| medium | identifiers | `qml.declarations-recorded-as-references` | Declaration names and binding names are emitted as variable_ref identifiers |
| medium | doc_comments | `qml.doc-comments-qdoc-and-signals` | QDoc `/*!` comments are ignored everywhere, and signals get no doc comment or visibility |
| medium | symbols | `qml.enum-assigned-members-dropped` | Enum members with explicit values are dropped |
| medium | structural_facts | `qml.grouped-property-block-facts` | Grouped property blocks produce fake object facts and unqualified binding names |
| medium | relationships | `qml.handler-calls-attributed-to-component` | Calls inside signal handlers use the component as caller, not the handler symbol |
| medium | symbols | `qml.handler-signature-is-whole-body` | A signal handler's signature holds the full handler body |
| medium | symbols | `qml.object-value-containment` | An object assigned to a property becomes an anonymous sibling row with no link to the property it fills |
| medium | body_spans | `qml.property-and-handler-body-spans` | Property, signal-handler, and signal body spans come from a text heuristic and select call argument lists or fragments |
| medium | relationships | `qml.property-initializer-calls-no-edges` | Calls in property initializers emit no calls relationship or pending row |
| medium | symbols | `qml.property-modifiers-missing` | `required`, `readonly`, and `default` property modifiers are not recorded in symbol metadata or the property fact |
| medium | relationships | `qml.qmltypes-component-semantics` | .qmltypes components get no prototype edge, no member types, and no Qt 5 enum members |
| medium | doc_comments | `qml.signal-and-qdoc-doc-comments-missing` | Signals never get doc comments, and qdoc /*! */ blocks are ignored everywhere |
| medium | relationships | `qml.value-source-objects-not-instantiated` | 'Type on property' value sources emit no instantiates edge, type usage or object fact |
| medium | identifiers | `qml.value-source-objects-untyped` | Value-source objects (`Behavior on x`, `NumberAnimation on x`) record their type as variable_ref with no instantiates edge or object fact |
| low | annotations | `qml.annotations-ignored-and-leak-bindings` | QML @annotations are ignored, and their members leak as binding facts |
| low | annotations | `qml.annotations-not-extracted` | QML `@Annotation { }` members parse but emit no annotation rows; the ledger marks annotations not_applicable |
| low | pending_relationships | `qml.js-alias-call-import-context` | Calls through a JavaScript import alias carry no import context on the pending row |
| low | symbols | `qml.nested-function-parent` | A function declared inside another function is parented to the component, not the enclosing function |
| low | symbols | `qml.property-modifiers-not-in-metadata` | required/readonly/default property modifiers are missing from metadata, contrary to the QML doc |
| low | identifiers | `qml.qualified-property-type-references` | Qualified property types break the reference contract, and the `alias` keyword is recorded as a type usage |
| low | identifiers | `qml.signal-connect-not-linked` | Dynamic `signal.connect(handler)` is not recorded as signal handling |
| low | literals | `qml.xhr-open-method-recorded-as-url` | xhr.open records the HTTP method as a url literal |
| low | source_regions | `qmldir.comments-and-static-system-directives` | qmldir `#` comments give no source region and `static`/`system` directives give no fact; the ledger marks comments not_applicable |
| low | structural_facts | `qmldir.mjs-resources-dropped` | .mjs script resources are dropped in qmldir and misclassified in QML imports; qmldir comments get no regions |

## r (unverified)

| Impact | Domain | Gap | Summary |
| --- | --- | --- | --- |
| high | relationships | `r.class-inheritance-edges-missing` | R6 `inherit`, S4 `contains` and RefClass `contains` give no extends relationship; RefClass `contains` is not recorded at all |
| high | symbols | `r.class-method-locals-parented-to-class` | Locals and nested functions inside R6/RefClass methods are parented to the class, not to the method |
| high | symbols | `r.class-method-locals-wrong-parent` | Locals and nested functions inside R6 and RefClass methods get the class as parent; RefClass method parameters are missing |
| high | relationships | `r.describe-container-captures-test-calls` | Calls inside testthat tests resolve to a describe() container with the same name, not to the function under test |
| high | symbols | `r.dotted-names-misclassified-as-s3` | Any dotted function name becomes an S3 method; leading-dot helpers get empty s3_method; same-file calls to them never resolve |
| high | symbols | `r.dotted-names-misclassified-s3` | Any dotted function name becomes an S3 method with garbage s3 metadata, and same-file calls to it stay unresolved |
| high | symbols | `r.extract-target-assignment-garbage-symbols` | Assignments to obj$x, obj[[k]], and self$field create variable symbols named 'self$name', 'cache[["key"]]', 'output$distPlot' |
| high | symbols | `r.member-assignment-garbage-symbols` | `x$y <-`, `x[[k]] <-`, `x@s <-` and `<<-` assignments create variable symbols named like `self$owner` and `raw$total` |
| high | relationships | `r.missing-extends-edges` | R6 inherit=, S4 contains=, and RefClass contains= emit no extends relationship (RefClass contains is not even recorded) |
| high | body_spans | `r.oneliner-body-span-is-parameter-list` | Brace-less functions get the parameter list as their body span, so different functions share one body hash |
| high | relationships | `r.pipe-duplicate-call-edges` | Every piped call gets a second call edge placed at the start of the pipe chain; bare magrittr targets get no edge |
| high | relationships | `r.r6-self-private-super-calls-unresolved` | R6 self$m()/private$m() calls to same-file methods never resolve; private$ and super$ calls carry no receiver_type |
| high | identifiers | `r.receiver-and-operator-identifiers-missing` | No identifier for the receiver object in obj$m()/obj$x (e.g. Queue$new()), for user %op% operator uses, or for pkg::fn value references |
| high | symbols | `r.s4-setmethod-argument-parsing` | S4 setMethod with signature()/c() gives garbage names; the named-argument forms of setMethod/setClass give no symbol; setReplaceMethod/setValidity are ignored |
| high | relationships | `r.top-level-calls-dropped` | Calls in top-level assignments and statements produce no relationship or pending row |
| medium | identifiers | `r.argument-names-emitted-as-variable-refs` | Named-argument keys, for-loop variables, and pipe placeholders are emitted as variable_ref identifiers |
| medium | structural_facts | `r.formula-fact-text-heuristic` | The formula fact fires on any binary_operator whose text contains `~` and misses one-sided formulas |
| medium | symbols | `r.import-forms-missed` | requireNamespace, box::use, import::from, pacman::p_load give no import; source(file.path(...)) records raw expression text |
| medium | structural_facts | `r.magrittr-pipe-fact-missing` | r.pipe_expression.v1 ignores `%>%`, although its contract names `\|>` or `%>%`; chains emit one fact per operator |
| medium | pending_relationships | `r.member-call-duplicate-uses-pending` | Every obj$m() call emits both a calls pending row and a duplicate uses pending row for the same target |
| medium | symbols | `r.member-visibility-not-set` | The visibility column is always NULL; R6 private members only carry metadata member_visibility |
| medium | identifiers | `r.named-argument-identifiers` | Named-argument labels, including R6 member names, are emitted as variable_ref identifiers |
| medium | relationships | `r.pipe-duplicate-edges-and-bare-magrittr` | Multi-line pipes duplicate call edges at the chain's first line; builtin filter is bypassed; '%>% fn' (no parens) gets no call edge |
| medium | structural_facts | `r.plumber-routes-missing` | plumber API routes (#* @get /path handlers and pr_get/pr_post) produce no route facts or handler symbols |
| medium | symbols | `r.r6-active-and-refclass-members-missing` | R6 active bindings, RefClass fields=c(...), and Class$methods(...) produce no member symbols |
| medium | types | `r.r6-private-super-receiver-type` | `private$m()` and `super$m()` calls have no receiver type; only `self$` gets one |
| medium | symbols | `r.s4-declaration-forms` | S4: setMethod with signature() yields garbage names, 'X <- setClass()' duplicates the class under a variable, and declarations emit self-pending calls |
| medium | types | `r.s4-slots-and-field-types` | S4 slots are not child Field symbols, representation() slots are ignored, and declared slot and RefClass field types give no type facts |
| medium | structural_facts | `r.shiny-framework-facts-missing` | Shiny apps emit no framework facts for inputs, outputs, reactives, observers or modules |
| medium | structural_facts | `r.structural-fact-matchers-wrong` | r.pipe_expression skips %>% despite its contract; r.formula_expression fires on any binary op whose text contains '~' |
| medium | test_detection | `r.test-role-gaps` | RUnit tests and lifecycle hooks, and testthat setup()/teardown(), are missed; production `test_*` helpers are flagged as tests; the not_applicable claim is w... |
| medium | pending_relationships | `r.variable-initializer-calls-dropped` | Calls in top-level variable initializers emit no relationship or pending row (Python and Ruby do) |
| medium | symbols | `r.visibility-not-populated` | The visibility column is always NULL: R6 private members, roxygen @export, and leading-dot internals are not reflected |
| low | pending_relationships | `r.dsl-self-referential-pending` | test_that/describe/it and setGeneric/setMethod symbols emit a pending call to their own DSL call |
| low | other | `r.namespace-file-unsupported` | R package NAMESPACE files are unsupported, although the R grammar parses them cleanly |
| low | symbols | `r.r6-active-bindings-missing` | R6 `active = list(...)` bindings produce no member symbols |
| low | symbols | `r.s7-classes-missing` | S7 classes, generics and methods (new_class/new_generic/method<-) are extracted as plain variables or not at all |
| low | test_detection | `r.testthat-namespaced-test-that-missed` | testthat::test_that(...) is not recognized as a test case |

## razor (unverified)

| Impact | Domain | Gap | Summary |
| --- | --- | --- | --- |
| high | symbols | `razor.code-block-fields-as-locals` | Most @code fields become `local-variable` variables; extra declarators, const and readonly are lost |
| high | symbols | `razor.code-block-members-unparented` | @code and @inject members get no parent; bUnit test components never become test containers |
| high | symbols | `razor.code-fields-misparsed-as-locals` | Only the first @code field is kind field; every later field becomes a variable with local-variable metadata |
| high | symbols | `razor.code-members-not-parented-to-component` | @code/@functions members and @inject properties have no parent although the file emits a component class symbol |
| high | identifiers | `razor.component-tag-usages-no-reference-rows` | Component tags produce no identifier or relationship rows, and tags inside @<...> templates (bUnit Render(@<Counter />)) produce no fact at all |
| high | identifiers | `razor.component-usage-invisible` | Component tags produce no identifier or pending row, so component usages have no references |
| high | structural_facts | `razor.cshtml-razor-pages-routes` | Razor Pages @page routes ignore the page's file path: bare @page gives no route, templates give wrong ones |
| high | symbols | `razor.error-node-drops-code-block-members` | A parse ERROR node that wraps the @code block drops every member symbol in it |
| high | types | `razor.inject-model-type-facts` | @inject properties and @model get no declared type fact; garbage 'unknown', 'var', and 'class' type facts appear instead |
| high | symbols | `razor.missing-csharp-member-kinds` | Constructors, enums, records, structs, interfaces, delegates, indexers and events in @code are dropped |
| high | symbols | `razor.modifierless-properties-dropped` | Properties without an access modifier are silently dropped, including the common [Inject] NavigationManager Nav { get; set; } |
| high | symbols | `razor.nested-type-declarations-dropped` | record, enum, interface, struct, constructor, destructor, delegate, indexer, and event declarations in @code produce no symbols; their members are orphaned |
| high | pending_relationships | `razor.pending-relationships-never-emitted` | Razor never emits pending rows for cross-file calls, @inherits/@implements, base lists, or component usage; the recorded exception's reasoning is factually w... |
| high | symbols | `razor.template-expression-phantom-symbols` | Every template expression (@x, @Model.Name, @item.Id, @(new ...)) creates a phantom variable symbol that duplicates real names and captures identifier contai... |
| medium | body_spans | `razor.body-span-heuristic` | Component body span starts at the first '{' or '(' anywhere in the file; directives and fields get garbage bodies |
| medium | relationships | `razor.call-edge-resolution` | this.X() calls produce no call edge, and bare calls resolve to the first symbol with that name in the file, even when it is in another class |
| medium | annotations | `razor.component-attribute-annotations` | @attribute [Authorize]/[Route]/[StreamRendering] do not become annotations on the component; fields never carry annotations |
| medium | annotations | `razor.component-directives-unmodeled` | @typeparam, @layout, @rendermode emit nothing; @attribute [Authorize] is not a component annotation |
| medium | symbols | `razor.cshtml-no-view-symbol` | .cshtml views get no file-level symbol, so their calls have no caller |
| medium | symbols | `razor.cshtml-view-no-containing-symbol` | .cshtml views and Razor Pages get no file-level symbol, so template identifiers have no container and template calls produce no edges |
| medium | symbols | `razor.default-visibility-public` | Members and nested classes without modifiers, and all locals, get visibility public; the C# rule is private |
| medium | symbols | `razor.directive-duplicates-and-coverage` | @namespace/@inherits/@implements each emit three symbols; @layout, @typeparam, @rendermode, and @preservewhitespace emit none; @model loses generic/qualified... |
| medium | relationships | `razor.event-handler-method-groups-no-edge` | Handler method groups bound in markup (@onclick, OnClick, OnValidSubmit) create no call edge |
| medium | identifiers | `razor.generic-method-call-identifiers` | Generic method calls lose their call identifier (Render<Counter>()) or get a call name that includes the type arguments (Empty<TItem>) |
| medium | literals | `razor.http-json-literal-carriers` | URL literals passed to GetFromJsonAsync/PostAsJsonAsync/PutAsJsonAsync are dropped because razor.toml lacks the System.Net.Http.Json carriers that csharp.tom... |
| medium | structural_facts | `razor.httpclient-relative-urls` | HttpClient calls with relative URLs (the Blazor WASM idiom) produce no http.client_request |
| medium | identifiers | `razor.implicit-expression-receiver-at-prefix` | Member-access receivers in markup keep the Razor '@' (receiver "@_form") |
| medium | relationships | `razor.inheritance-edges-missing` | No extends/implements relationships or base-type identifiers for classes in @code, even when the base is in the same file |
| medium | relationships | `razor.inherits-implements-directive-rows` | @inherits/@implements/@namespace emit 3 duplicate rows, no extends/implements edge, and lose generic or alias values |
| medium | structural_facts | `razor.mvc-tag-helper-facts` | MVC/Razor Pages tag helpers, partials, view components and layouts emit no facts |
| medium | structural_facts | `razor.navigation-route-references` | Relative NavigateTo targets and interpolated hrefs produce no route_reference |
| medium | identifiers | `razor.receiver-at-prefix` | Receivers of member access and calls inside implicit expressions include the Razor '@' transition (receiver "@Model", "@item") |
| medium | structural_facts | `razor.route-facts-razor-pages-and-relative` | Razor Pages routes ignore the file path; bare @page emits nothing; base-relative NavLink/href/NavigateTo and MVC asp-* tag helper links emit no route facts |
| low | structural_facts | `razor.render-fragment-params-as-components` | Render-fragment parameter tags (ChildContent, Template, Columns) are reported as component references |
| low | complexity_metrics | `razor.template-control-flow-complexity` | Template control flow (@if/else if, @foreach, @for, @while, @switch case, @try/catch) is not counted in complexity metrics |

## regex (unverified)

| Impact | Domain | Gap | Summary |
| --- | --- | --- | --- |
| high | relationships | `regex.backref-edge-self-loop-and-adjacent-containment` | A backreference or group name that directly follows a group is attributed to that group: the backref edge becomes a self-loop and identifiers bind to the wro... |
| high | body_spans | `regex.body-span-brace-heuristic` | Pattern and group body spans come from the generic first-`{`-to-last-`}` heuristic, so quantifier braces become the 'body' |
| high | body_spans | `regex.body-span-text-heuristic` | Body spans come from a brace/HTML/paren text heuristic and pick quantifier fragments |
| high | symbols | `regex.multiline-file-merged-into-one-pattern` | A multi-line .regex file becomes one pattern: capture numbers and backreferences cross lines |
| high | identifiers | `regex.named-group-name-mismatch` | Named group symbols take the full group text as the name, so name-keyed reference joins fail |
| high | symbols | `regex.root-pattern-kind-name-flip` | The root pattern symbol's kind comes from text heuristics and flips with content and a trailing newline. Its name is the whole file including the newline, an... |
| medium | structural_facts | `regex.backreference-facts-missing` | Backreferences (\1, \k<name>, (?P=name)) emit no structural facts |
| medium | structural_facts | `regex.boundary-anchor-facts-missing` | `\b`, `\B`, `\A`, `\z` and `\Z` never emit regex.anchor.v1 facts although the builder already classifies them |
| medium | structural_facts | `regex.inline-flags-no-facts` | Inline flag groups (?i), (?s:...) and (?x) emit no fact and no metadata |
| medium | symbols | `regex.lookaround-and-unicode-node-kinds-unhandled` | Lookaround and \p{} symbols come from a text scan: no parent, nested ones dropped, nesting ignored |
| medium | structural_facts | `regex.lookaround-direction-polarity-from-substring` | Lookaround direction and polarity come from substring checks, so a nested assertion flips them |
| medium | symbols | `regex.lookaround-unicode-orphaned` | Lookaround and \p{..} symbols come only from a raw-text rescan with parent None, because dispatch matches node kinds the grammar never emits |
| medium | symbols | `regex.named-group-symbol-name` | Named capture groups are named by their full text, so the capture name never matches the identifiers, and their metadata says capturing=false |
| medium | relationships | `regex.python-named-backreference` | Python/PCRE named backreference `(?P=name)` emits no references edge and no identifier |
| medium | relationships | `regex.python-named-backreference-ignored` | (?P=name) backreferences give no relationship and no identifier; \1 gives no identifier |
| medium | structural_facts | `regex.root-kind-heuristic-breaks-fact-binding` | The root pattern kind comes from a text heuristic, so structural facts bind to unrelated sibling symbols |
| medium | symbols | `regex.root-name-is-raw-file-text` | The root pattern name and span include the trailing newline or CRLF, and for multi-line files the whole file |
| medium | structural_facts | `regex.structural-facts-misbound` | When the root pattern is kind variable, anchor, alternation, quantifier and capture facts bind to an unrelated class or group elsewhere on the line, or to no... |
| medium | relationships | `regex.verbose-mode-comments-parsed-as-pattern` | (?x) comments are read as pattern text and give false groups, capture indices and backreference edges |
| low | structural_facts | `regex.alternation-branch-count` | regex.alternation.v1 branch_count counts every `\|` in the text, including nested groups and character classes |
| low | structural_facts | `regex.alternation-branch-count-overcounts` | branch_count counts every '\|' in the text, including nested alternations and '\|' in character classes |
| low | structural_facts | `regex.inline-flags-ignored` | Inline flag groups `(?i)`, `(?i:...)` and `(?-s)` produce no fact and no metadata |
| low | symbols | `regex.named-group-capturing-false` | Named capture groups record metadata capturing="false" |
| low | literals | `regex.pattern-literal-runs-not-recorded` | Fixed literal text in patterns is never recorded; the literal arm never runs for this grammar |
| low | literals | `regex.pattern-literals-dead-arm` | Literal text runs in a pattern are never recorded. The `literal`/`character` arm is dead because the grammar emits one pattern_character per char |
| low | other | `regex.regexp-extension-unsupported` | `.regexp` files are not scanned (GitHub Linguist maps both .regex and .regexp to Regular Expression) |

Recorded open gaps assessed:

- `regex_advanced_constructs`: closable now (medium). It can be closed now inside the extractor. Backreferences are first-class grammar nodes (decimal_escape, backreference_escape, named_group_backreference), so that part is small; the probe reproduced no facts for \1, \...

## ruby (unverified)

| Impact | Domain | Gap | Summary |
| --- | --- | --- | --- |
| high | relationships | `ruby.bare-method-call-no-call-edge` | Receiverless zero-arg method calls (`subtotal`, `user_params`, `current_user`) emit no call edge, pending row, or call identifier |
| high | symbols | `ruby.compact-scoped-declaration-names` | `class A::B` and `module A::B` are named 'A::B', so cross-file extends and references never match them |
| high | symbols | `ruby.constant-references-become-symbols` | Constant references in when, rescue, and assignment right sides are published as constant definitions |
| high | doc_comments | `ruby.doc-comment-attachment` | Doc comments are lost on the first member of a body, and magic comments or RuboCop directives become docs |
| high | symbols | `ruby.garbage-symbols-from-references-and-writers` | Constant reads, global-variable reads, and attribute/index writer calls are emitted as constant/variable symbols |
| high | pending_relationships | `ruby.pending-call-targets-from-text` | Pending call targets come from text splitting, so chained calls and calls without parentheses get lost or garbage targets |
| high | pending_relationships | `ruby.pending-target-from-call-text` | Pending targets are cut from raw call text at the first '(' so block calls, string args, and chains produce wrong method names and garbage receivers |
| high | relationships | `ruby.receiverless-calls-no-edge` | Calls with no receiver and no arguments, &:sym, send(:sym), method(:sym), and super produce no call edge |
| high | structural_facts | `ruby.sinatra-routes-no-facts` | Sinatra route DSL (`get '/users/:id' do`) emits no route facts and no handler symbols |
| high | test_detection | `ruby.test-roles-rails-testcase-bases` | Rails tests under ActionController/ActionMailer/ActiveJob/SystemTestCase bases get no test roles |
| medium | symbols | `ruby.assignment-target-symbols` | Setter calls, index assignments, compound assignments, and global-variable reads create variable symbols |
| medium | relationships | `ruby.callback-symbol-references` | Symbol-argument method references (`before_action :set_post`, `validate :m`, `rescue_from ..., with: :m`, `send(:m)`) emit no reference rows |
| medium | symbols | `ruby.class-new-data-define` | `Foo = Class.new(Base) do` and `Data.define` become constants, and methods in their blocks lose their parent |
| medium | identifiers | `ruby.comment-text-receiver` | Receiverless call identifiers get a `receiver` read from the preceding comment text |
| medium | types | `ruby.cross-file-new-type-facts` | `x = Foo.new` / `@x = Foo.new` records a type fact only when Foo is defined in the same file |
| medium | symbols | `ruby.definition-shapes-missing-or-misparented` | Singleton setters/operators vanish; `class << self`, alias, alias_method, define_method, Class.new, and Data.define definitions get wrong parent or kind |
| medium | structural_facts | `ruby.fact-metadata-receiver-and-rescue` | `params.require` makes an import fact, receiver `.extend` makes a mixin fact, and rescue exception_type reads the wrong node |
| medium | literals | `ruby.heredoc-sql-literals` | Heredoc arguments produce no literal row, so SQL in <<~SQL is not captured |
| medium | structural_facts | `ruby.http-client-and-sql-coverage` | HTTP client facts cover only Net::HTTP, and SQL literals miss heredocs and ActiveRecord `where`/`order` fragments |
| medium | structural_facts | `ruby.http-client-facts` | http.client_request covers only Net::HTTP with an inline URI literal; HTTParty, Faraday, and RestClient give none |
| medium | relationships | `ruby.inheritance-mixin-resolution` | extends/implements pick the first symbol with the same name, keep only the first include argument, and publish garbage superclass text |
| medium | identifiers | `ruby.instance-variable-references` | Reads and writes of @ivar and @@cvar emit no identifier rows, so field symbols have no references |
| medium | symbols | `ruby.metaprogrammed-method-symbols` | alias, define_method, and def_delegator symbols have no parent; alias_method, def_delegators, and delegate emit nothing |
| medium | symbols | `ruby.rails-model-dsl` | Rails scope, association, and enum declarations define no symbols, and callback symbols reference no method |
| medium | structural_facts | `ruby.rails-route-collector-line-based` | Rails route collector is line-based and misses symbol-path routes, multi-line calls, one-line blocks, and nested scopes |
| medium | structural_facts | `ruby.rails-route-dsl` | The Rails route scanner misses symbol-action routes and nested resources, and garbles namespace options |
| medium | test_detection | `ruby.rails-test-case-bases` | Rails framework test bases other than ActiveSupport::TestCase and IntegrationTest are not collected |
| medium | test_detection | `ruby.rspec-example-identity` | RSpec one-liner examples (`it { ... }`, `specify { ... }`) emit no test_case, and example names are mangled by quote trimming |
| medium | test_detection | `ruby.rspec-nested-hook-false-fixture` | `before`/`after` blocks nested inside an example or `let` body get fixture roles |
| medium | pending_relationships | `ruby.rspec-shared-example-references` | `it_behaves_like`/`include_examples` emit no reference to the shared example group |
| medium | other | `ruby.ruby-file-registration` | Gemfile, Rakefile, *.rake, *.gemspec, and config.ru are not recognized as Ruby |
| medium | other | `ruby.ruby-file-routing` | .rake, Rakefile, Gemfile, .gemspec, and config.ru files are not scanned as Ruby |
| medium | test_detection | `ruby.test-block-call-symbols` | RSpec one-line examples get no test symbol, and hook blocks record a call to themselves |
| medium | types | `ruby.type-fact-inference` | Literal type inference reads only the first character of the right side, and @ivar = SameFile.new records no fact |
| medium | symbols | `ruby.visibility-symbol-argument-forms` | `private :name` and `private_class_method :name` do not change the named method's visibility |
| medium | symbols | `ruby.visibility-tracking` | `private` leaks into nested classes and onto `def self.x`, and name-list visibility forms are ignored |
| low | structural_facts | `ruby.rescue-clause-exception-type` | The rescue_clause fact cuts a qualified exception to its first segment, drops later ones, and reports the bound variable as the type |
| low | annotations | `ruby.rspec-example-metadata-tags` | RSpec metadata tags (`:slow`, `type: :model`) reach no channel |

Recorded open gaps assessed:

- `rspec.shared_example_group_references`: closable now (small). Reproduced. The ledger's worry (a symbol row would be a second definition) does not block closure: RelationshipKind::References and the pending-relationship channel already model a name reference with no new symbol. E...
- `rspec.example_metadata_tags`: closable now (small). Reproduced. The symbol_annotations table (annotation, annotation_key, raw_text, carrier) already carries tag-like rows for other languages. Emit rows from extra simple_symbol and pair arguments of example/group calls,...

## rust (verified)

| Impact | Domain | Gap | Summary |
| --- | --- | --- | --- |
| high | body_spans | `rust.bodiless-body-spans` | Declarations without a block body get garbage body spans and body hashes from the textual brace/paren fallback |
| high | relationships | `rust.calls-in-macro-args-invisible` | Calls inside macro arguments (assert_eq!, assert!, println!, format!, vec!, user macros) emit no call edge, no pending call, and no call identifier |
| high | symbols | `rust.generic-impl-methods-dropped` | Every method in an impl block whose self type is generic, lifetime-parameterized, a reference, or dyn Trait is dropped, with its params, locals, calls, and c... |
| high | symbols | `rust.grouped-use-collapsed` | Grouped use trees collapse into one import named after the prefix; extern crate has no row; pub(in ..) use gives a garbage pending target |
| high | pending_relationships | `rust.trait-edges-missing` | No implements edge for path-qualified, generic, grouped-import, or prelude traits, and no extends edge for supertraits |
| high | relationships | `rust.turbofish-calls-no-edges` | Turbofish calls (f::<T>(), a::b::<T>(), x.collect::<T>()) emit no relationship and no pending call; Type::<T>::f gets a garbage namespace |
| medium | annotations | `rust.annotations-missing-non-fn-items` | Attributes on fields, enum variants, traits, consts, statics, type aliases, and unions produce no annotation rows |
| medium | doc_comments | `rust.doc-comment-defects` | //! module docs attach to the next item, /// docs are lost across a plain // comment, and trait/extern/associated-type docs keep raw markers |
| medium | symbols | `rust.impl-member-containment` | Impl associated consts/types have no parent, fn items nested in methods are dropped, items nested in method bodies lose their parent, and methods attach to t... |
| medium | symbols | `rust.item-macro-garbage-symbols` | Item-position macros (lazy_static!, thread_local!, bitflags!, proptest!) become public function symbols named after the macro, and the items they define are ... |
| medium | identifiers | `rust.macro-callee-unreferenced` | Macro invocations emit no call identifier and no call edge, so macro_rules! macros have zero references; a scoped macro path emits a type_usage |
| medium | types | `rust.return-and-const-type-facts` | Declared return types are dropped when they contain spaces or commas, kept as raw text otherwise, Self is never resolved, and const/static items have no decl... |
| medium | structural_facts | `rust.rocket-routes-missing` | Rocket attribute routes and mount/routes! registration emit no route facts |
| medium | symbols | `rust.trait-members-kinded-function` | Trait required and provided methods are kind function, not method |
| medium | symbols | `rust.visibility-mapping` | pub(crate)/pub(super)/pub(in ..) map to public, a private use maps to public, and trait members, trait-impl members, and enum variants get inconsistent visib... |
| low | structural_facts | `rust.axum-qualified-verb-routes` | axum routes whose method router is a path-qualified verb (routing::get(h), axum::routing::post(h)) emit no route fact |
| low | identifiers | `rust.garbage-identifier-rows` | Garbage identifier rows: lifetimes and macro keywords as variable_ref, '_' as a type usage and type argument, 'await' as a call receiver, and attribute paths... |
| low | complexity_metrics | `rust.match-arm-complexity` | A match counts as one decision no matter how many arms or guards it has, and let-else is not counted |

Recorded open gaps assessed:

- `actix.scope_route_cross_file_registration`: blocked (large). The routes sit in a configure/service target in another file. Joining the scope prefix to them is code-kb's cross-file job (decision 0004). actix.mount.v1 already records the prefix at the scope site. Agree with the r...
- `actix.scope_route_variable_binding`: closable now (medium). Reproduced in g22.rs: `let api = web::scope("/api"); cfg.service(api.route("/ping", web::get().to(ping)))` emits nothing. The case is same-file. axum.rs:404 collect_router_receivers already has the single-assignment a...
- `actix.resource_route_guard_forms`: closable now (medium). Reproduced in g22.rs: `.service(web::resource("/things").route(web::post().to(create)))` emits nothing while the sibling `.route("/", web::get().to(index))` emits. The shape is in one expression. It needs a new actix....
- `axum.param_flavor_under_report`: closable now (small). The recorded reason is factually wrong. The extractor does not need the crate version: axum 0.8.9 panics on a path segment that starts with `:` or `*` (~/.cargo/registry/src/*/axum-0.8.9/src/routing/path_router.rs:58 ...
- `axum.cross_file_nest_join`: blocked (large). The sub-router's routes are in another function or file, so applying the nest prefix is code-kb's cross-file join (decision 0004). axum.nest.v1 already records mount_path and mount_target at the nest site.
- `rust.http_client.instance_receiver`: closable now (medium). The premise 'if in-file type resolution becomes available' is already met. Reproduced in g23.rs: the extractor records `client:field Client {declared: reqwest::Client}` and `alt:field Client` (with `use reqwest::Clien...
- `rust.benchmark_harness_roles`: closable now (medium). Partly closable, and the recorded reason is factually wrong. libtest converts benchmarks to run-once tests when not benchmarking (library/test/src/lib.rs convert_benchmarks_to_tests -> StaticBenchAsTestFn), so `cargo ...

## scala (unverified)

| Impact | Domain | Gap | Summary |
| --- | --- | --- | --- |
| high | doc_comments | `scala.braceless-trailing-comment-docs-spans` | Scala 3 indentation syntax: doc comments after an indented body are lost and the previous symbol span swallows them |
| high | pending_relationships | `scala.chained-call-receiver-garbage` | Pending call targets for chained calls include argument and lambda identifiers; super calls get no receiver_type |
| high | relationships | `scala.generic-call-no-edge` | Calls with explicit type arguments emit no relationship and no pending relationship |
| high | pending_relationships | `scala.generic-call-no-pending` | Calls with explicit type arguments (foo[T](x), obj.m[T](x)) emit no pending or resolved call relationship |
| high | relationships | `scala.local-val-call-attribution` | Calls in local val/var initializers are attributed to the local variable, not the enclosing function or test |
| high | relationships | `scala.local-val-steals-caller` | Calls inside local val/var initializers use the local variable as caller, not the enclosing method |
| high | relationships | `scala.new-expression-no-call` | new Foo(...) emits no call/constructor relationship, pending row, or call identifier |
| high | symbols | `scala.qualified-access-visibility` | Qualified access modifiers private[this], private[pkg], protected[pkg] give public visibility and garbage identifiers |
| high | test_detection | `scala.scalatest-styles-missing` | ScalaTest WordSpec, FreeSpec and FeatureSpec tests produce no test symbols |
| high | test_detection | `scala.test-dsl-styles-missing` | WordSpec, FreeSpec, specs2, MUnit test options, fixture.test and ignore(...) tests produce no test symbols |
| high | test_detection | `scala.test-prefix-false-positive` | Any production method named test* or beforeAll/afterEach is marked as a test |
| high | test_detection | `scala.test-roles-unscoped` | Production methods named test*/beforeAll/afterAll and it(...) calls get test roles; suite classes never get test_container |
| medium | annotations | `scala.annotations-dropped-and-garbage-modifier` | Annotations on traits, enums, enum cases and vars are dropped; '@' leaks into modifiers/signatures; qualified annotation named 'scala' |
| medium | relationships | `scala.companion-inheritance-misattributed` | Companion object extends edges go to the class; qualified generic supertypes keep path and type args in the target |
| medium | symbols | `scala.constructor-param-lists` | Only the first constructor parameter list becomes properties; enum params are skipped; plain params are public 'val' |
| medium | doc_comments | `scala.doc-comment-after-braceless-body` | Scaladoc for a definition that follows a Scala 3 indentation-based body is lost |
| medium | doc_comments | `scala.enum-case-docs-and-self-refs` | Enum case doc comments are lost, enum case names are emitted as self variable_refs, enum parameters are not members |
| medium | relationships | `scala.given-no-type-link` | Scala 3 given instances get no implements edge, no type fact, and anonymous ones are all named <anonymous> |
| medium | structural_facts | `scala.http-route-facts-missing` | No route facts for Akka/Pekko HTTP routing DSL or http4s routes |
| medium | identifiers | `scala.infix-method-calls` | Alphanumeric infix method calls (a plus b, xs map f) emit no call identifier or call edge |
| medium | identifiers | `scala.infix-method-calls-dropped` | Infix method calls (xs map f, a max b, actor ! msg) emit no call identifier or pending row |
| medium | literals | `scala.interpolated-sql-url-untracked` | sql"..."/uri"..." interpolators and s"..." strings produce no literal or string region; Play WS/sttp URLs not captured |
| medium | literals | `scala.interpolated-string-literals` | Interpolated strings: s"..." call arguments decode to "{}", get no source region, and sql"..." is not captured |
| medium | symbols | `scala.member-visibility-and-kinds` | Plain constructor params are public 'val' members; trait vals are 'constant'; class/object/trait vars are 'variable' |
| medium | pending_relationships | `scala.new-expression-no-edge` | new Foo(...) emits no call edge, no pending relationship and no call identifier; this(...) delegation emits pending target 'this' |
| medium | symbols | `scala.package-block-and-package-object` | package x { ... } gets the whole block text as its name; package object gets no symbol and its members have no parent |
| medium | symbols | `scala.pattern-and-multi-name-vals` | Destructuring and multi-name vals produce no symbols or a symbol named after the type |
| medium | identifiers | `scala.qualified-type-paths` | Qualified type paths emit package segments as variable_ref, lose type arguments, and lose namespace in pending extends |
| medium | other | `scala.sbt-files-unsupported` | build.sbt files are not recognized although they are Scala and parse cleanly |
| medium | symbols | `scala.val-var-kind-scope` | val/var kinds ignore scope: trait vals are constant, class vars are variable, test-body vals are property |
| medium | structural_facts | `scala.web-framework-facts` | No route or HTTP client facts for any Scala web framework |
| low | annotations | `scala.annotation-garbage-and-drops` | Annotations add '@' modifiers to signatures, are dropped on traits/enums/vars, and type-argument annotations get garbage names |
| low | symbols | `scala.destructuring-val-garbage` | Pattern vals create a symbol named after the right-hand side; multi-name vals create none |
| low | symbols | `scala.given-extension-modeling` | given instances have no implements edge or type fact and anonymous givens are named '<anonymous>'; extension blocks are named 'extension' with no extended type |
| low | identifiers | `scala.qualified-name-variable-ref-garbage` | Package segments of qualified types, package-object names and enum case names are emitted as variable_ref reads |

## sql (unverified)

| Impact | Domain | Gap | Summary |
| --- | --- | --- | --- |
| high | symbols | `sql.alter-routine-no-symbol` | ALTER PROCEDURE and ALTER FUNCTION parse cleanly but emit no symbol and no routine fact |
| high | relationships | `sql.alter-table-add-constraint-column` | ALTER TABLE ADD CONSTRAINT FOREIGN KEY and ADD COLUMN lose the table: no FK edge, unparented constraint, no column symbol |
| high | relationships | `sql.alter-table-constraints-and-columns-orphaned` | ALTER TABLE ADD CONSTRAINT FOREIGN KEY emits no edge or pending, orphans the constraint, and ALTER TABLE ADD COLUMN emits no field |
| high | relationships | `sql.exec-and-routine-calls` | EXEC/EXECUTE emits nothing, and UDF calls and trigger EXECUTE FUNCTION emit no calls edges or pending rows |
| high | body_spans | `sql.routine-body-span` | Routine body span runs from the parameter list '(' to the last ')' and cuts off the body tail |
| high | body_spans | `sql.routine-body-spans-truncated` | Function body spans start at the parameter list and stop at the last ')' in the body; column bodies are random paren fragments |
| high | relationships | `sql.routine-calls-no-edges` | EXEC, function invocations and trigger EXECUTE FUNCTION produce no calls relationships or pending calls; EXEC produces no identifier at all |
| high | relationships | `sql.routine-table-dependencies` | Views, procedures, functions and triggers emit no identifiers or pending edges for the tables they read and write |
| high | symbols | `sql.schema-qualifier-dropped` | Schema qualifier is dropped from symbols, so FK resolution binds to a same-named table in another schema |
| high | relationships | `sql.table-references-no-identifiers-or-edges` | Table and view references in routines, views, triggers and CTAS emit no identifiers, no in-file edges and no pending relationships |
| high | symbols | `sql.tsql-recovery-misnames-routines` | Error recovery names T-SQL procedures after a parameter type and triggers after their schema; CREATE PROC is dropped |
| medium | doc_comments | `sql.comment-on-not-used-as-doc` | COMMENT ON statements and MySQL COMMENT attributes are not used as doc comments |
| medium | doc_comments | `sql.container-doc-comment-bleeds-into-children` | Columns, parameters and local variables inherit the table's or function's doc comment |
| medium | doc_comments | `sql.doc-comment-attribution` | Doc comments leak from table/routine to its columns, params and indexes; trailing column comments go to the next sibling; COMMENT ON is ignored |
| medium | symbols | `sql.index-table-link` | Index symbols have no link to their table, a wrong ON clause, and case-sensitive UNIQUE detection |
| medium | literals | `sql.literals-dropped-in-artifact` | All SQL literals are dropped from the artifact because there is no languages/sql.toml policy |
| medium | symbols | `sql.materialized-view-ignored` | CREATE MATERIALIZED VIEW emits no symbol, fact or source edges and leaves stray top-level alias fields |
| medium | structural_facts | `sql.postgres-ddl-variants` | CREATE MATERIALIZED VIEW has no symbol, fact or edge; POLICY/EXTENSION/SEQUENCE/ENUM have no facts |
| medium | relationships | `sql.schema-qualifier-ignored` | Schema qualifiers are ignored: FK edges bind to the wrong same-named table and symbols drop their schema |
| medium | symbols | `sql.select-alias-garbage-symbols` | Ad-hoc top-level SELECT aliases become parentless field symbols |
| medium | test_detection | `sql.tsqlt-test-roles` | tSQLt test classes, tests and SetUp get no test roles; pgTAP runtests('x'::name) does not mark the schema |
| medium | test_detection | `sql.tsqlt-test-roles-missing` | tSQLt test classes, test procedures and SetUp get no test roles |
| medium | types | `sql.type-facts-regex` | Type facts come from a case-sensitive 12-name regex, so most declared column types and all function return types are lost |
| medium | types | `sql.type-facts-regex-misses-most-types` | Type facts come from a case-sensitive regex over the signature, so most column types and all function return types are missing or wrong |
| medium | symbols | `sql.unnamed-constraint-names` | Unnamed FOREIGN KEY constraints are named after the referenced column |
| medium | identifiers | `sql.variable-refs-miskinded-member-access` | Parameter and variable references are emitted as member_access; the variable_ref not_applicable claim is wrong |
| low | identifiers | `sql.column-receiver-alias-unresolved` | Column references keep the table alias as receiver and never record the aliased table |
| low | complexity_metrics | `sql.complexity-control-flow` | WHILE loops and IF statements are not counted, and routine parameter_count is always NULL |
| low | complexity_metrics | `sql.complexity-misses-control-flow` | Complexity ignores IF/WHILE statements, CASE branches and boolean operators; views get no metric; parameter_count is always null |
| low | structural_facts | `sql.function-fact-parameter-count-zero` | function/procedure facts count only @-sigil parameters, so PostgreSQL and MySQL routines always report 0 |
| low | identifiers | `sql.insert-target-columns-no-identifiers` | INSERT and MERGE target column lists emit no identifiers |
| low | literals | `sql.literals-dropped` | The CLI drops every SQL literal (no languages/sql.toml), N'...' strings are not decoded, and dynamic SQL is not classified |
| low | symbols | `sql.local-variable-symbols` | PL/pgSQL DECLARE variables are duplicated; T-SQL DECLARE in procedure bodies gives no symbols |
| low | structural_facts | `sql.structural-fact-metadata` | View source_tables include aliases and functions, PG parameter_count is always 0, MERGE fact needs a VALUES source |
| low | structural_facts | `sql.trigger-fact-incomplete` | Trigger facts keep only the first event, drop INSTEAD OF timing and omit the executed function |
| low | symbols | `sql.tsql-trigger-recovery-name` | Error recovery names a schema-qualified T-SQL trigger 'dbo' |
| low | symbols | `sql.unnamed-constraint-garbage-names` | Unnamed constraints are named after the referenced column or a line number |
| low | identifiers | `sql.variable-ref-kind` | @variables and routine parameters are emitted as member_access, and the ledger marks variable_ref not_applicable |
| low | structural_facts | `sql.view-fact-source-tables-polluted` | view_definition source_tables includes aliases, function names and cast types |

Recorded open gaps assessed:

- `sql.vendor_specific_ddl_variants`: closable now (medium). Not blocked on code-kb or cross-file joins. The pinned grammar already produces create_materialized_view, create_policy, create_extension, create_sequence, create_type, create_role, comment_statement and alter_table >...

## swift (unverified)

| Impact | Domain | Gap | Summary |
| --- | --- | --- | --- |
| high | relationships | `swift.accessor-and-deinit-containment` | Calls in computed properties, observers, lazy initializers and deinit are attributed to the enclosing type |
| high | pending_relationships | `swift.call-target-from-call-text` | Call targets come from source text split at '(' so trailing-closure and chained calls produce wrong, garbage, or lost call edges |
| high | pending_relationships | `swift.call-target-text-split` | Call targets come from splitting source text on '(', so trailing-closure calls, chained calls and await-prefixed calls produce garbage rows or no rows |
| high | relationships | `swift.computed-property-call-attribution` | Calls in computed properties (SwiftUI body), property observers, initializers and deinit are attributed to the enclosing type |
| high | relationships | `swift.conformance-edges` | Protocol inheritance and extension conformances of non-local types emit no edges; modified structs/enums/extensions emit 'extends' |
| high | symbols | `swift.default-access-level-internal` | Swift access levels are wrong: no modifier reads as public, initializers are always public, and extension, protocol and enum members ignore their parent's ac... |
| high | relationships | `swift.inheritance-edges-protocol-and-extension` | Protocol inheritance and extension conformances of types declared elsewhere emit no relationship and no pending row |
| high | symbols | `swift.multi-name-declarations` | Only the first name is emitted for comma-separated enum cases and property bindings, and tuple destructuring gives one garbage-named variable |
| high | symbols | `swift.multi-name-enum-cases-and-bindings` | Only the first name of 'case a, b, c' and 'var x, y' is extracted; tuple patterns become one garbage symbol |
| high | types | `swift.return-and-type-shapes-dropped` | Optional, array, dictionary, tuple, opaque and existential return/property types are dropped, and 'async throws' vanish from signatures |
| high | types | `swift.return-type-dropped-to-void` | Optional, opaque, existential, array and dictionary return types are dropped and recorded as Void |
| high | types | `swift.sentinel-type-facts` | Declaration-kind markers and the "Any" placeholder leak into type_facts as resolved types |
| high | relationships | `swift.subscript-and-defer-as-calls` | Subscript access and defer blocks are emitted as calls |
| high | test_detection | `swift.xctest-inherited-test-case-roles` | XCTest classes that inherit XCTestCase through a project base class get no test roles, even when the base is in the same file |
| medium | body_spans | `swift.bodyless-declaration-body-spans` | Protocol requirements, stored properties and enum cases get a parenthesized fragment as body span and hash |
| medium | symbols | `swift.default-visibility-internal` | Undeclared access defaults to public, private init reports public, extension access is ignored, locals get a visibility |
| medium | structural_facts | `swift.http-client-request-facts` | No http.client_request.v1 facts for Alamofire or URLSession requests |
| medium | pending_relationships | `swift.implicit-member-calls` | Implicit member calls such as .factory(...) and .init(...) are recorded as variable references with no call row |
| medium | symbols | `swift.init-signature-first-parameter-only` | Initializer signatures keep only the first parameter and drop the failable marker |
| medium | symbols | `swift.initializer-signature-first-param-only` | Initializer signature and parameters metadata keep only the first parameter |
| medium | symbols | `swift.macro-declarations-and-expansions` | Macro declarations emit no symbol; macro expansions (#expect, #stringify, #Preview) are variable references with no call row |
| medium | symbols | `swift.operator-and-macro-declarations-missing` | Operator functions, operator/precedencegroup declarations and macro declarations produce no symbols |
| medium | symbols | `swift.operator-functions-missing` | Operator functions (==, <, +, prefix and custom operators) emit no symbol |
| medium | types | `swift.placeholder-type-facts` | Legacy inference records keyword and 'Any' placeholders as resolved types |
| medium | test_detection | `swift.quickspec-subclass-container` | QuickSpec and AsyncSpec subclasses are not test containers, and an it() placed directly in spec() loses its role (recorded gap) |
| medium | symbols | `swift.type-signature-where-clause-pollution` | Type signatures absorb body text containing 'where ' and drop final/weak modifiers |
| medium | symbols | `swift.type-signature-where-leak` | Type and extension signatures copy a 'where' clause from any text in the body, including comments and member constraints |
| medium | structural_facts | `swift.vapor-route-facts` | Vapor route registrations emit no route facts and no route literals |
| medium | structural_facts | `swift.vapor-routes-and-http-client-facts` | No Vapor route facts and no http.client_request facts for URLSession or Alamofire |
| medium | test_detection | `swift.xctest-indirect-subclass-roles` | XCTestCase subclasses through a same-file base test class get no test roles |
| low | symbols | `swift.actor-signature-keyword` | Actor declarations are published with the signature keyword 'class' |
| low | source_regions | `swift.block-comment-regions-and-markers` | Block and /** */ doc comments produce no source regions and no TODO marker facts |
| low | identifiers | `swift.identifier-noise-attributes-and-labels` | Built-in attributes emit type_usage rows and enum associated-value labels emit variable_ref rows |
| low | symbols | `swift.local-function-kind` | Nested (local) functions are emitted as methods |
| low | annotations | `swift.swift-testing-traits` | Swift Testing traits (.disabled, .tags, .serialized) are kept only as raw annotation text and never parsed into a structured row (recorded gap) |
| low | structural_facts | `swift.swiftpm-manifest-facts` | Package.swift manifests emit no package, product, target or dependency facts |

Recorded open gaps assessed:

- `quick.quickspec_subclass_container`: closable now (small). Reproduced. The closure is local to the per-file extractor: base_types metadata already records QuickSpec and AsyncSpec, and mark_base_type_test_containers already exists. Call it for both names in apply_swift_test_ro...
- `swift_testing.test_traits`: closable now (small). Reproduced, but the recorded reason is partly wrong. It says the normalizer drops the macro argument list so traits reach no channel. In fact symbol_annotations.raw_text already keeps the full list (`Test(.disabled("f...

## toml (unverified)

| Impact | Domain | Gap | Summary |
| --- | --- | --- | --- |
| high | body_spans | `toml.body-spans-text-heuristic` | Table and key body spans come from a text heuristic, so most are missing and the rest point at the wrong bytes |
| high | relationships | `toml.cargo-dependency-forms-missing` | Common Cargo dependency forms emit no imports edge, or an edge to the wrong name |
| high | pending_relationships | `toml.manifest-cross-file-refs-no-pending` | pyproject entry points and Cargo workspace inheritance emit no pending relationships; the capability exception that says all TOML references are file-local i... |
| high | body_spans | `toml.table-body-span-heuristic` | Table and key body spans come from text heuristics: they are missing, partial, or point into the header |
| high | symbols | `toml.trailing-comment-replaces-value` | A trailing # comment on a key line is taken as the value |
| high | symbols | `toml.trailing-comment-shadows-value` | A trailing comment on a key/value line replaces the value in the signature, doc_comment, literal and structural facts |
| medium | relationships | `toml.cargo-dependency-forms-missed` | Cargo imports edges miss [workspace.dependencies], [dependencies.<name>], and target dev/build dependency tables, and use the wrong dependency name for dotte... |
| medium | relationships | `toml.cargo-feature-references` | Cargo feature lists and required-features reference other features and optional dependencies in the same file, but no edges are emitted |
| medium | literals | `toml.cli-drops-literals` | julie-extract writes zero TOML literal rows although the ledger claims literals are supported |
| medium | pending_relationships | `toml.entry-points-no-pending` | Entry-point strings that name Python functions emit no pending relationship; the 'no cross-document construct' exception is factually wrong |
| medium | doc_comments | `toml.leading-comments-not-doc` | `#` comments above keys and tables never become doc comments, and string values with escaped quotes are cut |
| medium | literals | `toml.literals-dropped-by-scan` | The CLI scan drops every TOML literal, but the ledger claims literals are supported |
| medium | relationships | `toml.pyproject-dependencies-no-imports` | pyproject and Poetry dependency declarations emit no imports rows, but Cargo dependencies do |
| medium | symbols | `toml.quoted-key-segment-corruption` | Table headers with a quoted segment lose one quote, which corrupts names, key paths, and literal carriers |
| medium | test_detection | `toml.test-roles-miss-real-schemas` | trycmd and nextest role detection matches an invalid trycmd shape and misses real ones |
| low | structural_facts | `toml.array-table-key-path-no-index` | Array-of-tables elements share one key_path, but inline arrays of tables get indexed paths |
| low | relationships | `toml.domain-key-references-unlinked` | Links between keys in the same file (Cargo features, Gradle version catalog refs) emit no references rows |
| low | doc_comments | `toml.leading-comment-docs` | Leading # comments that document a table or key never become doc_comment |
| low | other | `toml.pipfile-not-detected` | Pipfile, a TOML manifest with no extension, is not scanned |
| low | relationships | `toml.pyproject-self-loop-references` | pyproject files without a [project] table emit self-loop references edges |
| low | symbols | `toml.quoted-key-names-garbled` | Quoted and spaced table/key names are garbled or inconsistent, and an empty quoted key is dropped |
| low | structural_facts | `toml.string-value-decoding` | String values are not decoded: trimming removes content quotes, escapes stay raw, and multi-line strings have no style metadata |

Recorded open gaps assessed:

- `toml_multiline_string_semantics`: closable now (small). The cited task (docs/plans/2026-07-04-glm-review-findings-remediation.md, 'Task 7 follow-up: TOML multi-line string facts') has no section of its own. The plan only lists 'TOML multi-line strings' as deferred domain s...

## typescript (verified)

| Impact | Domain | Gap | Summary |
| --- | --- | --- | --- |
| high | relationships | `tsx.jsx-component-usage-no-edges` | JSX component usage (<Badge />, <UserCard />, <UI.Panel>) emits only call identifiers, never calls relationships or pending rows |
| high | symbols | `typescript.abstract-class-dropped` | abstract class declarations and abstract members produce no symbols (members orphaned, heritage lost); generic bases record the type-argument list as the base |
| high | doc_comments | `typescript.doc-comments-lost-on-exports-and-consts` | JSDoc on exported declarations lands only on the `export` row, and const/let declarations (including arrow-function consts) never get docs |
| high | symbols | `typescript.enum-initialized-members-dropped` | Enum members with an initializer or a string-literal name are dropped (every member of a string enum) |
| high | structural_facts | `typescript.express-receiver-detection-misses` | Express/Fastify routes are silent for exported or type-annotated app/router declarations and for multi-line route chains |
| high | pending_relationships | `typescript.typed-receiver-member-calls-dropped` | Member calls on typed receivers (this.field.m(), typed params and locals, this.m()/super.m() to inherited members) emit no relationship and no pending row |
| medium | structural_facts | `typescript.angular-route-and-component-facts` | Angular route tables and programmatic navigation (navigate('/x'), router.push('/x')) produce no frontend_navigation facts |
| medium | relationships | `typescript.calls-outside-function-symbols-lost` | Arrow-function class fields and object-literal members are not callables: calls inside them, and in variable or field initializers, produce no relationship; ... |
| medium | symbols | `typescript.destructuring-garbage-variables` | Destructuring declarations produce one variable named with the pattern source text |
| medium | symbols | `typescript.export-visibility-and-export-rows` | export const/let get no visibility and no export row; #private members are not private; re-exports collapse to one row named after the module path; aliases a... |
| medium | types | `typescript.inferred-type-facts-wrong-symbol` | Inferred type facts attach to the first same-named symbol (often the wrong one or an export row); functions get `function`/`Promise<any>` placeholders instea... |
| medium | relationships | `typescript.interface-extends-no-edges` | interface X extends A, B emits no extends relationship or pending row |
| medium | symbols | `typescript.missing-function-forms` | Generators, function expressions, anonymous default exports, overload/declare signatures, and `module`/`declare global` blocks produce no symbols |
| medium | symbols | `typescript.module-dependency-forms-missing` | import x = require(), side-effect imports, require() and dynamic import() produce no import rows |
| medium | symbols | `typescript.parameter-properties-not-class-members` | Constructor parameter properties (private readonly x: T) are not class members |
| medium | annotations | `typescript.property-parameter-decorators-not-annotations` | Decorators on class properties and parameters (@Column, @Input, @Inject, @Body) produce no annotation rows |
| medium | symbols | `typescript.type-literal-members-orphaned` | Members of object type literals are orphaned in type aliases and misattributed to functions, classes and interfaces from inline types |
| low | identifiers | `typescript.comment-text-false-receiver` | A comment that ends in 'word.' gives the next bare call a false receiver |
| low | pending_relationships | `typescript.pending-row-quality` | Garbage pending targets (super, curried calls, IIFEs, it.each tables) and missing import_context on instantiates/extends rows |
| low | source_regions | `typescript.string-keyword-fake-string-regions` | Every `string` type keyword is emitted as a string_literal source region |

Recorded open gaps assessed:

- `nextjs.signal_free_pages_router_files`: blocked (medium). Reproduced: a pages/ file with no next/* import and no getStaticProps/getServerSideProps/getStaticPaths/NextPage stays silent. nextjs_nuxt.rs:908 has_nextjs_page_signal already uses every in-file signal, and the scan ...
- `nextjs.signal_free_pages_router_files`: blocked (medium). Same gap and code path as the typescript row: TSX pages with a next/* import or a data-fetching export are routed; signal-free ones are silent to avoid React-SPA false positives (fixtures tsx/react_spa_pages). It need...

## vbnet (verified)

| Impact | Domain | Gap | Summary |
| --- | --- | --- | --- |
| high | body_spans | `vbnet.body-span-text-heuristic` | Body spans and body hashes come from a paren-matching text fallback, so they cover the parameter list and not the body |
| high | relationships | `vbnet.call-forms-without-invocation-node` | Null-conditional calls (o?.M()), With-block calls (.M()) and parenless call statements (conn.Open, Call X) emit no call identifier and no relationship |
| high | doc_comments | `vbnet.doc-comments-truncated-and-type-docs` | Multi-line ''' doc blocks keep only the last line, and type declarations never get a doc comment |
| high | relationships | `vbnet.member-body-calls-dropped` | Calls and New expressions inside property accessors, operators, custom event accessors and field initializers emit no relationship and no pending row |
| high | pending_relationships | `vbnet.qualified-new-target` | Namespace-qualified New A.B.C() instantiates the first namespace segment, and the expression form also emits a false calls row |
| high | identifiers | `vbnet.type-usage-identifiers` | type_usage identifiers exist only for Function return types and generic types; every other type position emits nothing |
| medium | structural_facts | `vbnet.aspnet-http-framework-facts` | VB ASP.NET attribute routes and HttpClient requests emit no framework facts, and JSON HttpClient carriers are missing |
| medium | structural_facts | `vbnet.attribute-fact-name-and-owner` | vbnet.attribute.v1 names qualified and target-prefixed attributes by their first segment and attaches type-level attribute facts to the wrong symbol |
| medium | symbols | `vbnet.block-scoped-locals` | For/For Each/Using/Catch variables get no symbols or type facts, and Using/Catch names are emitted as reads |
| medium | pending_relationships | `vbnet.chained-call-pending-garbage` | Pending targets for chained calls absorb argument and lambda identifiers into the receiver and namespace path |
| medium | symbols | `vbnet.friend-visibility` | Friend (explicit and default) maps to private; namespace-level default types map to public; Structure Dim fields map to private |
| medium | identifiers | `vbnet.index-access-as-call` | Indexing a local, parameter or field (arr(0), row("Name"), _items("k")) is emitted as a call identifier plus a pending call |
| medium | symbols | `vbnet.keyword-operator-symbols-missing` | Operators named by keywords (Not, And, IsTrue, IsFalse, Mod, CType) produce no symbol, and their parameters attach to the type |
| medium | relationships | `vbnet.mybase-call-self-edge` | MyBase.M() and MyBase.New() resolve to the caller's own method, creating false self-recursion edges |
| medium | types | `vbnet.return-type-facts` | No return type fact for Functions returning generic or array types, and stated return types are flagged inferred |
| medium | annotations | `vbnet.type-attributes-dropped` | Attributes on non-first types in a Namespace and on Module/Structure/Interface/Enum are lost; annotation raw_text drops the argument list |
| medium | relationships | `vbnet.type-relationship-wrong-source` | Inherits/Implements edges start from the first symbol with the type's name (for example a property), and same-file generic targets stay pending |
| low | types | `vbnet.dim-declarator-type-facts` | `Dim a, b As T` types only the last name, and `Dim x = New Foo()` records no fact when Foo is declared later in the same file |
| low | relationships | `vbnet.member-implements-handles-links` | Member-level Implements IJob.Run and Handles _timer.Tick emit no identifiers or relationships |
| low | complexity_metrics | `vbnet.ternary-complexity` | If(cond, a, b) and If(x, y) are not counted as decisions because the config names a node kind the VB grammar does not have |

## vue (verified)

| Impact | Domain | Gap | Summary |
| --- | --- | --- | --- |
| high | symbols | `vue.component-name-from-any-name-key` | Component symbol takes its name from the first `name: '...'` anywhere in any script |
| high | body_spans | `vue.options-api-name-only-spans` | Options API members span only their key: no body span or hash, no parent, zero complexity, identifiers contained by the component |
| high | symbols | `vue.plain-script-declarations-dropped` | Non-setup <script> drops imports, functions, classes, interfaces, types, enums and consts; the regex fallback emits `if`/`for` methods on the wrong line |
| high | relationships | `vue.relationships-attributed-to-component` | Script call relationships and pending calls always come from the component, not the enclosing function or method |
| high | symbols | `vue.script-setup-declarations-missing` | <script setup> skips interfaces, type aliases, enums, classes, uninitialised let, namespace imports, and names destructuring patterns as one symbol |
| high | crash_or_parse | `vue.sfc-section-splitter-line-based` | Line-based SFC splitter ends the template at the first nested </template> and drops one-line or multi-line section tags |
| high | identifiers | `vue.template-bindings-no-references-or-identifiers` | Template directive expressions (v-bind, v-if, v-for, v-show, v-model, v-on: long form, @event.modifier) give no references, and the template gives no identif... |
| medium | symbols | `vue.options-api-coverage` | Options API misses setup() bindings, lifecycle hooks, watch, array props, arrow data and extends/mixins, and treats nested config objects as component options |
| medium | pending_relationships | `vue.pending-targets-lack-receiver-and-import` | Script member-call pending rows drop the receiver; template component pending rows ignore the matching import and keep kebab-case names |
| medium | doc_comments | `vue.script-setup-doc-comments-missing` | <script setup> functions, arrow functions and consts never get their JSDoc |
| medium | symbols | `vue.script-setup-props-emits-members-missing` | defineProps / withDefaults / defineEmits / defineModel members are not symbols in <script setup>, although Options API props and emits are |
| medium | symbols | `vue.template-symbols-shadow-script-bindings` | v-model and ref template symbols duplicate script binding names and delete their template references |
| medium | types | `vue.type-facts-descriptor-garbage` | type_facts hold kind labels (vue-sfc, ref, variable, Method, Property, Event, css-rule, vue-macro) and miss every declared TS type |
| low | symbols | `vue.import-symbol-fidelity` | Namespace imports are dropped, aliased imports lose the imported name, and import sources and import_context keep their quote characters |
| low | crash_or_parse | `vue.lang-tsx-parsed-with-js-grammar` | <script lang="tsx"> is parsed with the JavaScript grammar, which loses TS declarations and emits wrong names and junk identifiers |
| low | symbols | `vue.one-based-columns-and-component-body-span` | Component, Options API and template symbols store 1-based columns; the component body_span runs from the first `{` to the last `}`; the component doc is plac... |
| low | source_regions | `vue.script-comment-regions-and-markers-missing` | Comments and string literals inside <script> produce no source regions and no code.marker facts |
| low | structural_facts | `vue.script-navigation-and-fetch-facts-missing` | Script-side router.push/replace, Nuxt navigateTo and Nuxt useFetch/$fetch emit no navigation or http.client_request facts |
| low | crash_or_parse | `vue.script-parse-diagnostics-missing` | Syntax errors inside <script> produce no parse diagnostics and later declarations are silently lost |
| low | symbols | `vue.scss-style-garbage-symbols` | <style lang="scss"> blocks go through the CSS grammar and emit garbage selector symbols and facts |

## xml (unverified)

| Impact | Domain | Gap | Summary |
| --- | --- | --- | --- |
| high | body_spans | `xml.body-span-text-heuristic` | Element body spans come from a brace/paren text heuristic, so any {..} in attributes or text yields a garbage body span and body hash |
| high | literals | `xml.literals-dropped-in-artifact` | All XML attribute-value literals are dropped from the artifact because xml has no literal carrier policy file |
| high | structural_facts | `xml.maven-pom-coordinates` | A Maven pom.xml emits no symbols or facts for coordinates, parent, modules, dependencies, plugins, or profiles |
| high | structural_facts | `xml.maven-pom-semantics-missing` | Maven pom.xml emits no dependency, module, parent, or plugin rows |
| high | symbols | `xml.msbuild-capitalized-name-attribute` | The MSBuild/slnx `Name` attribute never makes an element a symbol, so .csproj/.targets/.props files emit zero symbols |
| high | symbols | `xml.msbuild-capitalized-name-not-promoted` | MSBuild Target/UsingTask and slnx Folder declarations emit no symbols because the Name attribute is capitalized |
| high | structural_facts | `xml.msbuild-nuget-build-graph` | MSBuild/NuGet package refs, project refs, imports, properties, and target dependencies emit no rows; nuspec dependencies become fake declarations |
| high | structural_facts | `xml.msbuild-project-dependencies-missing` | csproj/props/nuspec/slnx package references, project references, imports, and target frameworks produce no rows |
| high | identifiers | `xml.qname-identifier-keeps-prefix` | XSD/WSDL type_usage identifiers keep the `tns:` prefix, so they never match the symbol they name |
| high | identifiers | `xml.qname-identifier-name-keeps-prefix` | Schema type_usage identifiers are named with the raw QName (tns:Address), so exact-name reference joins never match the declared symbol (Address) |
| medium | symbols | `xml.android-resources-and-components` | Android manifest class references become declaration symbols, layout ids keep '@+id/', and classes, handlers, and resources get no identifiers |
| medium | relationships | `xml.build-target-dependency-edges` | Ant depends/antcall and MSBuild DependsOnTargets/BeforeTargets/AfterTargets/CallTarget produce no call edges |
| medium | identifiers | `xml.code-class-references-missing` | Class names wired in Spring, Android, web.xml, TestNG, and MyBatis XML emit no identifiers; some become false declaration symbols |
| medium | symbols | `xml.config-key-entries` | The <add key=... value=.../> config entries emit no symbol and no config-key fact |
| medium | doc_comments | `xml.doc-comments-ignored` | xs:documentation, wsdl:documentation, and preceding XML comments never become doc comments; the ledger wrongly marks doc_comments not_applicable |
| medium | doc_comments | `xml.doc-comments-not-extracted` | xs:documentation, wsdl:documentation, and preceding <!-- --> comments never become doc_comment; ledger wrongly marks doc_comments not_applicable |
| medium | structural_facts | `xml.java-xml-framework-facts` | No framework facts for Spring bean wiring, web.xml servlet routes, or Android components and permissions |
| medium | symbols | `xml.kind-depends-on-child-elements` | Symbol kind flips between module and variable by child-element presence, and facts on self-closing declarations lose their symbol |
| medium | literals | `xml.literals-dropped-from-artifact` | Every XML literal is dropped from the published artifact because xml has no literal carrier policy |
| medium | structural_facts | `xml.mybatis-mapper-statements` | MyBatis mapper statements lose their mapper namespace and emit no SQL query facts or class identifiers |
| medium | identifiers | `xml.qname-reference-attributes-incomplete` | WSDL message=/binding= and XSD itemType/memberTypes/substitutionGroup QName references emit no identifiers |
| medium | identifiers | `xml.reference-attributes-missing` | WSDL message/binding and XSD itemType/memberTypes/substitutionGroup/refer QNames emit no type_usage identifiers |
| medium | relationships | `xml.schema-reference-edges` | XSD/WSDL QName references produce no references/extends relationships or pending relationships, even within one file |
| medium | identifiers | `xml.spring-bean-wiring` | Spring bean XML emits no identifiers for bean classes or bean refs, and no DI facts |
| medium | other | `xml.unclaimed-xml-extensions` | .proj, .runsettings, .xaml/.axaml, .xsl/.xslt, and app/web/nuget .config XML files are dropped as unsupported |
| medium | other | `xml.unrouted-xml-extensions` | Common XML file types are recorded as unsupported: .config, .xaml/.axaml, .xsl/.xslt, .vcxproj/.proj/.sqlproj, .runsettings, .xlf, .xhtml |
| medium | structural_facts | `xml.wsdl-inline-schema-and-endpoints` | WSDL inline <types><xs:schema> emits no XSD facts, port facts omit soap:address, and no fact records targetNamespace |
| low | relationships | `xml.build-target-dependencies` | Ant depends/antcall/default target references and ${prop} refs emit no identifiers or relationships |
| low | structural_facts | `xml.document-links-and-includes` | xi:include, the xml-stylesheet processing instruction, and xsi:schemaLocation emit no import or link facts |
| low | symbols | `xml.dtd-declarations-and-entity-refs` | Internal DTD element, attribute, and entity declarations emit no symbols; entity references and CDATA emit nothing |
| low | source_regions | `xml.mybatis-mapper-sql` | MyBatis mapper statements get no embedded SQL region, query fact, or resultMap reference |
| low | symbols | `xml.resx-boilerplate-noise` | Every .resx emits 16 boilerplate symbols from its embedded schema and resheaders, and resource entries are kinded module |
| low | test_detection | `xml.testng-suite-test-roles` | TestNG suite files get no test roles; only Ant junit is detected |
| low | structural_facts | `xml.webxml-servlet-routes` | web.xml servlet and filter mappings produce no symbols, route facts, or class identifiers |
| low | symbols | `xml.xsd-component-kinds` | XSD component kind depends on whether the element has child elements, so an empty complexType is a variable and its schema fact loses its owner |

Recorded open gaps assessed:

- `references`: closable now (medium). Recorded reason is 'no namespace resolution in the XML tier', which is a scope choice, not a blocker: identifiers.rs already scans every xmlns binding. Closure: keep a prefix->URI map and the schema/definitions target...
- `relationships`: closable now (medium). Same closure as kind_coverage.relationships.references: same-document QName resolution is fully inside the extractor and needs no code-kb join. Once references/extends edges are emitted with golden plus negative evide...
- `pending_relationships`: closable now (medium). Cross-document refs are the extractor's job to emit as structured pending rows, which code-kb resolves by terminal name at query time. Closure: for QNames whose prefix maps to a namespace other than the targetNamespac...

## yaml (unverified)

| Impact | Domain | Gap | Summary |
| --- | --- | --- | --- |
| high | body_spans | `yaml.brace-heuristic-body-spans` | Text-heuristic body spans give YAML keys garbage body spans and shared body hashes |
| high | structural_facts | `yaml.ci-workflow-semantics` | GitHub Actions, GitLab CI, and Azure Pipelines files give no job-dependency edges, no action or include references, and no workflow facts |
| high | relationships | `yaml.cloudformation-intrinsic-refs` | CloudFormation !Ref / !GetAtt / DependsOn to resources in the same template emit no identifiers or references |
| high | doc_comments | `yaml.doc-comments-missed` | Comment lines directly above a key are not attached for the first key in a mapping, a key after a nested block, multi-line comments, or container keys |
| high | body_spans | `yaml.heuristic-body-spans` | YAML key symbols get garbage body spans and body hashes from brace and parenthesis text heuristics |
| high | pending_relationships | `yaml.json-schema-ref-not-extracted` | $ref in YAML OpenAPI/JSON Schema emits no relationship, pending row, or ref fact (JSON emits all three) |
| high | literals | `yaml.literals-dropped-in-artifact` | Every YAML literal is dropped before the artifact write because yaml has no literal carrier policy |
| high | relationships | `yaml.schema-ref-relationships` | OpenAPI and JSON Schema '$ref' in YAML gives no references edge, no pending row, and no ref fact (JSON handles the same document) |
| high | symbols | `yaml.sequence-mapping-items-flattened` | A key whose value is a sequence of mappings has kind variable, and all item keys flatten under it with no grouping per item |
| medium | relationships | `yaml.alias-binds-first-anchor` | An alias resolves to the first anchor with its name, even across documents or after the anchor is redefined |
| medium | relationships | `yaml.anchor-alias-resolution` | Scalar and sequence-item anchors are not recorded, and alias resolution ignores document scope and anchor redefinition |
| medium | structural_facts | `yaml.block-scalar-tag-multidoc` | Block scalar style, explicit tags, and per-document paths are not emitted (recorded open gap, reproduced) |
| medium | structural_facts | `yaml.ci-compose-dependency-edges` | No framework facts or edges for GitHub Actions, GitLab CI, Docker Compose, or Kustomize references |
| medium | doc_comments | `yaml.doc-comments-dropped` | Multi-line comments, comments after a nested block, and comments above container keys never become doc comments |
| medium | structural_facts | `yaml.docker-compose-semantics` | Docker Compose files give no service facts, no depends_on edges, and no extends pending rows |
| medium | symbols | `yaml.flow-mapping-keys-not-symbols` | Keys inside flow mappings ({a: 1}) are not symbols, and the owning key has kind variable |
| medium | symbols | `yaml.flow-mapping-pairs-no-symbols` | Flow-style mapping pairs make no symbols; a JSON-style .yml file makes zero symbols |
| medium | structural_facts | `yaml.key-path-not-unique` | key_path drops sequence indices and does not escape keys that contain dots |
| medium | structural_facts | `yaml.kubernetes-resource-facts` | Kubernetes manifests give no resource identity per document, and multi-document files give indistinguishable root symbols |
| medium | symbols | `yaml.leaf-value-not-in-signature` | Leaf YAML keys have no signature or other value carrier (TOML emits `key = value`) |
| medium | literals | `yaml.literals-dropped-from-artifact` | YAML config literals are built in-process but never reach the SQLite artifact |
| medium | structural_facts | `yaml.openapi-operation-route-facts` | OpenAPI operations (path plus HTTP method) give no route facts |
| medium | structural_facts | `yaml.openapi-operations-no-route-facts` | OpenAPI `paths` operations emit no route or operation facts |
| medium | relationships | `yaml.scalar-flow-value-anchor-missed` | Anchors on scalar or flow values (`key: &a 30`, `&a [..]`, `&a {..}`) are not detected, so aliases to them stay unresolved |
| low | identifiers | `yaml.alias-owner-too-coarse` | An alias under a leaf key is attributed to the enclosing module, not to the key that holds it |
| low | pending_relationships | `yaml.ansible-semantics` | Ansible playbooks: import_playbook, include_tasks, and roles give no pending rows, and notify gives no handler reference |
| low | test_detection | `yaml.cst-non-command-tests-no-roles` | container-structure-test file, content, license, and metadata tests get no test roles; test-case symbols are all named `name` |
| low | source_regions | `yaml.embedded-shell-regions` | Shell and PowerShell script blocks under run, script, powershell, and bash keys are string_literal regions, not embedded regions |
| low | symbols | `yaml.key-name-decoding` | Quoted keys are not unescaped, and non-scalar or alias keys take the value's text as the symbol name |
| low | symbols | `yaml.sequence-container-kinded-variable` | Keys whose value is a sequence of mappings (steps, tasks, containers) or a flow mapping are kinded variable but own child symbols |
| low | structural_facts | `yaml.sequence-item-key-path-collision` | key_path omits sequence indices, so keys in different list items share one path |
| low | test_detection | `yaml.test-roles-yaml-suites` | container-structure-test file tests and Tavern API tests get no test roles |

Recorded open gaps assessed:

- `yaml.block_scalar_tag_and_multidoc_semantics`: closable now (medium). This work is purely local to the extractor, and nothing blocks it on code-kb or cross-file joins. The grammar exposes every node needed: block_scalar (the text starts with the '|'/'>' indicator and the chomping mark),...

## zig (verified)

| Impact | Domain | Gap | Summary |
| --- | --- | --- | --- |
| high | symbols | `zig.assignment-statements-emit-variable-symbols` | Assignment statements and `_ = x;` become variable symbols; destructuring keeps only the first name |
| high | pending_relationships | `zig.call-on-call-result-no-pending` | Calls on a call result and decl-literal calls (`.init(...)`) produce no pending relationship |
| high | relationships | `zig.call-target-uses-first-identifier-argument` | Method and qualified calls take the first bare-identifier argument as the callee |
| high | relationships | `zig.decltest-name-collides-with-target` | A test named after a function makes every same-file call to that function ambiguous |
| high | types | `zig.function-return-type-missing` | Signatures say `void` for named, qualified and generic return types, and functions get no return type fact |
| high | relationships | `zig.negated-call-edges-lost` | Calls under logical not (`!f()`, `!self.m()`) lose their call edge and emit false type_usage rows |
| high | types | `zig.qualified-generic-types-no-type-facts` | Declared types written as qualified names or qualified generics (`std.mem.Allocator`, `*httpz.Request`, `std.ArrayList(T)`) record no type fact |
| medium | symbols | `zig.enum-and-error-set-members` | Enum values are emitted as `field` and error-set members emit no symbols |
| medium | body_spans | `zig.garbage-body-spans` | Extern prototypes and variables get parameter lists or argument fragments as bodies |
| medium | types | `zig.self-alias-receiver-type` | `const Self = @This()` receivers get type `Self` and no receiver_type; file-as-struct self calls stay unresolved |
| medium | symbols | `zig.text-heuristics-miskind-containers` | Substring checks on declaration text turn structs into imports or functions and values into function types |
| medium | identifiers | `zig.type-usage-missed-in-wrapped-type-positions` | Types inside ?T, []T, [N]T, generic arguments and struct literals are variable_ref; composition edges attach to the wrong struct |
| low | structural_facts | `zig.builtin-call-args-dropped` | Builtin calls passed as direct call arguments emit no builtin_call fact |
| low | doc_comments | `zig.container-doc-comments` | `//!` container doc comments are classified as plain comments |
| low | identifiers | `zig.enum-literal-garbage-receiver` | Enum and decl literals (`.info`, `.empty`) record the preceding keyword or label as their receiver |
| low | test_detection | `zig.false-test-roles-from-generic-heuristic` | Test-prefixed helper functions in test directories are flagged as test cases |
| low | annotations | `zig.ffi-annotations-incomplete` | Bare extern, noinline and callconv are not recorded, exported vars are private, and align(...) inside a value becomes an annotation |
| low | structural_facts | `zig.framework-structural-facts` | No facts for the build.zig build graph, http.zig routes or std.http.Client requests |
| low | symbols | `zig.opaque-types-mis-extracted` | opaque types become constants with function-kind members, and empty containers emit an empty-named field |
| low | symbols | `zig.variable-signature-ignores-declared-type` | var/const signatures show 'inferred' or the value instead of the declared type |

