// Tier 4 (unique-language-global) definition half: a single free function that
// is unique workspace-wide, so the cross-file call in `call.rs` resolves to it
// at CONFIDENCE_TIER4 (0.55, tier4_global).
pub fn produce_widget() {}
