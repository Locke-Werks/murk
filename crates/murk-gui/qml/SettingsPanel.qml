import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import Murk

Popup {
    id: root
    signal editNetwork(var net)
    modal: true
    focus: true
    width: Math.min(760, parent.width - 80)
    height: Math.min(640, parent.height - 80)
    x: (parent.width - width) / 2
    y: (parent.height - height) / 2
    padding: 0
    property int page: 0
    property var networks: []
    onOpened: networks = JSON.parse(Core.ctl.networkList())

    Overlay.modal: Rectangle { color: Theme.alpha("#000000", Theme.dark ? 0.5 : 0.25) }
    enter: Transition {
        NumberAnimation { property: "opacity"; from: 0; to: 1; duration: Theme.fast }
        NumberAnimation { property: "scale"; from: 0.97; to: 1; duration: Theme.med; easing.type: Easing.OutCubic }
    }
    exit: Transition { NumberAnimation { property: "opacity"; to: 0; duration: Theme.fast } }
    background: Rectangle { radius: 18; color: Theme.raised; border.color: Theme.line }

    component Section: Text {
        color: Theme.faint
        font.family: Theme.uiFont
        font.pixelSize: 11
        font.weight: Font.DemiBold
        font.letterSpacing: 1
        topPadding: 10
    }
    component Toggle: RowLayout {
        id: tg
        property string label
        property string hint
        property bool checked
        signal toggled(bool on)
        Layout.fillWidth: true
        spacing: 12
        ColumnLayout {
            Layout.fillWidth: true
            spacing: 2
            Text { text: tg.label; color: Theme.text; font.family: Theme.uiFont; font.pixelSize: 14 }
            Text { visible: tg.hint.length > 0; text: tg.hint; color: Theme.faint; font.family: Theme.uiFont; font.pixelSize: 12; wrapMode: Text.Wrap; Layout.fillWidth: true }
        }
        Rectangle {
            width: 42; height: 24; radius: 12
            color: tg.checked ? Theme.accent : Theme.alpha(Theme.text, 0.15)
            Behavior on color { ColorAnimation { duration: Theme.fast } }
            Rectangle {
                width: 18; height: 18; radius: 9
                y: 3
                x: tg.checked ? 21 : 3
                color: tg.checked ? Theme.bg : Theme.text
                Behavior on x { NumberAnimation { duration: Theme.med; easing.type: Easing.OutCubic } }
            }
            MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: tg.toggled(!tg.checked) }
        }
    }

    contentItem: RowLayout {
        spacing: 0
        // Section nav.
        Rectangle {
            Layout.fillHeight: true
            Layout.preferredWidth: 190
            color: Theme.alpha(Theme.panel, 0.6)
            radius: 18
            Rectangle { anchors.right: parent.right; width: 18; height: parent.height; color: parent.color }
            Column {
                anchors.fill: parent
                anchors.margins: 14
                anchors.topMargin: 22
                spacing: 4
                Text { text: "Settings"; color: Theme.text; font.family: Theme.displayFont; font.pixelSize: 20; font.weight: Font.DemiBold; bottomPadding: 14; leftPadding: 8 }
                Repeater {
                    model: ["Appearance", "Notifications", "Networks", "About"]
                    Rectangle {
                        required property string modelData
                        required property int index
                        width: parent.width
                        height: 36
                        radius: 9
                        color: root.page === index ? Theme.alpha(Theme.accent, 0.14) : navHover.hovered ? Theme.hover : "transparent"
                        HoverHandler { id: navHover }
                        Text { anchors.verticalCenter: parent.verticalCenter; x: 12; text: modelData; color: root.page === index ? Theme.accent : Theme.sub; font.family: Theme.uiFont; font.pixelSize: 14; font.weight: Font.Medium }
                        TapHandler { onTapped: root.page = index }
                    }
                }
            }
        }

        StackLayout {
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.margins: 26
            currentIndex: root.page

            // Appearance.
            Flickable {
                contentHeight: appearance.implicitHeight
                clip: true
                ColumnLayout {
                    id: appearance
                    width: parent.width
                    spacing: 14
                    Section { text: "THEME" }
                    GridLayout {
                        Layout.fillWidth: true
                        columns: 3
                        rowSpacing: 10
                        columnSpacing: 10
                        Repeater {
                            model: Theme.order
                            Rectangle {
                                required property string modelData
                                readonly property var pal: Theme.palettes[modelData]
                                Layout.fillWidth: true
                                Layout.preferredHeight: 92
                                radius: 12
                                color: pal.bg
                                border.width: Theme.name === modelData ? 2 : 1
                                border.color: Theme.name === modelData ? Theme.accent : Theme.line
                                clip: true
                                // A miniature of the window in this palette.
                                Rectangle { x: 0; y: 0; width: 34; height: parent.height; color: pal.panel; radius: 12 }
                                Column {
                                    x: 44; y: 14; spacing: 6
                                    Row { spacing: 6
                                        Rectangle { width: 14; height: 14; radius: 7; color: pal.accent }
                                        Rectangle { width: 46; height: 6; radius: 3; color: pal.sub; anchors.verticalCenter: parent.verticalCenter }
                                    }
                                    Rectangle { width: 120; height: 6; radius: 3; color: pal.text; opacity: 0.8 }
                                    Rectangle { width: 86; height: 6; radius: 3; color: pal.text; opacity: 0.5 }
                                    Row { spacing: 4
                                        Rectangle { width: 26; height: 10; radius: 5; color: pal.accent; opacity: 0.6 }
                                        Rectangle { width: 26; height: 10; radius: 5; color: pal.accent2; opacity: 0.6 }
                                    }
                                }
                                Text { anchors.right: parent.right; anchors.bottom: parent.bottom; anchors.margins: 9; text: pal.label; color: pal.text; font.family: Theme.uiFont; font.pixelSize: 12; font.weight: Font.DemiBold }
                                MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: Core.set("theme", modelData) }
                            }
                        }
                    }
                    Section { text: "LAYOUT" }
                    Row {
                        spacing: 8
                        Repeater {
                            model: [{ k: "cozy", l: "Cozy", h: "Avatars and grouped messages" }, { k: "compact", l: "Compact", h: "One line per message, classic IRC" }]
                            Rectangle {
                                required property var modelData
                                width: 220; height: 58; radius: 12
                                color: Core.settings.layout === modelData.k ? Theme.alpha(Theme.accent, 0.12) : Theme.alpha(Theme.text, 0.04)
                                border.color: Core.settings.layout === modelData.k ? Theme.accent : "transparent"
                                Column {
                                    anchors.verticalCenter: parent.verticalCenter
                                    x: 14
                                    Text { text: modelData.l; color: Theme.text; font.family: Theme.uiFont; font.pixelSize: 14; font.weight: Font.DemiBold }
                                    Text { text: modelData.h; color: Theme.faint; font.family: Theme.uiFont; font.pixelSize: 12 }
                                }
                                MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: Core.set("layout", modelData.k) }
                            }
                        }
                    }
                    Section { text: "TEXT SIZE" }
                    RowLayout {
                        Layout.fillWidth: true
                        Text { text: "A"; color: Theme.sub; font.pixelSize: 12 }
                        Slider {
                            Layout.fillWidth: true
                            from: 9; to: 16; stepSize: 1
                            value: Core.settings.fontSize || 11
                            onMoved: Core.set("fontSize", value)
                        }
                        Text { text: "A"; color: Theme.sub; font.pixelSize: 20 }
                    }
                    Section { text: "CHAT" }
                    Toggle {
                        label: "Show joins, parts and nick changes"
                        checked: Core.settings.showJoinsParts !== false
                        onToggled: on => Core.set("showJoinsParts", on)
                    }
                    Toggle {
                        label: "Inline image and video previews"
                        hint: "Loads pictures linked in chat. The site hosting them sees your address."
                        checked: Core.settings.inlineMedia !== false
                        onToggled: on => Core.set("inlineMedia", on)
                    }
                    Toggle {
                        label: "Member list"
                        checked: Core.settings.nicklist !== false
                        onToggled: on => Core.set("nicklist", on)
                    }
                }
            }

            // Notifications.
            ColumnLayout {
                spacing: 14
                Section { text: "HIGHLIGHT WORDS" }
                Text {
                    Layout.fillWidth: true
                    text: "Your nick always highlights. Add words that should too, separated by commas. You get a desktop notification when one appears and Murk is not focused."
                    color: Theme.sub; font.family: Theme.uiFont; font.pixelSize: 13; wrapMode: Text.Wrap
                }
                TextField {
                    id: hlField
                    Layout.fillWidth: true
                    text: (Core.settings.highlights || []).join(", ")
                    placeholderText: "rust, deploy, lunch"
                    placeholderTextColor: Theme.faint
                    color: Theme.text
                    font.family: Theme.uiFont
                    font.pixelSize: 14
                    leftPadding: 12
                    background: Rectangle { radius: 10; color: Theme.alpha(Theme.text, 0.06); border.color: hlField.activeFocus ? Theme.accent : "transparent" }
                    onEditingFinished: Core.set("highlights", text.split(","))
                }
                Section { text: "WHEN MURK IS IN THE BACKGROUND" }
                Toggle {
                    label: "Desktop notifications"
                    hint: "For mentions and private messages. The taskbar button flashes either way. Muted buffers stay quiet unless you are mentioned."
                    checked: Core.settings.notifyDesktop !== false
                    onToggled: on => Core.set("notifyDesktop", on)
                }
                Item { Layout.fillHeight: true }
            }

            // Networks.
            ColumnLayout {
                spacing: 10
                RowLayout {
                    Layout.fillWidth: true
                    Section { text: "NETWORKS"; Layout.fillWidth: true }
                    Button {
                        text: "Add network"
                        onClicked: root.editNetwork(null)
                        contentItem: Text { text: parent.text; color: Theme.bg; font.family: Theme.uiFont; font.pixelSize: 13; font.weight: Font.DemiBold; horizontalAlignment: Text.AlignHCenter }
                        background: Rectangle { radius: 9; color: Theme.accent; implicitWidth: 120; implicitHeight: 34 }
                    }
                }
                Repeater {
                    model: root.networks
                    Rectangle {
                        required property var modelData
                        Layout.fillWidth: true
                        Layout.preferredHeight: 62
                        radius: 12
                        color: Theme.alpha(Theme.text, 0.04)
                        RowLayout {
                            anchors.fill: parent
                            anchors.leftMargin: 16
                            anchors.rightMargin: 10
                            spacing: 12
                            Rectangle { width: 9; height: 9; radius: 5; color: modelData.connected ? Theme.ok : Theme.faint }
                            ColumnLayout {
                                Layout.fillWidth: true
                                spacing: 2
                                Text { text: modelData.name; color: Theme.text; font.family: Theme.uiFont; font.pixelSize: 14; font.weight: Font.DemiBold }
                                Text { text: modelData.host + ":" + modelData.port + (modelData.tls ? "  ·  TLS" : "") + (modelData.autojoin ? "  ·  " + modelData.autojoin : ""); color: Theme.faint; font.family: Theme.uiFont; font.pixelSize: 12; elide: Text.ElideRight; Layout.fillWidth: true }
                            }
                            IconButton { glyph: "✎"; tip: "Edit"; onClicked: root.editNetwork(modelData) }
                            IconButton {
                                glyph: "✕"; tip: "Remove"
                                onClicked: { Core.ctl.removeNetwork(modelData.name); root.networks = JSON.parse(Core.ctl.networkList()) }
                            }
                        }
                    }
                }
                Text {
                    visible: root.networks.length === 0
                    text: "No networks yet."
                    color: Theme.faint; font.family: Theme.uiFont; font.pixelSize: 13
                }
                Item { Layout.fillHeight: true }
            }

            // About.
            ColumnLayout {
                spacing: 10
                Text { text: "murk"; color: Theme.text; font.family: Theme.displayFont; font.pixelSize: 40; font.weight: Font.Bold }
                Text { text: "IRC, out of the fog."; color: Theme.sub; font.family: Theme.uiFont; font.pixelSize: 15 }
                Text { text: "Version " + Qt.application.version; color: Theme.faint; font.family: Theme.uiFont; font.pixelSize: 13; topPadding: 12 }
                Text { text: "Config: " + Core.ctl.configPath(); color: Theme.faint; font.family: Theme.monoFont; font.pixelSize: 12; wrapMode: Text.WrapAnywhere; Layout.fillWidth: true }
                Text {
                    topPadding: 12
                    Layout.fillWidth: true
                    wrapMode: Text.Wrap
                    textFormat: Text.StyledText
                    color: Theme.sub; font.family: Theme.uiFont; font.pixelSize: 13
                    text: "<b>Keys</b><br>Ctrl+K jump anywhere · Alt+1..0 buffers · Alt+A next activity<br>"
                        + "Ctrl+N / Ctrl+P next and previous · Tab completes nicks, commands and :emoji:<br>"
                        + "Shift+Enter new line · Ctrl+B / I / U formatting · Esc cancels a reply"
                }
                Item { Layout.fillHeight: true }
            }
        }
    }
}
