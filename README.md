<div align="center">

<img src="assets/murk.ico" width="96" alt="Murk">

# Murk

**An IRC client for the desktop and the terminal, built on one Rust core.**

[![license](https://img.shields.io/badge/license-GPLv3-d6262a?style=flat-square)](LICENSE)
[![platform](https://img.shields.io/badge/platform-Windows%2011%20%7C%20Linux-d6262a?style=flat-square)](#requirements)

</div>

---

Murk has two front ends over the same engine: `murk-gui`, a Qt Quick desktop
app, and `murk`, a terminal client. Both share connections, logs, scripts,
themes and settings.

![Murk desktop client](docs/screenshot.webp)

## What it does

- IRCv3: CAP 302, SASL PLAIN and EXTERNAL, server-time, message-tags, batch,
  echo-message, chathistory, labeled-response, away-notify, account-tag,
  extended-join, typing notifications, reactions (`+draft/react`) and replies
  (`+draft/reply`). Networks that deny client tags (CLIENTTAGDENY) get classic
  `nick:` replies, and reactions there stay local.
- TLS by default, reconnect with backoff, flood control that never delays PONG.
- A local SQLite log with full-text search, read markers and scrollback that
  pages in from the log, then from the server.
- Desktop client: grouped messages with avatars, day and unread dividers with a
  summary of what you missed, link preview cards (OpenGraph, images, YouTube),
  reactions and replies, emoji shortcodes and picker, folded join/part noise,
  a command palette, a channel browser, a mentions inbox, search, profile cards
  from WHOIS, spell check, DCC transfers with drag and drop, seven themes and a
  compact layout.
- Terminal client: buffers, nick list, completion, history, mIRC formatting,
  search, folded join/part runs, reactions and replies, the same themes.
- Lua 5.4 scripting, sandboxed: hooks, custom commands, timers.
- Passwords live in the system keyring (Windows Credential Manager, Secret
  Service on Linux), never in the config file.

## Requirements

- Windows 11, or Linux with Qt 6.8 or later for the desktop client.
- The terminal client needs only a terminal with 256 colors or true color.

## Building

```
# Rust stable and Qt 6.8
QMAKE=/path/to/Qt/6.8.3/<kit>/bin/qmake cargo build --release
```

This produces `target/release/murk-gui` and `target/release/murk`. Tagged
releases build a signed Windows installer and a Linux AppImage in CI.

### macOS

There are no macOS release builds, and macOS is not tested in CI. It builds
from source with the same toolchain:

```
xcode-select --install                     # Apple's compiler and linker
curl https://sh.rustup.rs -sSf | sh        # Rust stable
brew install qt                            # Qt 6.8 or later
QMAKE="$(brew --prefix qt)/bin/qmake" cargo build --release
```

The terminal client needs nothing beyond Rust: `cargo build --release -p murk-tui`.

To run the desktop client as an app rather than from a terminal, wrap it into a
bundle with Qt's deploy tool:

```
mkdir -p Murk.app/Contents/MacOS
cp target/release/murk-gui Murk.app/Contents/MacOS/
"$(brew --prefix qt)/bin/macdeployqt" Murk.app -qmldir=crates/murk-gui/qml
```

A bundle you build yourself runs on your own Mac. Giving it to someone else
needs a Developer ID signature and notarization, or Gatekeeper refuses to open
it.

Spell checking uses Hunspell dictionaries. Put `en_US.aff` and `en_US.dic` (or
your locale's pair) in `~/Library/Spelling`, or install them with Homebrew.
Without one the client runs normally with spell checking off.

## Using it

Start `murk-gui`, pick a network from the presets, and join channels from the
channel browser (Ctrl+J). Ctrl+K jumps anywhere; Ctrl+/ lists every shortcut.

The terminal client takes the same config:

```
murk [--config <path>] [--no-log] [network...]
```

`/help` lists commands in both clients. A few that are not standard IRC:

| Command | Does |
|---|---|
| `/react <emoji>` | React to the last message |
| `/reply <text>` | Reply to the last message |
| `/dcc send <nick> <path>` | Offer a file; `/dcc get` accepts an offer |
| `/theme <name>` | murk, aurora, ember, synthwave, mocha, phosphor, paper |
| `/script list`, `/script reload` | Manage Lua scripts |

## Configuration

The config file is TOML: `%APPDATA%\murk\murk.toml` on Windows,
`~/.config/murk/murk.toml` on Linux. It is written on first run and edited by
the desktop client's settings. Logs go to the platform data directory, or to
`MURK_DATA_DIR`.

## Scripting

Every `*.lua` file in the `scripts` folder next to the config loads at startup.
Scripts cannot touch files or run programs.

```lua
murk.command("slap", function(who)
  murk.me("slaps " .. who .. " around a bit with a large trout")
end, "/slap <nick>")

murk.on("message", function(m)
  if m.text:match("^!roll") then
    murk.say(m.nick .. " rolled a " .. math.random(1, 20))
  end
end)
```

Events: `message`, `input`, `join`, `part`, `connect`, `disconnect`.
Functions: `say`, `me`, `raw`, `print`, `react`, `run`, `timer`, `cancel`.
The first run writes a commented `example.lua.txt` into the folder.

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
