import QtQuick
import Murk

// Several toasts at once, newest on top. Each keeps its own timer and reply box,
// so a second DM never wipes out an answer half typed into the first.
Column {
    id: root
    spacing: 10
    readonly property int maxShown: 4

    // kind: "mention" (highlight colour, replyable) or "friend" (online colour, no reply).
    function show(heading, body, index, kind, nick) {
        // Repeated alerts from one buffer refresh that toast instead of stacking.
        for (let i = 0; i < toasts.count; i++) {
            const t = toasts.get(i)
            if (index >= 0 && t.target === index && t.kind === (kind || "mention")) {
                toasts.setProperty(i, "body", body)
                toasts.setProperty(i, "stamp", Date.now())
                return
            }
        }
        toasts.insert(0, { heading: heading, body: body, target: index, kind: kind || "mention", nick: nick || "", stamp: Date.now() })
        while (toasts.count > maxShown)
            toasts.remove(toasts.count - 1)
    }

    ListModel { id: toasts }

    Repeater {
        model: toasts
        Toast {
            id: card
            required property var model
            readonly property real stamp: model.stamp
            tint: model.kind === "friend" ? Theme.ok : Theme.hl
            replyable: model.kind !== "friend"
            query: model.nick
            Component.onCompleted: show(model.heading, model.body, model.target)
            // An update to an existing toast re-shows it and restarts its timer.
            onStampChanged: show(model.heading, model.body, model.target)
            onGone: {
                for (let i = 0; i < toasts.count; i++)
                    if (toasts.get(i).stamp === card.stamp) { toasts.remove(i); return }
            }
        }
    }

    move: Transition { NumberAnimation { properties: "y"; duration: Theme.med; easing.type: Easing.OutCubic } }
}
