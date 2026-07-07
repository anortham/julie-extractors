// Tier 2 (cross-file import) definition half: an exported function reached by a
// named import in `call.ts`. Resolution keys on the import symbol (tier2_import,
// CONFIDENCE_TIER2 = 0.85).
export function produceWidget(): number {
  return 1;
}
