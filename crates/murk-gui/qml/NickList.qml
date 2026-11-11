import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import Murk

Rectangle {
    id: root
    color: Theme.alpha(Theme.panel, 0.92)
    property string filter: ""

    // Members grouped by their highest prefix, in the server's rank order.
    readonly property var sections: {
        const names = { "~": "Owners", "&": "Admins", "@": "Operators", "%": "Half-ops", "+": "Voiced", "": "Members" }
        const out = []
        let cur = null
        for (const m of Core.nicks) {
            if (filter.length && m.nick.toLowerCase().indexOf(filter.toLowerCase()) < 0)
                continue
            const key = m.prefix || ""
            if (!cur || cur.key !== key) {
                cur = { key: key, title: names[key] || key, members: [] }
                out.push(cur)
            }
            cur.members.push(m)
        }
        return out
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        Item {
            Layout.fillWidth: true
            Layout.preferredHeight: 64
            TextField {
                anchors.fill: parent
                anchors.margins: 14
                anchors.topMargin: 16
                anchors.bottomMargin: 12
                placeholderText: "Filter " + Core.nicks.length + " members"
                placeholderTextColor: Theme.faint
                color: Theme.text
                font.family: Theme.uiFont
                font.pixelSize: 13
                leftPadding: 10
                background: Rectangle { radius: 9; color: Theme.alpha(Theme.text, 0.05); border.color: parent.activeFocus ? Theme.alpha(Theme.accent, 0.5) : "transparent" }
                onTextChanged: root.filter = text
            }
            Rectangle { anchors.bottom: parent.bottom; width: parent.width; height: 1; color: Theme.line }
        }

        ListView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            model: root.sections
            boundsBehavior: Flickable.StopAtBounds
            ScrollBar.vertical: ScrollBar { width: 6 }
            delegate: Column {
                required property var modelData
                width: ListView.view.width
                Text {
                    leftPadding: 16
                    topPadding: 14
                    bottomPadding: 4
                    text: modelData.title.toUpperCase() + "  ·  " + modelData.members.length
                    color: Theme.faint
                    font.family: Theme.uiFont
                    font.pixelSize: 11
                    font.weight: Font.DemiBold
                    font.letterSpacing: 0.8
                }
                Repeater {
                    model: modelData.members
                    Rectangle {
                        required property var modelData
                        width: parent.width - 16
                        x: 8
                        height: 36
                        radius: 8
                        color: nickHover.hovered ? Theme.hover : "transparent"
                        HoverHandler { id: nickHover }
                        RowLayout {
                            anchors.fill: parent
                            anchors.leftMargin: 8
                            anchors.rightMargin: 8
                            spacing: 10
                            Avatar {
                                size: 26
                                letter: modelData.initial
                                tint: Theme.nick(modelData.color)
                                away: modelData.away
                            }
                            Text {
                                Layout.fillWidth: true
                                text: modelData.nick
                                color: modelData.away ? Theme.faint : Theme.nick(modelData.color)
                                elide: Text.ElideRight
                                font.family: Theme.uiFont
                                font.pixelSize: 13
                                font.weight: Font.Medium
                            }
                        }
                        TapHandler {
                            onSingleTapped: function (ev) {
                                const p = parent.mapToItem(null, 0, ev.position.y)
                                Core.profileRequested(modelData.nick, modelData.color, p.x - 350, p.y)
                            }
                            onDoubleTapped: Core.ctl.submit("/query " + modelData.nick)
                        }
                        ToolTip.visible: nickHover.hovered
                        ToolTip.delay: 900
                        ToolTip.text: (modelData.away ? "Away. " : "") + "Click for profile, double-click to message"
                    }
                }
            }
        }
    }
}
