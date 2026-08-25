# Go support

`go` handles `.go` files. Go's module manifests, `go.mod` and `go.sum`, have no
extension and no registered language, so they are scanned as unsupported. That
gap is recorded as `go.module_manifest_language`.

## Continuous testing

Run the language target when changing Go extraction:

```bash
cargo xtask test language go
```

The command runs `tests::go::` and the golden extraction test with
`JULIE_GOLDEN_LANGUAGE=go`. The unfiltered golden target stays:

```bash
cargo xtask test golden
```

## Test-role contract

`go test` compiles only the files whose name ends in `_test.go`, so every Go
test role is gated on that suffix. The gate is the reason a production method
named `SetupTest` or a production function named `TestConnection` publishes no
role.

### Test cases

A function or method is a `test_case` when its name starts with `Test`,
`Benchmark`, `Fuzz`, or `Example` and the character after the prefix is not a
lower-case letter. That last rule is `cmd/go`'s own `isTest` check, so
`Testable` stays production code while `Test` and `Test_adds` are cases.

`Benchmark` is included by decision. `go test -list` lists benchmarks beside
tests, fuzz targets, and examples, and `go test -bench` runs them, so a
benchmark-only file must not be invisible to a test-aware consumer. The
measured cost of the decision is in the table below: 142 benchmark rows across
the five corpora, all inside `_test.go` files.

### Lifecycle hooks

| Name | Role | Framework |
| --- | --- | --- |
| `TestMain` | `fixture_setup` | standard library |
| `SetupSuite`, `SetupTest`, `SetupSubTest`, `BeforeTest` | `fixture_setup` | testify |
| `TearDownSuite`, `TearDownTest`, `TearDownSubTest`, `AfterTest` | `fixture_teardown` | testify |
| `SetUpSuite`, `SetUpTest` | `fixture_setup` | gocheck |
| `TearDownSuite`, `TearDownTest` | `fixture_teardown` | gocheck |

`TestMain` matches the `Test` prefix, so an earlier rule reported it as a test
case. It is not one: it wraps the whole package run around `m.Run()`, and
`go test -run` cannot select it. It is an around-style hook, so it reports the
single honest direction, `fixture_setup`, the same way a yielding pytest
fixture does.

### Test containers

A struct is a `test_container` when it is declared in a `_test.go` file and
embeds a qualified type whose final segment is `Suite`, which is how testify
spells `suite.Suite`. The final-segment rule keeps an aliased import
(`tsuite "github.com/stretchr/testify/suite"`) working. A struct embedding
`sync.Mutex` is the control that must stay unclassified.

Go attaches a suite's methods through their receiver type, not through
lexical nesting, so a suite method is not a child symbol of its struct. Method
roles therefore come from the name plus the `_test.go` gate, and the container
row records the suite itself.

gocheck registers a suite with `var _ = check.Suite(&MySuite{})` and embeds
nothing, so a gocheck suite struct carries no syntactic marker. Its methods
publish their roles; the struct does not. That is recorded as
`gocheck.suite_registration`.

### Ginkgo

Ginkgo declares specs as call expressions whose callee is a bare identifier:
`Describe`, `Context`, `When`, `It`, `BeforeEach`, and their focused and
pending variants. Those are ordinary Go identifiers, so two guards apply.

The first is an import-or-path gate. Ginkgo calls are read as tests only when
`go test` compiles the file or the file imports
`github.com/onsi/ginkgo`. Without the gate, a production package that defines
and calls its own `Describe("job queue", func(){ ... })` publishes test roles.

The second is container-ancestor scoping through `normalize_scoped_test_roles`.
Ginkgo builds its spec tree at file scope and treats the suite as the implicit
root, so a top-level `It` or `BeforeSuite` is a real node and keeps its role. A
spec or hook written inside an ordinary function body is different: Ginkgo
builds the tree before any spec runs, so a node declared from a plain helper at
run time never joins a suite. Those nodes lose their role.

The cost of the second guard is measured. Across the five corpora it removes
six rows out of 4,912 captured Ginkgo leaves and hooks:

- go-redis `internal/util_test.go` declares three `It` calls inside
  `func TestToLower(t *testing.T)`. Ginkgo rejects a spec built at run time, so
  those three are correctly dropped.
- Ginkgo's own `internal/test_helpers/set_up_server.go` calls `DeferCleanup`
  twice from a helper, and its
  `integration/_fixtures/.../never_see_this_file_test.go` declares one `It`
  inside `func OffsetIt()`. Those three are real shared-behaviour nodes and are
  a recall cost of the rule, not a defect in the corpus.

## Known gaps

`t.Run("name", func(t *testing.T){ ... })` subtests are not captured. The
subtest name is a call argument, not a declaration, and `go test -run
Parent/name` addresses it. The five corpora contain 363 literal `t.Run`
subtests across 33 files, so the gap is material. It is recorded as
`go.subtest_names`.

## Grammar freshness

The live maintenance report was run with:

```bash
node scripts/grammar-freshness-report.mjs --format json
```

