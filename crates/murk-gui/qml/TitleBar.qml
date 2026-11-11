import QtQuick
import Murk

// Our own window chrome: drag to move (with OS snapping), double-click to maximize,
// and caption buttons drawn in the theme instead of the system's.
Rectangle {
    id: root
    required property var window
    readonly property bool maxed: window.visibility === Window.Maximized
    height: 34
    color: Theme.alpha(Theme.panel, 0.92)

    Rectangle { anchors.bottom: parent.bottom; width: parent.width; height: 1; color: Theme.line }

    // Drag anywhere that is not a button.
    DragHandler {
        target: null
        grabPermissions: PointerHandler.TakeOverForbidden
        onActiveChanged: if (active) root.window.startSystemMove()
    }
    TapHandler {
        longPressThreshold: 0
        onDoubleTapped: root.toggleMax()
    }

    function toggleMax() {
        if (maxed)
            window.showNormal()
        else
            window.showMaximized()
    }

    Row {
        anchors.left: parent.left
        anchors.leftMargin: 14
        anchors.verticalCenter: parent.verticalCenter
        spacing: 8
        Rectangle {
            width: 12; height: 12; radius: 4
            anchors.verticalCenter: parent.verticalCenter
            rotation: 45
            gradient: Gradient {
                orientation: Gradient.Horizontal
                GradientStop { position: 0; color: Theme.accent }
                GradientStop { position: 1; color: Theme.accent2 }
            }
        }
        Text {
            anchors.verticalCenter: parent.verticalCenter
            text: root.window.title
            color: root.window.active ? Theme.sub : Theme.faint
            font.family: Theme.uiFont
            font.pixelSize: 12
            elide: Text.ElideRight
            width: Math.min(implicitWidth, root.width - 220)
        }
    }

    component Caption: Rectangle {
        id: cap
        property string glyph
        property bool danger: false
        signal clicked()
        width: 46
        height: root.height
        color: hov.hovered ? (danger ? "#e81123" : Theme.hover) : "transparent"
        Behavior on color { ColorAnimation { duration: Theme.fast } }
        Text {
            anchors.centerIn: parent
            text: cap.glyph
            color: cap.danger && hov.hovered ? "white" : Theme.sub
            font.pixelSize: 13
        }
        HoverHandler { id: hov }
        TapHandler { longPressThreshold: 0; onTapped: cap.clicked() }
    }

    Row {
        anchors.right: parent.right
        anchors.top: parent.top
        Caption { glyph: "—"; onClicked: root.window.showMinimized() }
        Caption {
            onClicked: root.toggleMax()
            // Drawn rather than a font glyph: the box characters vary wildly between fonts.
            Item {
                anchors.centerIn: parent
                width: 11; height: 11
                Rectangle {
                    visible: root.maxed
                    x: 3; y: 0; width: 8; height: 8
                    color: "transparent"; border.color: Theme.sub; border.width: 1
                }
                Rectangle {
                    x: 0; y: root.maxed ? 3 : 1
                    width: root.maxed ? 8 : 10; height: root.maxed ? 8 : 10
                    color: Theme.alpha(Theme.panel, 1); border.color: Theme.sub; border.width: 1
                }
            }
        }
        Caption { glyph: "✕"; danger: true; onClicked: root.window.close() }
    }
}
