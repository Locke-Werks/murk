import QtQuick

// Play and pause, drawn rather than typed: the font fallback for the dingbat
// pause bars shows garbage on Windows.
Item {
    id: root
    property bool playing: false
    property color color: "white"
    property real size: 14
    width: size
    height: size

    Row {
        visible: root.playing
        anchors.centerIn: parent
        spacing: root.size * 0.22
        Rectangle { width: root.size * 0.28; height: root.size; radius: 1.5; color: root.color }
        Rectangle { width: root.size * 0.28; height: root.size; radius: 1.5; color: root.color }
    }
    Canvas {
        visible: !root.playing
        anchors.fill: parent
        // Optical centre: a triangle looks left-heavy when centred on its box.
        anchors.leftMargin: root.size * 0.12
        onPaint: {
            const c = getContext("2d")
            c.reset()
            c.fillStyle = root.color
            c.beginPath()
            c.moveTo(0, 0)
            c.lineTo(width, height / 2)
            c.lineTo(0, height)
            c.closePath()
            c.fill()
        }
        onWidthChanged: requestPaint()
        Connections { target: root; function onColorChanged() { parent.requestPaint() } }
    }
}
