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
| Explicit expression with lambda, ternary, or collection | 164 | E1-E9: Terraform lambdas, assignment/coalesce, ternary, collection, object creation, interpolation, binary, conditional access; E10 FluentUI async callback | RED; every case contains `ERROR` on `07eab9c`; E10 is documentation-only and adds no Terraform rows |
| Directive-attribute modifier | 0 | M1 `preventDefault`; M2 `event`; M3 `after`; M4 `get`/`set` | M1 is covered; M2-M4 are RED at the modifier |
| Other | 41 | O1 generic component type values; O2-O4 render mode forms; O5 constrained type parameter; O6 render-fragment literal | O1 and O6 account for Terraform rows and remain RED; O2-O5 are documentation additions and remain RED |
| **Total** | **235** | **26 desired-semantics cases** | **2 green, 24 RED** |

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

## Row-addressable diagnostic appendix

Each coordinate below is one exact start-of-work diagnostic tuple. Grouping only combines rows with the same path and corpus-case mapping; line, column, and kind remain explicit.

| Path | Case | Coordinates `line:column:kind` | Rows |
|---|---|---|---:|
| `src/Terraform.Client/App.razor` | I4 | `5:25:error, 7:70:error, 15:32:error` | 3 |
| `src/Terraform.Client/App.razor` | I1 | `7:43:error, 12:40:error` | 2 |
| `src/Terraform.Client/Features/Admin/AdminAccessViewerPage.razor` | O1 | `24:34:error, 24:40:error` | 2 |
| `src/Terraform.Client/Features/Admin/AdminAccessViewerPage.razor` | E4 | `32:32:error, 32:69:error` | 2 |
| `src/Terraform.Client/Features/Admin/AdminAccessViewerPage.razor` | E3 | `35:39:error` | 1 |
| `src/Terraform.Client/Features/Admin/AdminAccessViewerPage.razor` | E5 | `36:46:error` | 1 |
| `src/Terraform.Client/Features/Admin/AdminAccessViewerPage.razor` | E6 | `43:33:error` | 1 |
| `src/Terraform.Client/Features/Admin/AdminAccessViewerPage.razor` | E1 | `89:54:error, 90:54:error` | 2 |
| `src/Terraform.Client/Features/Admin/AdminAuditPage.razor` | E6 | `31:37:error` | 1 |
| `src/Terraform.Client/Features/Admin/AdminAuditPage.razor` | E2 | `32:35:error` | 1 |
| `src/Terraform.Client/Features/Admin/AdminAuditPage.razor` | E1 | `45:37:error, 50:46:error, 51:46:error, 52:46:error, 53:46:error` | 5 |
| `src/Terraform.Client/Features/Admin/AdminMappingDialog.razor` | O1 | `13:30:error, 13:38:error, 33:27:error, 33:33:error, 49:27:error, 49:33:error` | 6 |
| `src/Terraform.Client/Features/Admin/AdminMappingDialog.razor` | E1 | `23:36:error, 24:37:error, 43:31:error, 59:31:error` | 4 |
| `src/Terraform.Client/Features/Admin/AdminMappingsPage.razor` | E6 | `18:33:error, 40:37:error, 63:53:error, 72:53:error` | 4 |
| `src/Terraform.Client/Features/Admin/AdminMappingsPage.razor` | E8 | `19:32:error` | 1 |
| `src/Terraform.Client/Features/Admin/AdminMappingsPage.razor` | E1 | `54:37:error, 57:46:error, 58:46:error, 59:46:error` | 4 |
| `src/Terraform.Client/Features/Admin/AdminMappingsPage.razor` | E2 | `70:51:error, 79:51:error` | 2 |
| `src/Terraform.Client/Features/Admin/AdminUsersPage.razor` | E6 | `34:37:error` | 1 |
| `src/Terraform.Client/Features/Admin/AdminUsersPage.razor` | E1 | `57:37:error, 60:46:error, 61:46:error, 73:54:error` | 4 |
| `src/Terraform.Client/Features/Admin/AdminUsersPage.razor` | E4 | `67:44:error` | 1 |
| `src/Terraform.Client/Features/Admin/AdminUsersPage.razor` | E8 | `74:50:error` | 1 |
| `src/Terraform.Client/Features/Admin/AdminUsersPage.razor` | E7 | `75:51:error, 75:87:error` | 2 |
| `src/Terraform.Client/Features/Edr/EdrDashboardPage.razor` | E6 | `12:71:error, 21:75:error, 64:82:error` | 3 |
| `src/Terraform.Client/Features/Edr/EdrDashboardPage.razor` | O1 | `17:35:error, 17:51:error` | 2 |
| `src/Terraform.Client/Features/Edr/EdrDashboardPage.razor` | I1 | `17:83:error` | 1 |
| `src/Terraform.Client/Features/Edr/EdrDashboardPage.razor` | E1 | `18:64:error, 42:42:error, 43:42:error, 44:42:error` | 4 |
| `src/Terraform.Client/Features/Edr/EdrDashboardPage.razor` | I3 | `46:74:error` | 1 |
| `src/Terraform.Client/Features/Edr/EdrDashboardPage.razor` | E2 | `64:134:error` | 1 |
| `src/Terraform.Client/Features/Edr/EdrDenyDialog.razor` | I1 | `11:82:error` | 1 |
| `src/Terraform.Client/Features/Edr/EdrFileUpload.razor` | E8 | `10:38:error` | 1 |
| `src/Terraform.Client/Features/Edr/EdrFileUpload.razor` | I1 | `13:31:error, 14:29:error, 30:56:error, 59:44:error` | 4 |
| `src/Terraform.Client/Features/Edr/EdrFileUpload.razor` | E6 | `19:65:error, 54:71:error, 58:45:error` | 3 |
| `src/Terraform.Client/Features/Edr/EdrFileUpload.razor` | I2 | `43:65:error` | 1 |
| `src/Terraform.Client/Features/Edr/EdrFileUpload.razor` | E2 | `62:43:error` | 1 |
| `src/Terraform.Client/Features/Edr/EdrFormPage.razor` | E2 | `49:44:error, 54:44:error, 59:44:error, 64:44:error, 70:44:error, 82:44:error, 88:44:error, 95:44:error, 101:44:error, 106:44:error, 112:44:error, 117:44:error` | 12 |
| `src/Terraform.Client/Features/Edr/EdrFormPage.razor` | O1 | `79:43:error, 79:59:error, 85:43:error, 85:59:error, 91:43:error, 91:59:error, 98:43:error, 98:59:error` | 8 |
| `src/Terraform.Client/Features/Edr/EdrFormPage.razor` | E9 | `158:43:error` | 1 |
| `src/Terraform.Client/Features/Edr/EdrFormPage.razor` | E6 | `166:37:error` | 1 |
| `src/Terraform.Client/Features/Edr/EdrFormPage.razor` | E8 | `167:36:error` | 1 |
| `src/Terraform.Client/Features/Edr/EdrReviewPage.razor` | E6 | `17:75:error, 82:94:error, 96:83:error, 97:105:error` | 4 |
| `src/Terraform.Client/Features/Edr/EdrReviewPage.razor` | I3 | `37:70:error` | 1 |
| `src/Terraform.Client/Features/Edr/EdrReviewPage.razor` | E2 | `82:148:error` | 1 |
| `src/Terraform.Client/Features/Home/HomePage.razor` | E7 | `1:0:error, 30:35:error, 30:49:error, 30:49:missing, 31:62:missing, 33:20:error, 34:24:error, 34:56:error, 36:20:error, 37:24:error, 37:28:error, 37:39:error, 38:27:error, 38:45:error, 39:20:error, 40:20:error, 40:50:error, 41:24:error, 41:34:error, 43:28:error, 44:32:error, 44:65:error, 45:38:error, 45:52:error, 46:28:error, 47:24:error, 49:16:error, 50:12:error, 52:4:error, 57:11:error, 64:39:error, 73:56:error, 85:38:error, 85:38:missing, 141:32:error` | 35 |
| `src/Terraform.Client/Features/Observer/ObserverDashboardPage.razor` | E6 | `17:37:error, 20:37:error, 34:87:error, 42:87:error, 50:87:error, 58:87:error, 84:41:error, 102:37:error, 123:57:error, 156:49:error, 159:49:error` | 11 |
| `src/Terraform.Client/Features/Observer/ObserverDashboardPage.razor` | E8 | `18:36:error, 74:37:error` | 2 |
| `src/Terraform.Client/Features/Observer/ObserverDashboardPage.razor` | E2 | `32:85:error, 40:84:error, 40:114:error, 48:82:error, 48:109:error, 56:97:error, 157:47:error, 160:47:error` | 8 |
| `src/Terraform.Client/Features/Observer/ObserverDashboardPage.razor` | O1 | `73:39:error, 73:55:error, 78:39:error, 78:55:error` | 4 |
| `src/Terraform.Client/Features/Observer/ObserverDashboardPage.razor` | E1 | `75:72:error, 80:42:error, 80:74:error` | 3 |
| `src/Terraform.Client/Features/Observer/ObserverDashboardPage.razor` | I2 | `79:37:error` | 1 |
| `src/Terraform.Client/Features/Observer/ObserverDashboardPage.razor` | I1 | `111:72:error, 114:71:error, 125:53:error, 126:55:error, 131:72:error, 137:65:error, 140:69:error, 147:73:error` | 8 |
| `src/Terraform.Client/Features/Observer/ObserverDashboardPage.razor` | I3 | `143:112:error` | 1 |
| `src/Terraform.Client/Features/Observer/ObserverEditDialog.razor` | E1 | `17:105:error, 22:105:error, 27:105:error, 32:105:error, 45:44:error, 46:70:error, 58:94:error, 60:90:error, 70:65:error` | 9 |
| `src/Terraform.Client/Features/Observer/ObserverEditDialog.razor` | I2 | `40:59:error, 42:59:error` | 2 |
| `src/Terraform.Client/Features/Observer/ObserverEditDialog.razor` | E3 | `40:91:error, 42:92:error` | 2 |
| `src/Terraform.Client/Features/Observer/ObserverEditDialog.razor` | O1 | `43:39:error, 43:55:error` | 2 |
| `src/Terraform.Client/Features/Observer/ObserverEditDialog.razor` | E8 | `44:80:error` | 1 |
| `src/Terraform.Client/Features/Observer/ObserverEditDialog.razor` | I1 | `82:38:error` | 1 |
| `src/Terraform.Client/Features/Observer/ObserverEditDialog.razor` | O6 | `354:42:error, 354:95:error, 354:155:error, 355:42:error, 355:95:error, 355:159:error` | 6 |
| `src/Terraform.Client/Features/Ser/SerDenyDialog.razor` | E6 | `33:37:error` | 1 |
| `src/Terraform.Client/Features/Ser/SerFormPage.razor` | O1 | `29:46:error, 29:54:error, 47:34:error, 69:48:error, 69:54:error, 71:76:error, 85:34:error, 92:48:error, 92:54:error, 94:76:error, 108:34:error` | 11 |
| `src/Terraform.Client/Features/Ser/SerFormPage.razor` | E1 | `37:52:error, 38:53:error, 76:52:error, 77:53:error, 99:52:error, 100:53:error` | 6 |
| `src/Terraform.Client/Features/Ser/SerFormPage.razor` | E2 | `42:44:error, 59:44:error, 134:44:error, 144:44:error` | 4 |
| `src/Terraform.Client/Features/Ser/SerFormPage.razor` | E6 | `152:37:error, 159:37:error` | 2 |
| `src/Terraform.Client/Features/Ser/SerListPage.razor` | E6 | `19:41:error, 26:37:error, 84:45:error` | 3 |
| `src/Terraform.Client/Features/Ser/SerListPage.razor` | E1 | `59:42:error, 74:42:error` | 2 |
| `src/Terraform.Client/Features/Ser/SerListPage.razor` | I3 | `70:40:error` | 1 |
| `src/Terraform.Client/Features/Ser/SerListPage.razor` | E2 | `85:43:error` | 1 |
| `src/Terraform.Client/Features/Ser/SerReviewPage.razor` | E6 | `15:33:error, 108:45:error, 117:45:error` | 3 |
| `src/Terraform.Client/Features/Ser/SerReviewPage.razor` | I3 | `45:36:error` | 1 |
| `src/Terraform.Client/Shared/ErrorPanel.razor` | I1 | `3:25:error` | 1 |

