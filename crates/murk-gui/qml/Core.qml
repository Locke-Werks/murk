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
    // Re-read once the controller has started, so script commands are included.
    readonly property var commands: { ctl.ready; return JSON.parse(ctl.commands()) }

    // A reply being composed: {msgid, sender, text}, or null.
    property var replyTo: null
    // Mentions that arrived since the mentions panel was last opened.
    property int unseenMentions: 0

    // Anyone can ask for a profile card; Main shows it.
    signal profileRequested(string nick, string color, real x, real y)
    // Images and video open in the in-app viewer; Main shows it. kind: "image" or "movie".
    signal mediaRequested(string url, string kind, string image)

    readonly property var imagePattern: /\.(png|jpe?g|gif|webp|avif|bmp)(\?|#|$)/i
    readonly property var moviePattern: /\.(mp4|m4v|webm|mov|mkv)(\?|#|$)/i

    // Every link click goes through here. Images and video stay in Murk, in the
    // viewer; anything else opens with whatever the system uses for it.
    function openLink(url) {
        const p = previewCache[url]
        const known = p && p !== "pending"
        if (known && p.kind === "image")
            return mediaRequested(url, "image", p.image || url)
        if (known && p.kind === "movie")
            return mediaRequested(url, "movie", "")
        if (!known && imagePattern.test(url))
            return mediaRequested(url, "image", url)
        if (!known && moviePattern.test(url))
            return mediaRequested(url, "movie", "")
        Qt.openUrlExternally(url)
    }

    // Rich text from Rust carries a placeholder link color; give links the theme's accent.
    function themed(html) {
        return html ? html.replace(/#0a0b0c/g, Theme.accent) : ""
    }

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
