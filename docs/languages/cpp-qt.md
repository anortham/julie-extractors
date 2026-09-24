# Qt C++ support

Julie extracts Qt C++ with the ordinary `cpp` registration. There is no separate
Qt language: a `.h`, `.hpp`, or `.cpp` file that uses Qt's macros is a C++ file,
and the Qt evidence is carried by extra symbol rows, extra metadata keys, and one
structural-fact pattern.

Qt's macros are preprocessor names. The C++ grammar cannot parse them, so before
this release a Qt class was mostly an error node: `columnview.h` in Kirigami
reported 92 parse diagnostics and no usable symbol table. A macro pre-pass now
blanks those macros before the parse, on both parse paths.

## The macro pre-pass

`cpp/qt_macros.rs::blank_macros` rewrites the source; `preprocess.rs::blanked_source`
is the single entry point both paths call. The scan is token-level and never uses a
tree. There are four rewrite rules.

**Statement.** A macro that owns the rest of its line becomes spaces, together with
its balanced argument list, which may span lines.

```cpp
Q_PROPERTY(int index READ index WRITE setIndex NOTIFY indexChanged FINAL)
```

becomes

```cpp

```

(the same byte count, all spaces).

**Prefix.** A macro that stands in front of a declaration becomes spaces, and the
declaration is left alone.

```cpp
Q_INVOKABLE void reset();
```

becomes

```cpp
             void reset();
```

**Section.** A bare `Q_SIGNALS:` or `signals:` label becomes a padded `public:`,
because everything in a Qt signals section is public. A bare `Q_SLOTS:` or `slots:`
becomes spaces. A prefixed `public Q_SLOTS:` keeps its access word and its colon.

```cpp
Q_SIGNALS:
    void indexChanged();
```

becomes

```cpp
public:
    void indexChanged();
```

**Export.** An export macro between a type introducer and the name becomes spaces.

```cpp
class KIRIGAMI2_EXPORT ColumnViewAttached : public QObject
```

becomes

```cpp
class                  ColumnViewAttached : public QObject
```

### The byte-length and newline invariant

The rewritten text has exactly the same byte length as the original, and every
`\n` and `\r` stays where it was. A byte offset and a line number therefore still
address the original text, which stays the source of record for every extractor:
a symbol's range, a doc comment, and a source region all read the real file.

### Excluded regions

The scan never rewrites a string literal, a character literal, a raw string
literal with any encoding prefix, a `//` comment including its backslash
continuation, a `/* */` comment, or a preprocessor line including its
`\`-continued lines. A macro used mid-line, such as `Q_ARG(bool, true)` inside a
`QMetaObject::invokeMethod(...)` call, is left alone, because a mid-line rewrite
would delete real arguments.

### The check path

`julie-extract check` blanks the same way. `syntax/mod.rs` and
`pipeline.rs::parse_for_language` both call `blanked_source`, and
`language_spec/source_detection.rs::detect_with_probe` blanks before the header
probe, so a `.h` file is detected from the blanked text as well. Without the last
one a Qt header never reaches the blanked source, because the probe's pre-parsed
tree is what every later stage uses.

## Qt rows in symbols

### Property rows

Every `Q_PROPERTY(...)` in a class or struct body is a `property` symbol at the
macro's own range, named by the property, parented to the innermost class or
struct, visibility public. Its signature is the whitespace-collapsed macro text,
so a multi-line macro reads as one line.

| Metadata key | Type | Source |
| --- | --- | --- |
| `property_type` | string | the declared type, `int` in `Q_PROPERTY(int index ...)` |
| `read` | string | the `READ` accessor |
| `write` | string | the `WRITE` accessor |
| `notify` | string | the `NOTIFY` signal |
| `member` | string | the `MEMBER` field |
| `reset` | string | the `RESET` method |
| `bindable` | string | the `BINDABLE` accessor |
| `designable` | string | the `DESIGNABLE` value or predicate |
| `scriptable` | string | the `SCRIPTABLE` value or predicate |
| `stored` | string | the `STORED` value or predicate |
| `user` | string | the `USER` value or predicate |
| `revision` | string | the `REVISION` number or tuple |
| `constant` | bool | `CONSTANT` is present |
| `final` | bool | `FINAL` is present |
| `required` | bool | `REQUIRED` is present |

A key is absent when the macro does not name it. A `//` or `/* */` comment inside
the argument list is stripped before the keys are read; the row's range still
covers the original bytes.

### Events, slots, and invokables

