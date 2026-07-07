// Tier 1 (same-file) for Java: the intra-class call alpha() is an
// extraction-time relationship propagated onto the co-located identifier.
class Sample {
    static void alpha() {}

    static void helper() {
        alpha();
    }
}
