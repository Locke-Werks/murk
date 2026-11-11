import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import Murk

ApplicationWindow {
    id: win
    width: 1200
    height: 760
    visible: true
    title: ctl.title.length ? ctl.title + " - Murk" : "Murk"
    color: theme.base

    QtObject {
        id: theme
        readonly property color base: "#1e1e2e"
        readonly property color mantle: "#181825"
        readonly property color crust: "#11111b"
        readonly property color surface: "#313244"
        readonly property color overlay: "#6c7086"
        readonly property color text: "#cdd6f4"
        readonly property color subtext: "#a6adc8"
        readonly property color accent: "#cba6f7"
        readonly property color highlight: "#f9e2af"
        readonly property color red: "#f38ba8"
        readonly property string mono: Qt.platform.os === "windows" ? "Cascadia Mono" : "monospace"
        readonly property int fontSize: 11
    }

    property var bufferList: JSON.parse(ctl.buffers || "[]")
    property var nickList: JSON.parse(ctl.nicks || "[]")
    property bool closing: false

    Controller {
        id: ctl
        onLinesReset: function (lines) {
            chat.model.clear()
            for (const l of JSON.parse(lines))
                chat.model.append({ html: l })
            chat.stick = true
            chat.positionViewAtEnd()
        }
        onLinesAppended: function (lines) {
            const atEnd = chat.atYEnd || chat.stick
            for (const l of JSON.parse(lines))
                chat.model.append({ html: l })
            if (atEnd)
                chat.positionViewAtEnd()
        }
        onAlert: function (text) {
            if (!win.active)
                win.alert(0)
        }
        onFinished: Qt.quit()
        Component.onCompleted: start()
    }

    Timer {
        interval: 1000
        running: true
        repeat: true
        onTriggered: ctl.tick()
    }

    // Lets an unresponsive network delay exit only so long.
    Timer {
        id: quitDeadline
        interval: 12000
        onTriggered: Qt.quit()
    }

    onClosing: function (close) {
        if (!closing) {
            closing = true
            close.accepted = false
            quitDeadline.start()
            ctl.shutdown()
        }
    }

    Shortcut { sequences: ["Ctrl+N", "Alt+Down"]; onActivated: ctl.switchRelative(1) }
    Shortcut { sequences: ["Ctrl+P", "Alt+Up"]; onActivated: ctl.switchRelative(-1) }
    Shortcut { sequence: "Alt+A"; onActivated: ctl.switchActivity() }
    Shortcut { sequence: "PgUp"; onActivated: chat.page(-1) }
    Shortcut { sequence: "PgDown"; onActivated: chat.page(1) }
    Repeater {
        model: 10
        Item {
            Shortcut {
                sequence: "Alt+" + ((index + 1) % 10)
                onActivated: ctl.switchTo(index)
            }
        }
    }

    RowLayout {
        anchors.fill: parent
        spacing: 0

        // Buffer list.
        Rectangle {
            Layout.fillHeight: true
            Layout.preferredWidth: 210
            color: theme.mantle

            ListView {
                id: buffers
                anchors.fill: parent
                anchors.topMargin: 8
                clip: true
                model: win.bufferList
                currentIndex: ctl.active
                delegate: Rectangle {
                    required property var modelData
                    required property int index
                    width: ListView.view.width
                    height: 28
                    color: modelData.active ? theme.surface : (hover.hovered ? Qt.darker(theme.surface, 1.3) : "transparent")
                    HoverHandler { id: hover }
                    TapHandler { onTapped: ctl.switchTo(index) }
                    RowLayout {
                        anchors.fill: parent
                        anchors.leftMargin: modelData.server ? 10 : 22
                        anchors.rightMargin: 10
                        Text {
                            Layout.fillWidth: true
                            text: modelData.name
                            elide: Text.ElideRight
                            font.pointSize: theme.fontSize
                            font.bold: modelData.server || modelData.highlight
                            color: modelData.highlight ? theme.highlight
                                 : !modelData.joined || (modelData.server && !modelData.connected) ? theme.overlay
                                 : modelData.unread > 0 ? theme.text
                                 : theme.subtext
                        }
                        Rectangle {
                            visible: modelData.unread > 0 && !modelData.active
                            radius: 8
                            implicitHeight: 16
                            implicitWidth: Math.max(16, badge.implicitWidth + 10)
                            color: modelData.highlight ? theme.highlight : theme.surface
                            Text {
                                id: badge
                                anchors.centerIn: parent
                                text: modelData.unread > 999 ? "999+" : modelData.unread
                                font.pointSize: theme.fontSize - 2
                                color: modelData.highlight ? theme.crust : theme.subtext
                            }
                        }
                    }
                }
            }
        }

        // Chat column.
        ColumnLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            spacing: 0

            Rectangle {
                Layout.fillWidth: true
                implicitHeight: header.implicitHeight + 16
                color: theme.base
                ColumnLayout {
                    id: header
                    anchors.fill: parent
                    anchors.margins: 8
                    anchors.leftMargin: 14
                    spacing: 2
                    Text {
                        text: ctl.title
                        color: theme.text
                        font.pointSize: theme.fontSize + 2
                        font.bold: true
                    }
                    Text {
                        Layout.fillWidth: true
                        visible: ctl.topic.length > 0
                        text: ctl.topic
                        textFormat: Text.RichText
                        color: theme.subtext
                        font.pointSize: theme.fontSize - 1
                        elide: Text.ElideRight
                        maximumLineCount: 2
                        wrapMode: Text.Wrap
                        onLinkActivated: function (link) { Qt.openUrlExternally(link) }
                    }
                }
                Rectangle { anchors.bottom: parent.bottom; width: parent.width; height: 1; color: theme.surface }
            }

            ListView {
                id: chat
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true
                model: ListModel {}
                // Follow new lines until the user scrolls up.
                property bool stick: true
                onMovementEnded: stick = atYEnd
                onAtYBeginningChanged: if (atYBeginning && count > 0 && !stick) ctl.loadOlder()
                ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded }
                function page(dir) {
                    contentY = Math.max(originY, Math.min(contentY + dir * height * 0.9, originY + contentHeight - height))
                    stick = atYEnd
                }
                delegate: TextEdit {
                    required property string html
                    width: ListView.view.width - 24
                    x: 14
                    text: html
                    textFormat: TextEdit.RichText
                    readOnly: true
                    selectByMouse: true
                    wrapMode: TextEdit.Wrap
                    color: theme.text
                    selectionColor: theme.surface
                    font.family: theme.mono
                    font.pointSize: theme.fontSize
                    topPadding: 1
                    bottomPadding: 1
                    onLinkActivated: function (link) { Qt.openUrlExternally(link) }
                    HoverHandler {
                        cursorShape: parent.hoveredLink ? Qt.PointingHandCursor : Qt.IBeamCursor
                    }
                }
            }

            Text {
                Layout.fillWidth: true
                Layout.leftMargin: 14
                Layout.preferredHeight: 18
                text: ctl.typing.length ? ctl.typing : ctl.status
                color: ctl.typing.length ? theme.overlay : theme.highlight
                font.pointSize: theme.fontSize - 1
                elide: Text.ElideRight
            }

            Rectangle {
                Layout.fillWidth: true
                Layout.margins: 10
                Layout.topMargin: 2
                implicitHeight: 40
                radius: 6
                color: theme.surface
                RowLayout {
                    anchors.fill: parent
                    anchors.leftMargin: 12
                    anchors.rightMargin: 12
                    Text {
                        text: ctl.nick
                        visible: ctl.nick.length > 0
                        color: theme.accent
                        font.family: theme.mono
                        font.pointSize: theme.fontSize
                    }
                    TextField {
                        id: input
                        Layout.fillWidth: true
                        focus: true
                        color: theme.text
                        placeholderText: "Message or /command"
                        placeholderTextColor: theme.overlay
                        font.family: theme.mono
                        font.pointSize: theme.fontSize
                        background: null
                        selectByMouse: true
                        onTextEdited: ctl.edited(text)
                        onAccepted: {
                            if (text.length) {
                                ctl.submit(text)
                                text = ""
                                chat.stick = true
                                chat.positionViewAtEnd()
                            }
                        }
                        Keys.onTabPressed: function (event) {
                            text = ctl.complete(text)
                            cursorPosition = text.length
                            event.accepted = true
                        }
                        Keys.onUpPressed: { text = ctl.history(text, true); cursorPosition = text.length }
                        Keys.onDownPressed: { text = ctl.history(text, false); cursorPosition = text.length }
                    }
                }
            }
        }

        // Nick list.
        Rectangle {
            Layout.fillHeight: true
            Layout.preferredWidth: 180
            visible: win.nickList.length > 0
            color: theme.mantle
            ColumnLayout {
                anchors.fill: parent
                anchors.topMargin: 8
                spacing: 4
                Text {
                    Layout.leftMargin: 12
                    text: win.nickList.length + (win.nickList.length === 1 ? " user" : " users")
                    color: theme.overlay
                    font.pointSize: theme.fontSize - 1
                }
                ListView {
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    clip: true
                    model: win.nickList
                    ScrollBar.vertical: ScrollBar {}
                    delegate: Text {
                        required property var modelData
                        x: 12
                        width: ListView.view.width - 16
                        height: 22
                        verticalAlignment: Text.AlignVCenter
                        elide: Text.ElideRight
                        textFormat: Text.StyledText
                        font.pointSize: theme.fontSize
                        opacity: modelData.away ? 0.5 : 1.0
                        text: "<font color='" + theme.accent + "'>" + (modelData.prefix || "&nbsp;") + "</font>"
                              + "<font color='" + modelData.color + "'>" + modelData.nick + "</font>"
                        TapHandler {
                            onDoubleTapped: ctl.submit("/query " + modelData.nick)
                        }
                    }
                }
            }
        }
    }
}
