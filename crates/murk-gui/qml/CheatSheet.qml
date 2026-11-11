import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import Murk

// Ctrl+/ : every shortcut on one card.
Popup {
    id: root
    modal: true
    focus: true
    width: Math.min(760, parent.width - 80)
    x: (parent.width - width) / 2
    y: Math.max(40, (parent.height - height) / 2)
    padding: 28
    Overlay.modal: Rectangle { color: Theme.alpha("#000000", Theme.dark ? 0.5 : 0.25) }
    enter: Transition {
        NumberAnimation { property: "opacity"; from: 0; to: 1; duration: Theme.fast }
        NumberAnimation { property: "scale"; from: 0.97; to: 1; duration: Theme.med; easing.type: Easing.OutCubic }
    }
    background: Rectangle { radius: 18; color: Theme.raised; border.color: Theme.line }

    readonly property var groups: [
        { title: "Get around", keys: [
            ["Ctrl K", "Jump to anything, run commands, change themes"],
            ["Alt 1-0", "Go to buffer 1 to 10"],
            ["Ctrl N / Ctrl P", "Next and previous buffer"],
            ["Alt A", "Next buffer with activity"],
            ["Ctrl J", "Explore channels"],
            ["Ctrl F", "Search everything you have logged"],
            ["Ctrl Shift M", "Mentions"]
        ] },
        { title: "Write", keys: [
            ["Enter", "Send"],
            ["Shift Enter", "New line"],
            ["Tab", "Complete nicks, /commands and :emoji:"],
            ["Up / Down", "Input history"],
            ["Esc", "Cancel a reply, or clear"],
            ["Ctrl B / I / U", "Bold, italic, underline"],
            ["Ctrl O", "Reset formatting"]
        ] },
        { title: "See", keys: [
            ["Ctrl = / Ctrl -", "Bigger or smaller text"],
            ["PgUp / PgDn", "Scroll the conversation"],
            ["Ctrl ,", "Settings"],
            ["Ctrl /", "This card"]
        ] }
    ]

    // Nothing in here takes focus, so hold it on the card for Escape to land.
    onOpened: contentItem.forceActiveFocus()

    contentItem: ColumnLayout {
        spacing: 18
        focus: true
        Keys.onEscapePressed: root.close()
        Text { text: "Keyboard"; color: Theme.text; font.family: Theme.displayFont; font.pixelSize: 24; font.weight: Font.DemiBold }
        GridLayout {
            Layout.fillWidth: true
            columns: 2
            columnSpacing: 28
            rowSpacing: 18
            Repeater {
                model: root.groups
                ColumnLayout {
                    required property var modelData
                    Layout.fillWidth: true
                    Layout.alignment: Qt.AlignTop
                    spacing: 7
                    Text { text: modelData.title.toUpperCase(); color: Theme.faint; font.family: Theme.uiFont; font.pixelSize: 11; font.weight: Font.DemiBold; font.letterSpacing: 1 }
                    Repeater {
                        model: modelData.keys
                        RowLayout {
                            required property var modelData
                            spacing: 12
                            Row {
                                Layout.preferredWidth: 140
                                spacing: 4
                                Repeater {
                                    model: modelData[0].split(" / ")
                                    Rectangle {
                                        required property string modelData
                                        width: keyText.implicitWidth + 12
                                        height: 22
                                        radius: 6
                                        color: Theme.alpha(Theme.text, 0.08)
                                        border.color: Theme.line
                                        Text { id: keyText; anchors.centerIn: parent; text: modelData; color: Theme.text; font.family: Theme.monoFont; font.pixelSize: 11 }
                                    }
                                }
                            }
                            Text { Layout.fillWidth: true; text: modelData[1]; color: Theme.sub; font.family: Theme.uiFont; font.pixelSize: 13; wrapMode: Text.Wrap }
                        }
                    }
                }
            }
        }
    }
}
