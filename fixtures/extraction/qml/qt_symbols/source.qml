pragma Singleton
pragma ComponentBehavior: Bound

import QtQuick 2.15
import QtQuick.Controls 2.15 as QQC2
import org.kde.kirigami 2.20 as Kirigami

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

        onLabelChanged: console.log(label)

        Kirigami.FormData.label: "Device"

        Behavior on color {
            NumberAnimation {
                duration: 120
            }
        }

        Connections {
            target: service

            function onCloseRequested(reason) {
                console.log(reason)
            }

            onReady: badgeRow.gap = 8
        }

        Row {
            id: badgeRow

            Layout.fillWidth: true

            property int gap: 4

            Text {
                text: badgeRow.gap
            }

            QQC2.Button {
                text: "reload"

                onClicked: service.close("badge")
            }
        }
    }

    function close(reason) {
        service.closeRequested(reason)
    }
}
