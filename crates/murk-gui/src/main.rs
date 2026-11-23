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

/// AppUserModelID, matching `aumid` in installer.toml.
pub const AUMID: &str = "LockeWerks.Murk";

/// Claim the AUMID for this process and register its toast name and icon under HKCU.
/// The installer's shortcut covers installed copies; the registry entry covers an exe
/// run from anywhere else, which Windows would otherwise refuse to toast for.
#[cfg(windows)]
fn register_app_id() {
    use windows::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey,
        RegCreateKeyExW, RegSetValueExW,
    };
    use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
    use windows::core::HSTRING;

    unsafe {
        let _ = SetCurrentProcessExplicitAppUserModelID(&HSTRING::from(AUMID));
    }
    let Some(dir) =
        std::env::var_os("LOCALAPPDATA").map(|d| std::path::PathBuf::from(d).join("Murk"))
    else {
        return;
    };
    let icon = dir.join("murk.png");
    if std::fs::create_dir_all(&dir).is_err()
        || std::fs::write(&icon, include_bytes!("../../../assets/murk.png")).is_err()
    {
        return;
    }
    let key = HSTRING::from(format!("Software\\Classes\\AppUserModelId\\{AUMID}"));
    let mut h = HKEY::default();
    unsafe {
        if RegCreateKeyExW(
            HKEY_CURRENT_USER,
            &key,
            None,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut h,
            None,
        )
        .is_err()
        {
            return;
        }
        for (name, value) in [
            ("DisplayName", "Murk".to_string()),
            ("IconUri", icon.display().to_string()),
        ] {
            let wide: Vec<u16> = value.encode_utf16().chain([0]).collect();
            let bytes = std::slice::from_raw_parts(wide.as_ptr().cast::<u8>(), wide.len() * 2);
            let _ = RegSetValueExW(h, &HSTRING::from(name), None, REG_SZ, Some(bytes));
        }
        let _ = RegCloseKey(h);
    }
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
    #[cfg(windows)]
    register_app_id();

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
