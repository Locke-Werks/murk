import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import Murk

// Ctrl+K: jump to any buffer, run a command, or change a setting, by typing.
Popup {
    id: root
    signal openSettings()
    signal addNetwork()
    signal openMentions()
    signal openSearch()

    modal: true
    focus: true
    width: Math.min(620, parent.width - 80)
    x: (parent.width - width) / 2
    y: parent.height * 0.14
    padding: 0
    onOpened: { query.text = ""; query.forceActiveFocus(); sel = 0 }

    Overlay.modal: Rectangle { color: Theme.alpha("#000000", Theme.dark ? 0.5 : 0.25) }
    enter: Transition {
        NumberAnimation { property: "opacity"; from: 0; to: 1; duration: Theme.fast }
        NumberAnimation { property: "scale"; from: 0.97; to: 1; duration: Theme.med; easing.type: Easing.OutCubic }
    }
    exit: Transition { NumberAnimation { property: "opacity"; to: 0; duration: Theme.fast } }

    property int sel: 0

    readonly property var actions: {
        const a = []
        for (const b of Core.buffers)
            a.push({ label: b.name, detail: b.server ? "network" : b.network, icon: b.server ? "◉" : b.channel ? "#" : "@",
                     run: () => Core.ctl.switchTo(b.index), weight: b.unread > 0 ? 3 : 0 })
        for (const k of Theme.order)
            a.push({ label: "Theme: " + Theme.palettes[k].label, detail: "appearance", icon: "◑", run: () => Core.set("theme", k), weight: 0 })
        a.push({ label: "Layout: Cozy", detail: "appearance", icon: "☰", run: () => Core.set("layout", "cozy"), weight: 0 })
        a.push({ label: "Layout: Compact", detail: "appearance", icon: "≡", run: () => Core.set("layout", "compact"), weight: 0 })
        a.push({ label: (Core.settings.showJoinsParts === false ? "Show" : "Hide") + " joins and parts", detail: "appearance", icon: "⇆",
                 run: () => Core.set("showJoinsParts", Core.settings.showJoinsParts === false), weight: 0 })
        a.push({ label: "Mentions", detail: "everything that highlighted you", icon: "★", run: () => root.openMentions(), weight: 1 })
        a.push({ label: "Search messages", detail: "full-text, every network", icon: "⌕", run: () => root.openSearch(), weight: 1 })
        a.push({ label: "Settings", detail: "preferences", icon: "⚙", run: () => root.openSettings(), weight: 0 })
        a.push({ label: "Add a network", detail: "connect", icon: "+", run: () => root.addNetwork(), weight: 0 })
        for (const c of Core.commands)
            a.push({ label: "/" + c.name, detail: c.usage, icon: "/", command: c.name, weight: -1 })
        return a
    }

    readonly property var results: {
        const q = query.text.trim()
        if (q.startsWith("/"))
            return [{ label: "Run " + q, detail: "command", icon: "↵", run: () => Core.ctl.submit(q), weight: 0 }]
        const r = []
        for (const a of actions) {
            const s = Core.fuzzy(q, a.label)
            if (s >= 0)
                r.push({ a: a, s: s + a.weight })
        }
        r.sort((x, y) => y.s - x.s)
        return r.slice(0, 9).map(x => x.a)
    }
    onResultsChanged: sel = 0

    function activate(i) {
        const a = results[i]
        if (!a)
            return
        close()
        if (a.command) {
            Core.ctl.submit("/" + a.command)
            return
        }
        a.run()
    }

    background: Rectangle {
        radius: 16
        color: Theme.raised
        border.color: Theme.line
    }

    contentItem: ColumnLayout {
        spacing: 0
        TextField {
            id: query
            Layout.fillWidth: true
            Layout.preferredHeight: 56
            placeholderText: "Jump to a channel, run /command, or change a setting"
            placeholderTextColor: Theme.faint
            color: Theme.text
            font.family: Theme.uiFont
            font.pixelSize: 16
            leftPadding: 20
            background: Item {}
            Keys.onDownPressed: root.sel = Math.min(root.sel + 1, root.results.length - 1)
            Keys.onUpPressed: root.sel = Math.max(root.sel - 1, 0)
            Keys.onReturnPressed: root.activate(root.sel)
            Keys.onEnterPressed: root.activate(root.sel)
        }
        Rectangle { Layout.fillWidth: true; height: 1; color: Theme.line }
        Column {
            Layout.fillWidth: true
            Layout.margins: 8
            Repeater {
                model: root.results
                Rectangle {
                    required property var modelData
                    required property int index
                    width: parent.width
                    height: 42
                    radius: 10
                    color: index === root.sel ? Theme.alpha(Theme.accent, 0.14) : "transparent"
                    RowLayout {
                        anchors.fill: parent
                        anchors.leftMargin: 14
                        anchors.rightMargin: 14
                        spacing: 12
                        Text { text: modelData.icon; color: index === root.sel ? Theme.accent : Theme.faint; font.family: Theme.monoFont; font.pixelSize: 15; Layout.preferredWidth: 18; horizontalAlignment: Text.AlignHCenter }
                        Text { text: modelData.label; color: Theme.text; font.family: Theme.uiFont; font.pixelSize: 14; font.weight: Font.Medium }
                        Text { Layout.fillWidth: true; text: modelData.detail; color: Theme.faint; font.family: Theme.uiFont; font.pixelSize: 12; elide: Text.ElideRight; horizontalAlignment: Text.AlignRight }
                    }
                    MouseArea { anchors.fill: parent; hoverEnabled: true; onEntered: root.sel = index; onClicked: root.activate(index) }
                }
            }
            Text {
                visible: root.results.length === 0
                width: parent.width
                height: 48
                verticalAlignment: Text.AlignVCenter
                horizontalAlignment: Text.AlignHCenter
                text: "Nothing matches. Try /join #" + query.text.replace(/^#/, "")
                color: Theme.faint
                font.family: Theme.uiFont
                font.pixelSize: 13
            }
        }
        Text {
            Layout.fillWidth: true
            Layout.bottomMargin: 10
            horizontalAlignment: Text.AlignHCenter
            text: "↑↓ to move  ·  Enter to go  ·  Esc to close"
            color: Theme.faint
            font.family: Theme.uiFont
            font.pixelSize: 11
        }
    }
}
