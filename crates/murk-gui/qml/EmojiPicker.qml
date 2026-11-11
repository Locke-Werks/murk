import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import Murk

// Searchable emoji grid. `picked` fires with the emoji; the opener decides what it is for.
Popup {
    id: root
    signal picked(string emoji)
    width: 340
    height: 360
    padding: 10
    focus: true
    onOpened: { search.text = ""; search.forceActiveFocus() }

    background: Rectangle { radius: 14; color: Theme.raised; border.color: Theme.line }
    enter: Transition {
        NumberAnimation { property: "opacity"; from: 0; to: 1; duration: Theme.fast }
        NumberAnimation { property: "scale"; from: 0.95; to: 1; duration: Theme.med; easing.type: Easing.OutBack }
    }

    readonly property var shown: {
        const q = search.text.trim().toLowerCase().replace(/:/g, "")
        return Core.emoji.filter(e => !q.length || e.code.indexOf(q) >= 0 || e.category.indexOf(q) === 0)
    }

    contentItem: ColumnLayout {
        spacing: 8
        TextField {
            id: search
            Layout.fillWidth: true
            placeholderText: "Search emoji"
            placeholderTextColor: Theme.faint
            color: Theme.text
            font.family: Theme.uiFont
            font.pixelSize: 13
            leftPadding: 10
            background: Rectangle { radius: 9; color: Theme.alpha(Theme.text, 0.06) }
            Keys.onReturnPressed: if (root.shown.length) { root.picked(root.shown[0].emoji); root.close() }
        }
        GridView {
            id: grid
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            cellWidth: width / 8
            cellHeight: cellWidth
            model: root.shown
            ScrollBar.vertical: ScrollBar { width: 6 }
            delegate: Rectangle {
                required property var modelData
                width: grid.cellWidth
                height: grid.cellHeight
                radius: 8
                color: cellHover.hovered ? Theme.hover : "transparent"
                HoverHandler { id: cellHover }
                Text {
                    anchors.centerIn: parent
                    text: modelData.emoji
                    font.pixelSize: 22
                    scale: cellHover.hovered ? 1.2 : 1
                    Behavior on scale { NumberAnimation { duration: Theme.fast } }
                }
                ToolTip.visible: cellHover.hovered
                ToolTip.delay: 400
                ToolTip.text: ":" + modelData.code + ":"
                TapHandler { longPressThreshold: 0; onTapped: { root.picked(modelData.emoji); root.close() } }
            }
        }
    }
}
