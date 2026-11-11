import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import Murk

// Who is this? Opens at once with what we know, then fills in from WHOIS.
Popup {
    id: root
    property string nick: ""
    property string tint: Theme.accent
    property var info: null
    signal mention(string nick)

    function show(n, color, x, y) {
        nick = n
        tint = color || Theme.accent
        info = null
        root.x = Math.max(8, Math.min(x, parent.width - width - 8))
        root.y = Math.max(8, Math.min(y, parent.height - height - 8))
        open()
        Core.ctl.whois(n)
    }

    Connections {
        target: Core.ctl
        function onWhoisReady(n, json) {
            if (root.opened && n.toLowerCase() === root.nick.toLowerCase())
                root.info = JSON.parse(json)
        }
    }

    parent: Overlay.overlay
    width: 340
    padding: 0
    focus: true
    enter: Transition {
        NumberAnimation { property: "opacity"; from: 0; to: 1; duration: Theme.fast }
        NumberAnimation { property: "scale"; from: 0.94; to: 1; duration: Theme.med; easing.type: Easing.OutBack }
    }
    background: Rectangle { radius: 16; color: Theme.raised; border.color: Theme.line; clip: true }

    contentItem: ColumnLayout {
        spacing: 0

        // Banner in the nick's color, the avatar overlapping its edge.
        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: 64
            radius: 16
            gradient: Gradient {
                orientation: Gradient.Horizontal
                GradientStop { position: 0; color: Theme.nick(root.tint) }
                GradientStop { position: 1; color: Theme.accent2 }
            }
            opacity: 0.85
        }
        Item {
            Layout.fillWidth: true
            Layout.preferredHeight: 44
            Rectangle {
                x: 18; y: -34
                width: 72; height: 72; radius: 36
                color: Theme.raised
                Avatar { anchors.centerIn: parent; size: 64; letter: root.nick.replace(/^[^A-Za-z0-9]+/, "").charAt(0).toUpperCase() || "?"; tint: Theme.nick(root.tint) }
            }
            Row {
                anchors.right: parent.right
                anchors.rightMargin: 14
                anchors.verticalCenter: parent.verticalCenter
                spacing: 6
                Repeater {
                    model: {
                        const b = []
                        if (root.info && root.info.secure) b.push({ t: "TLS", c: Theme.ok })
                        if (root.info && root.info.operator) b.push({ t: "IRCop", c: Theme.warn })
                        if (root.info && root.info.away) b.push({ t: "Away", c: Theme.faint })
                        return b
                    }
                    Rectangle {
                        required property var modelData
                        width: badgeText.implicitWidth + 12; height: 20; radius: 10
                        color: Theme.alpha(modelData.c, 0.18)
                        Text { id: badgeText; anchors.centerIn: parent; text: modelData.t; color: modelData.c; font.family: Theme.uiFont; font.pixelSize: 11; font.weight: Font.DemiBold }
                    }
                }
            }
        }

        ColumnLayout {
            Layout.fillWidth: true
            Layout.margins: 18
            Layout.topMargin: 0
            spacing: 4
            Text { text: root.nick; color: Theme.nick(root.tint); font.family: Theme.displayFont; font.pixelSize: 22; font.weight: Font.Bold }
            Text {
                Layout.fillWidth: true
                text: root.info ? (root.info.realname || "") : "Asking the server…"
                visible: text.length > 0
                color: Theme.sub
                font.family: Theme.uiFont
                font.pixelSize: 13
                elide: Text.ElideRight
            }
            Text {
                Layout.fillWidth: true
                visible: !!(root.info && root.info.away)
                text: root.info ? "“" + root.info.away + "”" : ""
                color: Theme.faint
                font.family: Theme.uiFont
                font.pixelSize: 12
                font.italic: true
                wrapMode: Text.Wrap
            }

            GridLayout {
                Layout.fillWidth: true
                Layout.topMargin: 10
                visible: !!root.info
                columns: 2
                columnSpacing: 14
                rowSpacing: 6
                component Key: Text { color: Theme.faint; font.family: Theme.uiFont; font.pixelSize: 12 }
                component Val: Text { Layout.fillWidth: true; color: Theme.text; font.family: Theme.uiFont; font.pixelSize: 12; elide: Text.ElideRight }
                Key { text: "Account"; visible: !!(root.info && root.info.account) }
                Val { text: root.info ? root.info.account : ""; visible: !!(root.info && root.info.account) }
                Key { text: "Online for"; visible: !!(root.info && root.info.online) }
                Val { text: root.info ? root.info.online : ""; visible: !!(root.info && root.info.online) }
                Key { text: "Idle"; visible: !!(root.info && root.info.idle) }
                Val { text: root.info ? root.info.idle : ""; visible: !!(root.info && root.info.idle) }
                Key { text: "Server"; visible: !!(root.info && root.info.server) }
                Val { text: root.info ? root.info.server : ""; visible: !!(root.info && root.info.server) }
                Key { text: "Host" }
                Val { text: root.info ? root.info.mask : ""; font.family: Theme.monoFont; font.pixelSize: 11 }
            }

            // Shared channels, clickable.
            Flow {
                Layout.fillWidth: true
                Layout.topMargin: 8
                spacing: 6
                visible: !!(root.info && root.info.channels.length)
                Repeater {
                    model: root.info ? root.info.channels.slice(0, 12) : []
                    Rectangle {
                        required property string modelData
                        width: chanText.implicitWidth + 14; height: 24; radius: 12
                        color: chanHover.hovered ? Theme.hover : Theme.alpha(Theme.text, 0.06)
                        HoverHandler { id: chanHover }
                        Text { id: chanText; anchors.centerIn: parent; text: modelData; color: Theme.sub; font.family: Theme.monoFont; font.pixelSize: 11 }
                        TapHandler { longPressThreshold: 0; onTapped: { Core.ctl.submit("/join " + modelData.replace(/^[~&@%+]+/, "")); root.close() } }
                    }
                }
            }

            RowLayout {
                Layout.fillWidth: true
                Layout.topMargin: 14
                spacing: 8
                Button {
                    Layout.fillWidth: true
                    text: "Message"
                    onClicked: { Core.ctl.submit("/query " + root.nick); root.close() }
                    contentItem: Text { text: parent.text; color: Theme.bg; font.family: Theme.uiFont; font.pixelSize: 13; font.weight: Font.DemiBold; horizontalAlignment: Text.AlignHCenter }
                    background: Rectangle { radius: 10; implicitHeight: 36; color: Theme.accent }
                }
                Button {
                    Layout.fillWidth: true
                    text: "Mention"
                    onClicked: { root.mention(root.nick); root.close() }
                    contentItem: Text { text: parent.text; color: Theme.text; font.family: Theme.uiFont; font.pixelSize: 13; horizontalAlignment: Text.AlignHCenter }
                    background: Rectangle { radius: 10; implicitHeight: 36; color: parent.hovered ? Theme.hover : Theme.alpha(Theme.text, 0.06) }
                }
                Button {
                    readonly property bool isFriend: (Core.settings.friends || []).some(f => f.nick.toLowerCase() === root.nick.toLowerCase())
                    Layout.preferredWidth: 44
                    text: isFriend ? "★" : "☆"
                    ToolTip.visible: hovered
                    ToolTip.text: isFriend ? "Remove friend" : "Add friend: see when they come online"
                    onClicked: Core.ctl.submit("/friend " + (isFriend ? "remove " : "add ") + root.nick)
                    contentItem: Text { text: parent.text; color: parent.isFriend ? Theme.warn : Theme.text; font.pixelSize: 16; horizontalAlignment: Text.AlignHCenter }
                    background: Rectangle { radius: 10; implicitHeight: 36; color: parent.hovered ? Theme.hover : Theme.alpha(Theme.text, 0.06) }
                }
            }
        }
    }
}