Self-check against the appendix table:

```bash
awk -F'|' 'NF==6 && /^\| `src\/Terraform.Client/ { case_id=$3; gsub(/ /, "", case_id); rows=$5+0; total+=rows; class_total[substr(case_id,1,1)]+=rows } END { printf "total=%d I=%d E=%d M=%d O=%d\\n", total, class_total["I"], class_total["E"], class_total["M"], class_total["O"] }' docs/plans/2026-07-11-blazor-corpus-classification.md
```

Result: `total=235 I=30 E=164 M=0 O=41`.

## Documentation additions

Terraform contains M1 but none of M2-M4, O2-O5, or E10. The added cases come from current official documentation:

- Data binding: `@bind:event`, `@bind:after`, and paired `@bind:get`/`@bind:set`: <https://learn.microsoft.com/en-us/aspnet/core/blazor/components/data-binding?view=aspnetcore-10.0>
- Render modes: custom identifiers, explicit render-mode construction, and component `@rendermode`: <https://learn.microsoft.com/en-us/aspnet/core/blazor/components/render-modes?view=aspnetcore-10.0>
- Generic components: constrained `@typeparam`: <https://learn.microsoft.com/en-us/aspnet/core/blazor/components/generic-type-support?view=aspnetcore-10.0>
- Fluent UI Blazor Autocomplete: E10 is the official `OnDismissClick="@(async () => await ContactList.RemoveSelectedItemAsync(context))"` example, absent from Terraform: <https://github.com/microsoft/fluentui-blazor/blob/dev/examples/Demo/Shared/Pages/List/Autocomplete/Examples/AutocompleteCustomized.razor>

## Re-estimate

- **Task 2 remains high risk and grows from one rule tweak to three seams:** PascalCase component values with leading transitions, quoted C# strings inside the outer attribute quotes, and mixed lower-case HTML values. Implement in E/I frequency order and preserve the opaque fallback only after expression alternatives.
- **Task 3 owns all remaining directive/other closure:** extend modifier suffixes, make `razor_rendermode` expression-capable, add a C# type-constraint clause after `@typeparam`, and implement O6 render-fragment literals in C# switch expressions.
- **Add one Task 2 closure case for O1:** generic component type values such as `TValue="string"` are 35 recovery rows within O and must parse as `predefined_type`, not opaque text.
- **O6 is assigned to Task 3:** render-fragment literals inside C# switch expressions account for 6 rows and must close there to meet the zero-diagnostics gate.

The direct-composition path is still viable, but the 24 RED cases confirm the architecture risk remains high until Tasks 2-3 are green.

## 2026-07-13 certification closeout

The start-of-work diagnostic ledger above remains the historical baseline. The
release CLI built at julie-extractors commit
`8b9a860b379a60fab1ff2c034cc6f01a05998395`, pinned to certified parser commit
`e38a509720eb54652d7079380acaa62064a2c66a`, reprocessed the live Terraform
corpus at `821e6b1a268cb392b1abb5080243a299db2a9bc9` with these results:

- 28/28 Razor files processed, zero failed files, and zero Razor diagnostics.
- The immediate rescan reported `no_change` with all 28 Razor files unchanged.
- SQLite integrity, JSON reports, and all 103,079 JSONL records validated.
- `cargo xtask test certification`, `cargo xtask test real-world-smoke`, and the
  strict language-quality report passed; `silent_cells=0` and
  `quality_bar_debts=0`.

The earlier `69/69` value was the Razor language-test count, not the number of
Terraform files. The earlier release-binary corpus measurement was 28 Razor
files, and the current tracked corpus is also 28, so there is no file-count
drift. Reproducible commands, stable and preview documentation inputs, exact
artifact counts, and SHA evidence are recorded in
`docs/release-evidence/2026-07-13-razor-parser-hardening.md`.
