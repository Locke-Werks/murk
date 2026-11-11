import QtQuick
import Murk

// A short burst of confetti, for 🎉 and good news.
Item {
    id: root
    function burst() {
        for (let i = 0; i < 70; i++)
            piece.createObject(root, { startX: width * (0.3 + Math.random() * 0.4) })
    }

    Component {
        id: piece
        Rectangle {
            id: p
            property real startX: 0
            readonly property real drift: (Math.random() - 0.5) * root.width * 0.8
            width: 6 + Math.random() * 6
            height: width * (0.4 + Math.random() * 0.6)
            radius: Math.random() < 0.3 ? width / 2 : 1
            color: [Theme.accent, Theme.accent2, Theme.hl, Theme.ok, Theme.bad][Math.floor(Math.random() * 5)]
            x: startX
            y: root.height * 0.55
            ParallelAnimation {
                running: true
                onFinished: p.destroy()
                NumberAnimation { target: p; property: "x"; to: p.startX + p.drift; duration: 1800; easing.type: Easing.OutQuad }
                SequentialAnimation {
                    NumberAnimation { target: p; property: "y"; to: root.height * (0.05 + Math.random() * 0.25); duration: 650; easing.type: Easing.OutQuad }
                    NumberAnimation { target: p; property: "y"; to: root.height + 20; duration: 1150 + Math.random() * 500; easing.type: Easing.InQuad }
                }
                NumberAnimation { target: p; property: "rotation"; to: (Math.random() - 0.5) * 1080; duration: 1800 }
                SequentialAnimation {
                    PauseAnimation { duration: 1300 }
                    NumberAnimation { target: p; property: "opacity"; to: 0; duration: 500 }
                }
            }
        }
    }
}
