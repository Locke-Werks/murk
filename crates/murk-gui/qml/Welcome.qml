import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts
import Murk

// First run, or nothing connected: one obvious way in.
Item {
    id: root
    signal addNetwork()

    ColumnLayout {
        anchors.centerIn: parent
        width: Math.min(520, parent.width - 80)
        spacing: 18

        Rectangle {
            Layout.alignment: Qt.AlignHCenter
            width: 76; height: 76; radius: 24
            rotation: 45
            gradient: Gradient {
                orientation: Gradient.Horizontal
                GradientStop { position: 0; color: Theme.accent }
                GradientStop { position: 1; color: Theme.accent2 }
            }
            SequentialAnimation on rotation {
                loops: Animation.Infinite
                NumberAnimation { to: 55; duration: 3200; easing.type: Easing.InOutSine }
                NumberAnimation { to: 35; duration: 3200; easing.type: Easing.InOutSine }
            }
        }
        Text {
            Layout.alignment: Qt.AlignHCenter
            Layout.topMargin: 12
            text: "murk"
            color: Theme.text
            font.family: Theme.displayFont
            font.pixelSize: 56
            font.weight: Font.Bold
            font.letterSpacing: -2
        }
        Text {
            Layout.alignment: Qt.AlignHCenter
            text: "IRC, out of the fog."
            color: Theme.sub
            font.family: Theme.uiFont
            font.pixelSize: 17
        }
        Button {
            Layout.alignment: Qt.AlignHCenter
            Layout.topMargin: 14
            text: "Connect to a network"
            onClicked: root.addNetwork()
            contentItem: Text { text: parent.text; color: Theme.bg; font.family: Theme.uiFont; font.pixelSize: 15; font.weight: Font.DemiBold; horizontalAlignment: Text.AlignHCenter; verticalAlignment: Text.AlignVCenter }
            background: Rectangle {
                radius: 14; implicitWidth: 230; implicitHeight: 48
                gradient: Gradient {
                    orientation: Gradient.Horizontal
                    GradientStop { position: 0; color: Theme.accent }
                    GradientStop { position: 1; color: Theme.accent2 }
                }
                scale: parent.down ? 0.97 : parent.hovered ? 1.03 : 1
                Behavior on scale { NumberAnimation { duration: Theme.fast } }
            }
        }
        Text {
            Layout.alignment: Qt.AlignHCenter
            Layout.topMargin: 8
            text: "Press Ctrl+K anywhere to jump, run a command, or switch themes."
            color: Theme.faint
            font.family: Theme.uiFont
            font.pixelSize: 13
        }
    }
}
