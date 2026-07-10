import QtQuick 2.0
import QtTest 1.0

TestCase {
    name: "CalculatorTests"

    function initTestCase() {
    }

    function test_addition() {
        compare(2 + 2, 4)
    }

    function verify_addition() {
        return 2 + 2 === 4
    }
}
