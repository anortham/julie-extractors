import XCTest
import Quick
import Nimble

final class CalculatorTests: XCTestCase {
    func testAddition() {
        XCTAssertEqual(2 + 2, 4)
    }

    func calculateTotal() -> Int {
        4
    }
}

describe("calculator") {
    beforeEach { }

    it("adds two numbers") {
        expect(2 + 2).to(equal(4))
    }
}

func itNamedButNotCalled() { }
