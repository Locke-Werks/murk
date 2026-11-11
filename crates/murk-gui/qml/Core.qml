pragma Singleton
import QtQuick
import Murk

// App-wide state: the Rust controller plus parsed views of its JSON properties.
QtObject {
    id: core

    readonly property Controller ctl: Controller {}

    readonly property var buffers: JSON.parse(ctl.buffers || "[]")
    readonly property var nicks: JSON.parse(ctl.nicks || "[]")
    readonly property var settings: ctl.settings ? JSON.parse(ctl.settings) : ({})
    readonly property var emoji: JSON.parse(ctl.emojiCatalog())
    readonly property var quickReactions: JSON.parse(ctl.quickReactions())
    readonly property var commands: JSON.parse(ctl.commands())

    // A reply being composed: {msgid, sender, text}, or null.
    property var replyTo: null

    function set(key, value) {
        ctl.setSetting(key, JSON.stringify(value))
    }

    // Subsequence match with a bonus for word starts; -1 when `q` does not match.
    function fuzzy(q, s) {
        q = q.toLowerCase()
        s = s.toLowerCase()
        if (!q.length)
            return 0
        let score = 0, j = 0, run = 0
        for (let i = 0; i < s.length && j < q.length; i++) {
            if (s[i] === q[j]) {
                run++
                score += 1 + run * 2 + (i === 0 || "#_- ".indexOf(s[i - 1]) >= 0 ? 6 : 0)
                j++
            } else {
                run = 0
            }
        }
        return j === q.length ? score - s.length * 0.1 : -1
    }

    // The first two links in a message, for preview cards.
    function links(text) {
        const out = []
        const re = /https?:\/\/[^\s<>"]+/g
        let m
        while ((m = re.exec(text)) !== null && out.length < 2) {
            const url = m[0].replace(/[.,);:!?']+$/, "")
            if (out.indexOf(url) < 0)
                out.push(url)
        }
        return out
    }

    // Previews by URL: undefined while unknown, "pending" while fetching, then the card or null.
    property var previewCache: ({})
    property int previewTick: 0

    function preview(url) {
        const p = previewCache[url]
        if (p === undefined) {
            previewCache[url] = "pending"
            ctl.requestPreview(url)
            return null
        }
        return p === "pending" ? null : p
    }

    readonly property Connections _previews: Connections {
        target: core.ctl
        function onPreviewReady(url, preview) {
            core.previewCache[url] = JSON.parse(preview)
            core.previewTick++
        }
    }
}
