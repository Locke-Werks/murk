import QtQuick

// Small drawn icons for the few places that used colour emoji. Emoji depend on
// an emoji font being installed, which many Linux systems (WSL among them) lack,
// and then show as empty boxes.
// kind: "bell", "bell-off", "speaker", "speaker-off".
Canvas {
    id: root
    property string kind: "bell"
    property color color: "white"
    property real size: 14
    width: size
    height: size
    onKindChanged: requestPaint()
    onColorChanged: requestPaint()
    onWidthChanged: requestPaint()

    onPaint: {
        const c = getContext("2d")
        c.reset()
        const s = width
        c.strokeStyle = root.color
        c.fillStyle = root.color
        c.lineWidth = Math.max(1.2, s * 0.09)
        c.lineCap = "round"
        c.lineJoin = "round"
        if (kind === "bell" || kind === "bell-off") {
            // Dome and flared rim.
            c.beginPath()
            c.moveTo(s * 0.18, s * 0.74)
            c.lineTo(s * 0.82, s * 0.74)
            c.quadraticCurveTo(s * 0.72, s * 0.62, s * 0.72, s * 0.44)
            c.bezierCurveTo(s * 0.72, s * 0.22, s * 0.28, s * 0.22, s * 0.28, s * 0.44)
            c.quadraticCurveTo(s * 0.28, s * 0.62, s * 0.18, s * 0.74)
            c.closePath()
            c.fill()
            // Clapper.
            c.beginPath()
            c.arc(s * 0.5, s * 0.84, s * 0.08, 0, Math.PI * 2)
            c.fill()
            // Top knob.
            c.beginPath()
            c.arc(s * 0.5, s * 0.2, s * 0.06, 0, Math.PI * 2)
            c.fill()
        } else {
            // Speaker body.
            c.beginPath()
            c.moveTo(s * 0.12, s * 0.38)
            c.lineTo(s * 0.3, s * 0.38)
            c.lineTo(s * 0.52, s * 0.18)
            c.lineTo(s * 0.52, s * 0.82)
            c.lineTo(s * 0.3, s * 0.62)
            c.lineTo(s * 0.12, s * 0.62)
            c.closePath()
            c.fill()
            if (kind === "speaker") {
                // Sound waves.
                c.beginPath()
                c.arc(s * 0.52, s * 0.5, s * 0.18, -0.9, 0.9)
                c.stroke()
                c.beginPath()
                c.arc(s * 0.52, s * 0.5, s * 0.34, -0.9, 0.9)
                c.stroke()
            } else {
                // A cross where the waves would be.
                c.beginPath()
                c.moveTo(s * 0.64, s * 0.36); c.lineTo(s * 0.9, s * 0.64)
                c.moveTo(s * 0.9, s * 0.36); c.lineTo(s * 0.64, s * 0.64)
                c.stroke()
            }
        }
        if (kind === "bell-off") {
            c.beginPath()
            c.moveTo(s * 0.12, s * 0.1)
            c.lineTo(s * 0.88, s * 0.9)
            c.stroke()
        }
    }
}
