import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import Murk

Rectangle {
    id: root
    color: Theme.alpha(Theme.panel, 0.92)
    signal openSettings()
    signal addNetwork()
    signal openMentions()
    signal editNetwork(string name)
    signal openBrowser()

    component MenuEntry: MenuItem {
        id: me
        property bool danger: false
        contentItem: Text { text: me.text; color: me.danger ? Theme.bad : Theme.text; font.family: Theme.uiFont; font.pixelSize: 13 }
        background: Rectangle { radius: 6; color: me.highlighted ? Theme.hover : "transparent" }
        height: visible ? implicitHeight : 0
    }

    // Right-click on a buffer.
    Menu {
        id: bufferMenu
        property var target: ({})
        background: Rectangle { implicitWidth: 220; radius: 10; color: Theme.raised; border.color: Theme.line }
        MenuEntry {
            text: bufferMenu.target.muted ? "Unmute" : "Mute (mentions still notify)"
            onTriggered: Core.ctl.bufferAction(bufferMenu.target.index, bufferMenu.target.muted ? "unmute" : "mute")
        }
        MenuEntry {
            text: "Mark as read"
            visible: bufferMenu.target.unread > 0
            onTriggered: Core.ctl.bufferAction(bufferMenu.target.index, "read")
        }
        MenuEntry {
            text: "Edit network…"
            visible: !!bufferMenu.target.server
            onTriggered: root.editNetwork(bufferMenu.target.network)
        }
        MenuEntry {
            text: bufferMenu.target.state === "disconnected" ? "Connect" : "Disconnect"
            visible: !!bufferMenu.target.server
            onTriggered: Core.ctl.bufferAction(bufferMenu.target.index, bufferMenu.target.state === "disconnected" ? "connect" : "disconnect")
        }
        MenuEntry {
            text: bufferMenu.target.channel ? "Leave channel" : "Close"
            visible: !bufferMenu.target.server
            danger: true
            onTriggered: Core.ctl.bufferAction(bufferMenu.target.index, "close")
        }
    }
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
            Row {
                anchors.right: parent.right
                anchors.rightMargin: 12
                anchors.verticalCenter: parent.verticalCenter
                spacing: 2
                IconButton { glyph: "+"; tip: "Add a network"; onClicked: root.addNetwork() }
                IconButton { glyph: "◎"; tip: "Explore channels  (Ctrl+J)"; onClicked: root.openBrowser() }
                IconButton {
                    glyph: "⌕"
                    tip: "Jump to…  (Ctrl+K)"
                    onClicked: root.openPalette()
                }
            }
        }

        ListView {
            id: list
            SmoothWheel { flick: list }
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

                    // Activity over the last half hour, drawn faintly behind the name.
                    Row {
                        visible: !row.modelData.server && !row.modelData.muted && row.modelData.pulse.length > 0
                        anchors.right: parent.right
                        anchors.rightMargin: 44
                        anchors.bottom: parent.bottom
                        anchors.bottomMargin: 6
                        spacing: 2
                        opacity: row.modelData.active ? 0.35 : 0.22
                        Repeater {
                            model: row.modelData.pulse
                            Rectangle {
                                required property int modelData
                                required property int index
                                anchors.bottom: parent.bottom
                                width: 3
                                radius: 1.5
                                height: 2 + modelData * 2
                                color: row.modelData.highlight ? Theme.hl : Theme.accent
                                opacity: 0.4 + 0.6 * index / 11
                                Behavior on height { NumberAnimation { duration: Theme.med; easing.type: Easing.OutCubic } }
                            }
                        }
                    }

                    HoverHandler { id: hover }
                    TapHandler { longPressThreshold: 0; onTapped: Core.ctl.switchTo(row.modelData.index) }
                    TapHandler {
                        longPressThreshold: 0
                        acceptedButtons: Qt.RightButton
                        onTapped: { bufferMenu.target = row.modelData; bufferMenu.popup() }
                    }

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
                                 : row.modelData.channel ? row.modelData.name.replace(/^#/, "") : row.modelData.name
                            elide: Text.ElideRight
                            font.family: Theme.uiFont
                            font.pixelSize: row.modelData.server ? 12 : 14
                            font.weight: row.modelData.server ? Font.DemiBold
                                       : (row.modelData.unread > 0 || row.modelData.active) ? Font.DemiBold : Font.Normal
                            font.capitalization: row.modelData.server ? Font.AllUppercase : Font.MixedCase
                            font.letterSpacing: row.modelData.server ? 0.8 : 0
                            color: row.modelData.highlight ? Theme.hl
                                 : row.modelData.active ? Theme.text
                                 : !row.modelData.joined || row.modelData.muted ? Theme.faint
                                 : row.modelData.unread > 0 ? Theme.text : Theme.sub
                        }
                        Text {
                            visible: row.modelData.muted
                            text: "🔕"
                            font.pixelSize: 11
                            opacity: 0.6
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

        // Friends, watched with MONITOR: who is around right now.
        Item {
            id: friendsBox
            readonly property var friends: Core.settings.friends || []
            Layout.fillWidth: true
            Layout.preferredHeight: friends.length ? friendsCol.implicitHeight + 16 : 0
            visible: friends.length > 0
            Rectangle { width: parent.width; height: 1; color: Theme.line }
            Column {
                id: friendsCol
                x: 18; y: 10
                width: parent.width - 30
                spacing: 4
                Text {
                    text: "FRIENDS · " + friendsBox.friends.filter(f => f.online.length).length + " online"
                    color: Theme.faint
                    font.family: Theme.uiFont
                    font.pixelSize: 10
                    font.weight: Font.DemiBold
                    font.letterSpacing: 0.8
                }
                Flow {
                    width: parent.width
                    spacing: 6
                    Repeater {
                        model: friendsBox.friends
                        Rectangle {
                            required property var modelData
                            readonly property bool on: modelData.online.length > 0
                            height: 24
                            width: fr.implicitWidth + 16
                            radius: 12
                            color: fHover.hovered ? Theme.hover : Theme.alpha(Theme.text, 0.05)
                            opacity: on ? 1 : 0.55
                            Row {
                                id: fr
                                anchors.centerIn: parent
                                spacing: 6
                                Rectangle {
                                    anchors.verticalCenter: parent.verticalCenter
                                    width: 7; height: 7; radius: 3.5
                                    color: parent.parent.on ? Theme.ok : Theme.faint
                                }
                                Text {
                                    text: modelData.nick
                                    color: Theme.text
                                    font.family: Theme.uiFont
                                    font.pixelSize: 12
                                }
                            }
                            HoverHandler { id: fHover }
                            ToolTip.visible: fHover.hovered
                            ToolTip.delay: 500
                            ToolTip.text: on ? "Online on " + modelData.online.join(", ") + ". Click to chat." : "Offline"
                            TapHandler { longPressThreshold: 0; onTapped: Core.ctl.submit("/query " + modelData.nick) }
                            TapHandler {
                                longPressThreshold: 0
                                acceptedButtons: Qt.RightButton
                                onTapped: Core.ctl.submit("/friend remove " + modelData.nick)
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
                IconButton {
                    glyph: "🔔"
                    tip: "Mentions  (Ctrl+Shift+M)"
                    active: Core.unseenMentions > 0
                    onClicked: root.openMentions()
                    Rectangle {
                        visible: Core.unseenMentions > 0
                        anchors.right: parent.right
                        anchors.top: parent.top
                        width: 16; height: 16; radius: 8
                        color: Theme.hl
                        Text { anchors.centerIn: parent; text: Math.min(Core.unseenMentions, 9); color: Theme.bg; font.pixelSize: 10; font.weight: Font.Bold }
                    }
                }
                IconButton { glyph: "⚙"; tip: "Settings"; onClicked: root.openSettings() }
            }
        }
    }
}
