import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import Murk

// Add or edit a network. Presets fill in the well-known ones.
Popup {
    id: root
    modal: true
    focus: true
    width: Math.min(560, parent.width - 80)
    x: (parent.width - width) / 2
    y: Math.max(40, (parent.height - height) / 2)
    padding: 26
    property var original: null
    property string error: ""

    readonly property var presets: [
        { name: "Libera", host: "irc.libera.chat", port: 6697 },
        { name: "OFTC", host: "irc.oftc.net", port: 6697 },
        { name: "hackint", host: "irc.hackint.org", port: 6697 },
        { name: "Rizon", host: "irc.rizon.net", port: 6697 },
        { name: "EsperNet", host: "irc.esper.net", port: 6697 },
        { name: "Snoonet", host: "irc.snoonet.org", port: 6697 },
        { name: "IRCnet", host: "open.ircnet.net", port: 6697 },
        { name: "DALnet", host: "irc.dal.net", port: 6697 }
    ]

    function edit(net) {
        original = net
        error = ""
        name.text = net ? net.name : ""
        host.text = net ? net.host : ""
        port.text = net ? String(net.port) : "6697"
        tls.checked = net ? net.tls : true
        nick.text = net ? net.nick : (Core.ctl.nick || "")
        channels.text = net ? net.autojoin : ""
        password.text = ""
        autoconnect.checked = net ? net.autoconnect : true
        open()
        if (net)
            channels.forceActiveFocus()
        else
            name.forceActiveFocus()
    }

    function save() {
        const msg = Core.ctl.saveNetwork(JSON.stringify({
            original: original ? original.name : "",
            name: name.text, host: host.text, port: parseInt(port.text) || 6697, tls: tls.checked,
            nick: nick.text, autojoin: channels.text, password: password.text, autoconnect: autoconnect.checked
        }), true)
        if (msg.length)
            error = msg
        else
            close()
    }

    Overlay.modal: Rectangle { color: Theme.alpha("#000000", Theme.dark ? 0.5 : 0.25) }
    enter: Transition {
        NumberAnimation { property: "opacity"; from: 0; to: 1; duration: Theme.fast }
        NumberAnimation { property: "scale"; from: 0.97; to: 1; duration: Theme.med; easing.type: Easing.OutCubic }
    }
    background: Rectangle { radius: 18; color: Theme.raised; border.color: Theme.line }

    component Field: TextField {
        Layout.fillWidth: true
        color: Theme.text
        placeholderTextColor: Theme.faint
        font.family: Theme.uiFont
        font.pixelSize: 14
        leftPadding: 12
        implicitHeight: 40
        background: Rectangle { radius: 10; color: Theme.alpha(Theme.text, 0.06); border.color: parent.activeFocus ? Theme.accent : "transparent" }
        Keys.onReturnPressed: root.save()
    }
    component Label: Text { color: Theme.sub; font.family: Theme.uiFont; font.pixelSize: 12; font.weight: Font.Medium }
    component Check: CheckBox {
        id: cb
        contentItem: Text { leftPadding: cb.indicator.width + 8; text: cb.text; color: Theme.text; font.family: Theme.uiFont; font.pixelSize: 13; verticalAlignment: Text.AlignVCenter }
        indicator: Rectangle {
            implicitWidth: 20; implicitHeight: 20; radius: 6
            y: (cb.height - height) / 2
            color: cb.checked ? Theme.accent : "transparent"
            border.color: cb.checked ? Theme.accent : Theme.faint
            Text { anchors.centerIn: parent; text: "✓"; visible: cb.checked; color: Theme.bg; font.pixelSize: 13; font.weight: Font.Bold }
        }
    }

    contentItem: ColumnLayout {
        spacing: 12
        Text {
            text: root.original ? "Edit " + root.original.name : "Connect to a network"
            color: Theme.text
            font.family: Theme.displayFont
            font.pixelSize: 22
            font.weight: Font.DemiBold
        }
        Flow {
            visible: !root.original
            Layout.fillWidth: true
            spacing: 6
            Repeater {
                model: root.presets
                Rectangle {
                    required property var modelData
                    width: presetText.implicitWidth + 22
                    height: 30
                    radius: 15
                    color: host.text === modelData.host ? Theme.alpha(Theme.accent, 0.18) : presetHover.hovered ? Theme.hover : Theme.alpha(Theme.text, 0.05)
                    border.color: host.text === modelData.host ? Theme.accent : "transparent"
                    HoverHandler { id: presetHover }
                    Text { id: presetText; anchors.centerIn: parent; text: modelData.name; color: Theme.text; font.family: Theme.uiFont; font.pixelSize: 13 }
                    TapHandler {
                        onTapped: {
                            name.text = modelData.name; host.text = modelData.host
                            port.text = String(modelData.port); tls.checked = true
                            channels.forceActiveFocus()
                        }
                    }
                }
            }
        }
        GridLayout {
            Layout.fillWidth: true
            columns: 2
            columnSpacing: 12
            rowSpacing: 6
            Label { text: "Name" }
            Label { text: "Your nick" }
            Field { id: name; placeholderText: "Libera" }
            Field { id: nick; placeholderText: "nick" }
            Label { text: "Server" }
            Label { text: "Port" }
            Field { id: host; placeholderText: "irc.example.net" }
            Field { id: port; placeholderText: "6697"; validator: IntValidator { bottom: 1; top: 65535 } }
        }
        Label { text: "Channels to join" }
        Field { id: channels; placeholderText: "#rust #linux #murk" }
        Label { text: root.original && root.original.hasPassword ? "Password (saved; leave empty to keep it)" : "Password (server or NickServ, optional)" }
        Field { id: password; echoMode: TextInput.Password; placeholderText: "optional" }
        RowLayout {
            spacing: 18
            Check { id: tls; text: "Use TLS"; checked: true }
            Check { id: autoconnect; text: "Connect at startup"; checked: true }
        }
        Text { visible: root.error.length > 0; text: root.error; color: Theme.bad; font.family: Theme.uiFont; font.pixelSize: 13 }
        RowLayout {
            Layout.fillWidth: true
            Layout.topMargin: 6
            Item { Layout.fillWidth: true }
            Button {
                text: "Cancel"
                onClicked: root.close()
                contentItem: Text { text: parent.text; color: Theme.sub; font.family: Theme.uiFont; font.pixelSize: 14; horizontalAlignment: Text.AlignHCenter }
                background: Rectangle { radius: 10; color: parent.hovered ? Theme.hover : "transparent"; implicitWidth: 96; implicitHeight: 40 }
            }
            Button {
                text: root.original ? "Save and connect" : "Connect"
                onClicked: root.save()
                contentItem: Text { text: parent.text; color: Theme.bg; font.family: Theme.uiFont; font.pixelSize: 14; font.weight: Font.DemiBold; horizontalAlignment: Text.AlignHCenter }
                background: Rectangle {
                    radius: 10; implicitWidth: 150; implicitHeight: 40
                    gradient: Gradient {
                        orientation: Gradient.Horizontal
                        GradientStop { position: 0; color: Theme.accent }
                        GradientStop { position: 1; color: Theme.accent2 }
                    }
                }
            }
        }
    }
}
