import QtQuick
import org.example.backend as Backend

Rectangle {
    id: root
    property color background: "transparent"
    property color tooltipBackground: Color.tooltip.background
    readonly property real size: Math.max(Style.font.body, localHelper(4))
    readonly property alias count: view.count

    function localHelper(x: int): int { return x * 2 }
    function refresh() { }
    function save(target: Item, doc: Backend.DocumentModel) { docModel.flush() }

    Backend.DocumentModel { id: docModel }
    ListView { id: view }
    Timer { id: timer; onTriggered: root.refresh() }

    IpcHandler {
        target: "svc"
        function refresh(): void { root.refresh() }
    }

    Text { color: root.background }
}
