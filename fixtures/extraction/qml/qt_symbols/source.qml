pragma Singleton
pragma ComponentBehavior: Bound

import QtQuick 2.15
import QtQuick.Controls 2.15 as QQC2

QtObject {
    id: service

    property var shellValues: {
        "shell": "zsh",
        "editor": "vim"
    }

    signal closeRequested(string reason)
    signal ready()

    component DeviceBadge: Rectangle {
        required deviceItem

        property string label: "device"

        signal activated(int index, string name)

        Behavior on color {
            NumberAnimation {
                duration: 120
            }
        }

        Row {
            id: badgeRow

            property int gap: 4

            Text {
                text: badgeRow.gap
            }

            QQC2.Button {
                text: "reload"
            }
        }
    }

    function close(reason) {
        service.closeRequested(reason)
    }
}
