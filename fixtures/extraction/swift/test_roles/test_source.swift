import XCTest
import Quick
import Nimble

final class CalculatorTests: XCTestCase {
    override func setUp() {
        super.setUp()
    }

    override func setUpWithError() throws {
        try super.setUpWithError()
    }

    override func tearDown() {
        super.tearDown()
    }

    override func tearDownWithError() throws {
        try super.tearDownWithError()
    }

    func testAddition() {
        XCTAssertEqual(2 + 2, 4)
    }

    func calculateTotal() -> Int {
        4
    }
}

extension CalculatorTests {
    func testSubtraction() {
        XCTAssertEqual(4 - 2, 2)
    }
}

struct CalculatorSupport {
    func testHelperNamedLikeACase() { }

    func setUp() { }
}

extension CalculatorSupport {
    func testHelperInAnExtension() { }
}

sharedExamples("a calculator") {
    it("starts at zero") {
        expect(0).to(equal(0))
    }
}

describe("calculator") {
    beforeSuite { }

    context("addition") {
        beforeEach { }

        aroundEach { runExample in
            runExample()
        }

        it("adds two numbers") {
            expect(2 + 2).to(equal(4))
        }

        afterEach { }
    }

    itBehavesLike("a calculator")

    afterSuite { }
}

func itNamedButNotCalled() { }
