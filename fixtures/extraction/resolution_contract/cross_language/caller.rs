// Cross-language-name-collision-stays-unresolved (Rust half): this Rust file
// calls `shared_widget`, a name that is defined ONLY in the sibling Python file
// `other.py`. Tier 4 is restricted to same-language candidates, so a global
// unique match across languages is forbidden — the call must stay unresolved
// (no cross-language best guess).
pub fn consume() {
    shared_widget();
}
