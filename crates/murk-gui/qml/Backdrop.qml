import QtQuick
import QtQuick.Shapes
import Murk

// Slow-drifting colored fog behind the chat, the thing the client is named for.
Item {
    id: root
    clip: true
    property real strength: Theme.dark ? 0.16 : 0.35

    Repeater {
        model: 3
        Shape {
            id: blob
            required property int index
            readonly property real r: Math.max(root.width, root.height) * (0.55 + index * 0.12)
            width: r * 2
            height: r * 2
            opacity: root.strength
            x: root.width * [0.75, -0.15, 0.35][index] - r + dx
            y: root.height * [-0.1, 0.55, 1.05][index] - r + dy
            property real dx: 0
            property real dy: 0
            layer.enabled: false

            ShapePath {
                strokeWidth: -1
                fillGradient: RadialGradient {
                    centerX: blob.r; centerY: blob.r; centerRadius: blob.r
                    focalX: blob.r; focalY: blob.r
                    GradientStop { position: 0; color: Theme.fog[blob.index % Theme.fog.length] }
                    GradientStop { position: 1; color: "transparent" }
                }
                PathRectangle { x: 0; y: 0; width: blob.width; height: blob.height }
            }

            SequentialAnimation on dx {
                loops: Animation.Infinite
                NumberAnimation { to: 60 + blob.index * 30; duration: 23000 + blob.index * 7000; easing.type: Easing.InOutSine }
                NumberAnimation { to: -50 - blob.index * 20; duration: 29000 + blob.index * 5000; easing.type: Easing.InOutSine }
            }
            SequentialAnimation on dy {
                loops: Animation.Infinite
                NumberAnimation { to: -40 - blob.index * 25; duration: 31000 - blob.index * 4000; easing.type: Easing.InOutSine }
                NumberAnimation { to: 45 + blob.index * 15; duration: 27000 + blob.index * 6000; easing.type: Easing.InOutSine }
            }
        }
    }
}
