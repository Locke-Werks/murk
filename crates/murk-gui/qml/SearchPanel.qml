import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import Murk

// Slides in from the right: every mention you got, or a full-text search of the log.
Popup {
    id: root
    property int tab: 0 // 0 mentions, 1 search
    property var results: []

    function openTab(t) {
        tab = t
        open()
    }
    function refresh() {
        results = tab === 0 ? JSON.parse(Core.ctl.mentions())
                            : JSON.parse(Core.ctl.search(query.text))
    }
    onTabChanged: refresh()
    onOpened: {
        refresh()
        if (tab === 1)
            query.forceActiveFocus()
    }

    parent: Overlay.overlay
    modal: true
    focus: true
    width: Math.min(480, parent.width - 120)
    height: parent.height
    x: parent.width - width
    y: 0
    padding: 0
    Overlay.modal: Rectangle { color: Theme.alpha("#000000", Theme.dark ? 0.35 : 0.15) }
    enter: Transition { NumberAnimation { property: "x"; from: root.parent.width; to: root.parent.width - root.width; duration: Theme.med; easing.type: Easing.OutCubic } }
    exit: Transition { NumberAnimation { property: "x"; to: root.parent.width; duration: Theme.fast; easing.type: Easing.InCubic } }
    background: Rectangle {
        color: Theme.raised
        Rectangle { width: 1; height: parent.height; color: Theme.line }
    }

    Timer { id: debounce; interval: 220; onTriggered: root.refresh() }

    contentItem: ColumnLayout {
        spacing: 0

        RowLayout {
            Layout.fillWidth: true
            Layout.margins: 18
            Layout.bottomMargin: 10
            spacing: 6
            Repeater {
                model: ["Mentions", "Search"]
                Rectangle {
                    required property string modelData
                    required property int index
                    implicitWidth: tabText.implicitWidth + 26
                    implicitHeight: 32
                    radius: 16
                    color: root.tab === index ? Theme.alpha(Theme.accent, 0.16) : tabHover.hovered ? Theme.hover : "transparent"
                    HoverHandler { id: tabHover }
                    Text { id: tabText; anchors.centerIn: parent; text: modelData; color: root.tab === index ? Theme.accent : Theme.sub; font.family: Theme.uiFont; font.pixelSize: 14; font.weight: Font.DemiBold }
                    TapHandler { onTapped: root.tab = index }
                }
            }
            Item { Layout.fillWidth: true }
            IconButton { glyph: "✕"; tip: "Close (Esc)"; onClicked: root.close() }
        }

        TextField {
            id: query
            visible: root.tab === 1
            Layout.fillWidth: true
            Layout.leftMargin: 18
            Layout.rightMargin: 18
            Layout.bottomMargin: 8
            placeholderText: "Search every message you have logged"
            placeholderTextColor: Theme.faint
            color: Theme.text
            font.family: Theme.uiFont
            font.pixelSize: 15
            leftPadding: 14
            implicitHeight: 42
            background: Rectangle { radius: 12; color: Theme.alpha(Theme.text, 0.06); border.color: query.activeFocus ? Theme.accent : "transparent" }
            onTextChanged: debounce.restart()
        }

        Text {
            Layout.leftMargin: 20
            Layout.bottomMargin: 6
            text: root.tab === 0 ? (root.results.length ? root.results.length + " mentions" : "")
                : query.text.length ? root.results.length + (root.results.length === 1 ? " result" : " results") : ""
            visible: text.length > 0
            color: Theme.faint
            font.family: Theme.uiFont
            font.pixelSize: 12
        }

        ListView {
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            model: root.results
            spacing: 4
            ScrollBar.vertical: ScrollBar { width: 6 }
            delegate: Rectangle {
                required property var modelData
                width: ListView.view.width - 24
                x: 12
                height: hitCol.implicitHeight + 20
                radius: 12
                color: hitHover.hovered ? Theme.hover : "transparent"
                HoverHandler { id: hitHover }
                RowLayout {
                    anchors.fill: parent
                    anchors.margins: 10
                    spacing: 12
                    Avatar {
                        Layout.alignment: Qt.AlignTop
                        size: 32
                        letter: modelData.initial
                        tint: Theme.nick(modelData.color)
                    }
                    ColumnLayout {
                        id: hitCol
                        Layout.fillWidth: true
                        spacing: 3
                        RowLayout {
                            spacing: 8
                            Text { text: modelData.sender; color: Theme.nick(modelData.color); font.family: Theme.uiFont; font.pixelSize: 13; font.weight: Font.DemiBold }
                            Text { text: "in " + modelData.buffer; color: Theme.sub; font.family: Theme.uiFont; font.pixelSize: 12 }
                            Text { text: modelData.network; color: Theme.faint; font.family: Theme.uiFont; font.pixelSize: 11 }
                            Item { Layout.fillWidth: true }
                            Text { text: modelData.when; color: Theme.faint; font.family: Theme.uiFont; font.pixelSize: 11 }
                        }
                        Text {
                            Layout.fillWidth: true
                            text: modelData.html
                            textFormat: Text.StyledText
                            wrapMode: Text.Wrap
                            maximumLineCount: 4
                            elide: Text.ElideRight
                            color: Theme.text
                            font.family: Theme.uiFont
                            font.pixelSize: 14
                        }
                    }
                }
                TapHandler {
                    onTapped: {
                        if (Core.ctl.openStored(modelData.network, modelData.buffer))
                            root.close()
                    }
                }
            }

            // Empty states.
            Column {
                anchors.centerIn: parent
                spacing: 8
                visible: root.results.length === 0
                Text {
                    anchors.horizontalCenter: parent.horizontalCenter
                    text: root.tab === 0 ? "✨" : "⌕"
                    font.pixelSize: 34
                    color: Theme.faint
                }
                Text {
                    anchors.horizontalCenter: parent.horizontalCenter
                    text: root.tab === 0 ? "No mentions yet. When someone says your nick\nor a highlight word, it lands here."
                        : query.text.length ? "Nothing in the log matches." : "Type to search everything you have logged."
                    horizontalAlignment: Text.AlignHCenter
                    color: Theme.sub
                    font.family: Theme.uiFont
                    font.pixelSize: 13
                }
            }
        }
    }
}
