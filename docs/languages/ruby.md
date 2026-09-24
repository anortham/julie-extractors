# Ruby support

Julie registers one Ruby language: `ruby` handles `.rb`, `.rbw`, `.rake`,
`.gemspec`, `.ru`, `.jbuilder`, `.builder`, and `.thor` files. The
extensionless files `Gemfile`, `Rakefile`, `Guardfile`, `Capfile`,
`Vagrantfile`, `Brewfile`, `Podfile`, `Fastfile`, `Appfile`, `Dangerfile`,
`Berksfile`, and `Thorfile` are Ruby by exact file name.

## Continuous testing

Run the language target when changing Ruby extraction:

```bash
cargo xtask test language ruby
```

The command runs the Ruby unit-test modules and the golden extraction test
with `JULIE_GOLDEN_LANGUAGE=ruby`. The normal golden target stays unfiltered:

```bash
cargo xtask test golden
```

## Test-role contract

Ruby names no test construct with syntax. Every case, hook, and suite is an
ordinary method call or an ordinary class, so the detector reads three
frameworks and guards each rule three ways.

| Idiom | Role | Source of the rule |
| --- | --- | --- |
| `describe`, `context`, `feature` block | `test_container` | RSpec example group |
| `xdescribe`, `xcontext`, `fdescribe`, `fcontext` block | `test_container` | RSpec skip and focus aliases |
| `shared_examples`, `shared_examples_for`, `shared_context` block | `test_container` | RSpec shared example group |
| `it`, `specify`, `example`, `scenario` block | `test_case` | RSpec example |
| `xit`, `fit`, `xspecify`, `fspecify`, `xexample`, `fexample` block | `test_case` | RSpec skip and focus aliases |
| `before` block | `fixture_setup` | RSpec hook |
| `after` block | `fixture_teardown` | RSpec hook |
| `around` block | `fixture_setup` | RSpec wrapping hook |
| `let`, `let!`, `subject`, `subject!` block | `fixture_setup` | RSpec helper method |
| `test "name" do` | `test_case` | Rails `ActiveSupport::TestCase` macro |
| `setup do`, `teardown do` | `fixture_setup`, `fixture_teardown` | Rails block-form callbacks |
| `def setup`, `def teardown` | `fixture_setup`, `fixture_teardown` | Minitest and Test::Unit hooks |
| `def test_x` | `test_case` | Minitest and Test::Unit collection prefix |
| class with a `Minitest::Test`, `Test::Unit::TestCase`, `ActiveSupport::TestCase`, `ActionDispatch::IntegrationTest`, or Rails component test case base | `test_container` | the collected base classes |
| class in a test path whose base is named `*TestCase` or `*Test` | `test_container` | application bases such as `ApplicationSystemTestCase` |

The Rails component test cases are `ActionDispatch::SystemTestCase`,
`ActionController::TestCase`, `ActionMailer::TestCase`,
`ActionMailbox::TestCase`, `ActionView::TestCase`, `ActiveJob::TestCase`,
`ActionCable::TestCase`, `ActionCable::Connection::TestCase`,
`ActionCable::Channel::TestCase`, and `Rails::Generators::TestCase`.

### The three guards

A role needs all three guards, not one of them.

- **Path.** The file must read as a test path. `_spec.rb`, `_test.rb`, a
  `spec/` or `test/` directory, and the other shared rules all qualify. Without
  this guard a production `before do` block or a `def setup` would carry a role.
- **Receiver.** A recognised call must be bare or sent to `RSpec` itself.
  `RSpec.describe Order do` is a suite; `runner.describe "x" do` is an ordinary
  message to an object that answers `describe`.
- **Container.** A callable must sit inside a test container. RSpec blocks are
  containers themselves. A Minitest-family suite is found through the
  `base_types` metadata the class extractor emits. A `def setup` in a
  spec-directory support class therefore earns no role, and neither does a
  top-level `def test_x` in a spec file, which no runner collects.

`fixtures/extraction/ruby/test_roles/production_roles.rb` is the production
control. It carries `def setup`, `def teardown`, `def test_connection`, a bare
`describe`, a bare `before`, a bare `after`, a bare `test` macro, and a bare
`let`, and it publishes no role at all.
`fixtures/extraction/ruby/test_roles/minitest_class_test.rb` carries
`FixtureBuilder` as the in-test-path control: a class with `setup` and
`test_rows` members and no collected base class.

