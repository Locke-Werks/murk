import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import Murk

// Find somewhere to talk: the network's channel list, searchable, one click to join.
Popup {
    id: root
    modal: true
    // Stop blocking the window as soon as closing starts, not when the fade ends.
    onAboutToHide: modal = false
    onAboutToShow: modal = true
    focus: true
    width: Math.min(720, parent.width - 80)
    height: Math.min(680, parent.height - 80)
    x: (parent.width - width) / 2
    y: (parent.height - height) / 2
    padding: 0
    // The whole list is fetched once per network and searched here: servers rate-limit
    // LIST, and local search is instant and can match topics too.
    property var all: []
    property string fetchedFor: ""
    property real fetchedAt: 0
    property bool loading: false
    property bool failed: false
    readonly property string asked: query.text.trim().replace(/^#+/, "").toLowerCase()
    readonly property var channels: {
        if (!asked.length)
            return all
        const words = asked.split(/\s+/)
        return all.filter(c => {
            const hay = (c.name + " " + c.topic).toLowerCase()
            return words.every(w => hay.indexOf(w) >= 0)
        })
    }

    function fetch() {
        loading = true
        failed = false
        // A small user floor (ELIST U) skips thousands of empty channels.
        Core.ctl.submit("/list >2")
        timeout.restart()
    }

    onOpened: {
        query.text = ""
        query.forceActiveFocus()
        const stale = Date.now() - fetchedAt > 10 * 60 * 1000
        if (fetchedFor !== Core.ctl.network || stale || !all.length)
            fetch()
    }

    Timer { id: timeout; interval: 20000; onTriggered: { root.loading = false; root.failed = !root.all.length } }
    Connections {
        target: Core.ctl
        function onChannelList(json) {
            const list = JSON.parse(json)
            root.loading = false
            // An empty answer right after a refusal means the server is rate limiting us.
            if (!list.length && root.all.length)
                return
            root.all = list
            root.failed = !list.length
            root.fetchedFor = Core.ctl.network
            root.fetchedAt = Date.now()
        }
    }

    Overlay.modal: Rectangle { color: Theme.alpha("#000000", Theme.dark ? 0.5 : 0.25) }
    enter: Transition {
        NumberAnimation { property: "opacity"; from: 0; to: 1; duration: Theme.fast }
        NumberAnimation { property: "scale"; from: 0.97; to: 1; duration: Theme.med; easing.type: Easing.OutCubic }
    }
    background: Rectangle { radius: 18; color: Theme.raised; border.color: Theme.line }

    readonly property int maxUsers: channels.length ? Math.max(1, channels[0].users) : 1

    contentItem: ColumnLayout {
        spacing: 0
        RowLayout {
            Layout.fillWidth: true
            Layout.margins: 22
            Layout.bottomMargin: 10
            Text { text: "Explore " + (Core.ctl.network || "channels"); color: Theme.text; font.family: Theme.displayFont; font.pixelSize: 22; font.weight: Font.DemiBold; Layout.fillWidth: true }
            IconButton { glyph: "✕"; tip: "Close"; onClicked: root.close() }
        }
        TextField {
            id: query
            Layout.fillWidth: true
            Layout.leftMargin: 22
            Layout.rightMargin: 22
            implicitHeight: 44
            placeholderText: "Search channels: rust, linux, music, gaming…"
            placeholderTextColor: Theme.faint
            color: Theme.text
            font.family: Theme.uiFont
            font.pixelSize: 15
            leftPadding: 14
            background: Rectangle { radius: 12; color: Theme.alpha(Theme.text, 0.06); border.color: query.activeFocus ? Theme.accent : "transparent" }
            Keys.onReturnPressed: if (root.channels.length) { Core.ctl.submit("/join " + root.channels[0].name); root.close() }
        }
        Text {
            Layout.leftMargin: 24
            Layout.topMargin: 10
            Layout.bottomMargin: 4
            text: root.loading ? "Fetching the channel list…"
                : root.failed ? "The server did not send a list. It may be rate limiting; try again in a minute."
                : root.channels.length + (root.channels.length === 1 ? " channel" : " channels")
                  + (root.asked.length ? " matching “" + root.asked + "”" : "")
                  + "  ·  Enter joins the top match"
            color: Theme.faint
            font.family: Theme.uiFont
            font.pixelSize: 12
        }
        ListView {
            id: browseList
            SmoothWheel { flick: browseList }
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.bottomMargin: 12
            clip: true
            model: root.channels.slice(0, 500)
            spacing: 2
            ScrollBar.vertical: ScrollBar { width: 6 }
            delegate: Rectangle {
                required property var modelData
                width: ListView.view.width - 24
                x: 12
                height: 62
                radius: 12
                color: rowHover.hovered ? Theme.hover : "transparent"
                HoverHandler { id: rowHover }
                RowLayout {
                    anchors.fill: parent
                    anchors.leftMargin: 12
                    anchors.rightMargin: 12
                    spacing: 14
                    // How busy, at a glance.
                    ColumnLayout {
                        Layout.preferredWidth: 58
                        spacing: 4
                        Text { text: modelData.users; color: Theme.text; font.family: Theme.uiFont; font.pixelSize: 14; font.weight: Font.DemiBold; Layout.alignment: Qt.AlignHCenter }
                        Rectangle {
                            Layout.preferredWidth: 46
                            Layout.preferredHeight: 4
                            Layout.alignment: Qt.AlignHCenter
                            radius: 2
                            color: Theme.alpha(Theme.text, 0.1)
                            Rectangle {
                                width: parent.width * Math.max(0.06, Math.sqrt(modelData.users / root.maxUsers))
                                height: parent.height
                                radius: 2
                                color: Theme.accent
                            }
                        }
                    }
                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 2
                        Text { text: modelData.name; color: Theme.accent; font.family: Theme.monoFont; font.pixelSize: 14; font.weight: Font.DemiBold; elide: Text.ElideRight; Layout.fillWidth: true }
                        Text { text: modelData.topic || "No topic"; color: modelData.topic ? Theme.sub : Theme.faint; font.family: Theme.uiFont; font.pixelSize: 12; elide: Text.ElideRight; Layout.fillWidth: true }
                    }
                    Button {
                        visible: rowHover.hovered
                        text: "Join"
                        onClicked: { Core.ctl.submit("/join " + modelData.name); root.close() }
                        contentItem: Text { text: parent.text; color: Theme.bg; font.family: Theme.uiFont; font.pixelSize: 13; font.weight: Font.DemiBold; horizontalAlignment: Text.AlignHCenter }
                        background: Rectangle { radius: 9; color: Theme.accent; implicitWidth: 70; implicitHeight: 32 }
                    }
                }
                TapHandler { longPressThreshold: 0; onDoubleTapped: { Core.ctl.submit("/join " + modelData.name); root.close() } }
            }
            BusyIndicator {
                anchors.centerIn: parent
                running: root.loading
                visible: running
            }
        }
    }
}
