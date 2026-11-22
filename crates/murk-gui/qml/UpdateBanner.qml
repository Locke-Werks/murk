import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import Murk

// "Murk X.Y.Z is available", bottom left, out of the way of the conversation.
// Driven entirely by Core.ctl.updateInfo; the automatic check only ever shows
// the offer, while a manual check also reports "up to date" and failures.
Rectangle {
    id: root
    readonly property var info: Core.ctl.updateInfo ? JSON.parse(Core.ctl.updateInfo) : ({})
    readonly property string st: info.state || ""
    readonly property bool busy: st === "checking" || st === "downloading" || st === "installing"
    readonly property bool shown: st === "available" || st === "downloading" || st === "installing"
        || (info.manual === true && (st === "checking" || st === "current" || st === "error"))

    width: 380
    height: col.implicitHeight + 24
    radius: 14
    color: Theme.raised
    border.color: Theme.alpha(st === "error" ? Theme.bad : Theme.accent, 0.6)
    opacity: shown ? 1 : 0
    visible: opacity > 0
    Behavior on opacity { NumberAnimation { duration: Theme.med } }

    // "Up to date" needs no answer, so it goes away by itself.
    Timer {
        interval: 5000
        running: root.st === "current"
        onTriggered: Core.ctl.dismissUpdate()
    }

    ColumnLayout {
        id: col
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.margins: 12
        spacing: 8

        RowLayout {
            Layout.fillWidth: true
            spacing: 10
            BusyIndicator {
                visible: root.busy
                running: root.busy
                Layout.preferredWidth: 20
                Layout.preferredHeight: 20
            }
            Text {
                Layout.fillWidth: true
                wrapMode: Text.Wrap
                color: Theme.text
                font.family: Theme.uiFont
                font.pixelSize: 14
                font.weight: Font.DemiBold
                text: root.st === "available" ? "Murk " + root.info.version + " is available"
                    : root.st === "downloading" ? "Downloading Murk " + root.info.version
                    : root.st === "installing" ? "Installing Murk " + root.info.version + "..."
                    : root.st === "checking" ? "Checking for updates..."
                    : root.st === "current" ? "Murk " + root.info.version + " is up to date"
                    : root.st === "error" ? "Update failed"
                    : ""
            }
        }

        Text {
            Layout.fillWidth: true
            visible: text.length > 0
            wrapMode: Text.Wrap
            color: Theme.sub
            font.family: Theme.uiFont
            font.pixelSize: 12
            text: root.st === "error" ? (root.info.message || "")
                : root.st === "installing" ? "Murk will close and start again when it is done."
                : ""
        }

        ProgressBar {
            Layout.fillWidth: true
            visible: root.st === "downloading"
            from: 0
            to: 1
            value: root.info.progress || 0
        }

        RowLayout {
            Layout.fillWidth: true
            visible: root.st === "available" || root.st === "error"
            spacing: 8
            Item { Layout.fillWidth: true }
            Button {
                text: "Skip this version"
                visible: root.st === "available"
                flat: true
                onClicked: Core.ctl.skipUpdate()
            }
            Button {
                text: root.st === "error" ? "Close" : "Later"
                flat: true
                onClicked: Core.ctl.dismissUpdate()
            }
            Button {
                text: root.st === "error" ? "Retry" : "Install"
                visible: root.st === "available" || root.info.retry === true
                highlighted: true
                onClicked: Core.ctl.installUpdate()
            }
        }
    }
}
