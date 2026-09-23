import QtQuick
import QtQuick.LocalStorage as Sql
import "utils.js" as Utils
import "math.mjs" as MathLib
import org.kde.kirigami as Kirigami

/*!
    \qmltype Dashboard
    \brief Shows the status panels.
*/
@Deprecated { reason: "Use NewDashboard" }
Rectangle {
    id: root

    /** Emitted when the user dismisses the card. */
    signal dismissed(string reason)
    /// Emitted after a refresh.
    @Since { version: "2.1" }
    signal refreshed()

    /** Loading states. */
    enum Status { Idle = 0, Busy = 1, Done = 2 }
    enum Mode { Compact, Full = 3 }

    /*! The urgency level. */
    required property int urgency
    readonly property color tint: Qt.rgba(0.1, 0.2, 0.3, 1)
    default property list<Item> content
    property Kirigami.Action action
    property alias label: body.text
    property QtObject wifi: QtObject { property bool connected: true }

    @Designer { importance: 100 }
    property bool hot: area.containsMouse || focus

    Behavior on opacity { NumberAnimation { duration: 200 } }
    Kirigami.FancyBehavior on x { }

    Text {
        id: body
        font { bold: true; pixelSize: 12 }
        border.width: 1
    }

    /** Hover help. */
    @Designer { importance: 10 }
    ToolTip {
        background: Rectangle { color: "black" }
    }

    StackView { id: stack; initialItem: "pages/Home.qml" }
    Loader { source: Qt.resolvedUrl("pages/Settings.qml") }
    MouseArea { id: area; onClicked: root.save() }

    /*! Saves the dashboard. */
    function save() {
        Utils.clamp(1, 2, 3)
        Sql.LocalStorage.openDatabaseSync("notes", "1.0", "Notes", 1000)
    }

    @Deprecated { reason: "old" }
    function openDetails(id, count) {
        function inner(x) { return MathLib.norm(x) }
        let total = count + 1
        var request = new XMLHttpRequest()
        request.open("GET", "https://api.example.com/items")
        stack.push(Qt.resolvedUrl("pages/Details.qml"), { itemId: id })
        const dialog = Qt.createComponent("Dialog.qml")
        return inner(total)
    }

    function onRefreshedHandler() { }

    Component.onCompleted: {
        save()
        root.refreshed.connect(onRefreshedHandler)
    }
    onWidthChanged: relayout()
    Keys.onReturnPressed: if (enabled) save()
    onPressed: (mouse) => { track(mouse) }
}
