pragma Singleton
pragma ComponentBehavior: Bound

import QtQuick 2.15

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
    }

    function close(reason) {
        service.closeRequested(reason)
    }
}
