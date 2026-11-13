import QtCore
import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import Murk

ApplicationWindow {
    id: win
    width: 1320
    height: 840
    minimumWidth: 760
    minimumHeight: 480
    visible: true
    flags: Qt.Window | Qt.FramelessWindowHint
    // Unread messages elsewhere show in the taskbar: "(3) #rust · Murk".
    readonly property int unreadElsewhere: Core.buffers.reduce((n, b) => n + (b.active || b.muted ? 0 : b.unread), 0)
    title: (unreadElsewhere > 0 ? "(" + unreadElsewhere + ") " : "")
        + (Core.ctl.title.length ? Core.ctl.title + "  ·  Murk" : "Murk")
    color: Theme.bg
    font.family: Theme.uiFont

    property bool closing: false
    readonly property bool hasBuffers: Core.buffers.length > 0

    // Window size and place survive restarts.
    Settings {
        category: "window"
        property alias x: win.x
        property alias y: win.y
        property alias width: win.width
        property alias height: win.height
    }

    Connections {
        target: Core.ctl
        function onAlert(title, text, index) {
            if (!(searchPanel.opened && searchPanel.tab === 0))
                Core.unseenMentions++
            if (!win.active) {
                if (Core.settings.notifyDesktop !== false)
                    Core.ctl.notify(title, text)
                win.alert(0)
            } else if (index !== Core.ctl.active) {
                toast.show(title, text, index, "mention")
            }
        }
        function onFinished() { Qt.quit() }
    }
    Component.onCompleted: { Core.ctl.start(); chat.composer.focusInput() }

    // Typing should always land in the composer: on activation, buffer switch, or closing a popup.
    onActiveChanged: if (active) chat.composer.focusInput()
    Connections {
        target: Core.ctl
        function onActiveChanged() { chat.composer.focusInput() }
    }
    onActiveFocusItemChanged: {
        const f = activeFocusItem
        // Anything but an editable field (or a message being selected) hands focus back.
        const editable = f && f.cursorPosition !== undefined && f.readOnly !== true
        const selecting = f && f.readOnly === true && f.selectedText && f.selectedText.length > 0
        if (!editable && !selecting && !palette.opened && !settings.opened && !networkEditor.opened && !emoji.opened && !searchPanel.opened && !profile.opened && !browser.opened && !cheats.opened && !lightbox.opened)
            Qt.callLater(chat.composer.focusInput)
    }

    Timer { interval: 1000; running: true; repeat: true; onTriggered: Core.ctl.tick() }

    // A friend coming online gets a toast. Who is already online when we connect
    // does not: that is the sidebar's job, and a burst of toasts at startup is noise.
    property var friendsOnline: ({})
    property bool friendsSettled: false
    Timer { interval: 15000; running: true; onTriggered: win.friendsSettled = true }
    Connections {
        target: Core
        function onSettingsChanged() {
            const now = {}
            for (const f of (Core.settings.friends || []))
                if (f.online.length)
                    now[f.nick.toLowerCase()] = f
            if (win.friendsSettled)
                for (const k in now)
                    if (!win.friendsOnline[k])
                        toast.show(now[k].nick + " is online", "On " + now[k].online.join(", ") + ". Click to say hi.", -1, "friend", now[k].nick)
            win.friendsOnline = now
        }
    }
    // An unresponsive network may delay exit only so long.
    Timer { id: quitDeadline; interval: 12000; onTriggered: Qt.quit() }

    onClosing: function (close) {
        if (!closing) {
            closing = true
            close.accepted = false
            quitDeadline.start()
            Core.ctl.shutdown()
        }
    }

    Shortcut { sequences: ["Ctrl+K", "Ctrl+T"]; onActivated: palette.open() }
    Shortcut { sequences: ["Ctrl+N", "Alt+Down"]; onActivated: Core.ctl.switchRelative(1) }
    Shortcut { sequences: ["Ctrl+P", "Alt+Up"]; onActivated: Core.ctl.switchRelative(-1) }
    Shortcut { sequence: "Alt+A"; onActivated: Core.ctl.switchActivity() }
    Shortcut { sequence: "Ctrl+,"; onActivated: settings.open() }
    Shortcut { sequence: "Ctrl+F"; onActivated: searchPanel.openTab(1) }
    Shortcut { sequence: "Ctrl+Shift+M"; onActivated: searchPanel.openTab(0) }
    Shortcut { sequence: "Ctrl+="; onActivated: Core.set("fontSize", Math.min(16, (Core.settings.fontSize || 11) + 1)) }
    Shortcut { sequence: "Ctrl+-"; onActivated: Core.set("fontSize", Math.max(9, (Core.settings.fontSize || 11) - 1)) }
    Repeater {
        model: 10
        Item {
            required property int index
            Shortcut { sequence: "Alt+" + ((index + 1) % 10); onActivated: Core.ctl.switchTo(index) }
        }
    }

    Backdrop { anchors.fill: parent }
    TitleBar { id: titleBar; window: win; anchors.left: parent.left; anchors.right: parent.right; anchors.top: parent.top; z: 10 }
    ResizeFrame { window: win; anchors.fill: parent }

    RowLayout {
        anchors.fill: parent
        anchors.topMargin: titleBar.height
        spacing: 0

        Sidebar {
            Layout.fillHeight: true
            Layout.preferredWidth: 248
            onOpenSettings: settings.open()
            onAddNetwork: networkEditor.edit(null)
            onOpenPalette: palette.open()
            onOpenMentions: searchPanel.openTab(0)
            onOpenBrowser: browser.open()
            onEditNetwork: function (name) {
                const nets = JSON.parse(Core.ctl.networkList())
                const n = nets.find(x => x.name.toLowerCase() === name.toLowerCase())
                networkEditor.edit(n || null)
            }
        }
        Rectangle { Layout.fillHeight: true; width: 1; color: Theme.line }

        Item {
            Layout.fillWidth: true
            Layout.fillHeight: true

            ChatView {
                id: chat
                anchors.fill: parent
                visible: win.hasBuffers
                onPickReaction: function (msgid, anchor) {
                    emoji.target = msgid
                    const p = anchor.mapToItem(win.contentItem, 0, 0)
                    emoji.x = Math.max(8, Math.min(p.x - emoji.width + anchor.width, win.width - emoji.width - 8))
                    emoji.y = Math.max(8, Math.min(p.y + anchor.height + 6, win.height - emoji.height - 8))
                    emoji.open()
                }
                onCelebrate: confetti.burst()
            }
            Connections {
                target: chat.composer
                function onOpenEmoji(anchor) {
                    emoji.target = ""
                    const p = anchor.mapToItem(win.contentItem, 0, 0)
                    emoji.x = p.x
                    emoji.y = p.y - emoji.height - 10
                    emoji.open()
                }
            }

            Welcome {
                anchors.fill: parent
                visible: !win.hasBuffers
                onAddNetwork: networkEditor.edit(null)
            }

            Confetti { id: confetti; anchors.fill: parent; z: 50 }

            TransferTray {
                anchors.right: parent.right
                anchors.bottom: parent.bottom
                anchors.rightMargin: 22
                anchors.bottomMargin: 100
                z: 30
            }

            // Drop a file into a private conversation to send it over DCC.
            DropArea {
                id: drop
                anchors.fill: parent
                // No `keys` filter: Explorer, browsers and chat apps package a file drag
                // under different formats. Anything that carries file URLs is welcome.
                onEntered: function (ev) {
                    ev.accepted = ev.hasUrls
                    if (ev.hasUrls)
                        ev.acceptProposedAction()
                }
                // With an upload host every file is shared by link, media shown inline.
                // Without one, files can only travel by DCC, to one person.
                readonly property bool canShare: chat.composer.uploadReady
                onDropped: function (ev) {
                    if (Core.ctl.canUpload()) {
                        chat.composer.uploadFiles(ev.urls)
                        return
                    }
                    if (Core.ctl.kind !== "query") {
                        toast.show("Files go to people", "Open a private conversation, then drop the file there.", -1)
                        return
                    }
                    for (const u of ev.urls)
                        Core.ctl.dccSend(Core.ctl.title, u.toString())
                }
                Rectangle {
                    anchors.fill: parent
                    anchors.margins: 14
                    visible: drop.containsDrag
                    radius: 18
                    color: Theme.alpha(Theme.accent, 0.08)
                    border.width: 2
                    border.color: Theme.accent
                    Text {
                        anchors.centerIn: parent
                        text: drop.canShare ? "Drop to share" + (Core.ctl.kind === "query" ? " with " + Core.ctl.title : Core.ctl.kind === "channel" ? " in " + Core.ctl.title : "")
                            : Core.ctl.kind === "query" ? "Drop to send to " + Core.ctl.title : "Open a private conversation to send files"
                        color: Theme.accent
                        font.family: Theme.displayFont
                        font.pixelSize: 22
                        font.weight: Font.DemiBold
                    }
                }
            }

            ToastStack {
                id: toast
                anchors.right: parent.right
                anchors.rightMargin: 20
                anchors.top: parent.top
                anchors.topMargin: 76
                z: 40
            }
        }

        Rectangle {
            Layout.fillHeight: true
            width: 1
            color: Theme.line
            visible: nicks.visible
        }
        NickList {
            id: nicks
            Layout.fillHeight: true
            Layout.preferredWidth: 228
            visible: Core.ctl.kind === "channel" && chat.showNicks && Core.settings.nicklist !== false && win.width > 980
        }
    }

    CommandPalette {
        id: palette
        parent: Overlay.overlay
        onOpenSettings: settings.open()
        onAddNetwork: networkEditor.edit(null)
        onOpenMentions: searchPanel.openTab(0)
        onOpenSearch: searchPanel.openTab(1)
        onOpenBrowser: browser.open()
    }
    SettingsPanel {
        id: settings
        parent: Overlay.overlay
        onEditNetwork: function (net) { settings.close(); networkEditor.edit(net) }
    }
    NetworkEditor { id: networkEditor; parent: Overlay.overlay }
    ChannelBrowser { id: browser; parent: Overlay.overlay }
    Shortcut { sequence: "Ctrl+J"; onActivated: browser.open() }
    CheatSheet { id: cheats; parent: Overlay.overlay }
    Shortcut { sequences: ["Ctrl+/", "F1"]; onActivated: cheats.open() }
    Lightbox { id: lightbox }
    Connections {
        target: Core
        function onMediaRequested(url, kind, image) { lightbox.show(url, kind, image) }
    }
    ProfileCard {
        id: profile
        onMention: function (n) { chat.composer.insert(n + ": ") }
    }
    Connections {
        target: Core
        function onProfileRequested(nick, color, x, y) { profile.show(nick, color, x, y) }
    }
    SearchPanel {
        id: searchPanel
        onOpened: if (tab === 0) Core.unseenMentions = 0
        onTabChanged: if (tab === 0) Core.unseenMentions = 0
    }
    EmojiPicker {
        id: emoji
        parent: Overlay.overlay
        property string target: ""
        onPicked: function (e) {
            if (target.length)
                Core.ctl.react(target, e)
            else
                chat.composer.insert(e)
        }
    }
}
