# Ruby support

Julie registers one Ruby language: `ruby` handles `.rb` and `.rbw` files.

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

## Doc comments

A doc comment is the `#` block directly above a declaration, with no blank
line between. A comment before the first member of a class or module body
documents that member. A shebang, a magic comment (`frozen_string_literal:`,
`encoding:`, `coding:`, `warn_indent:`, `shareable_constant_value:`,
`typed:`), and a `rubocop:` directive are never docs and end the block. They
are plain `comment` source regions.

## Sinatra routes

`sinatra.route.v1` records a `get`, `post`, `put`, `patch`, `delete`, `head`,
`options`, `link`, or `unlink` block with a static string path.
`sinatra.filter.v1` records a `before` or `after` block with a static path.
Both count inside a `Sinatra::Base` or `Sinatra::Application` subclass, or at
the top level of a file that requires `sinatra`. An interpolated path emits
nothing. Handler blocks get no symbol, so calls in a handler belong to the
app class.

## Recorded gaps

Two RSpec surfaces are recorded as `open_gaps` on the ruby row in
`fixtures/extraction/capabilities.json`, under
`kind_coverage.structural_facts.open_gaps`. The `test_detection` vocabulary is
frozen to `test_case`, `test_container`, and `test_lifecycle`, and ruby
classifies each exactly once, so a ruby-specific gap cannot live there.

- `rspec.shared_example_group_references`. `it_behaves_like "a countable"` and
  `include_examples "a countable"` run a group that `shared_examples` defines
  somewhere else, often in another file. The call names the group; it does not
  define it, so a symbol row would publish a second definition. The extractor
  emits the `shared_examples` container and leaves the call as an ordinary
  identifier row.
- `rspec.example_metadata_tags`. `it "is slow", :slow do` and
  `describe Order, type: :model do` attach metadata through extra call
  arguments. Ruby has no annotation syntax, so the ruby row classifies every
  annotation kind as not applicable and these tags reach no channel today.

## Evidence

The golden fixture `ruby:test_roles` registers four sources:

| Source | What it proves |
| --- | --- |
| `test_source.rb` | RSpec groups, examples, skip and focus aliases, hooks, helper methods, shared groups, and the two member-call controls |
| `minitest_class_test.rb` | `Minitest::Test` and `Test::Unit::TestCase` suites plus the `FixtureBuilder` in-path control |
| `rails_macro_test.rb` | the Rails `test` macro, block-form `setup`/`teardown`, and `ActionDispatch::IntegrationTest` |
| `production_roles.rb` | the production-path control |

The registered goldens observe 11 `test_case` rows, 9 `test_container` rows,
and 11 `test_lifecycle` rows for ruby.

No real-world corpus scan was run for this contract. The evidence above is
golden-fixture evidence only.
