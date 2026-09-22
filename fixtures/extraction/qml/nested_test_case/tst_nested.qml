import QtQuick
import QtTest

Item {
    id: root
    width: 200

    SignalSpy { id: toggledSpy; signalName: "toggled" }

    TestCase {
        id: toggleTests
        name: "ToggleSwitchTests"
        when: windowShown
        function initTestCase() { verify(true) }
        function cleanup() { toggledSpy.clear() }
        function test_click_data() { return [] }
        function test_click(data) { compare(toggledSpy.count, 1) }
        function benchmark_toggle() { }
    }

    QtTest.TestCase {
        name: "KeyboardTests"
        function test_space() { keyClick(Qt.Key_Space) }
    }
}
