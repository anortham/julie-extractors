import QtQuick

QtObject {
    function target() {}
    function lexicalCaller() { target(); }
    function parameterShadow(target) { target(); }
    function unresolvedReceiver(item) { item.refresh(); }
}