### `around` reports setup

An RSpec `around` hook wraps the example on both sides, so its true direction
is "both". The extractor cannot split a wrapping hook without reading the
body, and the setup half always runs first, so the contract publishes the
single honest direction: `fixture_setup`. Ruby is the first supported language
to use that direction.

### The method field, not the first identifier

`extract_method_name_from_call` reads the grammar's `method` field. Scanning a
call node for its first `identifier` child returns the *receiver* of
`receiver.method`, because the receiver comes first in the child order. That
older rule reported `ordinary` for `ordinary.it "x"`, which happened to keep
the member call out of the test roles — for the wrong reason, and it named
every member call after its receiver everywhere else in the Ruby extractor.

Two channels changed with the fix. A call whose method is a constant, such as
Ruby's `Kernel#URI("...")`, now emits a call relationship. And
`include`/`extend`/`prepend`/`using` now require a bare or `self` receiver
before they count as a mixin on the enclosing class, which keeps
`other.include Formatting` out of that class's mixin list.

## Symbols and calls

- A constant defines a symbol only as an assignment target (`FOO = 1`) or a
  class or module name. A constant read in `when`, `rescue`, or a value
  position is a `type_usage` identifier. A global variable defines a symbol
  only as `$g = ...`.
- `obj.attr = x` and `h[k] = x` call writer methods. They define no symbol;
  `self.mode = x` gives a pending call to `mode=`.
- A compact declaration `class Api::V1::Base` is named `Base`. Its signature
  keeps the written name, and `metadata.qualifiedName` holds
  `Api::V1::Base`. A same-file superclass resolves by name or qualified name.
- Call targets come from the `call` node fields. The terminal is the method
  name. The receiver is the receiver text, or the method name of a call
  receiver, so `Mailer.with(to: x).receipt` has receiver `with`. Argument and
  block text never reach a target.
- A bare identifier with no earlier binding in its scope is a receiverless
  method call, which is Ruby's own parse rule. It gives a `call` identifier
  and a `calls` relationship to a same-class method, or a pending call. A
  local, parameter, block parameter, or rescue variable stays a
  `variable_ref`. `def`, `class`, `module`, and the program start a scope; a
  block sees the enclosing scope.
- `send(:m)`, `public_send(:m)`, `__send__(:m)`, `method(:m)`, and `&:m` call
  `m`. `super` gives a pending call to the enclosing method's name with
  receiver `super` and the declared superclass as `receiver_type`.
- A DSL call that declares its own symbol, such as `test "x" do` or
  `setup do`, is that declaration and gives no call row.
- Each `class`, `module`, and `class << self` body starts public. A bare
  `private` does not reach `def self.x`. `private :a, :b`, `protected :a`,
  `public :a`, `private_class_method :m`, `private attr_reader :x`, and
  `private def x` change the named members, wherever the call sits in the
  class body.
- `def self.x`, `def self.x=`, `def self.[]`, and methods in a `class << self`
  body are methods of the enclosing class with `metadata.isStatic = true`.
  No symbol is emitted for the `class << self` block itself.
- `alias`, `alias_method`, `define_method`, `define_singleton_method`,
  `def_delegator(s)`, and `delegate` define methods of the enclosing class.
  `has_many`, `has_one`, `belongs_to`, and `has_and_belongs_to_many` define
  properties; `scope :name` defines a class method.
- `X = Class.new(Base) do ... end`, `X = Struct.new(:a)`, and
  `X = Data.define(:a)` declare a class. Block methods belong to it, `Base`
  is its superclass, and the symbol arguments are properties.
- Callback and dispatch symbols call the named method: `before_*`,
  `after_*`, `around_*`, `skip_*`, `prepend_*`, `append_*`, `validate`,
  `helper_method`, the `with:`/`if:`/`unless:` options of callbacks,
  `rescue_from`, and `validates`.
- A superclass constant resolves lexically: `class Card < Base` inside
  `module Payments` prefers `Payments::Base`. Every argument of
  `include A, B` gives an `implements` row. A computed superclass such as
  `Struct.new(...)` gives no edge.
- A local variable is defined once, at its first assignment. `x += 1` on a
  bound local is a `variable_ref`. Instance, class, and global variable reads
  are `variable_ref` identifiers; the declaring assignment is not.
