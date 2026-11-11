import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import Murk

// The message box: reply banner, slash-command hints, emoji, multi-line input.
Rectangle {
    id: root
    signal sent()
    signal pageUp()
    signal pageDown()
    signal openEmoji(Item anchor)

    implicitHeight: col.implicitHeight + 2
    radius: 14
    color: Theme.raised
    border.color: input.activeFocus ? Theme.alpha(Theme.accent, 0.55) : Theme.line
    Behavior on border.color { ColorAnimation { duration: Theme.med } }

    function insert(s) {
        input.insert(input.cursorPosition, s)
        input.forceActiveFocus()
    }
    function focusInput() { input.forceActiveFocus() }

    // Slash command suggestions for what is typed so far.
    readonly property var hints: {
        const t = input.text
        if (!t.startsWith("/") || t.indexOf(" ") >= 0 || t.indexOf("\n") >= 0)
            return []
        const q = t.slice(1).toLowerCase()
        return Core.commands.filter(c => c.name.startsWith(q)).slice(0, 7)
    }
    property int hintIndex: 0
    onHintsChanged: hintIndex = 0

    function send() {
        const t = input.text.replace(/\s+$/, "")
        if (!t.length)
            return
        if (Core.replyTo)
            Core.ctl.reply(Core.replyTo.msgid, t)
        else
            Core.ctl.submit(t)
        Core.replyTo = null
        input.text = ""
        root.sent()
    }

    ColumnLayout {
        id: col
        anchors.left: parent.left
        anchors.right: parent.right
        spacing: 0

        // Command hints.
        Column {
            Layout.fillWidth: true
            Layout.margins: 6
            visible: root.hints.length > 0
            Repeater {
                model: root.hints
                Rectangle {
                    required property var modelData
                    required property int index
                    width: parent.width
                    height: 30
                    radius: 8
                    color: index === root.hintIndex ? Theme.alpha(Theme.accent, 0.14) : "transparent"
                    Row {
                        anchors.verticalCenter: parent.verticalCenter
                        x: 10
                        spacing: 12
                        Text { text: "/" + modelData.name; color: Theme.accent; font.family: Theme.monoFont; font.pixelSize: 13; font.weight: Font.DemiBold }
                        Text { text: modelData.usage; color: Theme.sub; font.family: Theme.monoFont; font.pixelSize: 12 }
                    }
                    MouseArea {
                        anchors.fill: parent
                        onClicked: { input.text = "/" + modelData.name + " "; input.cursorPosition = input.text.length; input.forceActiveFocus() }
                    }
                }
            }
            Rectangle { width: parent.width; height: 1; color: Theme.line }
        }

        // Replying to.
        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: 34
            visible: !!Core.replyTo
            color: "transparent"
            RowLayout {
                anchors.fill: parent
                anchors.leftMargin: 14
                anchors.rightMargin: 8
                spacing: 8
                Text { text: "↳ Replying to"; color: Theme.sub; font.family: Theme.uiFont; font.pixelSize: 12 }
                Text { text: Core.replyTo ? Core.replyTo.sender : ""; color: Theme.accent; font.family: Theme.uiFont; font.pixelSize: 12; font.weight: Font.DemiBold }
                Text {
                    Layout.fillWidth: true
                    text: Core.replyTo ? Core.replyTo.text : ""
                    color: Theme.faint
                    elide: Text.ElideRight
                    font.family: Theme.uiFont
                    font.pixelSize: 12
                }
                IconButton { glyph: "✕"; size: 24; tip: "Cancel reply (Esc)"; onClicked: Core.replyTo = null }
            }
            Rectangle { anchors.bottom: parent.bottom; width: parent.width; height: 1; color: Theme.line }
        }

        RowLayout {
            Layout.fillWidth: true
            Layout.leftMargin: 8
            Layout.rightMargin: 8
            spacing: 4

            IconButton {
                Layout.alignment: Qt.AlignBottom
                Layout.bottomMargin: 7
                glyph: "☺"
                tip: "Emoji"
                onClicked: root.openEmoji(this)
            }

            ScrollView {
                Layout.fillWidth: true
                Layout.preferredHeight: Math.min(input.implicitHeight, 180)
                ScrollBar.horizontal.policy: ScrollBar.AlwaysOff

                TextArea {
                    id: input
                    focus: true
                    wrapMode: TextArea.Wrap
                    color: Theme.text
                    selectionColor: Theme.alpha(Theme.accent, 0.35)
                    placeholderText: Core.ctl.kind === "channel" ? "Message #" + Core.ctl.title.replace(/^#+/, "")
                                   : Core.ctl.kind === "query" ? "Message " + Core.ctl.title
                                   : "Type /join #channel, or /help"
                    placeholderTextColor: Theme.faint
                    font.family: Theme.p.mono ? Theme.monoFont : Theme.uiFont
                    font.pixelSize: Theme.size + 4
                    topPadding: 13
                    bottomPadding: 13
                    leftPadding: 6
                    background: null
                    onTextChanged: {
                        if (text.length && !text.startsWith("/"))
                            Core.ctl.edited(text)
                        spellTimer.restart()
                    }
                    onWidthChanged: spellTimer.restart()

                    // Spell check: ranges come from the OS checker, minus nicks, URLs and commands.
                    property var typos: []
                    Timer {
                        id: spellTimer
                        interval: 350
                        onTriggered: input.typos = input.text.length ? JSON.parse(Core.ctl.misspelled(input.text)) : []
                    }
                    Repeater {
                        model: input.typos
                        Canvas {
                            required property var modelData
                            readonly property rect a: input.positionToRectangle(modelData[0])
                            readonly property rect b: input.positionToRectangle(modelData[0] + modelData[1])
                            // Words that wrap across lines get underlined on their first line only.
                            x: a.x
                            y: a.y + a.height - 3
                            width: Math.max(4, (b.y === a.y ? b.x : input.width - input.rightPadding) - a.x)
                            height: 4
                            onWidthChanged: requestPaint()
                            onPaint: {
                                const ctx = getContext("2d")
                                ctx.reset()
                                ctx.strokeStyle = Theme.bad
                                ctx.lineWidth = 1.3
                                ctx.beginPath()
                                for (let x = 0; x <= width; x += 2)
                                    ctx[x === 0 ? "moveTo" : "lineTo"](x, (x / 2) % 2 === 0 ? 3 : 1)
                                ctx.stroke()
                            }
                        }
                    }

                    // Right-click a flagged word for suggestions.
                    TapHandler {
                        acceptedButtons: Qt.RightButton
                        onTapped: function (ev) {
                            const pos = input.positionAt(ev.position.x, ev.position.y)
                            for (const r of input.typos) {
                                if (pos >= r[0] && pos <= r[0] + r[1]) {
                                    spellMenu.range = r
                                    spellMenu.word = input.text.substr(r[0], r[1])
                                    spellMenu.items = JSON.parse(Core.ctl.suggestions(spellMenu.word))
                                    spellMenu.popup()
                                    return
                                }
                            }
                        }
                    }
                    Menu {
                        id: spellMenu
                        property var range: [0, 0]
                        property string word: ""
                        property var items: []
                        background: Rectangle { implicitWidth: 200; radius: 10; color: Theme.raised; border.color: Theme.line }
                        Repeater {
                            model: spellMenu.items
                            MenuItem {
                                required property string modelData
                                text: modelData
                                contentItem: Text { text: parent.text; color: Theme.text; font.family: Theme.uiFont; font.pixelSize: 14; font.weight: Font.DemiBold }
                                background: Rectangle { radius: 6; color: parent.highlighted ? Theme.alpha(Theme.accent, 0.18) : "transparent" }
                                onTriggered: {
                                    const r = spellMenu.range
                                    input.remove(r[0], r[0] + r[1])
                                    input.insert(r[0], modelData)
                                    input.cursorPosition = r[0] + modelData.length
                                }
                            }
                        }
                        MenuItem {
                            enabled: false
                            visible: spellMenu.items.length === 0
                            height: visible ? implicitHeight : 0
                            contentItem: Text { text: "No suggestions"; color: Theme.faint; font.family: Theme.uiFont; font.pixelSize: 13 }
                            background: Item {}
                        }
                        MenuSeparator { contentItem: Rectangle { implicitHeight: 1; color: Theme.line } }
                        MenuItem {
                            text: "Add '" + spellMenu.word + "' to dictionary"
                            contentItem: Text { text: parent.text; color: Theme.sub; font.family: Theme.uiFont; font.pixelSize: 13 }
                            background: Rectangle { radius: 6; color: parent.highlighted ? Theme.hover : "transparent" }
                            onTriggered: { Core.ctl.learnWord(spellMenu.word); spellTimer.restart() }
                        }
                    }

                    Keys.onPressed: function (e) {
                        const plain = !(e.modifiers & (Qt.ShiftModifier | Qt.ControlModifier | Qt.AltModifier))
                        if ((e.key === Qt.Key_Return || e.key === Qt.Key_Enter) && plain) {
                            if (root.hints.length && input.text.slice(1) !== root.hints[root.hintIndex].name) {
                                input.text = "/" + root.hints[root.hintIndex].name + " "
                                input.cursorPosition = input.text.length
                            } else {
                                root.send()
                            }
                            e.accepted = true
                        } else if (e.key === Qt.Key_Tab) {
                            if (root.hints.length) {
                                input.text = "/" + root.hints[root.hintIndex].name + " "
                            } else {
                                // Completion works on the text before the cursor.
                                const before = input.text.slice(0, input.cursorPosition)
                                const after = input.text.slice(input.cursorPosition)
                                const done = Core.ctl.complete(before)
                                input.text = done + after
                                input.cursorPosition = done.length
                                e.accepted = true
                                return
                            }
                            input.cursorPosition = input.text.length
                            e.accepted = true
                        } else if (e.key === Qt.Key_Up && root.hints.length) {
                            root.hintIndex = (root.hintIndex + root.hints.length - 1) % root.hints.length
                            e.accepted = true
                        } else if (e.key === Qt.Key_Down && root.hints.length) {
                            root.hintIndex = (root.hintIndex + 1) % root.hints.length
                            e.accepted = true
                        } else if (e.key === Qt.Key_Up && input.text.indexOf("\n") < 0 && plain) {
                            input.text = Core.ctl.history(input.text, true)
                            input.cursorPosition = input.text.length
                            e.accepted = true
                        } else if (e.key === Qt.Key_Down && input.text.indexOf("\n") < 0 && plain) {
                            input.text = Core.ctl.history(input.text, false)
                            input.cursorPosition = input.text.length
                            e.accepted = true
                        } else if (e.key === Qt.Key_Escape) {
                            if (Core.replyTo)
                                Core.replyTo = null
                            else
                                input.text = ""
                            e.accepted = true
                        } else if (e.key === Qt.Key_PageUp) {
                            root.pageUp(); e.accepted = true
                        } else if (e.key === Qt.Key_PageDown) {
                            root.pageDown(); e.accepted = true
                        } else if (e.modifiers & Qt.ControlModifier) {
                            // mIRC formatting: Ctrl+B bold, Ctrl+I italic, Ctrl+U underline, Ctrl+O reset.
                            const codes = { [Qt.Key_B]: "\x02", [Qt.Key_I]: "\x1d", [Qt.Key_U]: "\x1f", [Qt.Key_O]: "\x0f" }
                            if (codes[e.key] !== undefined) {
                                input.insert(input.cursorPosition, codes[e.key])
                                e.accepted = true
                            }
                        }
                    }
                }
            }

            // Send.
            Rectangle {
                Layout.alignment: Qt.AlignBottom
                Layout.bottomMargin: 7
                width: 34; height: 34; radius: 11
                opacity: input.text.length ? 1 : 0.35
                Behavior on opacity { NumberAnimation { duration: Theme.fast } }
                gradient: Gradient {
                    orientation: Gradient.Horizontal
                    GradientStop { position: 0; color: Theme.accent }
                    GradientStop { position: 1; color: Theme.accent2 }
                }
                Text { anchors.centerIn: parent; text: "↑"; color: Theme.bg; font.pixelSize: 17; font.weight: Font.Bold }
                MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: root.send() }
            }
        }
    }
}
