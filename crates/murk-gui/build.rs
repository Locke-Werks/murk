use cxx_qt_build::{CxxQtBuilder, QmlFile, QmlModule};

const COMPONENTS: &[&str] = &[
    "qml/Main.qml",
    "qml/Avatar.qml",
    "qml/Backdrop.qml",
    "qml/Sidebar.qml",
    "qml/ChatView.qml",
    "qml/MessageDelegate.qml",
    "qml/Composer.qml",
    "qml/NickList.qml",
    "qml/CommandPalette.qml",
    "qml/EmojiPicker.qml",
    "qml/SettingsPanel.qml",
    "qml/NetworkEditor.qml",
    "qml/Welcome.qml",
    "qml/Toast.qml",
    "qml/IconButton.qml",
    "qml/Confetti.qml",
    "qml/SearchPanel.qml",
    "qml/TransferTray.qml",
    "qml/ProfileCard.qml",
    "qml/ChannelBrowser.qml",
];

fn main() {
    let mut files: Vec<QmlFile> = COMPONENTS.iter().map(|f| QmlFile::from(*f)).collect();
    files.push(QmlFile::from("qml/Theme.qml").singleton(true));
    files.push(QmlFile::from("qml/Core.qml").singleton(true));
    CxxQtBuilder::new_qml_module(QmlModule::new("Murk").qml_files(files))
        .qt_module("Quick")
        .qt_module("QuickControls2")
        .files(["src/bridge.rs"])
        .build();
    windows_resources("Murk desktop client", "murk-gui");
}
/// Icon and version resource. The .rc is a template so the version comes from Cargo.
fn windows_resources(desc: &str, name: &str) {
    println!("cargo:rerun-if-changed=murk.rc");
    println!("cargo:rerun-if-changed=../../assets/murk.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let ver = std::env::var("CARGO_PKG_VERSION").unwrap();
    let mut parts: Vec<String> = ver
        .split(['.', '-'])
        .take(3)
        .map(|p| p.parse::<u16>().unwrap_or(0).to_string())
        .collect();
    parts.resize(4, "0".into());
    let dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let icon = dir.join("../../assets/murk.ico").canonicalize().unwrap();
    // rc.exe wants a plain path, without the \\?\ prefix canonicalize adds on Windows.
    let icon = icon
        .display()
        .to_string()
        .replace("\\\\?\\", "")
        .replace('\\', "/");
    let rc = std::fs::read_to_string(dir.join("murk.rc"))
        .unwrap()
        .replace("@COMMA@", &parts.join(","))
        .replace("@VER@", &ver)
        .replace("@DESC@", desc)
        .replace("@NAME@", name)
        .replace("@FILE@", &format!("{name}.exe"))
        .replace("@ICON@", &icon);
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("murk.rc");
    std::fs::write(&out, rc).unwrap();
    embed_resource::compile(&out, embed_resource::NONE)
        .manifest_optional()
        .unwrap();
}