- Inferred type facts come from literal initializers (`String`, `Array`,
  `Hash`, `Symbol`, `Regexp`, `Boolean`, `NilClass`, `Integer`, `Float`,
  `Range`) and from same-file `Foo.new(...)`.
  `docs/decisions/2026-09-08-receiver-type-facts-wave-2.md` keeps imported
  and namespace-qualified constructors out.
- A method's declared return type comes from the annotations a Ruby type
  checker enforces: a Sorbet `sig { ... returns(T) }` or an RBS inline
  `#: (..) -> T` (with `#|` continuation lines), `# @rbs (..) -> T`, or
  `# @rbs return: T` comment directly above the `def`. Several overloads must
  agree; the `# @rbs ... | (..) -> U` overload form records no fact. YARD
  `@return` tags are unchecked documentation and are not read.
- A local or instance variable assigned with `=` or `||=` gets an inferred
  type fact from a call to a same-file method with a declared return type: a
  receiverless or `self.` call on the current `self` (instance methods in an
  instance method, class methods in a class body or class method), or
  `Foo.m` for a class method of a same-file class or module. Methods are keyed
  by the full lexical path of their class, so `B::Item` never takes a type
  from `A::Item`. `Foo` resolves through the call's lexical nesting,
  innermost first, then at the top level. The first same-file constant found
  must be a class or module: `Widget = Other` in the nesting records no fact.
  The top-level step is skipped when Ruby would look in the innermost
  class's ancestors first: a superclass, an `include` or `prepend`, an
  `extend` seen from `class << self`, or `Foo.include(M)` on that class
  anywhere in the file. A `def` after a bare
  `module_function`, named by `module_function :m`, written as
  `module_function def m`, or in a module with `extend self` is both an
  instance and a module method. Same-named methods on that `self` must agree,
  and a same-named `def` whose `self` the file does not settle blocks the
  name. `alias`, `alias_method`, `attr_reader`, `attr_accessor`, `attr`,
  `define_method`, `define_singleton_method`, `delegate`, `def_delegator`,
  and `def_delegators` also define the name without a type, so it records no
  fact. A method name the file does not spell out (`define_method(name)`,
  `delegate ..., prefix:`) blocks every lookup on that class, and a
  `prepend` blocks every lookup on the class it targets (every class for
  `base.prepend(M)`). `T.nilable(X)`, `X?`, and `X | nil` record `X`; `T.must(x)` and
  `#: as !nil` keep the type of `x`.
- These record no fact: a chained or other-receiver call, a `::Foo.m` or
  `A::Foo.m` receiver, a local that shadows the method name, a block that
  rebinds `self` (`instance_eval`, `class_eval`, `define_method`, ...), any
  block or lambda directly in a class or module body (`before_action`,
  `scope`, `included`, ...) or at the top level (RSpec `describe`, `it`,
  `let`, ...), `void`, a union, `T.untyped`, `T::Boolean`, a
  `T::` generic, a type parameter, a Sorbet `type_member`, or a
  `T.type_alias`. Other files are out of scope because each file is
  extracted alone.
- An instance variable field gets no type fact, literal, inferred, or
  written, when its class uses that `@x` at more than one `self` level: in
  instance methods, and also in a class body, a class method, `class << self`,
  or a block whose `self` is not settled. The class-level `@x` is a different
  variable from the instance `@x`, and the one field stands for both. A field
  used at one level only keeps its type.
- A reassigned local or field keeps the type of its first assignment. This
  holds for literal, `Foo.new`, and call initializers alike, so
  `x = name; x = size` records the type of `name`. Include and extend
  calls on a receiver the file does not name, such as `base.extend(M)` in an
  `included` hook, are not tracked.
- A written type wins over inference and is not inferred: Sorbet
  `T.let(x, T)` / `T.cast(x, T)`, or a trailing RBS `#: T` / `#: as T`
  comment. A trailing written type with no single class records no fact.

## Rake files

In a `.rake` file or a `Rakefile`, `namespace :db do` is a `namespace` symbol
and `task seed: :environment do` (or `task :seed`) is a `function` symbol.
A preceding `desc "..."` is the task doc comment. Calls in a task body
therefore have a containing symbol and give pending rows. Other files keep
`task` and `namespace` as ordinary calls.

## RSpec references and tags

