import QtQuick
import QtQuick.Controls.Basic
import QtMultimedia
import Murk

// A video or audio link, playable in place. Nothing loads until it is clicked,
// so a busy channel full of clips costs nothing until someone wants one.
Rectangle {
    id: root
    required property string url
    required property bool video
    property string title: ""
    property string site: ""
    readonly property bool started: player.source.toString().length > 0
    readonly property bool playing: player.playbackState === MediaPlayer.PlayingState

    width: 460
    height: video ? (started && out.sourceRect.height > 0
                         ? Math.min(320, width * out.sourceRect.height / Math.max(1, out.sourceRect.width))
                         : 240)
                  : 64
    radius: 12
    color: Theme.dark ? "#000" : Theme.raised
    border.color: Theme.line
    clip: true

    MediaPlayer {
        id: player
        audioOutput: AudioOutput { id: sound }
        videoOutput: root.video ? out : null
        loops: root.video ? MediaPlayer.Infinite : 1
        onErrorOccurred: function (err, msg) { root.failed = msg || "cannot play this file" }
    }
    property string failed: ""

    VideoOutput {
        id: out
        anchors.fill: parent
        visible: root.video
        fillMode: VideoOutput.PreserveAspectFit
    }

    function toggle() {
        if (!started)
            player.source = url
        if (playing)
            player.pause()
        else
            player.play()
    }

    // Poster state and pause overlay: a big play button.
    Rectangle {
        visible: root.video && !root.playing
        anchors.centerIn: parent
        width: 64; height: 64; radius: 32
        color: Theme.alpha("#000", 0.55)
        border.color: Theme.alpha("#fff", 0.6)
        Text { anchors.centerIn: parent; anchors.horizontalCenterOffset: 3; text: "▶"; color: "white"; font.pixelSize: 26 }
    }
    Text {
        visible: root.video && !root.started
        anchors.left: parent.left
        anchors.bottom: parent.bottom
        anchors.margins: 12
        width: parent.width - 24
        text: (root.site.length ? root.site + "  ·  " : "") + root.title
        color: Theme.alpha("#fff", 0.75)
        elide: Text.ElideMiddle
        font.family: Theme.uiFont
        font.pixelSize: 12
    }

    // Audio: a compact bar.
    Row {
        visible: !root.video
        anchors.verticalCenter: parent.verticalCenter
        x: 14
        spacing: 12
        Rectangle {
            width: 36; height: 36; radius: 18
            color: Theme.accent
            Text { anchors.centerIn: parent; anchors.horizontalCenterOffset: root.playing ? 0 : 2; text: root.playing ? "❚❚" : "▶"; color: Theme.bg; font.pixelSize: 14 }
        }
        Column {
            anchors.verticalCenter: parent.verticalCenter
            spacing: 4
            Text { text: root.title; color: Theme.text; font.family: Theme.uiFont; font.pixelSize: 13; elide: Text.ElideMiddle; width: root.width - 90 }
            Rectangle {
                width: root.width - 90; height: 4; radius: 2
                color: Theme.alpha(Theme.text, 0.12)
                Rectangle {
                    height: parent.height; radius: 2
                    color: Theme.accent
                    width: player.duration > 0 ? parent.width * player.position / player.duration : 0
                }
            }
        }
    }

    // Video: progress along the bottom edge while playing.
    Rectangle {
        visible: root.video && root.started
        anchors.bottom: parent.bottom
        height: 3
        color: Theme.accent
        width: player.duration > 0 ? parent.width * player.position / player.duration : 0
    }

    // Mute toggle, top right, once playing.
    Rectangle {
        visible: root.started && root.video
        anchors.top: parent.top
        anchors.right: parent.right
        anchors.margins: 8
        width: 30; height: 30; radius: 15
        color: Theme.alpha("#000", 0.55)
        z: 2
        Text { anchors.centerIn: parent; text: sound.muted ? "🔇" : "🔊"; font.pixelSize: 13 }
        TapHandler { longPressThreshold: 0; onTapped: sound.muted = !sound.muted }
    }

    Text {
        visible: root.failed.length > 0
        anchors.centerIn: parent
        width: parent.width - 40
        horizontalAlignment: Text.AlignHCenter
        wrapMode: Text.Wrap
        text: "Could not play this here. Click to open it in your browser."
        color: Theme.bad
        font.family: Theme.uiFont
        font.pixelSize: 12
    }

    TapHandler {
        longPressThreshold: 0
        onTapped: root.failed.length ? Qt.openUrlExternally(root.url) : root.toggle()
    }
    // Stop anything still playing when its line scrolls out of existence.
    Component.onDestruction: player.stop()
}
