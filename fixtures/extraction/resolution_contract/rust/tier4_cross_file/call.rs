// Tier 4 (unique-language-global) reference half: a cross-file call is deferred
// to a `pending_relationships` row (no same-file target), so the workspace pass
// must resolve it against the unique `produce_widget` in `def.rs`.
pub fn consume() {
    produce_widget();
}
