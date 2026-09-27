import Testing

@Suite("süite", .tags(.slow, .network), .serialized, .timeLimit(.minutes(1)))
struct SwiftTraitSuite {
    @Test("café", .disabled("flaky"), arguments: [1, 2])
    func disabledWithDisplayName() {}

    @Test(.disabled(if: flag))
    func disabledConditionally() {}

    @Test(.enabled(if: featureIsReady))
    func enabledConditionally() {}

    @Test(.serialized)
    func serialized() {}

    @Test(.timeLimit(.minutes(1)))
    func limited() {}
}
