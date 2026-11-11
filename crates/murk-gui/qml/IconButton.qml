import QtQuick
import QtQuick.Controls.Basic
import Murk

// A small round button showing a glyph, with a tooltip.
AbstractButton {
    id: b
    property string glyph: ""
    property string tip: ""
    property bool active: false
    property int size: 30

    implicitWidth: size
    implicitHeight: size
    hoverEnabled: true

    background: Rectangle {
        radius: b.size / 2.6
        color: b.active ? Theme.alpha(Theme.accent, 0.18)
             : b.down ? Theme.line : b.hovered ? Theme.hover : "transparent"
        Behavior on color { ColorAnimation { duration: Theme.fast } }
    }
    contentItem: Text {
        text: b.glyph
        color: b.active ? Theme.accent : b.hovered ? Theme.text : Theme.sub
        font.pixelSize: b.size * 0.5
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter
    }
    ToolTip.visible: hovered && tip.length > 0
    ToolTip.delay: 500
    ToolTip.text: tip
}
