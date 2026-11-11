import QtQuick
import QtQuick.Controls.Basic
import QtMultimedia
import Murk

// Full-window viewer for images and video. Wheel zooms around the pointer, drag
// pans, double-click flips between fit and actual size. Esc, the close button or
// a click on the backdrop dismisses it.
Popup {
    id: root
    property string url: ""
    property string kind: "image"
    // For images, the local cached copy when there is one; otherwise the URL.
    property string source: ""

    function show(u, k, s) {
        url = u
        kind = k
        source = s && s.length ? s : u
        open()
    }

    parent: Overlay.overlay
    x: 0
    y: 0
    width: parent ? parent.width : 0
    height: parent ? parent.height : 0
    modal: true
    focus: true
    padding: 0
    closePolicy: Popup.CloseOnEscape
    onAboutToHide: { modal = false; player.stop() }
    onAboutToShow: modal = true
    onOpened: { fit(); if (kind === "movie") player.play() }

    enter: Transition { NumberAnimation { property: "opacity"; from: 0; to: 1; duration: Theme.med } }
    exit: Transition { NumberAnimation { property: "opacity"; from: 1; to: 0; duration: Theme.fast } }

    background: Rectangle { color: Qt.rgba(0, 0, 0, 0.88) }
    Overlay.modal: Item {}

    readonly property string fileName: decodeURIComponent(url.split(/[?#]/)[0].split("/").pop())

    // ---- Image -----------------------------------------------------------
    property real zoom: 1
    property real fitZoom: 1
    readonly property bool zoomed: zoom > fitZoom * 1.01

    function fit() {
        if (kind !== "image" || pic.status !== Image.Ready)
            return
        fitZoom = Math.min(1, (stage.width - 80) / pic.implicitWidth, (stage.height - 80) / pic.implicitHeight)
        zoom = fitZoom
        flick.contentX = 0
        flick.contentY = 0
    }

    // Zoom to `z`, keeping the image point under (px, py) still.
    function zoomAt(z, px, py) {
        z = Math.max(fitZoom * 0.5, Math.min(8, z))
        const ix = (flick.contentX + px - holder.x) / zoom
        const iy = (flick.contentY + py - holder.y) / zoom
        zoom = z
        flick.returnToBounds()
        flick.contentX = Math.max(0, ix * zoom + holder.x - px)
        flick.contentY = Math.max(0, iy * zoom + holder.y - py)
    }

    Item {
        id: stage
        anchors.fill: parent
        anchors.topMargin: 56

        // A click on empty backdrop closes, as in every lightbox.
        TapHandler { onTapped: root.close() }

        Flickable {
            id: flick
            visible: root.kind === "image"
            anchors.fill: parent
            clip: true
            contentWidth: Math.max(width, pic.implicitWidth * root.zoom)
            contentHeight: Math.max(height, pic.implicitHeight * root.zoom)
            boundsBehavior: Flickable.StopAtBounds
            interactive: root.zoomed

            Item {
                id: holder
                width: pic.implicitWidth * root.zoom
                height: pic.implicitHeight * root.zoom
                x: Math.max(0, (flick.width - width) / 2)
                y: Math.max(0, (flick.height - height) / 2)
                Image {
                    id: pic
                    anchors.fill: parent
                    source: root.kind === "image" ? root.source : ""
                    fillMode: Image.PreserveAspectFit
                    smooth: root.zoom < 2
                    mipmap: true
                    asynchronous: true
                    onStatusChanged: if (status === Image.Ready) root.fit()
                }
                // Swallow taps on the picture itself so they do not close the viewer.
                TapHandler {
                    longPressThreshold: 0
                    onDoubleTapped: function (pt) {
                        if (root.zoomed)
                            root.fit()
                        else
                            root.zoomAt(1 / Math.max(0.01, root.fitZoom) > 1 ? 1 : root.fitZoom * 2,
                                        pt.position.x + holder.x - flick.contentX,
                                        pt.position.y + holder.y - flick.contentY)
                    }
                }
            }

            WheelHandler {
                target: null
                acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
                onWheel: function (ev) {
                    const step = ev.angleDelta.y !== 0 ? ev.angleDelta.y / 120 : ev.pixelDelta.y / 60
                    root.zoomAt(root.zoom * Math.pow(1.18, step), ev.x, ev.y)
                }
            }
            PinchHandler {
                target: null
                onScaleChanged: function (delta) {
                    root.zoomAt(root.zoom * delta, centroid.position.x, centroid.position.y)
                }
            }
        }

        BusyIndicator {
            anchors.centerIn: parent
            running: root.kind === "image" && pic.status === Image.Loading
            visible: running
        }
        Text {
            anchors.centerIn: parent
            visible: root.kind === "image" && pic.status === Image.Error
            text: "Could not load this image."
            color: "#ddd"
            font.family: Theme.uiFont
            font.pixelSize: 15
        }

        // ---- Video -------------------------------------------------------
        Item {
            visible: root.kind === "movie"
            anchors.fill: parent
            anchors.margins: 40

            MediaPlayer {
                id: player
                source: root.opened && root.kind === "movie" ? root.url : ""
                audioOutput: AudioOutput { id: sound }
                videoOutput: video
            }
            VideoOutput {
                id: video
                anchors.fill: parent
                anchors.bottomMargin: 84
                fillMode: VideoOutput.PreserveAspectFit
                TapHandler {
                    longPressThreshold: 0
                    onTapped: player.playbackState === MediaPlayer.PlayingState ? player.pause() : player.play()
                }
            }
            // Controls, on their own pill and clear of the window edge.
            Rectangle {
                anchors.fill: controls
                anchors.margins: -10
                radius: height / 2
                color: Qt.rgba(0.08, 0.08, 0.1, 0.92)
                border.color: Qt.rgba(1, 1, 1, 0.08)
            }
            Row {
                id: controls
                anchors.bottom: parent.bottom
                anchors.bottomMargin: 14
                anchors.horizontalCenter: parent.horizontalCenter
                width: Math.min(parent.width - 40, 760)
                height: 40
                spacing: 12
                Rectangle {
                    width: 40; height: 40; radius: 20
                    color: Theme.accent
                    PlayGlyph {
                        anchors.centerIn: parent
                        playing: player.playbackState === MediaPlayer.PlayingState
                        color: Theme.bg
                        size: 14
                    }
                    TapHandler {
                        longPressThreshold: 0
                        onTapped: player.playbackState === MediaPlayer.PlayingState ? player.pause() : player.play()
                    }
                }
                Slider {
                    id: seek
                    width: parent.width - 40 - 44 - 120 - 36
                    anchors.verticalCenter: parent.verticalCenter
                    from: 0
                    to: Math.max(1, player.duration)
                    value: player.position
                    enabled: player.seekable
                    onMoved: player.position = value
                }
                Text {
                    width: 120
                    anchors.verticalCenter: parent.verticalCenter
                    horizontalAlignment: Text.AlignHCenter
                    text: root.clock(player.position) + " / " + root.clock(player.duration)
                    color: "#ddd"
                    font.family: Theme.monoFont
                    font.pixelSize: 13
                }
                Rectangle {
                    width: 40; height: 40; radius: 20
                    color: Qt.rgba(1, 1, 1, 0.12)
                    IconGlyph { anchors.centerIn: parent; kind: sound.muted ? "speaker-off" : "speaker"; color: "#eee"; size: 16 }
                    TapHandler { longPressThreshold: 0; onTapped: sound.muted = !sound.muted }
                }
            }
            Text {
                anchors.centerIn: parent
                visible: player.error !== MediaPlayer.NoError
                text: "Could not play this here."
                color: "#ddd"
                font.family: Theme.uiFont
                font.pixelSize: 15
            }
        }
    }

    function clock(ms) {
        const s = Math.floor(ms / 1000)
        return Math.floor(s / 60) + ":" + String(s % 60).padStart(2, "0")
    }

    // ---- Top bar ----------------------------------------------------------
    Rectangle {
        anchors.left: parent.left
        anchors.right: parent.right
        height: 56
        color: Qt.rgba(0, 0, 0, 0.35)
        Text {
            anchors.left: parent.left
            anchors.leftMargin: 20
            anchors.right: actions.left
            anchors.rightMargin: 16
            anchors.verticalCenter: parent.verticalCenter
            text: root.fileName + (root.kind === "image" && pic.status === Image.Ready
                    ? "    " + pic.implicitWidth + " × " + pic.implicitHeight + "    " + Math.round(root.zoom * 100) + "%"
                    : "")
            color: "#eee"
            elide: Text.ElideMiddle
            font.family: Theme.uiFont
            font.pixelSize: 14
        }
        Row {
            id: actions
            anchors.right: parent.right
            anchors.rightMargin: 12
            anchors.verticalCenter: parent.verticalCenter
            spacing: 6
            component Action: Rectangle {
                property alias label: t.text
                signal clicked()
                height: 34
                width: t.implicitWidth + 24
                radius: 17
                color: h.hovered ? Qt.rgba(1, 1, 1, 0.18) : Qt.rgba(1, 1, 1, 0.08)
                Text { id: t; anchors.centerIn: parent; color: "#eee"; font.family: Theme.uiFont; font.pixelSize: 13 }
                HoverHandler { id: h }
                TapHandler { longPressThreshold: 0; onTapped: parent.clicked() }
            }
            Action { visible: root.kind === "image"; label: "Fit"; onClicked: root.fit() }
            Action { visible: root.kind === "image"; label: "100%"; onClicked: root.zoomAt(1, stage.width / 2, stage.height / 2) }
            Action { label: "Copy link"; onClicked: Core.ctl.copyText(root.url) }
            Action { label: "Open in browser"; onClicked: { Qt.openUrlExternally(root.url); root.close() } }
            Action { label: "✕"; onClicked: root.close() }
        }
    }
}
