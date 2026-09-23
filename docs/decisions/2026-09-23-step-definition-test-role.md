# Step-definition test role

Date: 2026-09-23. Plan: [gap follow-ups](../plans/2026-09-23-gap-followups.md),
task 1. Closes `specflow.step_definition_role` (csharp) and
`behat.step_definition_roles` (php).

## Context

A BDD runner (SpecFlow, Reqnroll, Behat) reads scenarios from a `.feature` file
and binds each step line to a method by the step text. The method is test code,
but it is neither a test case (the scenario is the case) nor a fixture hook (it
runs as a step, not around one). The five `TestRole` values had no place for
it, so both frameworks published no role for their step methods.

## Decision

- `TestRole` gains `step_definition`. The value appears as
  `test_role = "step_definition"` in `symbols.metadata_json`.
- A step definition sets none of the `is_test`, `test_container`, or
  `test_lifecycle` columns, and assigning the role clears any flag an earlier
  path or name rule set. The schema contract defines `is_test = 1` as a test
  case or a hook, and consumers count test cases as `is_test = 1` and
  `test_lifecycle = 0`. A step with `is_test = 1` would count as a case.
- The `test_detection` capability units stay `test_case`, `test_container`, and
  `test_lifecycle`. A step definition is metadata evidence, not a new unit.
- SpecFlow and Reqnroll: a method of a `[Binding]` class with a `[Given]`,
  `[When]`, `[Then]`, or `[StepDefinition]` attribute is a step definition.
  The attribute names are common words, so outside a `[Binding]` class they
  publish no role.
- Behat: a class that implements `Context`, `SnippetAcceptingContext`,
  `CustomSnippetAcceptingContext`, or `TranslatableContext` from the `Behat\`
  namespace is a test container. The base type must be written qualified or
  bound by a `use Behat\...` import, because `Context` is a common class name.
  PHPUnit name rules do not apply to its members. Its methods with a `Given`,
  `When`, or `Then` attribute or docblock tag are step definitions. Its `Before*` hooks (suite, feature,
  scenario, step) are `fixture_setup`, and its `After*` hooks are
  `fixture_teardown`.

## Consequences

- A reader that knows only the five earlier values must accept a sixth
  `test_role` string. The typed columns do not change.
- Rust callers: the public `TestRole` enum gains `StepDefinition`, so an
  exhaustive `match` on it needs a new arm.
- Golden evidence: `csharp:step_definitions`, `csharp:language_idioms`, and
  `php:behat_context`. Each fixture has a class with step attributes outside a
  binding or context class as the control.
