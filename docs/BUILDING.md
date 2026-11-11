# Building Murk

Murk is a Cargo workspace with two programs:

| Program    | Crate      | Needs Qt |
|------------|------------|----------|
| `murk-gui` | `murk-gui` | yes      |
| `murk`     | `murk-tui` | no       |

Everything else is plain Rust plus three C libraries that are compiled from
source during the build (Lua 5.4 for scripting, SQLite for the log, and `ring`
for TLS). There is no OpenSSL dependency and nothing to install for those
three beyond a C compiler.

## What every platform needs

| Tool | Version | Used for |
|------|---------|----------|
| Rust (via rustup) | latest stable; the workspace uses edition 2024, so 1.85 or newer | everything |
| A C and C++ compiler | MSVC 2022 on Windows, GCC or Clang elsewhere | Lua, SQLite, `ring`, the Qt bridge |
| Qt | 6.8 or newer (CI uses 6.8.3) | `murk-gui` only |
| Git | any | fetching the source |

The desktop client links the Qt modules Core, Gui, Qml, Quick and
QuickControls2, and loads Qt Multimedia at runtime to play video and audio
inline. Multimedia is an add-on module: the install steps below include it.

The build finds Qt through `qmake`. Point the `QMAKE` environment variable at
the `qmake` of the Qt you want, or put that Qt's `bin` directory first on
`PATH`.

## Windows 10 and 11

Supported and released: CI builds and tests here, and tagged releases ship a
signed installer.

### 1. Visual Studio Build Tools

