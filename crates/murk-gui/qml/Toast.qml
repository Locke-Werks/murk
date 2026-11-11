import QtQuick
import QtQuick.Layouts
import Murk

// In-window mention card; click to jump to the buffer it came from.
Rectangle {
    id: root
    property string heading: ""
    property string body: ""
    property int target: -1

    function show(h, b, idx) {
        heading = h
        body = b
        target = idx
        shown = true
        hide.restart()
    }
    property bool shown: false

    width: 340
    height: col.implicitHeight + 24
    radius: 14
    color: Theme.raised
    border.color: Theme.alpha(Theme.hl, 0.6)
    opacity: shown ? 1 : 0
    visible: opacity > 0
    y: shown ? 0 : -20
    Behavior on opacity { NumberAnimation { duration: Theme.med } }
    Behavior on y { NumberAnimation { duration: Theme.med; easing.type: Easing.OutCubic } }

    Timer { id: hide; interval: 6000; onTriggered: root.shown = false }

    RowLayout {
        anchors.fill: parent
        anchors.margins: 12
        spacing: 12
        Rectangle { Layout.preferredWidth: 4; Layout.fillHeight: true; radius: 2; color: Theme.hl }
        ColumnLayout {
            id: col
            Layout.fillWidth: true
            spacing: 3
            Text { text: root.heading; color: Theme.hl; font.family: Theme.uiFont; font.pixelSize: 13; font.weight: Font.DemiBold; elide: Text.ElideRight; Layout.fillWidth: true }
            Text { text: root.body; color: Theme.text; font.family: Theme.uiFont; font.pixelSize: 13; wrapMode: Text.Wrap; maximumLineCount: 3; elide: Text.ElideRight; Layout.fillWidth: true }
        }
    }
    MouseArea {
        anchors.fill: parent
        cursorShape: Qt.PointingHandCursor
        onClicked: { if (root.target >= 0) Core.ctl.switchTo(root.target); root.shown = false }
    }
}
