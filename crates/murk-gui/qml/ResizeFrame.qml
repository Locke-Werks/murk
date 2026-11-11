import QtQuick

// Invisible edges and corners that hand resizing to the OS, for a frameless window.
Item {
    id: root
    required property var window
    readonly property int grip: 6
    visible: window.visibility !== Window.Maximized && window.visibility !== Window.FullScreen
    z: 1000

    component Edge: MouseArea {
        property int edges
        hoverEnabled: true
        acceptedButtons: Qt.LeftButton
        onPressed: root.window.startSystemResize(edges)
    }

    Edge { edges: Qt.LeftEdge; cursorShape: Qt.SizeHorCursor
        x: 0; y: root.grip; width: root.grip; height: root.height - 2 * root.grip }
    Edge { edges: Qt.RightEdge; cursorShape: Qt.SizeHorCursor
        x: root.width - root.grip; y: root.grip; width: root.grip; height: root.height - 2 * root.grip }
    Edge { edges: Qt.TopEdge; cursorShape: Qt.SizeVerCursor
        x: root.grip; y: 0; width: root.width - 2 * root.grip; height: root.grip }
    Edge { edges: Qt.BottomEdge; cursorShape: Qt.SizeVerCursor
        x: root.grip; y: root.height - root.grip; width: root.width - 2 * root.grip; height: root.grip }
    Edge { edges: Qt.TopEdge | Qt.LeftEdge; cursorShape: Qt.SizeFDiagCursor
        x: 0; y: 0; width: root.grip * 2; height: root.grip * 2 }
    Edge { edges: Qt.BottomEdge | Qt.RightEdge; cursorShape: Qt.SizeFDiagCursor
        x: root.width - 2 * root.grip; y: root.height - 2 * root.grip; width: root.grip * 2; height: root.grip * 2 }
    Edge { edges: Qt.TopEdge | Qt.RightEdge; cursorShape: Qt.SizeBDiagCursor
        x: root.width - 2 * root.grip; y: 0; width: root.grip * 2; height: root.grip * 2 }
    Edge { edges: Qt.BottomEdge | Qt.LeftEdge; cursorShape: Qt.SizeBDiagCursor
        x: 0; y: root.height - 2 * root.grip; width: root.grip * 2; height: root.grip * 2 }
}