- `it_behaves_like`, `it_should_behave_like`, `include_examples`, and
  `include_context` give a `references` relationship to a same-file shared
  group, or a pending `references` row named after the group.
- Metadata after a group or example description is a symbol annotation with
  carrier `rspec_metadata`: `:slow` gives annotation `slow`, and
  `type: :model` gives key `type` with the pair text in `raw_text`.
- A one-liner `it { ... }` is named `example at line N`, the way RSpec names
  it. Descriptions are decoded string contents. Test block signatures are the
  call text up to the block.
- A hook is a hook only directly in an example group. A `before` block inside
  an example, a `let` body, or a mock app is an ordinary call and never
  resolves to a hook symbol.

## Doc comments

A doc comment is the `#` block directly above a declaration, with no blank
line between. A comment before the first member of a class or module body
documents that member. A shebang, a magic comment (`frozen_string_literal:`,
`encoding:`, `coding:`, `warn_indent:`, `shareable_constant_value:`,
`typed:`), and a `rubocop:` directive are never docs and end the block. They
are plain `comment` source regions.

## Rails routes

The collector reads the AST inside `*.routes.draw do ... end`, or the whole
file for split route files under `config/routes/`. `namespace`, `scope`
(positional or `path:`), `resources`, `resource`, `member`, `collection`, the
HTTP verbs, `match` with `via:`, `root`, and `mount` are read with their
multi-line arguments and one-line `do ... end` blocks. Member routes join
`/:id`; routes written directly in a `resources :users` block and nested
resources join `/users/:user_id`. See `docs/contracts/sqlite-schema-v4.md`
for the metadata rules.

## Facts, literals, and HTTP clients

- `ruby.require_call.v1` and `ruby.mixin_call.v1` count only bare, `self`,
  or `Kernel` receivers. `params.require(:user)` is not a require.
- `ruby.rescue_clause.v1` reads the `exceptions` field: `exception_type` is
  the first class and `exception_types` lists every class. A bare `rescue`
  and `rescue => e` carry neither key.
- A heredoc argument is a literal with its dedented body text. The first
  argument of `where`, `order`, `reorder`, `joins`, and `having` is a SQL
  fragment carrier.
- `http.client_request.v1` covers `HTTParty`, `RestClient`, `Faraday`
  (constant, `conn = Faraday.new(...)` receiver, or a chained
  `Faraday.new(...).get`), and `Net::HTTP.get/get_response/post/post_form`
  with a same-scope `URI("...")` binding.

## Sinatra routes

`sinatra.route.v1` records a `get`, `post`, `put`, `patch`, `delete`, `head`,
`options`, `link`, or `unlink` block with a static string path.
`sinatra.filter.v1` records a `before` or `after` block with a static path.
Both count inside a `Sinatra::Base` or `Sinatra::Application` subclass, or at
the top level of a file that requires `sinatra`. An interpolated path emits
nothing. Handler blocks get no symbol, so calls in a handler belong to the
app class.

## Recorded gaps

The ruby row in `fixtures/extraction/capabilities.json` has no open gaps.
The RSpec shared-group reference and metadata-tag gaps closed in wave 2 of
`docs/plans/2026-09-22-language-gap-closure.md`.

## Evidence

The golden fixture `ruby:test_roles` registers four sources:

| Source | What it proves |
| --- | --- |
| `test_source.rb` | RSpec groups, examples, skip and focus aliases, hooks, helper methods, shared groups, and the two member-call controls |
| `minitest_class_test.rb` | `Minitest::Test` and `Test::Unit::TestCase` suites plus the `FixtureBuilder` in-path control |
| `rails_macro_test.rb` | the Rails `test` macro, block-form `setup`/`teardown`, and `ActionDispatch::IntegrationTest` |
| `production_roles.rb` | the production-path control |

The golden fixture `ruby:wave2_semantics` registers five sources: a class
file with the wave-2 symbol, visibility, callback, fact, literal, and HTTP
client shapes; a shared example group in `spec/support/` that
`spec/list_spec.rb` runs from another file; a Rake task file; and a
`config/routes.rb` with the AST route shapes.

The `ruby:test_roles` goldens observe 11 `test_case` rows, 9 `test_container` rows,
and 11 `test_lifecycle` rows for ruby.

No real-world corpus scan was run for this contract. The evidence above is
golden-fixture evidence only.