The Go-specific finding was that `tree-sitter-go` is current: declared and
locked at `0.25.0`, matching the latest stable release. The shared
`tree-sitter` runtime is marked drift at locked `0.26.11` versus latest stable
`0.26.13`; that is a repository-wide freshness finding, not a Go dependency
change.

## Real-world evidence

Five corpora were cloned shallowly into a temporary directory. No project build
scripts, hooks, or third-party binaries were executed. gin, testify, and
ginkgo are MIT licensed; go-redis is BSD-2-Clause; go-grpc-middleware is
Apache-2.0. Treat source redistribution as subject to each repository's own
licence file.

| Corpus | Commit |
| --- | --- |
| `redis/go-redis` | `18837034a1b96d331567d4fea23b303fd8bf2800` |
| `onsi/ginkgo` | `f2d0f65b6d1e99c58d1f9a31b41c53a2754a6c2c` |
| `grpc-ecosystem/go-grpc-middleware` | `80d77aa2945c107cee10670d66fae3321cef7267` |
| `gin-gonic/gin` | `dcaa4296d111981ffb31ac3eba90bb63e1eb5ab9` |
| `stretchr/testify` | `9f9d4f4cd868b1667991148401d30a012470b9c9` |

Reproducible checkout and scan commands, shown for one corpus:

```bash
CORPUS="$(mktemp -d)"
git clone --depth 1 https://github.com/redis/go-redis "$CORPUS"
git -C "$CORPUS" checkout --detach \
  18837034a1b96d331567d4fea23b303fd8bf2800

cargo build --locked --bin julie-extract
ARTIFACT="$(mktemp -d)"
./target/debug/julie-extract scan \
  --root "$CORPUS" \
  --db "$ARTIFACT/artifact.sqlite" \
  --json >"$ARTIFACT/scan-report.json" \
  2>"$ARTIFACT/scan-stderr.log"
```

Every scan reported `status=ok`, `files_failed=0`, and empty `warnings` and
`errors`. The only Go parse diagnostic in the whole set is one error in
Ginkgo's `integration/_fixtures/reporting_fixture/malformed_sub_package/`
fixture, which is deliberately malformed source.

| Artifact evidence | go-redis | ginkgo | grpc-middleware | gin | testify |
| --- | ---: | ---: | ---: | ---: | ---: |
| Files scanned | 536 | 531 | 159 | 130 | 91 |
| Indexed `go` files | 378 | 411 | 105 | 99 | 62 |
| of those, `_test.go` | 217 | 260 | 43 | 40 | 21 |
| Go symbols | 15,879 | 10,591 | 2,022 | 3,280 | 2,886 |
| `test_case` | 2,507 | 2,511 | 169 | 658 | 511 |
| `test_container` | 212 | 1,329 | 2 | 0 | 0 |
| `fixture_setup` | 110 | 603 | 9 | 0 | 16 |
| `fixture_teardown` | 90 | 282 | 4 | 0 | 18 |
| Files publishing a role | 204 | 255 | 43 | 40 | 19 |
| Production files publishing a role | 0 | 0 | 0 | 0 | 0 |

The last row is the precision measurement that matters. Across 1,055 indexed
Go files, of which 474 are production files, not one production file publishes
a test role. The `_test.go` gate and the Ginkgo import-or-path gate together
produce zero observed false positives.

The gates close a real risk rather than an observed defect. A separate text
audit of the same corpora found only two bare Ginkgo container calls with a
description string outside a `_test.go` file, and both are inside a raw-string
code template in Ginkgo's own generator, where the parser sees string content,
not a call. The synthetic proof that the gate works is the unit test
`bare_ginkgo_vocabulary_in_production_go_is_not_a_test`, which defines and
calls a package-local `Describe`/`It` pair in a production file.

Representative rows prove each framework arm:

- go-redis `maintnotifications/e2e/main_test.go` publishes `TestMain` as
  `fixture_setup`, the only `TestMain` in the corpus set.
- go-redis publishes 109 `Benchmark*` rows as `test_case`; under the previous
  contract all of them were invisible.
- grpc-middleware `interceptors/client_test.go` and `interceptors/server_test.go`
  publish `ClientInterceptorTestSuite` and `ServerInterceptorTestSuite` as
  `test_container`, each through its embedded `suite.Suite`.
- testify's own `suite/suite_test.go` publishes 16 `fixture_setup` and 18
  `fixture_teardown` rows. Its suite structs embed a package-local `Suite`
  rather than a qualified `suite.Suite`, so they are not marked as containers.
  An in-package suite base is a testify-repository special case.
- grpc-middleware `testing/testpb/interceptor_suite.go` defines
  `InterceptorTestSuite` with an embedded `suite.Suite` in a production file,
  and seven `_test.go` suites embed it. The `_test.go` gate keeps that struct
  unmarked, which is a deliberate recall cost of gating on the suffix `go test`
  itself uses.
- go-redis indexes 58 `go.mod` and `go.sum` files with status `unsupported`,
  which is the evidence behind `go.module_manifest_language`.

The temporary checkouts and SQLite artifacts were removed after recording this
evidence.
