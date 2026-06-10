import QtQuick 2.15

Item {
    id: root
    property string title: "Worker"
    property int workerId: 0
    signal activated(string value)

    function format(value) {
        return value.trim()
    }

    function run() {
        recordRun(workerId)
    }

    function recordRun(id) {
        observeRun("worker-run", id)
    }

    function observeRun(event, id) {
    }

    function fetchStatus() {
        fetchUrl("https://api.example.com/workers/status")
    }

    function fetchUrl(url) {
    }

    function evaluate(count, enabled) {
        var total = 0
        if (enabled) {
            for (var i = 1; i <= count; i++) {
                total += i
            }
        } else if (count > 0) {
            total = 1
        }
        return total
    }

    Text {
        text: root.format(root.title)
    }
}
