# Decision: dialect language identity stays honest in the artifact

Date: 2026-08-25
Status: accepted

## Context

A CT readiness audit found that Miller's continuous-test selector drops a test
whose source file and test file differ only by dialect: change `Button.tsx`,
keep the test in `Button.test.ts`, and the `path_stem` evidence tier produces
nothing. Both sides were verified before this decision.

### What this repo publishes

The registry treats each dialect as its own `LanguageSpec`. `specs.rs` declares
four separate specs for the JS/TS family:

| Spec name | `extensions` | `parser_crate` | parser entry |
| --- | --- | --- | --- |
| `typescript` | `ts`, `mts`, `cts` | `tree-sitter-typescript` | `parser_typescript` |
| `tsx` | `tsx` | `tree-sitter-typescript` | `parser_tsx` |
| `javascript` | `js`, `mjs`, `cjs` | `tree-sitter-javascript` | `parser_javascript` |
| `jsx` | `jsx` | `tree-sitter-javascript` | `parser_javascript` |

Source: `crates/julie-extractors/src/language_spec/specs.rs:44-75`.

The spec name is the published value. `detect_language_from_extension` returns
`spec.name` for the matched extension, and `detect_language_for_path` wraps it
with the `qmldir` filename rule and the `.h` content sniff
(`crates/julie-extractors/src/language_spec/mod.rs:290-322`). The CLI discovery
walk classifies files with that function
(`crates/julie-extract-cli/src/discovery.rs:580-582`), extraction re-detects the
same value and carries it into `ArtifactFile.language`
(`crates/julie-extract-cli/src/extraction.rs:180-188` and `:486`), and the
writer binds that one string into `files.language` and into every
`symbols.language` row of the file
(`crates/julie-extract-artifact/src/writer/rows.rs:76-94` and `:443-447`).

So `Button.tsx` publishes `tsx` on its file row and on all of its symbol rows.
There is no base-language relation anywhere in the registry: `LanguageSpec`
carries an `aliases` field, but the `spec` constructor sets it to `&[]` for
every language (`language_spec/specs.rs:318-334`), so no spec has ever declared
a family.

### What Miller does with it

`src/Miller.Testing/Selection/ContinuousTestImpactSelector.cs`
(miller `f02ac37`, read-only):

- `LanguageFromPath` (`:1247-1267`) is a hand-written extension map that answers
  `".jsx" => "javascript"` and `".tsx" => "typescript"`.
- `ResolveChangedFiles` (`:339-367`) prefers the indexed symbol language and
  falls back to `LanguageFromPath` only for a path with no indexed symbols
  (`:355`).
- The indexed value is the artifact value: `CtFactAdapter.ToSymbolFact`
  (`src/Miller.Indexing/Testing/CtFactAdapter.cs:245-257`) copies
  `IndexedSymbol.Language`, which `SqliteSymbolReader` selects as `s.language`
  from the artifact `symbols` table
  (`src/Miller.Indexing/SqliteSymbolReader.cs:70`).
- `ChangedPathStem.FromPath` (`:1377-1391`) and `MatchesChangedStem`
  (`:1072-1085`) feed both languages to `LanguagesAreCompatible` (`:1087-1095`),
  which is an ordinal string compare plus exactly one widening case,
  `IsCsharpOrRazor` (`:1097-1099`).

The miss follows directly. `LanguagesAreCompatible("tsx", "typescript")` is
false, so the `path_stem` tier drops the test.

Miller also disagrees with itself, independent of anything this repo decides.
The same `.tsx` path answers `tsx` when it is indexed and `typescript` when it
is not, so a newly added `Button.tsx` compared against an already indexed
`Button.test.tsx` misses in the other direction.

### The same hazard outside CT

Query-time resolution has the same defect and it costs more than one evidence
tier:

- `ImportBinding.ModuleCandidates`
  (`src/Miller.Core/Resolution/ImportBinding.cs:104-109`) expands relative
  import specifiers into candidate paths with a switch on `"typescript"` and
  `"javascript"` only. For a file whose language is `tsx` the switch falls to
  `_ => []`, so the import produces no candidates at all.