- A method declared in a `signals:` or `Q_SIGNALS:` section, or prefixed by
  `Q_SIGNAL`, is an `event` row, not a `method` row. Its visibility is public.
- A method in a `slots:` or `Q_SLOTS:` section, or prefixed by `Q_SLOT`, carries
  `qt_slot: true`.
- A method prefixed by `Q_INVOKABLE` carries `qt_invokable: true`. A method can
  carry both.

A member's section is the last section site or access label before it inside its
own class body. A section inside a nested class does not reach the outer class's
members.

### Class metadata

| Key | Set by | Value |
| --- | --- | --- |
| `qt_object` | `Q_OBJECT` | `true` |
| `qt_gadget` | `Q_GADGET` | `true` |
| `qml_element` | `QML_ELEMENT`, `QML_NAMED_ELEMENT(X)` | the class name, or `X` |
| `qml_singleton` | `QML_SINGLETON` | `true` |
| `qml_anonymous` | `QML_ANONYMOUS` | `true` |
| `qml_uncreatable` | `QML_UNCREATABLE("reason")` | the reason text |
| `qml_attached` | `QML_ATTACHED(X)` | `X` |

A macro outside a class body sets nothing.

## Structural facts

Each `Q_PROPERTY` site also emits one `cpp.qt_property.v1` fact, with
`capture_name` `qt_property`, `node_kind` `macro`, `containing_symbol_id` set to
the enclosing class or struct symbol, and the same metadata keys as the property
row. Facts are emitted at `--level facts` and above; a `--level symbols` scan
emits none.

## Other C++ fixes in 3.3.0

These are not Qt-specific. They were found on Qt headers because the headers had
never parsed before, and they apply to every C++ file.

- A forward declaration such as `class ColumnView;` emits no `class` row. It names
  no new type; only a definition with a member list does.
- A declared constructor or destructor emits one row, not two. The declaration and
  its inner declarator no longer both produce a row at the same line.
- A constructor or destructor row carries the visibility of its access section.
  Both were hardcoded `public` before, so a `private:` constructor was wrong.
- A destructor signature no longer ends in `;`.
- A method declared in a class keeps its whole declared return type: the `const`
  qualifier, one `*`, `&`, or `&&` per pointer or reference wrapper, template
  arguments, and namespace qualifiers. A member reads `QQuickItem *contentItem() const`,
  `const QString &name() const`, `QList<int> *items()`, and
  `static ColumnViewAttached *instance()`.
- `override` and `final` are trailing specifiers, written after the parameters and
  the `const` qualifier. They were leading modifiers before.
- A modifier no longer leaks from a sibling member. The signature builder read the
  declaration's grandparent, which is the class body, so one member's `explicit`,
  `static`, `virtual`, or `override` became a modifier on every method of the
  class.

### Known limits

- `Q_DECLARE_FLAGS(Modes, Mode)` and a `Q_OBJECT_BINDABLE_PROPERTY(...)` member are
  statement macros. They are blanked, so the typedef and the member emit nothing.
- Lowercase `signals:` and `slots:` are recognized only at class-member depth, so
  ordinary function labels retain their C++ meaning.
- A lowercase `emit` at the start of a line, followed by an identifier, is blanked.
  A variable named `emit` in that position would be blanked too.
- A macro used mid-line, such as `Q_ARG(bool, true)`, is untouched by design.
- The macro pre-pass is deliberately limited to declaration syntax. Runtime
  expression macros such as `Q_ASSERT(condition)` remain in the parsed source so
  calls and identifiers inside their arguments are retained. `Q_UNUSED(value)`
  is normalized to its argument expression plus a semicolon when needed, so it
  also preserves nested calls while accommodating the macro's optional semicolon.
- `Q_ENUM(Mode)` is blanked and marks nothing on the enum. The enum and its members
  are extracted as ordinary C++ rows.
- `= 0`, `= default`, and `= delete` on a method are not part of its signature. A
  constructor or destructor keeps them.