Install [Build Tools for Visual Studio 2022](https://visualstudio.microsoft.com/downloads/#build-tools-for-visual-studio-2022)
(the full Visual Studio works too). In the installer, select the
**Desktop development with C++** workload, which brings the MSVC v143
compiler and a Windows SDK.

From a terminal, the same thing:

```
winget install Microsoft.VisualStudio.2022.BuildTools --override "--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
```

### 2. Rust

```
winget install Rustlang.Rustup
rustup default stable-x86_64-pc-windows-msvc
```

Use the MSVC toolchain, not GNU: Qt's Windows kit is built with MSVC and the
two do not link together.

### 3. Qt

Either the [Qt Online Installer](https://www.qt.io/download-qt-installer-oss)
(free account; select Qt 6.8.x, component **MSVC 2022 64-bit**, and under Additional Libraries **Qt Multimedia**), or without an
account through `aqtinstall`, which is what CI uses:

```
pip install aqtinstall
aqt install-qt windows desktop 6.8.3 win64_msvc2022_64 -m qtmultimedia --outputdir C:\Qt
```

Either way you end up with `C:\Qt\6.8.3\msvc2022_64\bin\qmake.exe`.

### 4. Git

```
winget install Git.Git
```

### 5. Build

PowerShell:

```
git clone https://github.com/Locke-Werks/murk.git
cd murk
$env:QMAKE = "C:\Qt\6.8.3\msvc2022_64\bin\qmake.exe"
cargo build --release
```

Git Bash:

```
QMAKE=/c/Qt/6.8.3/msvc2022_64/bin/qmake.exe cargo build --release
```

In Git Bash, `/usr/bin/link` (a GNU coreutils program) shadows MSVC's
`link.exe` if it comes first on `PATH`, and the build fails at the link step
with a usage message from the wrong tool. Run from PowerShell, or make sure
Git's `usr/bin` is not ahead of the MSVC tools on `PATH`.

### 6. Run

The binary needs Qt's DLLs. To run it from the build tree, put Qt's `bin` on
`PATH` first:

```
$env:PATH = "C:\Qt\6.8.3\msvc2022_64\bin;$env:PATH"
.\target\release\murk-gui.exe
```

To make a folder that runs anywhere, copy in Qt's runtime with `windeployqt`:

```
mkdir stage
copy target\release\murk-gui.exe, target\release\murk.exe stage\
C:\Qt\6.8.3\msvc2022_64\bin\windeployqt.exe --release --no-translations --qmldir crates\murk-gui\qml stage\murk-gui.exe
```

The terminal client `murk.exe` has no Qt dependency and runs on its own.

## Linux

Supported and released: CI builds and tests on Ubuntu 22.04, and tagged
releases ship an AppImage.

### 1. Compiler and system libraries

Debian and Ubuntu:

```
sudo apt-get update
sudo apt-get install -y build-essential git curl pkg-config libdbus-1-dev \
  libgl1-mesa-dev libxkbcommon-x11-0 libxcb-cursor0
```

Fedora:

```
sudo dnf install -y gcc gcc-c++ make git curl pkgconf-pkg-config dbus-devel \
  mesa-libGL-devel libxkbcommon-x11 xcb-util-cursor
```

Arch:

```
sudo pacman -S --needed base-devel git curl pkgconf dbus mesa libxkbcommon-x11 xcb-util-cursor
```

`libdbus-1-dev` is for desktop notifications and the keyring (Secret Service).
`libxcb-cursor0` is needed by Qt 6.5 and newer to start under X11.

### 2. Rust

```
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
```

Distribution Rust packages are often too old for edition 2024; use rustup.

### 3. Qt

Murk needs Qt 6.8 or newer. Check what your distribution ships:

- Fedora 41 and newer, Arch, openSUSE Tumbleweed: new enough. Install the
  base and declarative packages (`qt6-qtbase-devel qt6-qtdeclarative-devel qt6-qtmultimedia-devel` on
  Fedora, `qt6-base qt6-declarative qt6-multimedia` on Arch).
- Ubuntu 24.04 and Debian 12: too old (Qt 6.4). Use `aqtinstall`:

```
pip install aqtinstall
aqt install-qt linux desktop 6.8.3 linux_gcc_64 -m qtmultimedia --outputdir ~/Qt
```

### 4. Build

```
git clone https://github.com/Locke-Werks/murk.git
cd murk
export QMAKE=~/Qt/6.8.3/gcc_64/bin/qmake     # or /usr/bin/qmake6 for a distribution Qt
cargo build --release
```

With a Qt from `aqtinstall`, run with its libraries on the path:

```
LD_LIBRARY_PATH=~/Qt/6.8.3/gcc_64/lib ./target/release/murk-gui
```

To produce a self-contained AppImage, follow the `AppImage` step in
`.github/workflows/release.yml`, which uses `linuxdeploy` and its Qt plugin.

### Spell checking

Spell checking reads Hunspell dictionaries from `/usr/share/hunspell` or
`/usr/share/myspell`. Install `hunspell-en-us` (Debian, Ubuntu, Arch) or
`hunspell-en-US` (Fedora), or your language's equivalent. Without one, spell
checking is off and nothing else changes.

## macOS

Not tested. No CI job and no release builds exist for macOS, and Murk has never
been compiled on a Mac. Every dependency supports macOS and these steps should
work; reports either way are welcome.

### 1. Command line tools and Homebrew

```
xcode-select --install
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
```

### 2. Rust

```
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
```

### 3. Qt

```
brew install qt
```

Homebrew's `qt` is Qt 6 and current. If it is ever behind 6.8, use
`aqt install-qt mac desktop 6.8.3 clang_64 -m qtmultimedia --outputdir ~/Qt` instead.

### 4. Build

```
git clone https://github.com/Locke-Werks/murk.git
cd murk
export QMAKE="$(brew --prefix qt)/bin/qmake"
cargo build --release
```

### 5. Make an app bundle

```
mkdir -p Murk.app/Contents/MacOS
cp target/release/murk-gui Murk.app/Contents/MacOS/
"$(brew --prefix qt)/bin/macdeployqt" Murk.app -qmldir=crates/murk-gui/qml
```

A bundle you build runs on your own Mac. Giving it to someone else needs a
Developer ID signature and notarization, or Gatekeeper refuses to open it.

Spell checking reads Hunspell dictionaries from `~/Library/Spelling` or
Homebrew's `share/hunspell`. Passwords go to the macOS Keychain.

## Terminal client only

`murk` needs no Qt on any platform. With Rust and a C compiler installed:

```
cargo build --release -p murk-tui
```

## Checks

These are the same three commands CI runs, and must pass before a change is
merged:

```
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

`QMAKE` must be set for the last two, since they build `murk-gui`. To skip the
desktop client, add `--exclude murk-gui`.

## Troubleshooting

| Symptom | Cause |
|---------|-------|
| `qmake` not found, or the build picks the wrong Qt | `QMAKE` unset or pointing at Qt 5. Set it to a Qt 6.8+ `qmake`. |
| Link errors mentioning `link: extra operand` (Windows) | GNU `link` from Git's `usr/bin` shadows MSVC's. See the Windows build step. |
| `cl.exe` or `link.exe` not found (Windows) | The C++ workload of the Build Tools is missing, or was removed by a Visual Studio update. Re-run the installer. |
| `murk-gui` exits at once with "could not find the Qt platform plugin" | Qt's libraries are not on the path. Run with Qt's `bin` (Windows) or `lib` (Linux) on the search path, or deploy with `windeployqt` / `macdeployqt` / `linuxdeploy`. |
| `qt.qpa.plugin: Could not load the Qt platform plugin "xcb"` (Linux) | `libxcb-cursor0` is missing. |
| A Lua or SQLite build script fails | No C compiler on `PATH`. Install the compiler for your platform from step 1. |
