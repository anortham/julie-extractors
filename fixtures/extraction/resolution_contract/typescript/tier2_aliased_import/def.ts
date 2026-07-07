// Tier 2 aliased-import definition half. The importer in `call.ts` renames the
// binding, so the imported name differs from the local binding. This isolates
// tier 2 (the alias name has no global definition, so tier 4 cannot resolve it).
export function produceWidget(): number {
  return 1;
}
