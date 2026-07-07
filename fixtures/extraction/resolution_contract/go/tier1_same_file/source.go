// Tier 1 (same-file) for Go: the intra-file call alpha() is an extraction-time
// relationship propagated onto the co-located identifier.
package sample

func alpha() {}

func helper() { alpha() }
