# Test Detection Precision Hardening Design

## Purpose

Prevent helper functions and fixtures from becoming positive test facts while preserving the framework-native test cases, containers, and lifecycle roles already published by `julie-extract`.

## Root Cause

The shared detector treats a conventional test path as sufficient evidence for every Scala and Elixir callable. Python treats every decorator whose normalized key begins with `pytest` or `unittest` as test evidence. These shortcuts turn ordinary Scala/Elixir helpers, `@pytest.fixture` functions, and `@unittest.mock.patch` helpers into `is_test = true` rows.

Existing Scala and Elixir golden sources live outside paths recognized by `is_test_path`, so their ordinary-callable negatives do not exercise the failing branch. Python has positive `pytest.mark.parametrize` coverage but no fixture or mock-decorator negative.

## Design

Keep the existing `is_test_symbol` interface and metadata contract unchanged.

- Scala callable detection accepts explicit JUnit-style test annotations, known Scala lifecycle method names, and the existing `test` name prefix. Framework DSL calls such as `test`, `describe`, `it`, and infix specifications continue through the existing call-style extractor. A test-directory path alone is not callable-level evidence.
- Elixir callable detection accepts the existing `test_` and generated `test ` name forms. ExUnit `test`, `describe`, `setup`, and `setup_all` continue through the existing call-style extractor. A `test/` path alone is not callable-level evidence.
- Python decorator evidence accepts `pytest.mark.*` plus the explicit unittest test-control decorators `unittest.skip`, `unittest.skipIf`, `unittest.skipUnless`, and `unittest.expectedFailure`. `pytest.fixture` and `unittest.mock.*` are not test evidence. Existing lifecycle-name and `test_`-in-test-path detection remains unchanged.

The Python allowlist follows the official framework contracts: pytest marks apply metadata to tests while `@pytest.fixture` defines fixture functions; unittest skip/expected-failure decorators control tests while `unittest.mock.patch` only replaces objects during a decorated callable.

## Test Surface

Tests exercise the public extractor behavior rather than private helper output.

- Shared dispatch tests pin the exact Python decorator allowlist and the Scala/Elixir path-only negatives.
- Python full-extraction tests prove fixture and mock-decorated helpers in a conventional test file remain ordinary symbols while a marked test remains positive.
- Scala and Elixir full-extraction tests prove ordinary helpers in conventional test paths remain ordinary while their existing DSL and lifecycle tests remain positive.
- Existing language tiers, golden fixtures, capability contracts, and the strict data-quality report prove no role or capability regression.

## Architecture Quality

**Affected modules:** `crates/julie-extractors/src/test_detection.rs` and language-local detector tests.

**Caller-facing interface:** `is_test_symbol` and the emitted `is_test` / `test_lifecycle` metadata remain unchanged in shape.

**Depth/locality check:** Detection policy stays centralized in the existing detector; language extractors and artifact writers need no new obligations.

**Test surface:** Shared dispatch tests plus full language extraction and registered goldens.

**Seams/adapters:** No new seam or adapter.

**Rejected shortcuts:** Adding more path exceptions, recognizing every `pytest*` / `unittest*` decorator, or changing artifact schemas.

**Architecture risk:** Low. This is a behavior-local precision correction with caller-visible row changes but no module or contract-shape change.

## Acceptance Criteria

- [ ] Ordinary Scala and Elixir callables in conventional test paths do not emit `is_test = true` solely because of their paths.
- [ ] ScalaTest/Specs2/MUnit and ExUnit call-style cases, containers, and lifecycle hooks keep their current role output.
- [ ] `pytest.mark.*` remains path-independent positive evidence.
- [ ] `pytest.fixture` and `unittest.mock.*` do not mark helpers as tests.
- [ ] The four explicit unittest test-control decorators remain positive evidence.
- [ ] SQLite, JSONL, capability, and metadata shapes do not change.
- [ ] Focused language, golden, capability, strict-quality, formatting, and default gates pass.

## External Evidence

- pytest marks: https://docs.pytest.org/en/stable/how-to/mark.html
- pytest fixtures: https://docs.pytest.org/en/stable/reference/reference.html#fixtures
- Python unittest decorators: https://docs.python.org/3/library/unittest.html#skipping-tests-and-expected-failures
- Python mock patching: https://docs.python.org/3/library/unittest.mock.html#the-patchers
- ScalaTest styles: https://www.scalatest.org/user_guide/selecting_a_style
- ExUnit cases: https://hexdocs.pm/ex_unit/ExUnit.Case.html
