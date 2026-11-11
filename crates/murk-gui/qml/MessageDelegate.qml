import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import Murk

// One buffer line. `j` is the line object as JSON, decided in Rust (model.rs).
Item {
    id: root
    required property string j
    required property int index
    readonly property var d: JSON.parse(j)
    readonly property bool chat: d.kind === "msg" || d.kind === "action" || d.kind === "notice"
    readonly property bool membership: d.kind === "join" || d.kind === "leave" || d.kind === "nick"
    // Folded lines are summarised by the last line of their run; keep any divider they carry.
    readonly property bool hidden: eventHidden && !d.newDay && !d.unread
    readonly property bool eventHidden: d.folded || (membership && Core.settings.showJoinsParts === false)
    readonly property bool cozy: !Theme.compact
    readonly property color nickColor: Theme.nick(d.color)
    property ListView view: ListView.view

    width: view ? view.width : 0
    height: hidden ? 0 : col.implicitHeight
    visible: !hidden

    HoverHandler { id: hover }

    Column {
        id: col
        width: parent.width

        // Day divider.
        Item {
            visible: root.d.newDay
            width: parent.width
            height: visible ? 44 : 0
            Rectangle { anchors.verticalCenter: parent.verticalCenter; x: 20; width: parent.width - 40; height: 1; color: Theme.line }
            Rectangle {
                anchors.centerIn: parent
                width: dayText.implicitWidth + 22
                height: 22
                radius: 11
                color: Theme.bg
                border.color: Theme.line
                Text {
                    id: dayText
                    anchors.centerIn: parent
                    text: root.d.day
                    color: Theme.sub
                    font.family: Theme.uiFont
                    font.pixelSize: 12
                    font.weight: Font.DemiBold
                }
            }
        }

        // Unread divider.
        Item {
            visible: root.d.unread
            width: parent.width
            height: visible ? 34 : 0
            Rectangle { anchors.verticalCenter: parent.verticalCenter; x: 20; width: parent.width - 40; height: 1; color: Theme.bad; opacity: 0.7 }
            // While you were away: who said how much.
            Rectangle {
                visible: root.d.digest.length > 0
                anchors.centerIn: parent
                width: digestText.implicitWidth + 24
                height: 24
                radius: 12
                color: Theme.bg
                border.color: Theme.alpha(Theme.bad, 0.6)
                Text {
                    id: digestText
                    anchors.centerIn: parent
                    text: root.d.digest
                    color: Theme.bad
                    font.family: Theme.uiFont
                    font.pixelSize: 12
                    font.weight: Font.DemiBold
                }
            }
            Rectangle {
                anchors.right: parent.right
                anchors.rightMargin: 20
                anchors.verticalCenter: parent.verticalCenter
                width: newText.implicitWidth + 12
                height: 16
                radius: 4
                color: Theme.bad
                Text {
                    id: newText
                    anchors.centerIn: parent
                    text: "NEW"
                    color: Theme.bg
                    font.family: Theme.uiFont
                    font.pixelSize: 10
                    font.weight: Font.Bold
                    font.letterSpacing: 1
                }
            }
        }

        // Chat line.
        Rectangle {
            id: card
            visible: root.chat
            width: parent.width
            height: visible ? body.implicitHeight + (root.cozy ? (root.d.grouped ? 4 : 10) : 2) : 0
            color: root.d.highlight ? Theme.alpha(Theme.hl, hover.hovered ? 0.13 : 0.08)
                 : hover.hovered ? Theme.alpha(Theme.text, Theme.dark ? 0.03 : 0.04) : "transparent"
            Behavior on color { ColorAnimation { duration: Theme.fast } }

            Rectangle {
                visible: root.d.highlight
                width: 3
                height: parent.height
                color: Theme.hl
            }

            // Avatar column (cozy) or the gutter clock for grouped lines.
            Avatar {
                visible: root.cozy && !root.d.grouped
                x: 20
                y: (root.d.reply ? 26 : 0) + 8
                size: 38
                letter: root.d.initial
                tint: root.nickColor
                TapHandler {
                    longPressThreshold: 0
                    cursorShape: Qt.PointingHandCursor
                    onTapped: function (ev) {
                        const p = parent.mapToItem(null, ev.position.x, ev.position.y)
                        Core.profileRequested(root.d.sender, root.d.color, p.x + 8, p.y + 8)
                    }
                }
            }
            Text {
                visible: root.cozy && root.d.grouped && hover.hovered
                x: 20
                width: 38
                y: 4
                horizontalAlignment: Text.AlignHCenter
                text: root.d.time
                color: Theme.faint
                font.family: Theme.uiFont
                font.pixelSize: 10
                topPadding: 3
            }

            Column {
                id: body
                x: root.cozy ? 72 : 20
                width: parent.width - x - 24
                y: root.cozy ? (root.d.grouped ? 2 : 8) : 1
                spacing: 3

                // Reply context.
                Row {
                    visible: !!root.d.reply
                    spacing: 6
                    height: visible ? 20 : 0
                    Text { text: "↳"; color: Theme.faint; font.pixelSize: 13 }
                    Text {
                        text: root.d.reply ? (root.d.reply.sender ? root.d.reply.sender : "") : ""
                        color: root.d.reply && root.d.reply.color ? Theme.nick(root.d.reply.color) : Theme.sub
                        font.family: Theme.uiFont
                        font.pixelSize: 12
                        font.weight: Font.DemiBold
                    }
                    Text {
                        text: root.d.reply ? root.d.reply.text : ""
                        color: Theme.sub
                        font.family: Theme.uiFont
                        font.pixelSize: 12
                        elide: Text.ElideRight
                        width: Math.min(implicitWidth, body.width - 140)
                    }
                }

                // Header: nick and time.
                Row {
                    visible: root.cozy && !root.d.grouped
                    spacing: 8
                    Text {
                        text: root.d.sender
                        color: root.nickColor
                        font.family: Theme.uiFont
                        font.pixelSize: Theme.size + 3
                        font.weight: Font.DemiBold
                        MouseArea {
                            anchors.fill: parent
                            cursorShape: Qt.PointingHandCursor
                            onClicked: function (mouse) {
                                const p = mapToItem(null, mouse.x, mouse.y)
                                Core.profileRequested(root.d.sender, root.d.color, p.x + 8, p.y + 8)
                            }
                        }
                    }
                    Rectangle {
                        visible: root.d.kind === "notice"
                        width: noticeTag.implicitWidth + 10; height: 16; radius: 4
                        anchors.verticalCenter: parent.verticalCenter
                        color: Theme.alpha(Theme.accent2, 0.18)
                        Text { id: noticeTag; anchors.centerIn: parent; text: "notice"; color: Theme.accent2; font.pixelSize: 10; font.family: Theme.uiFont; font.weight: Font.DemiBold }
                    }
                    Text {
                        text: root.d.time
                        color: Theme.faint
                        font.family: Theme.uiFont
                        font.pixelSize: 11
                        anchors.baseline: parent.children[0].baseline
                        ToolTip.visible: stampHover.hovered
                        ToolTip.delay: 400
                        ToolTip.text: root.d.stamp
                        HoverHandler { id: stampHover }
                    }
                }

                TextEdit {
                    id: text
                    width: body.width
                    readOnly: true
                    selectByMouse: true
                    textFormat: TextEdit.RichText
                    wrapMode: TextEdit.Wrap
                    color: root.d.history ? Theme.sub : Theme.text
                    selectionColor: Theme.alpha(Theme.accent, 0.35)
                    selectedTextColor: Theme.text
                    font.family: Theme.compact || Theme.p.mono ? Theme.monoFont : Theme.uiFont
                    font.pixelSize: Theme.size + 4
                    text: {
                        const d = root.d
                        const nc = root.nickColor
                        let prefix = ""
                        if (!root.cozy)
                            prefix = "<span style='color:" + Theme.faint + "'>" + d.time + "</span>&nbsp;&nbsp;"
                                + (d.kind === "action" ? "" : "<b style='color:" + nc + "'>" + d.sender + "</b>&nbsp;&nbsp;")
                        if (d.kind === "action")
                            return prefix + "<i><span style='color:" + nc + "'>• " + d.sender + "</span> " + Core.themed(d.body) + "</i>"
                        return prefix + Core.themed(d.body)
                    }
                    onLinkActivated: function (link) { Qt.openUrlExternally(link) }
                    HoverHandler { cursorShape: text.hoveredLink ? Qt.PointingHandCursor : Qt.IBeamCursor }
                    // Typing while a message is selected goes to the composer, not into the void.
                    Keys.onPressed: function (e) {
                        if (e.text.length && !(e.modifiers & Qt.ControlModifier)) {
                            root.view.typeInto(e.text)
                            e.accepted = true
                        }
                    }
                }

                // Link previews: images inline, pages as cards, video with a play badge.
                Column {
                    width: body.width
                    spacing: 8
                    visible: Core.settings.inlineMedia !== false
                    Repeater {
                        model: root.chat && Core.settings.inlineMedia !== false ? Core.links(root.d.plain) : []
                        Item {
                            id: pv
                            required property string modelData
                            readonly property var p: { Core.previewTick; return Core.preview(modelData) }
                            readonly property bool isImage: !!p && p.kind === "image"
                            readonly property bool isCard: !!p && (p.kind === "card" || p.kind === "video")
                            width: Math.min(body.width, isImage ? 460 : 520)
                            height: isImage ? imgBox.height : isCard ? card.height : 0
                            visible: height > 0

                            // Direct image.
                            Rectangle {
                                id: imgBox
                                visible: pv.isImage
                                width: Math.min(pv.width, img.status === Image.Ready ? img.implicitWidth * height / Math.max(1, img.implicitHeight) : 300)
                                height: pv.isImage ? (img.status === Image.Ready ? Math.min(300, img.implicitHeight) : 180) : 0
                                radius: 12
                                color: Theme.raised
                                clip: true
                                Image {
                                    id: img
                                    anchors.fill: parent
                                    source: pv.isImage ? pv.p.image : ""
                                    fillMode: Image.PreserveAspectCrop
                                    asynchronous: true
                                    sourceSize.height: 600
                                    opacity: status === Image.Ready ? 1 : 0
                                    Behavior on opacity { NumberAnimation { duration: Theme.med } }
                                }
                                MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: Qt.openUrlExternally(pv.modelData) }
                            }

                            // Page card.
                            Rectangle {
                                id: card
                                visible: pv.isCard
                                width: pv.width
                                height: pv.isCard ? cardCol.implicitHeight + 24 : 0
                                radius: 12
                                color: cardHover.hovered ? Theme.hover : Theme.alpha(Theme.raised, 0.85)
                                border.color: Theme.line
                                HoverHandler { id: cardHover }
                                Rectangle { width: 3; height: parent.height - 20; y: 10; x: 0; radius: 2; color: pv.p && pv.p.kind === "video" ? Theme.bad : Theme.accent }
                                Column {
                                    id: cardCol
                                    x: 16
                                    y: 12
                                    width: parent.width - 30
                                    spacing: 5
                                    Text { text: pv.p ? pv.p.site : ""; color: Theme.faint; font.family: Theme.uiFont; font.pixelSize: 11; font.weight: Font.DemiBold; visible: text.length > 0 }
                                    Text {
                                        width: parent.width
                                        text: pv.p ? pv.p.title : ""
                                        visible: text.length > 0
                                        color: Theme.accent
                                        font.family: Theme.uiFont
                                        font.pixelSize: 14
                                        font.weight: Font.DemiBold
                                        wrapMode: Text.Wrap
                                        maximumLineCount: 2
                                        elide: Text.ElideRight
                                    }
                                    Text {
                                        width: parent.width
                                        text: pv.p ? pv.p.description : ""
                                        visible: text.length > 0
                                        color: Theme.sub
                                        font.family: Theme.uiFont
                                        font.pixelSize: 13
                                        wrapMode: Text.Wrap
                                        maximumLineCount: 3
                                        elide: Text.ElideRight
                                    }
                                    Rectangle {
                                        visible: !!(pv.p && pv.p.image) && thumb.status !== Image.Error
                                        width: parent.width
                                        height: visible ? Math.min(260, width * 9 / 16) : 0
                                        radius: 9
                                        clip: true
                                        color: Theme.raised
                                        Image {
                                            id: thumb
                                            anchors.fill: parent
                                            source: pv.p && pv.p.image ? pv.p.image : ""
                                            fillMode: Image.PreserveAspectCrop
                                            asynchronous: true
                                            sourceSize.width: 900
                                        }
                                        Rectangle {
                                            visible: pv.p && pv.p.kind === "video"
                                            anchors.centerIn: parent
                                            width: 58; height: 58; radius: 29
                                            color: Theme.alpha("#000000", 0.6)
                                            Text { anchors.centerIn: parent; anchors.horizontalCenterOffset: 2; text: "▶"; color: "white"; font.pixelSize: 24 }
                                        }
                                    }
                                }
                                MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: Qt.openUrlExternally(pv.modelData) }
                            }
                        }
                    }
                }

                // Reactions.
                Flow {
                    width: body.width
                    spacing: 6
                    visible: root.d.reactions.length > 0
                    topPadding: 2
                    Repeater {
                        model: root.d.reactions
                        Rectangle {
                            required property var modelData
                            height: 26
                            width: chip.implicitWidth + 18
                            radius: 13
                            color: modelData.mine ? Theme.alpha(Theme.accent, 0.18) : Theme.alpha(Theme.text, 0.06)
                            border.color: modelData.mine ? Theme.alpha(Theme.accent, 0.6) : "transparent"
                            scale: chipHover.hovered ? 1.06 : 1
                            Behavior on scale { NumberAnimation { duration: Theme.fast } }
                            Row {
                                id: chip
                                anchors.centerIn: parent
                                spacing: 5
                                Text { text: modelData.emoji; font.pixelSize: 14 }
                                Text {
                                    text: modelData.count
                                    color: modelData.mine ? Theme.accent : Theme.sub
                                    font.family: Theme.uiFont
                                    font.pixelSize: 12
                                    font.weight: Font.DemiBold
                                    anchors.verticalCenter: parent.verticalCenter
                                }
                            }
                            HoverHandler { id: chipHover }
                            ToolTip.visible: chipHover.hovered
                            ToolTip.text: modelData.who
                            TapHandler { longPressThreshold: 0; onTapped: if (!modelData.mine && root.d.msgid) Core.ctl.react(root.d.msgid, modelData.emoji) }
                        }
                    }
                }
            }

            // Hover actions.
            Rectangle {
                visible: hover.hovered && root.d.msgid.length > 0 && Core.ctl.kind !== "server"
                anchors.right: parent.right
                anchors.rightMargin: 22
                y: -12
                z: 5
                height: 34
                width: actions.implicitWidth + 10
                radius: 10
                color: Theme.raised
                border.color: Theme.line
                Row {
                    id: actions
                    anchors.centerIn: parent
                    spacing: 1
                    Repeater {
                        model: Core.quickReactions.slice(0, 4)
                        IconButton {
                            required property string modelData
                            glyph: modelData
                            size: 28
                            onClicked: Core.ctl.react(root.d.msgid, modelData)
                        }
                    }
                    IconButton {
                        glyph: "☺"
                        tip: "Add reaction"
                        size: 28
                        onClicked: root.view.pickReaction(root.d.msgid, this)
                    }
                    IconButton {
                        glyph: "↩"
                        tip: "Reply"
                        size: 28
                        onClicked: root.view.replyTo(root.d)
                    }
                }
            }
        }

        // Membership and channel events: small and quiet.
        Item {
            visible: !root.chat && !root.eventHidden && root.d.kind !== "server" && root.d.kind !== "error"
            width: parent.width
            height: visible ? 24 : 0
            Row {
                x: root.cozy ? 72 : 20
                anchors.verticalCenter: parent.verticalCenter
                spacing: 8
                Text {
                    text: root.d.summary ? "⇅" : root.d.kind === "join" ? "→" : root.d.kind === "leave" ? "←"
                        : root.d.kind === "nick" ? "⇄" : root.d.kind === "topic" ? "✎" : "•"
                    color: root.d.summary ? Theme.accent : root.d.kind === "join" ? Theme.ok : root.d.kind === "leave" ? Theme.bad : Theme.faint
                    font.pixelSize: 13
                }
                Text {
                    text: root.d.event
                    color: Theme.faint
                    font.family: Theme.uiFont
                    font.pixelSize: Theme.size + 2
                }
                Text {
                    text: root.d.time
                    color: Theme.alpha(Theme.faint, 0.7)
                    font.family: Theme.uiFont
                    font.pixelSize: 11
                    visible: hover.hovered
                }
            }
        }

        // Server text and errors.
        Item {
            visible: root.d.kind === "server" || root.d.kind === "error"
            width: parent.width
            height: visible ? srv.implicitHeight + 4 : 0
            TextEdit {
                id: srv
                x: 20
                y: 2
                width: parent.width - 44
                readOnly: true
                selectByMouse: true
                wrapMode: TextEdit.Wrap
                textFormat: TextEdit.RichText
                font.family: Theme.monoFont
                font.pixelSize: Theme.size + 2
                color: root.d.kind === "error" ? Theme.bad : Theme.sub
                selectionColor: Theme.alpha(Theme.accent, 0.35)
                text: "<span style='color:" + Theme.faint + "'>" + root.d.time + "</span>&nbsp;&nbsp;" + Core.themed(root.d.body)
                onLinkActivated: function (link) { Qt.openUrlExternally(link) }
            }
        }
    }
}
