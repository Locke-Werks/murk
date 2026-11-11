//! `murk-gui`: the Qt Quick desktop client.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod bridge;
mod html;
mod model;
mod spell;

use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QString, QUrl};
use std::sync::OnceLock;

static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

/// `--config <path>` from the command line, else the platform default.
pub fn config_path() -> std::path::PathBuf {
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if (a == "--config" || a == "-c")
            && let Some(p) = args.next()
        {
            return p.into();
        }
    }
    murk_config::default_path()
}

/// The shared tokio runtime. Network tasks and the event pump run here.
pub fn runtime() -> &'static tokio::runtime::Runtime {
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("tokio runtime")
    })
}

fn main() {
    // Engine::add_network spawns onto the ambient runtime, and it is called from the Qt thread.
    let _guard = runtime().enter();

    let mut app = QGuiApplication::new();
    if let Some(mut app) = app.as_mut() {
        app.as_mut().set_application_name(&QString::from("Murk"));
        app.as_mut()
            .set_organization_name(&QString::from("Locke Werks"));
        app.as_mut()
            .set_application_version(&QString::from(env!("CARGO_PKG_VERSION")));
    }
    let mut engine = QQmlApplicationEngine::new();
    if let Some(engine) = engine.as_mut() {
        engine.load(&QUrl::from("qrc:/qt/qml/Murk/qml/Main.qml"));
    }
    let code = app.as_mut().map(|app| app.exec()).unwrap_or(1);
    // QML objects first, then COM objects, while Qt and COM are both still alive.
    drop(engine);
    spell::shutdown();
    drop(app);
    std::process::exit(code);
}
