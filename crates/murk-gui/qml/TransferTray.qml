import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import Murk

// DCC transfers float above the composer: offers to answer, progress, and results.
Column {
    id: root
    spacing: 8
    width: 360

    readonly property var all: JSON.parse(Core.ctl.transfers || "[]")
    // Finished transfers the user closed stay closed.
    property var dismissed: ({})
    readonly property var shown: all.filter(t => !dismissed[t.id])

    function dismiss(id) {
        const d = Object.assign({}, dismissed)
        d[id] = true
        dismissed = d
    }

    Repeater {
        model: root.shown
        Rectangle {
            id: card
            required property var modelData
            readonly property var t: modelData
            width: root.width
            height: col.implicitHeight + 24
            radius: 14
            color: Theme.raised
            border.color: t.state === "offered" ? Theme.alpha(Theme.accent, 0.7)
                        : t.state === "failed" ? Theme.alpha(Theme.bad, 0.6) : Theme.line

            ColumnLayout {
                id: col
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.top: parent.top
                anchors.margins: 12
                spacing: 6

                RowLayout {
                    Layout.fillWidth: true
                    spacing: 10
                    Rectangle {
                        width: 34; height: 34; radius: 10
                        color: Theme.alpha(card.t.outgoing ? Theme.accent2 : Theme.accent, 0.18)
                        Text {
                            anchors.centerIn: parent
                            text: card.t.outgoing ? "↑" : "↓"
                            color: card.t.outgoing ? Theme.accent2 : Theme.accent
                            font.pixelSize: 18
                            font.weight: Font.Bold
                        }
                    }
                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 1
                        Text {
                            Layout.fillWidth: true
                            text: card.t.file
                            elide: Text.ElideMiddle
                            color: Theme.text
                            font.family: Theme.uiFont
                            font.pixelSize: 14
                            font.weight: Font.DemiBold
                        }
                        Text {
                            Layout.fillWidth: true
                            elide: Text.ElideRight
                            color: card.t.state === "failed" ? Theme.bad : Theme.sub
                            font.family: Theme.uiFont
                            font.pixelSize: 12
                            text: {
                                const t = card.t
                                const who = (t.outgoing ? "to " : "from ") + t.peer
                                switch (t.state) {
                                case "offered": return who + " wants to send you " + t.sizeText
                                case "waiting": return "Offered " + who + ", waiting for them"
                                case "active": return t.doneText + " of " + t.sizeText + "  ·  " + t.rateText
                                case "done": return (t.outgoing ? "Sent " : "Saved, ") + t.sizeText
                                case "declined": return "Declined"
                                default: return t.error
                                }
                            }
                        }
                    }
                    IconButton {
                        visible: card.t.state !== "active" && card.t.state !== "waiting" && card.t.state !== "offered"
                        glyph: "✕"
                        size: 24
                        tip: "Dismiss"
                        onClicked: root.dismiss(card.t.id)
                    }
                }

                // Progress.
                Rectangle {
                    visible: card.t.state === "active"
                    Layout.fillWidth: true
                    height: 6
                    radius: 3
                    color: Theme.alpha(Theme.text, 0.08)
                    Rectangle {
                        width: parent.width * Math.min(1, card.t.progress)
                        height: parent.height
                        radius: 3
                        Behavior on width { NumberAnimation { duration: 250 } }
                        gradient: Gradient {
                            orientation: Gradient.Horizontal
                            GradientStop { position: 0; color: Theme.accent }
                            GradientStop { position: 1; color: Theme.accent2 }
                        }
                    }
                }

                // Answer an offer. Nothing is downloaded until you say so.
                RowLayout {
                    visible: card.t.state === "offered"
                    Layout.fillWidth: true
                    spacing: 8
                    Text {
                        Layout.fillWidth: true
                        text: "Only accept files you expect."
                        color: Theme.faint
                        font.family: Theme.uiFont
                        font.pixelSize: 11
                    }
                    Button {
                        text: "Decline"
                        onClicked: Core.ctl.dccDecline(card.t.id)
                        contentItem: Text { text: parent.text; color: Theme.sub; font.family: Theme.uiFont; font.pixelSize: 13; horizontalAlignment: Text.AlignHCenter }
                        background: Rectangle { radius: 9; color: parent.hovered ? Theme.hover : "transparent"; implicitWidth: 80; implicitHeight: 32 }
                    }
                    Button {
                        text: "Download"
                        onClicked: Core.ctl.dccAccept(card.t.id)
                        contentItem: Text { text: parent.text; color: Theme.bg; font.family: Theme.uiFont; font.pixelSize: 13; font.weight: Font.DemiBold; horizontalAlignment: Text.AlignHCenter }
                        background: Rectangle { radius: 9; color: Theme.accent; implicitWidth: 96; implicitHeight: 32 }
                    }
                }

                RowLayout {
                    visible: card.t.state === "done" && !card.t.outgoing && card.t.path.length > 0
                    spacing: 8
                    Button {
                        text: "Open"
                        onClicked: Qt.openUrlExternally("file:///" + card.t.path.replace(/\\/g, "/"))
                        contentItem: Text { text: parent.text; color: Theme.accent; font.family: Theme.uiFont; font.pixelSize: 13; font.weight: Font.DemiBold; horizontalAlignment: Text.AlignHCenter }
                        background: Rectangle { radius: 9; color: Theme.alpha(Theme.accent, 0.14); implicitWidth: 70; implicitHeight: 30 }
                    }
                    Button {
                        text: "Show folder"
                        onClicked: {
                            const p = card.t.path.replace(/\\/g, "/")
                            Qt.openUrlExternally("file:///" + p.substring(0, p.lastIndexOf("/")))
                        }
                        contentItem: Text { text: parent.text; color: Theme.sub; font.family: Theme.uiFont; font.pixelSize: 13; horizontalAlignment: Text.AlignHCenter }
                        background: Rectangle { radius: 9; color: parent.hovered ? Theme.hover : "transparent"; implicitWidth: 100; implicitHeight: 30 }
                    }
                }
            }
        }
    }
}
