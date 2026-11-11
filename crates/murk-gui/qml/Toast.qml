import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import Murk

// In-window mention card. Click to jump to the buffer; or answer right here
// without leaving the conversation you are in.
Rectangle {
    id: root
    property string heading: ""
    property string body: ""
    property int target: -1
    property bool sent: false
    // Mentions are highlighted; other kinds (a friend coming online) pass their own colour.
    property color tint: Theme.hl
    property bool replyable: true
    // When set, clicking opens a conversation with this nick instead of jumping to a buffer.
    property string query: ""
    signal gone()

    function show(h, b, idx) {
        heading = h
        body = b
        // A fresh message from the same buffer keeps whatever reply is half typed.
        if (idx !== target || sent) {
            sent = false
            reply.text = ""
        }
        target = idx
        shown = true
        hide.restart()
    }
    function dismiss() {
        shown = false
        reply.focus = false
    }
    property bool shown: false
    // Hold still while the user is reading or typing.
    readonly property bool engaged: hover.hovered || reply.activeFocus || reply.text.length > 0

    width: 360
    height: col.implicitHeight + 24
    radius: 14
    color: Theme.raised
    border.color: Theme.alpha(root.tint, 0.6)
    opacity: shown ? 1 : 0
    visible: opacity > 0
    transform: Translate { y: root.shown ? 0 : -20; Behavior on y { NumberAnimation { duration: Theme.med; easing.type: Easing.OutCubic } } }
    onVisibleChanged: if (!visible && !shown) gone()
    Behavior on opacity { NumberAnimation { duration: Theme.med } }

    Timer { id: hide; interval: 6000; onTriggered: if (root.engaged) restart(); else root.dismiss() }
    Timer { id: afterSend; interval: 900; onTriggered: root.dismiss() }

    HoverHandler { id: hover }
    MouseArea {
        anchors.fill: parent
        cursorShape: Qt.PointingHandCursor
        onClicked: {
            if (root.query.length)
                Core.ctl.submit("/query " + root.query)
            else if (root.target >= 0)
                Core.ctl.switchTo(root.target)
            root.dismiss()
        }
    }

    RowLayout {
        anchors.fill: parent
        anchors.margins: 12
        spacing: 12
        Rectangle { Layout.preferredWidth: 4; Layout.fillHeight: true; radius: 2; color: root.tint }
        ColumnLayout {
            id: col
            Layout.fillWidth: true
            spacing: 6
            Text { text: root.heading; color: root.tint; font.family: Theme.uiFont; font.pixelSize: 13; font.weight: Font.DemiBold; elide: Text.ElideRight; Layout.fillWidth: true }
            Text { text: root.body; color: Theme.text; font.family: Theme.uiFont; font.pixelSize: 13; wrapMode: Text.Wrap; maximumLineCount: 3; elide: Text.ElideRight; Layout.fillWidth: true }
            TextField {
                id: reply
                Layout.fillWidth: true
                Layout.topMargin: 2
                visible: root.replyable && root.target >= 0 && !root.sent
                placeholderText: "Quick reply…  Enter to send, Esc to dismiss"
                placeholderTextColor: Theme.faint
                color: Theme.text
                font.family: Theme.uiFont
                font.pixelSize: 13
                leftPadding: 10
                rightPadding: 10
                background: Rectangle {
                    implicitHeight: 32
                    radius: 8
                    color: Theme.alpha(Theme.text, 0.06)
                    border.color: reply.activeFocus ? Theme.accent : "transparent"
                }
                Keys.onReturnPressed: {
                    if (!text.trim().length)
                        return
                    Core.ctl.submitTo(root.target, text)
                    root.sent = true
                    afterSend.restart()
                }
                Keys.onEscapePressed: root.dismiss()
            }
            Text {
                visible: root.sent
                text: "✓ Sent"
                color: Theme.ok
                font.family: Theme.uiFont
                font.pixelSize: 12
                font.weight: Font.DemiBold
            }
        }
    }
}
