// Aliased import: `produceWidget as pw`. The extractor stores the imported name
// under the camelCase metadata key `importedName`, which the current
// `import_binding` loader does not read, so `pw()` does NOT resolve today.
//
// This fixture documents Task 5 concern #3 and is asserted by the `#[ignore]`d
// test `tier2_aliased_import_resolves_after_lead_fix`. See task-6-report.md
// REQUIRED-LEAD-FIX: teach `resolution::import_binding` to read `importedName`.
import { produceWidget as pw } from "./def";

export function consume(): number {
  return pw();
}
