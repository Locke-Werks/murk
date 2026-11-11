import QtQuick
import Murk

// A nick's initial on a two-tone disc derived from its color.
Rectangle {
    id: root
    property string letter: "?"
    property color tint: Theme.accent
    property bool away: false
    property int size: 36

    width: size
    height: size
    radius: size / 2
    opacity: away ? 0.45 : 1
    gradient: Gradient {
        GradientStop { position: 0; color: Qt.lighter(root.tint, Theme.dark ? 1.05 : 1.3) }
        GradientStop { position: 1; color: Qt.darker(root.tint, Theme.dark ? 1.7 : 1.15) }
    }

    Text {
        anchors.centerIn: parent
        text: root.letter
        color: Theme.dark ? "#0b0d14" : "#ffffff"
        font.family: Theme.displayFont
        font.pixelSize: root.size * 0.46
        font.weight: Font.DemiBold
    }
}
