pragma Singleton
import QtQuick

QtObject {
    id: t

    // Each palette: bg (window), panel (sidebars), raised (cards, composer), line (hairlines),
    // text, sub (secondary), faint (timestamps), accent + accent2 (gradient pair),
    // hl (mentions), ok, warn, bad, fog (backdrop glow colors), mono (monospace everywhere).
    readonly property var palettes: ({
        "murk": {
            label: "Murk", dark: true,
            bg: "#0b0d14", panel: "#10131d", raised: "#171b28", hover: "#1c2131", line: "#232839",
            text: "#e7e9f3", sub: "#9aa0b8", faint: "#5c6280",
            accent: "#5eead4", accent2: "#a78bfa", hl: "#fbbf24",
            ok: "#4ade80", warn: "#fbbf24", bad: "#fb7185",
            fog: ["#5eead4", "#a78bfa", "#38bdf8"], mono: false
        },
        "aurora": {
            label: "Aurora", dark: true,
            bg: "#07120f", panel: "#0b1915", raised: "#10221d", hover: "#152b25", line: "#1c352e",
            text: "#e3f5ee", sub: "#93b8aa", faint: "#557a6c",
            accent: "#34d399", accent2: "#f472b6", hl: "#fde047",
            ok: "#34d399", warn: "#fde047", bad: "#fb7185",
            fog: ["#34d399", "#f472b6", "#22d3ee"], mono: false
        },
        "ember": {
            label: "Ember", dark: true,
            bg: "#120d0b", panel: "#18110e", raised: "#211814", hover: "#2a1f1a", line: "#33261f",
            text: "#f5ebe4", sub: "#bfa597", faint: "#7d665a",
            accent: "#fb923c", accent2: "#f43f5e", hl: "#facc15",
            ok: "#a3e635", warn: "#facc15", bad: "#f43f5e",
            fog: ["#fb923c", "#f43f5e", "#facc15"], mono: false
        },
        "synthwave": {
            label: "Synthwave", dark: true,
            bg: "#0f0a1f", panel: "#150d2b", raised: "#1d1238", hover: "#241645", line: "#2e1d55",
            text: "#f3e8ff", sub: "#b9a3d9", faint: "#7a64a0",
            accent: "#22d3ee", accent2: "#f472b6", hl: "#fde047",
            ok: "#4ade80", warn: "#fde047", bad: "#fb7185",
            fog: ["#f472b6", "#22d3ee", "#8b5cf6"], mono: false
        },
        "mocha": {
            label: "Mocha", dark: true,
            bg: "#1e1e2e", panel: "#181825", raised: "#252537", hover: "#2c2c40", line: "#313244",
            text: "#cdd6f4", sub: "#a6adc8", faint: "#6c7086",
            accent: "#cba6f7", accent2: "#89b4fa", hl: "#f9e2af",
            ok: "#a6e3a1", warn: "#f9e2af", bad: "#f38ba8",
            fog: ["#cba6f7", "#89b4fa", "#f5c2e7"], mono: false
        },
        "phosphor": {
            label: "Phosphor", dark: true,
            bg: "#030703", panel: "#050b05", raised: "#081208", hover: "#0b190b", line: "#0f240f",
            text: "#7dff9b", sub: "#4fbf68", faint: "#2d7a3f",
            accent: "#7dff9b", accent2: "#c6ff7d", hl: "#ffd37d",
            ok: "#7dff9b", warn: "#ffd37d", bad: "#ff7d7d",
            fog: ["#7dff9b", "#2dff6e", "#c6ff7d"], mono: true
        },
        "paper": {
            label: "Paper", dark: false,
            bg: "#f6f3ec", panel: "#efebe2", raised: "#fffdf8", hover: "#e8e3d7", line: "#ddd6c8",
            text: "#26231d", sub: "#5f584b", faint: "#9a917f",
            accent: "#0f766e", accent2: "#7c3aed", hl: "#b45309",
            ok: "#15803d", warn: "#b45309", bad: "#be123c",
            fog: ["#99f6e4", "#ddd6fe", "#fde68a"], mono: false
        }
    })

    readonly property var order: ["murk", "aurora", "ember", "synthwave", "mocha", "phosphor", "paper"]

    readonly property string name: palettes[Core.settings.theme] ? Core.settings.theme : "murk"
    readonly property var p: palettes[name]

    readonly property bool dark: p.dark
    readonly property color bg: p.bg
    readonly property color panel: p.panel
    readonly property color raised: p.raised
    readonly property color hover: p.hover
    readonly property color line: p.line
    readonly property color text: p.text
    readonly property color sub: p.sub
    readonly property color faint: p.faint
    readonly property color accent: p.accent
    readonly property color accent2: p.accent2
    readonly property color hl: p.hl
    readonly property color ok: p.ok
    readonly property color warn: p.warn
    readonly property color bad: p.bad
    readonly property var fog: p.fog

    readonly property bool compact: Core.settings.layout === "compact"
    readonly property int size: Core.settings.fontSize || 11
    readonly property string uiFont: p.mono ? monoFont
        : Qt.platform.os === "windows" ? "Segoe UI Variable Text" : "Inter"
    readonly property string monoFont: Qt.platform.os === "windows" ? "Cascadia Code" : "JetBrains Mono"
    readonly property string displayFont: p.mono ? monoFont
        : Qt.platform.os === "windows" ? "Segoe UI Variable Display" : "Inter"

    readonly property int radius: 10
    readonly property int fast: 120
    readonly property int med: 220

    function alpha(c, a) {
        return Qt.rgba(c.r, c.g, c.b, a)
    }
    // Nick colors come from a dark-background palette; darken them on light themes.
    function nick(c) {
        return dark ? c : Qt.darker(c, 1.9)
    }
}