- `RevisionFactCacheLoader.ResolveModule`
  (`src/Miller.Indexing/Resolution/RevisionFactCacheLoader.cs:671-689`) then
  requires `file.Language == language` with `StringComparison.Ordinal`, where
  both values are `files.language` read straight from the artifact
  (`:32-49`). A `.tsx` file importing a `.ts` module fails that equality even if
  candidates existed.
- `ResolutionPolicy.IsEsModuleLanguage`
  (`src/Miller.Core/Resolution/ResolutionPolicy.cs:76`) already accepts
  `"jsx"` and `"tsx"`, while `IsTier2Language` on the next line accepts only
  `"typescript"` and `"javascript"`.

Miller therefore already knows the dialect names in one place and not in the
next. The inconsistency is consumer-side, not producer-side.

## Options

### Option A — the artifact keeps dialect names; consumers map at read time

`files.language` and `symbols.language` stay the registry spec name. Miller
folds dialects into families where its own query needs a family.

### Option B — the artifact publishes a `base_language` fact

A new column on `files` and `symbols`, or a new column on
`language_capabilities`, carrying `typescript` for `tsx` and `javascript` for
`jsx`. Consumers compare base languages and never write a mapping table.

Option B has a real case and it is not a straw man. The producer owns the
grammar registry, so it knows the family better than any consumer guesses. A
mapping table copied into each consumer will drift when this repo adds a spec.
And `CLAUDE.md` says to prefer clean new contracts over compatibility modes
while this repo is not yet consumed in production, which makes a schema bump
cheaper today than it will ever be again. If `base_language` were the right
fact, now would be the right time.

It is not the right fact, for five reasons:

1. **Base language is not something extraction knows.** The registry has no
   family relation to publish (`specs.rs:318-334`, `aliases: &[]` for every
   spec). `tsx` and `typescript` share `tree-sitter-typescript` but use
   different parser entry points; `jsx` and `javascript` share one parser and
   are still separate specs; `vue` is parsed with `tree-sitter-html`. Deriving a
   base from the parser crate would make `vue`'s base `html`, which serves
   nobody.
2. **Any answer is a consumer policy, and consumers disagree.** A CT selector
   wants `tsx ≈ typescript`. The capability ledger and the language data-quality
   report must keep them apart, because they are separately claimed rows in
   `fixtures/extraction/capabilities.json` with separate golden evidence.
   Publishing one grouping picks one consumer's policy and forces it on the
   rest. That is the same boundary this repo drew when it stopped materializing
   workspace-global reference resolution
   ([2026-08-18-resolution-write-path-retirement.md](2026-08-18-resolution-write-path-retirement.md)):
   facts here, policy at query time.
3. **It duplicates data already published.** `language_capabilities` carries
   `extensions_json` per language
   (`crates/julie-extract-artifact/src/schema.rs:466-469`, exported to JSONL at
   `crates/julie-extract-artifact/src/jsonl.rs:279-326`). Every artifact already
   ships the complete language-to-extension map, versioned with the binary that
   wrote it. A consumer can build any grouping it wants from that plus its own
   policy without a new column.
4. **Row cost.** `base_language` on `symbols` would repeat a per-language
   constant on every symbol row, against the compactness goal in
   [schema-principles.md](../architecture/schema-principles.md).
5. **The clean-contract rule points the other way.** That rule exists to stop us
   keeping producer-side shims for a consumer's convenience. Honest per-file
   language *is* the clean contract; `base_language` would be the shim, added
   because one consumer's string compare is too narrow.

## Decision

1. `files.language` and `symbols.language` keep the registry `LanguageSpec.name`
   exactly. `.tsx` publishes `tsx`, `.jsx` publishes `jsx`. No `base_language`
   column, no schema version bump, no `EXTRACTION_IDENTITY_EPOCH` bump.