- Diagnostics remain on both corpora: 31 on Kirigami and 298 on plasma-workspace.
  None is macro-shaped. The top causes are tree-sitter-cpp grammar limits and
  project-local macros: a file-local `#define` used as a statement
  (`src/imagecolors.cpp`, 12 of Kirigami's 31), a default argument written `= {}`
  (`src/platform/platformpluginfactory.h`), `decltype(...)` in a parameter list or
  a member initializer, a lambda with an explicit trailing return type,
  `namespace A::B`, `#if __has_include(...)`, a bare `return {`, and `%{APPNAMEUC}`
  placeholders in a project template.

## Declarations and scopes

- A declarator is read the way C++ binds it. The name is the innermost
  identifier, so `Widget* w`, `Widget& w`, `int w[4]`, and `void (*w)(int)` are
  variables or fields named `w`. A declaration emits one row per name, including
  each name of `auto [key, value]`.
- A prototype takes its name from its declarator, not from a qualified return
  type: `std::optional<int> find(int id) const;` is the method `find`.
- In a function body, `Writer writer(out, options_);` is a variable. A declarator
  whose parameters are all bare type names reads as a direct initialization there.
- An out-of-line definition keeps its written name, such as `Widget::draw`, and
  records `scope` metadata. `X::X` is a constructor, `X::~X` a destructor,
  `X::operator==` an operator, and any other `X::m` a method. When the owner is
  defined in the file, the row is its child and takes the visibility of the
  in-class declaration. `int X::count = 0;` and `X::operator bool() {}` emit rows.
- An `auto` variable (`auto`, `auto*`, `auto&`, `const auto&`,
  `decltype(auto)`) gets an inferred type fact from its initializer: a
  same-file class built by `Foo()` or `new Foo()`, or a call to a same-file
  callable with a stated or trailing return type. Accepted callees are a free
  function (`load()`), a member of the enclosing class (`load()`,
  `this->load()`, `(*this).load()`), and a qualified member (`Type::create()`,
  `ns::Type::create()`). Same-named candidates must agree on the base type.
  Classes are matched by full identity (namespace path and enclosing classes),
  so a same-named class in another namespace never answers. A qualifier
  resolves from the call's scope outward and must name that exact same-file
  class. A member of a union, of an unnamed class, or of a template
  specialization is never taken for a free function. A class defined twice, or
  `W<int>::create()` when the file specializes `W`, records no fact. A
  using-declaration that brings a base-class overload of the name into the
  class records no fact. An out-of-line method of a class defined in another
  file sees only the members this file defines out of line with the same
  qualifier in the same namespace; any other unqualified call there records no
  fact. `std::unique_ptr<Foo>` and `std::optional<Foo>` stay as written. A
  deduced return type, or one that names a template parameter anywhere (`T`,
  `Box<T>`, `-> std::vector<T>`), records no fact. A call on any other
  receiver or a chained call also records no fact. A same-file friend
  function of the callee's name (a hidden friend or a friend declaration)
  records no fact, since argument-dependent lookup may pick it. A `#define`
  anywhere in the file of the callee name, a qualifier name, or a return type
  name records no fact. A namespace alias counts as a declared name, so
  `a::Maker::create()` where `namespace a = ::b;` is in scope records no
  fact. Each first name the return type writes (`A` and `C` in
  `A::Node<C>`) must mean the same entity at the call as at the callee:
  a nested type or alias of the callee's class (`Node`, `Ptr`), a name a
  nearer namespace or a local declaration at the call redeclares, or a name
  that a class with an unseen base stops, records no fact. A name no
  same-file scope declares passes only when the callee's namespace encloses
  the call. Inside an out-of-line method of a class defined in another file,
  a member's return type name therefore records no fact, and a primitive
  return type (`int`) still does. A function declared in a block records no
  fact. An
  unqualified call records no fact when a parameter, template parameter,
  lambda capture, or local declaration of an enclosing function or block binds
  the same name. Inside a method, an unqualified call falls back to a free
  function only when the class is defined once in the file at top level with
  no base class and declares no member of that name (a field, a function
  pointer, a type alias, a nested type, or an enumerator). A free function
  must be declared in the call's namespace or an enclosing one, and the
  innermost such namespace that declares the name decides; a non-function name
  there, or a same-file function of that name in any other namespace, records
  no fact. Anonymous and inline namespaces count as their enclosing namespace.
  A structured binding (`auto [x] = f();`) records no type fact; each of its
  names, also in `const auto& [a, b]`, gets a variable row. Plain `auto` drops
  a reference from the declared text; `decltype(auto)` keeps it. Callees in
  other files are out of scope because each file is extracted alone.
- An unknown macro before a return type, such as
  `JSON_HEDLEY_WARN_UNUSED_RESULT static basic_json diff(...)`, makes the
  parser report the macro as the type. That callable records no return type
  fact, and `auto` calls to it record none.
- `namespace a::b::c {}` emits one namespace row named `a::b::c`.
  `class Outer::Inner {}` emits a class row `Inner` under `Outer`, and a
  specialization `struct hash<Point> {}` emits a struct row `hash`.
- `using Handler = ...;` and an alias template emit `type` rows.
- A base class emits an `extends` edge, or a pending `extends` with the
  namespace of a qualified base when the base is not in the file.
- A visibility macro between the class key and the class name, such as
  `class ENGINE_API Renderer : Base {`, is blanked like an `*_EXPORT` macro.
- A Catch2 `TEST_CASE` or `SECTION` row spans its block and owns it, so locals,
  nested sections, and calls in the block belong to the test. doctest
  `TEST_SUITE`, `SUBCASE`, and `TEST_CASE_FIXTURE` follow the same rule.
- In a file with `QTEST_MAIN`, `QTEST_GUILESS_MAIN`, or `QTEST_APPLESS_MAIN`,
  the named class is a test container. Its private slots, and their out-of-line
  definitions, are tests: `initTestCase` and `init` are fixture setup,
  `cleanupTestCase` and `cleanup` are fixture teardown, a `name_data` slot has
  no role, and a slot with a `name_data` partner is a parameterized test.
- `BOOST_AUTO_TEST_CASE(name)` and `BOOST_FIXTURE_TEST_CASE(name, Fixture)` are
  test rows named `name`. `BOOST_AUTO_TEST_SUITE(name)` is a container row.

## Continuous testing

Unit tests:

- `crates/julie-extractors/src/tests/cpp/qt_macros.rs` covers the four rewrite
  rules, the excluded regions, and the check path.
- `crates/julie-extractors/src/tests/cpp/qt_symbols.rs` covers the property rows,
  the events, the slot and invokable metadata, and the class metadata.

The check-path tests need the `syntax-api` feature, which the language target does
not enable:

```bash
cargo test -p julie-extractors --features syntax-api --lib tests::cpp::qt_
```

The golden fixture is `fixtures/extraction/cpp/qt_header`, a Kirigami-style header
with `Q_OBJECT`, three `Q_PROPERTY` lines, `Q_SIGNALS:`, `public Q_SLOTS:`,
`Q_INVOKABLE`, `QML_ELEMENT`, `QML_ATTACHED`, an export macro, a forward
declaration, and four declared return types.

Gate commands:

```bash
cargo xtask test language cpp
cargo xtask test golden
cargo xtask test capability
cargo xtask test contract
```

## Real-world evidence

Two KDE corpora, scanned at `--level facts` with the 3.3.0 release binary and with
the published 3.2.0 binary. Both were cloned shallowly; no project build script,
hook, or third-party binary was executed. Both checkouts are multi-licensed
(LGPL-2.0-or-later, LGPL-2.1/LGPL-3.0 with `LicenseRef-KDE-Accepted-LGPL`,
GPL-2.0-or-later, and Qt commercial exception expressions); treat source
redistribution as subject to the per-file license metadata.

| Measure | Kirigami `ca7d636` 3.2.0 | Kirigami 3.3.0 | plasma-workspace `a45871a` 3.2.0 | plasma-workspace 3.3.0 |
| --- | ---: | ---: | ---: | ---: |
| C++ files | 102 | 102 | 1,051 | 1,051 |
| Files with parse diagnostics | 70 | 12 | 649 | 77 |
| Parse diagnostics | 981 | 31 | 4,627 | 298 |
| Rows named `Q_PROPERTY` | 294 | 0 | 786 | 0 |
| `property` rows | 0 | 297 | 0 | 920 |
| `cpp.qt_property.v1` facts | 0 | 297 | 0 | 920 |
| `event` rows | 0 | 196 | 0 | 1,060 |
| Empty-name rows | 5 | 0 | 28 | 0 |
| Forward-declaration `class` rows | 57 | 0 | 653 | 0 |
| Duplicate constructor or destructor rows | 43 | 0 | 493 | 0 |
| Signatures starting `override ` | 412 | 0 | 2,365 | 0 |

Diagnostics fall 97% on Kirigami and 94% on plasma-workspace.

The plan's acceptance file is Kirigami `src/layouts/columnview.h`. At `ca7d636` it
reports **0 parse diagnostics** (it reported 92 under 3.2.0), **38 `property`
rows** for its 38 `Q_PROPERTY` lines, 37 `event` rows for its signals, and **one
row per declared constructor and destructor** (3 of each). It has 3 `class` rows,
all definitions, and no forward-declaration row. `qt_slot` and `qt_invokable` are
set on the expected members, and its method signatures read `ColumnView *view()`
and `QQuickItem *originalParent() const`.
