// Tier 1 (same-file): the extractor emits an extraction-time `relationships`
// edge for the intra-file call `alpha()`, which the workspace pass propagates
// onto the co-located call identifier at CONFIDENCE_TIER1 (0.95, tier1_local).
pub fn alpha() {}

pub fn helper() {
    alpha();
}
