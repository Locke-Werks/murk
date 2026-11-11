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

    function show(h, b, idx) {
        heading = h
        body = b
        target = idx
        sent = false
        reply.text = ""
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
    border.color: Theme.alpha(Theme.hl, 0.6)
    opacity: shown ? 1 : 0
    visible: opacity > 0
    y: shown ? 0 : -20
    Behavior on opacity { NumberAnimation { duration: Theme.med } }
    Behavior on y { NumberAnimation { duration: Theme.med; easing.type: Easing.OutCubic } }

    Timer { id: hide; interval: 6000; onTriggered: if (root.engaged) restart(); else root.dismiss() }
    Timer { id: afterSend; interval: 900; onTriggered: root.dismiss() }

    HoverHandler { id: hover }
    MouseArea {
        anchors.fill: parent
        cursorShape: Qt.PointingHandCursor
        onClicked: { if (root.target >= 0) Core.ctl.switchTo(root.target); root.dismiss() }
    }

    RowLayout {
        anchors.fill: parent
        anchors.margins: 12
        spacing: 12
        Rectangle { Layout.preferredWidth: 4; Layout.fillHeight: true; radius: 2; color: Theme.hl }
        ColumnLayout {
            id: col
            Layout.fillWidth: true
            spacing: 6
            Text { text: root.heading; color: Theme.hl; font.family: Theme.uiFont; font.pixelSize: 13; font.weight: Font.DemiBold; elide: Text.ElideRight; Layout.fillWidth: true }
            Text { text: root.body; color: Theme.text; font.family: Theme.uiFont; font.pixelSize: 13; wrapMode: Text.Wrap; maximumLineCount: 3; elide: Text.ElideRight; Layout.fillWidth: true }
            TextField {
                id: reply
                Layout.fillWidth: true
                Layout.topMargin: 2
                visible: !root.sent
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
