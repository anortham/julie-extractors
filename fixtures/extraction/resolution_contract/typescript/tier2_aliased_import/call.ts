// Aliased import: `produceWidget as pw`. The resolver reads the imported name
// from `importedName` and resolves the relative module source before trusting
// the alias.
import { produceWidget as pw } from "./def";

export function consume(): number {
  return pw();
}
