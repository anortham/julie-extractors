import Testing

@Suite("Math suite")
struct MathSuite {
    let subject: Int

    init() {
        subject = 0
    }

    deinit {
        print("done")
    }

    @Test func addsTwoNumbers() {
        #expect(2 + 2 == 4)
    }

    @Test("adds many", arguments: [1, 2, 3])
    func addsManyNumbers(value: Int) {
        #expect(value > 0)
    }

    func makeSubject() -> Int {
        subject
    }
}

@Test func addsAtTopLevel() {
    #expect(1 == 1)
}

final class NetworkClient {
    init() { }

    deinit { }

    func testConnection() -> Bool {
        true
    }

    func setUp() { }
}