2. The published language is a **per-file identity, not a family**. A consumer
   that needs family behavior maps at read time and owns that mapping.
3. `language_capabilities.extensions_json` is the supported way for a consumer
   to learn which extensions a language claims. A consumer's own extension map
   is a fallback that may disagree with the artifact; where they disagree the
   indexed value wins.
4. Adding a `LanguageSpec` is a contract-visible change even when it changes no
   DDL, because it adds a value that consumers may see in `files.language`.

## Miller-side change

Owner: Miller. This section is the complete hand-off.

**File:** `src/Miller.Testing/Selection/ContinuousTestImpactSelector.cs`

**Change 1 — widen the comparison (replaces `:1087-1099`).** Fold language
values into families before comparing, instead of the single `IsCsharpOrRazor`
case:

```csharp
private static bool LanguagesAreCompatible(string? changedLanguage, string? testLanguage)
{
    if (string.IsNullOrEmpty(changedLanguage) || string.IsNullOrEmpty(testLanguage))
        return false;

    return string.Equals(
        LanguageFamily(changedLanguage),
        LanguageFamily(testLanguage),
        StringComparison.Ordinal);
}

private static string LanguageFamily(string language) => language.ToLowerInvariant() switch
{
    "jsx" or "javascript" => "javascript",
    "tsx" or "typescript" => "typescript",
    "razor" or "csharp" => "dotnet",
    "c" or "cpp" => "c-family",
    string other => other,
};
```

Mapping table, with the reason for each row:

| Family | Members | Why |
| --- | --- | --- |
| `javascript` | `jsx`, `javascript` | separate specs, one parser, one file family |
| `typescript` | `tsx`, `typescript` | separate specs, one parser crate |
| `dotnet` | `razor`, `csharp` | preserves today's `IsCsharpOrRazor` behavior |
| `c-family` | `c`, `cpp` | a `.h` file publishes `c` or `cpp` depending on a content sniff, so the same path can change language between scans |

`typescript` and `javascript` stay **separate** families. Folding them would
also match a `.tsx` change to a `.js` test, which is a real pattern but a much
wider net at a tier that already scores `0.35` confidence. Treat ts↔js folding
as a later change justified by measured misses, not part of this one.

**Change 2 — make the fallback agree with the artifact (replaces `:1247-1267`).**
`LanguageFromPath` must answer what the artifact would publish for the same
path, so that an indexed and a non-indexed file of the same extension never
compare differently. Change `".jsx" => "jsx"` and `".tsx" => "tsx"`, and add the
extensions the artifact classifies but this map currently answers `null` for:

| Extension | Answer | Source in this repo |
| --- | --- | --- |
| `.jsx` | `jsx` | `specs.rs:69-70` |
| `.tsx` | `tsx` | `specs.rs:53-54` |
| `.mts`, `.cts` | `typescript` | `specs.rs:45-46` |
| `.mjs`, `.cjs` | `javascript` | `specs.rs:61-62` |
| `.vue` | `vue` | `specs.rs:93-94` |
| `.c`, `.h` | `c` | `specs.rs:13-14`; see the `.h` caveat below |
| `.cpp`, `.cc`, `.cxx`, `.hpp`, `.hh`, `.hxx` | `cpp` | `specs.rs:21-22` |

`.h` cannot be resolved from the extension alone: this repo answers `cpp` when
the header contains C++ syntax and `c` otherwise
(`language_spec/mod.rs:316-318`). The fallback answering `c` is safe because the
`c-family` fold covers the disagreement.

**Change 3 — audit the other language compares.** Both other call sites of
`LanguageFromPath` inside the selector (`:1388` in `ChangedPathStem.FromPath`,
`:1472` in the `file_language` metadata fallback) inherit the fix. The
resolution path does **not**, and is tracked separately:

- `src/Miller.Core/Resolution/ImportBinding.cs:104-109` — add `"tsx"` and
  `"jsx"` to the extension-candidate switch.
- `src/Miller.Indexing/Resolution/RevisionFactCacheLoader.cs:680-684` — the
  `file.Language == language` ordinal compare should use the same family fold.
