// Cross-file call to the ambiguous `produce_widget` (two candidates) — must stay
// unresolved (no best guess).
pub fn consume() {
    produce_widget();
}
