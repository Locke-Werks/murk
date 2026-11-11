import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import Murk

Rectangle {
    id: root
    color: Theme.alpha(Theme.panel, 0.92)
    signal openSettings()
    signal addNetwork()
    signal openPalette()

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        // Wordmark.
        Item {
            Layout.fillWidth: true
            Layout.preferredHeight: 58
            Row {
                anchors.left: parent.left
                anchors.leftMargin: 18
                anchors.verticalCenter: parent.verticalCenter
                spacing: 9
                Rectangle {
                    width: 22; height: 22; radius: 7
                    anchors.verticalCenter: parent.verticalCenter
                    gradient: Gradient {
                        orientation: Gradient.Horizontal
                        GradientStop { position: 0; color: Theme.accent }
                        GradientStop { position: 1; color: Theme.accent2 }
                    }
                    rotation: 45
                    scale: 0.8
                }
                Text {
                    text: "murk"
                    color: Theme.text
                    font.family: Theme.displayFont
                    font.pixelSize: 21
                    font.weight: Font.Bold
                    font.letterSpacing: -0.5
                    anchors.verticalCenter: parent.verticalCenter
                }
            }
            IconButton {
                anchors.right: parent.right
                anchors.rightMargin: 12
                anchors.verticalCenter: parent.verticalCenter
                glyph: "⌕"
                tip: "Jump to…  (Ctrl+K)"
                onClicked: root.openPalette()
            }
        }

        ListView {
            id: list
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            model: Core.buffers
            spacing: 2
            boundsBehavior: Flickable.StopAtBounds
            ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded; width: 6 }

            delegate: Item {
                id: row
                required property var modelData
                width: ListView.view.width
                height: (modelData.firstOfNetwork && modelData.index > 0 ? 14 : 0) + 34

                Rectangle {
                    id: pill
                    anchors.bottom: parent.bottom
                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.leftMargin: 10
                    anchors.rightMargin: 10
                    height: 32
                    radius: 9
                    color: row.modelData.active ? Theme.alpha(Theme.accent, Theme.dark ? 0.13 : 0.16)
                         : hover.hovered ? Theme.hover : "transparent"
                    Behavior on color { ColorAnimation { duration: Theme.fast } }

                    // Accent bar on the active buffer.
                    Rectangle {
                        width: 3; radius: 2
                        height: row.modelData.active ? 16 : 0
                        anchors.verticalCenter: parent.verticalCenter
                        anchors.left: parent.left
                        anchors.leftMargin: -1
                        color: Theme.accent
                        Behavior on height { NumberAnimation { duration: Theme.med; easing.type: Easing.OutCubic } }
                    }

                    HoverHandler { id: hover }
                    TapHandler { onTapped: Core.ctl.switchTo(row.modelData.index) }

                    RowLayout {
                        anchors.fill: parent
                        anchors.leftMargin: row.modelData.server ? 12 : 22
                        anchors.rightMargin: 10
                        spacing: 8

                        // Connection dot for networks, sigil for channels and queries.
                        Item {
                            Layout.preferredWidth: 14
                            Layout.preferredHeight: 14
                            Rectangle {
                                visible: row.modelData.server
                                anchors.centerIn: parent
                                width: 8; height: 8; radius: 4
                                color: row.modelData.state === "connected" ? Theme.ok
                                     : row.modelData.state === "connecting" ? Theme.warn : Theme.faint
                                SequentialAnimation on opacity {
                                    running: row.modelData.state === "connecting"
                                    loops: Animation.Infinite
                                    NumberAnimation { to: 0.3; duration: 600 }
                                    NumberAnimation { to: 1; duration: 600 }
                                }
                            }
                            Text {
                                visible: !row.modelData.server
                                anchors.centerIn: parent
                                text: row.modelData.channel ? "#" : "@"
                                color: row.modelData.active ? Theme.accent : Theme.faint
                                font.family: Theme.monoFont
                                font.pixelSize: 13
                                font.weight: Font.DemiBold
                            }
                        }
                        Text {
                            Layout.fillWidth: true
                            text: row.modelData.server ? row.modelData.name
                                 : row.modelData.channel ? row.modelData.name.replace(/^#+/, "") : row.modelData.name
                            elide: Text.ElideRight
                            font.family: Theme.uiFont
                            font.pixelSize: row.modelData.server ? 12 : 14
                            font.weight: row.modelData.server ? Font.DemiBold
                                       : (row.modelData.unread > 0 || row.modelData.active) ? Font.DemiBold : Font.Normal
                            font.capitalization: row.modelData.server ? Font.AllUppercase : Font.MixedCase
                            font.letterSpacing: row.modelData.server ? 0.8 : 0
                            color: row.modelData.highlight ? Theme.hl
                                 : row.modelData.active ? Theme.text
                                 : !row.modelData.joined ? Theme.faint
                                 : row.modelData.unread > 0 ? Theme.text : Theme.sub
                        }
                        // Typing dots.
                        Row {
                            visible: row.modelData.typing && !row.modelData.active
                            spacing: 2
                            Repeater {
                                model: 3
                                Rectangle {
                                    required property int index
                                    width: 4; height: 4; radius: 2
                                    color: Theme.sub
                                    SequentialAnimation on opacity {
                                        loops: Animation.Infinite
                                        PauseAnimation { duration: index * 150 }
                                        NumberAnimation { to: 0.2; duration: 300 }
                                        NumberAnimation { to: 1; duration: 300 }
                                        PauseAnimation { duration: (2 - index) * 150 }
                                    }
                                }
                            }
                        }
                        Rectangle {
                            visible: row.modelData.unread > 0 && !row.modelData.active
                            radius: 9
                            implicitHeight: 18
                            implicitWidth: Math.max(18, badge.implicitWidth + 12)
                            color: row.modelData.highlight ? Theme.hl : Theme.alpha(Theme.text, 0.1)
                            Text {
                                id: badge
                                anchors.centerIn: parent
                                text: row.modelData.unread > 99 ? "99+" : row.modelData.unread
                                font.family: Theme.uiFont
                                font.pixelSize: 11
                                font.weight: Font.DemiBold
                                color: row.modelData.highlight ? Theme.bg : Theme.sub
                            }
                        }
                    }
                }
            }
        }

        // Footer: who we are, and the doors to everything else.
        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: 60
            color: "transparent"
            Rectangle { width: parent.width; height: 1; color: Theme.line }
            RowLayout {
                anchors.fill: parent
                anchors.leftMargin: 14
                anchors.rightMargin: 10
                spacing: 10
                Avatar {
                    size: 32
                    letter: Core.ctl.nick.length ? Core.ctl.nick.replace(/^[^A-Za-z0-9]+/, "")[0].toUpperCase() : "M"
                    tint: Theme.accent
                }
                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 0
                    Text {
                        text: Core.ctl.nick || "not connected"
                        color: Theme.text
                        font.family: Theme.uiFont
                        font.pixelSize: 13
                        font.weight: Font.DemiBold
                        elide: Text.ElideRight
                        Layout.fillWidth: true
                    }
                    Text {
                        text: Core.ctl.network || "murk"
                        color: Theme.faint
                        font.family: Theme.uiFont
                        font.pixelSize: 11
                        elide: Text.ElideRight
                        Layout.fillWidth: true
                    }
                }
                IconButton { glyph: "+"; tip: "Add a network"; onClicked: root.addNetwork() }
                IconButton { glyph: "⚙"; tip: "Settings"; onClicked: root.openSettings() }
            }
        }
    }
}
