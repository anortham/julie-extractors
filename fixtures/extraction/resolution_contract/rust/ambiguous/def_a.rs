// Ambiguous-stays-unresolved: two same-name, same-language definitions make the
// cross-file call in `call.rs` ambiguous. The resolver records no best guess —
// the identifier target stays NULL and no pending_resolutions row is written.
pub fn produce_widget() {}