- `src/Miller.Core/Resolution/ResolutionPolicy.cs:77` — `IsTier2Language` omits
  `jsx` and `tsx` while `IsEsModuleLanguage` on line 76 includes them.

**Change 4 — acceptance tests.** Named cases for the selector change:

- changed `src/Button.tsx` (indexed), test case backed by `src/Button.test.ts`
  (indexed) — `path_stem` evidence is produced.
- changed `src/Button.jsx` (indexed), test backed by `src/Button.test.js` —
  evidence is produced.
- changed `src/Button.tsx` **not** in the index, test backed by
  `src/Button.test.tsx` in the index — evidence is produced, proving the
  fallback and the indexed value now agree.
- changed `src/Button.tsx`, test backed by `src/Widget.test.ts` — no evidence;
  the stem guard still holds.
- existing razor and csharp cases keep passing unchanged.

## Other dialect and language splits in the registry

Every registry spec was checked for the same hazard, not only `jsx`/`tsx`.

| Spec | Hazard | Disposition |
| --- | --- | --- |
| `tsx`, `jsx` | dialect name vs. Miller's base-language map | the reported defect; fixed by the family fold |
| `typescript` `.mts`/`.cts`, `javascript` `.mjs`/`.cjs` | not a dialect split; Miller's map has no entry, so the fallback answers `null` and every compare fails | same class of defect, fixed in Change 2 |
| `vue` | Miller's map has no `.vue`, so an unindexed `.vue` answers `null` | add the extension. Do **not** fold `vue` into `typescript`: a `.vue` file is template plus script plus style parsed with `tree-sitter-html` (`specs.rs:93-94`), so a `Button.vue` / `Button.spec.ts` pair is a cross-language companion rule like razor↔csharp, decided on its own evidence |
| `razor` | `.razor` and `.cshtml` both publish `razor` (`specs.rs:253-254`) and Miller's map already agrees; razor↔csharp is already folded | no hazard |
| `qml`, `qmldir` | `qmldir` is a filename rule with no extension (`language_spec/mod.rs:303-309`); its `language_capabilities` extensions list is empty, and `Path.GetExtension` answers `""` so Miller's fallback returns `null` | genuinely a different language (a module manifest), so no fold. The `null` fallback is a real gap but low value: a file literally named `qmldir` rarely shares a stem with a test |
| `c`, `cpp` | `.h` publishes `c` or `cpp` by content sniff (`language_spec/mod.rs:316-318`), so the artifact language for one path is content-dependent and no extension map can reproduce it | folded into `c-family` |
| `html`, `css`, `markdown`, `json`, `toml`, `yaml`, `xml`, and the remaining specs | one name, plain extensions, no split | no hazard |

## Consequences

- Doc-only in this repo. No schema change, no contract version bump, no epoch
  bump, no golden regeneration.
- Miller owns the family table and must update it when this repo adds a spec
  whose extensions belong to a family already folded. Adding a spec is the
  named trigger.
- If a second consumer needs the same fold, the answer is a shared policy
  document, not a producer column. Revisit this decision only if three or more
  consumers need the same grouping and drift becomes measurable.
- The resolution-path findings (`ImportBinding`, `RevisionFactCacheLoader`,
  `ResolutionPolicy`) are Miller defects surfaced by this audit. They are listed
  here so the Miller session fixes the class, not one call site.

## Pointers

- Plan: [2026-08-25-ct-test-detection-readiness.md](../plans/2026-08-25-ct-test-detection-readiness.md)
- Boundary: [continuous-testing-evidence-boundary.md](../architecture/continuous-testing-evidence-boundary.md)
- Language identity contract note: [schema-principles.md](../architecture/schema-principles.md)
- Registry: `crates/julie-extractors/src/language_spec/specs.rs`
- Precedent for facts-here-policy-at-query-time:
  [2026-08-18-resolution-write-path-retirement.md](2026-08-18-resolution-write-path-retirement.md)
