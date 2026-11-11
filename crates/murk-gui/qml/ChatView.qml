import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import Murk

Item {
    id: root
    property alias composer: composer
    property bool showNicks: true
    signal pickReaction(string msgid, Item anchor)
    signal celebrate()

    function scrollToEnd() {
        list.stick = true
        list.positionViewAtEnd()
    }

    Connections {
        target: Core.ctl
        function onLinesReset(lines) {
            model.clear()
            for (const l of JSON.parse(lines))
                model.append({ j: JSON.stringify(l) })
            root.scrollToEnd()
            // Delegates settle their heights over a frame; pin to the end once more.
            settle.restart()
        }
        function onLinesAppended(lines) {
            const atEnd = list.stick
            let party = false
            for (const l of JSON.parse(lines)) {
                model.append({ j: JSON.stringify(l) })
                if (!l.history && /🎉|🎊|\bparty\b|\bcongrat/i.test(l.plain))
                    party = true
            }
            if (atEnd)
                list.positionViewAtEnd()
            else
                list.missed += 1
            if (party)
                root.celebrate()
        }
        function onLineChanged(index, line) {
            if (index >= 0 && index < model.count)
                model.set(index, { j: line })
        }
    }

    Timer { id: settle; interval: 30; onTriggered: if (list.stick) list.positionViewAtEnd() }

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        // Header.
        Item {
            Layout.fillWidth: true
            Layout.preferredHeight: 64
            RowLayout {
                anchors.fill: parent
                anchors.leftMargin: 24
                anchors.rightMargin: 16
                spacing: 12
                Text {
                    text: Core.ctl.kind === "channel" ? "#" : Core.ctl.kind === "query" ? "@" : "◉"
                    color: Theme.accent
                    font.family: Theme.monoFont
                    font.pixelSize: 22
                    font.weight: Font.Bold
                }
                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 1
                    RowLayout {
                        Layout.fillWidth: true
                        spacing: 10
                        Text {
                            text: Core.ctl.kind === "channel" ? Core.ctl.title.replace(/^#+/, "") : Core.ctl.title
                            color: Theme.text
                            font.family: Theme.displayFont
                            font.pixelSize: 18
                            font.weight: Font.DemiBold
                        }
                        Rectangle {
                            visible: Core.ctl.modes.length > 0
                            implicitWidth: modesText.implicitWidth + 12
                            implicitHeight: 18
                            radius: 5
                            color: Theme.alpha(Theme.text, 0.07)
                            Text { id: modesText; anchors.centerIn: parent; text: Core.ctl.modes; color: Theme.faint; font.family: Theme.monoFont; font.pixelSize: 11 }
                        }
                        Text {
                            visible: Core.ctl.kind === "channel"
                            text: Core.nicks.length + (Core.nicks.length === 1 ? " member" : " members")
                            color: Theme.faint
                            font.family: Theme.uiFont
                            font.pixelSize: 12
                        }
                        Item { Layout.fillWidth: true }
                    }
                    Text {
                        Layout.fillWidth: true
                        visible: text.length > 0
                        text: Core.ctl.topic
                        textFormat: Text.RichText
                        color: Theme.sub
                        font.family: Theme.uiFont
                        font.pixelSize: 13
                        elide: Text.ElideRight
                        maximumLineCount: 1
                        onLinkActivated: function (link) { Qt.openUrlExternally(link) }
                        HoverHandler { id: topicHover }
                        ToolTip.visible: topicHover.hovered && truncated
                        ToolTip.text: text
                        ToolTip.delay: 600
                    }
                }
                IconButton {
                    visible: Core.ctl.kind === "channel"
                    glyph: "☷"
                    tip: root.showNicks ? "Hide members" : "Show members"
                    active: root.showNicks
                    onClicked: root.showNicks = !root.showNicks
                }
            }
            Rectangle { anchors.bottom: parent.bottom; width: parent.width; height: 1; color: Theme.line }
        }

        Item {
            Layout.fillWidth: true
            Layout.fillHeight: true

            ListView {
                id: list
                anchors.fill: parent
                clip: true
                model: ListModel { id: model }
                topMargin: Math.max(10, height - contentHeight - bottomMargin)
                bottomMargin: 10
                boundsBehavior: Flickable.StopAtBounds
                reuseItems: true
                cacheBuffer: 1200
                // Follow new lines until the user scrolls up.
                property bool stick: true
                property int missed: 0
                onMovementEnded: { stick = atYEnd; if (stick) missed = 0 }
                // Previews and reactions grow lines after they land; stay pinned while that happens.
                onContentHeightChanged: if (stick && !moving) Qt.callLater(positionViewAtEnd)
                onAtYEndChanged: if (atYEnd) { stick = true; missed = 0 }
                onAtYBeginningChanged: if (atYBeginning && count > 0 && !stick) Core.ctl.loadOlder()
                ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded; width: 8 }

                function page(dir) {
                    contentY = Math.max(originY - topMargin, Math.min(contentY + dir * height * 0.85, originY + contentHeight - height + bottomMargin))
                    stick = atYEnd
                }
                function typeInto(s) { composer.insert(s) }
                function mention(nick) {
                    composer.insert(nick + ": ")
                }
                function pickReaction(msgid, anchor) { root.pickReaction(msgid, anchor) }
                function replyTo(d) {
                    Core.replyTo = { msgid: d.msgid, sender: d.sender, text: d.plain }
                    composer.focusInput()
                }

                delegate: MessageDelegate {}
            }

            // Jump back to the newest line.
            Rectangle {
                anchors.right: parent.right
                anchors.bottom: parent.bottom
                anchors.margins: 18
                visible: opacity > 0
                opacity: list.stick ? 0 : 1
                Behavior on opacity { NumberAnimation { duration: Theme.med } }
                width: jumpRow.implicitWidth + 24
                height: 34
                radius: 17
                color: Theme.raised
                border.color: list.missed > 0 ? Theme.accent : Theme.line
                Row {
                    id: jumpRow
                    anchors.centerIn: parent
                    spacing: 6
                    Text {
                        text: list.missed > 0 ? list.missed + " new" : "Jump to present"
                        color: list.missed > 0 ? Theme.accent : Theme.sub
                        font.family: Theme.uiFont
                        font.pixelSize: 12
                        font.weight: Font.DemiBold
                    }
                    Text { text: "↓"; color: Theme.sub; font.pixelSize: 13 }
                }
                MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    onClicked: { list.missed = 0; root.scrollToEnd() }
                }
            }
        }

        // Typing indicator.
        Item {
            Layout.fillWidth: true
            Layout.preferredHeight: 20
            Row {
                x: 26
                spacing: 8
                visible: Core.ctl.typing.length > 0
                anchors.verticalCenter: parent.verticalCenter
                Row {
                    spacing: 3
                    anchors.verticalCenter: parent.verticalCenter
                    Repeater {
                        model: 3
                        Rectangle {
                            required property int index
                            width: 5; height: 5; radius: 2.5
                            color: Theme.accent
                            SequentialAnimation on y {
                                loops: Animation.Infinite
                                PauseAnimation { duration: index * 140 }
                                NumberAnimation { from: 0; to: -4; duration: 220; easing.type: Easing.OutQuad }
                                NumberAnimation { from: -4; to: 0; duration: 220; easing.type: Easing.InQuad }
                                PauseAnimation { duration: (2 - index) * 140 + 300 }
                            }
                        }
                    }
                }
                Text {
                    text: Core.ctl.typing
                    color: Theme.sub
                    font.family: Theme.uiFont
                    font.pixelSize: 12
                }
            }
            Text {
                visible: Core.ctl.typing.length === 0 && Core.ctl.status.length > 0
                x: 26
                anchors.verticalCenter: parent.verticalCenter
                width: parent.width - 52
                elide: Text.ElideRight
                text: Core.ctl.status
                color: Theme.hl
                font.family: Theme.uiFont
                font.pixelSize: 12
            }
        }

        Composer {
            id: composer
            Layout.fillWidth: true
            Layout.leftMargin: 18
            Layout.rightMargin: 18
            Layout.bottomMargin: 16
            onSent: root.scrollToEnd()
            onPageUp: list.page(-1)
            onPageDown: list.page(1)
        }
    }
}
