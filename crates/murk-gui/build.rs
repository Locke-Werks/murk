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
}
