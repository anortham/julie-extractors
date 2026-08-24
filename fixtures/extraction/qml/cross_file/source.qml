import QtQuick 2.15
import QtQuick.Controls 2.15 as Controls
import "components" as Components
import "./js/helpers.js" as Helpers
import "OtherModule"

Item {
    id: root
    property alias title: localCard.title
    property string status: localHelper()
    signal completed(string value)

    function localHelper() {
        return "ready"
    }

    function entry() {
        localHelper()
        external_helper()
        return localCard.title
    }

    LocalCard {
        id: localCard
        title: Helpers.label("Card")
    }

    Components.RemoteCard {
        id: remoteCard
    }
}
