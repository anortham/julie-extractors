# Blazor/Razor corpus classification

## Baseline

The start-of-work query on 2026-07-11 was:

```sql
SELECT path, start_line, start_column
FROM parse_diagnostics
WHERE language = 'razor';
```

It returned **235 rows**: 232 `error` and 3 `missing`. The 3 missing rows are all recovery fallout in `HomePage.razor` after the interpolated component-attribute expression. The branch parser was commit `07eab9cff90d462571c05526520686abb077dc4d`; all classifications below were checked against that parser with Tree-sitter CLI 0.26.10, not the Julie-pinned grammar.

## Classification

| Class | Diagnostic rows | Corpus cases | Branch state |
|---|---:|---|---|
| Attribute-value implicit expression | 30 | I1 identifier, I2 member access, I3 invocation, I4 `typeof`; I5 bare identifier; I6 mixed text | I5 is covered by `07eab9c`; I1-I4 are RED with `ERROR`; I6 parses only the first identifier and is semantically RED |
| Explicit expression with lambda, ternary, or collection | 164 | E1-E9: lambdas, assignment/coalesce, ternary, collection, object creation, interpolation, binary, conditional access | RED; every case contains `ERROR` on `07eab9c` |
| Directive-attribute modifier | 0 | M1 `preventDefault`; M2 `event`; M3 `after`; M4 `get`/`set` | M1 is covered; M2-M4 are RED at the modifier |
| Other | 41 | O1 generic component type values; O2-O4 render mode forms; O5 constrained type parameter; O6 render-fragment literal | O1 and O6 account for Terraform rows and remain RED; O2-O5 are documentation additions and remain RED |
| **Total** | **235** | **25 desired-semantics cases** | **2 green, 23 RED** |

`07eab9c` also covers the existing `@bind-Value="field"` identifier/member-access tests. It does not cover a leading `@` inside a PascalCase component value, `string` used as a generic component type value, or mixed HTML text where the implicit expression continues through member access.

## Complete duplicate ledger

Every start-of-work row maps through the table below. `I`, `E`, `M`, and `O` refer to the named corpus cases above. Counts are diagnostic rows, not source occurrences; repeated parser recovery rows are named duplicates of the triggering construct. The `HomePage.razor` rows all map to E7 because its first component interpolation causes the remainder of the file to recover as one root `ERROR`.

| Terraform path | Rows | I | E | M | O | Duplicate mapping |
|---|---:|---:|---:|---:|---:|---|
| `src/Terraform.Client/App.razor` | 5 | 5 | 0 | 0 | 0 | I1/I4 |
| `src/Terraform.Client/Features/Admin/AdminAccessViewerPage.razor` | 9 | 0 | 7 | 0 | 2 | E1/E3/E4/E5/E6; O1 recovery |
| `src/Terraform.Client/Features/Admin/AdminAuditPage.razor` | 7 | 0 | 7 | 0 | 0 | E1/E2/E6 |
| `src/Terraform.Client/Features/Admin/AdminMappingDialog.razor` | 10 | 0 | 4 | 0 | 6 | E1; O1 |
| `src/Terraform.Client/Features/Admin/AdminMappingsPage.razor` | 11 | 0 | 11 | 0 | 0 | E1/E2/E6/E8 |
| `src/Terraform.Client/Features/Admin/AdminUsersPage.razor` | 9 | 0 | 9 | 0 | 0 | E1/E3/E4/E7/E8 |
| `src/Terraform.Client/Features/Edr/EdrDashboardPage.razor` | 12 | 2 | 8 | 0 | 2 | I1/I3; E1/E2/E6; O1 |
| `src/Terraform.Client/Features/Edr/EdrDenyDialog.razor` | 1 | 1 | 0 | 0 | 0 | I1 |
| `src/Terraform.Client/Features/Edr/EdrFileUpload.razor` | 10 | 5 | 5 | 0 | 0 | I1-I3; E2/E5/E6 |
| `src/Terraform.Client/Features/Edr/EdrFormPage.razor` | 23 | 0 | 15 | 0 | 8 | E2/E6/E8/E9; O1 recovery |
| `src/Terraform.Client/Features/Edr/EdrReviewPage.razor` | 6 | 1 | 5 | 0 | 0 | I3; E2/E6 |
| `src/Terraform.Client/Features/Home/HomePage.razor` | 35 | 0 | 35 | 0 | 0 | E7 root-error recovery, including all 3 missing rows |
| `src/Terraform.Client/Features/Observer/ObserverDashboardPage.razor` | 38 | 10 | 24 | 0 | 4 | I1-I3; E1/E2/E6-E8; O1 |
| `src/Terraform.Client/Features/Observer/ObserverEditDialog.razor` | 23 | 3 | 12 | 0 | 8 | I1/I2; E1/E3/E4; O1 and O6 |
| `src/Terraform.Client/Features/Ser/SerDenyDialog.razor` | 1 | 0 | 1 | 0 | 0 | E6 |
| `src/Terraform.Client/Features/Ser/SerFormPage.razor` | 23 | 0 | 12 | 0 | 11 | E1-E3/E6; O1 recovery |
| `src/Terraform.Client/Features/Ser/SerListPage.razor` | 7 | 1 | 6 | 0 | 0 | I3; E1/E2/E6 |
| `src/Terraform.Client/Features/Ser/SerReviewPage.razor` | 4 | 1 | 3 | 0 | 0 | I3; E6 |
| `src/Terraform.Client/Shared/ErrorPanel.razor` | 1 | 1 | 0 | 0 | 0 | I1 |
| **Total** | **235** | **30** | **164** | **0** | **41** | |

## Documentation additions

Terraform contains M1 but none of M2-M4, O2-O5. The added cases come from current Microsoft Blazor documentation:

- Data binding: `@bind:event`, `@bind:after`, and paired `@bind:get`/`@bind:set`: <https://learn.microsoft.com/en-us/aspnet/core/blazor/components/data-binding?view=aspnetcore-10.0>
- Render modes: custom identifiers, explicit render-mode construction, and component `@rendermode`: <https://learn.microsoft.com/en-us/aspnet/core/blazor/components/render-modes?view=aspnetcore-10.0>
- Generic components: constrained `@typeparam`: <https://learn.microsoft.com/en-us/aspnet/core/blazor/components/generic-type-support?view=aspnetcore-10.0>

## Re-estimate

- **Task 2 remains high risk and grows from one rule tweak to three seams:** PascalCase component values with leading transitions, quoted C# strings inside the outer attribute quotes, and mixed lower-case HTML values. Implement in E/I frequency order and preserve the opaque fallback only after expression alternatives.
- **Task 3 remains bounded:** extend modifier suffixes, make `razor_rendermode` expression-capable, and add a C# type-constraint clause after `@typeparam`.
- **Add one Task 2 closure case for O1:** generic component type values such as `TValue="string"` are 35 recovery rows within O and must parse as `predefined_type`, not opaque text.
- **O6 is a separate grammar gap:** render-fragment literals inside C# switch expressions account for 6 rows. It should be added to Task 3 or explicitly carried as a named limitation; it is not fixed by attribute or directive work.

The direct-composition path is still viable, but the 23 RED cases confirm the architecture risk remains high until Tasks 2-3 are green.
