//! Self-update. Asks GitHub for the latest release, picks the asset built for
//! this install, downloads and verifies it, and swaps it in.
//!
//! Nothing here touches the UI: frontends call [`check`], [`download`] and
//! [`install`] off their UI thread and decide how to present the result.

use futures_util::StreamExt;
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::AsyncWriteExt;

pub const LATEST_URL: &str = "https://api.github.com/repos/Locke-Werks/murk/releases/latest";
pub const RELEASES_PAGE: &str = "https://github.com/Locke-Werks/murk/releases/latest";
/// Generic on purpose: nothing about the user or their machine goes out.
const USER_AGENT: &str = "murk-updater";
/// The largest asset we will download. The Windows installer is the biggest at
/// well under this; a release claiming more is wrong, not big.
pub const MAX_DOWNLOAD: u64 = 512 * 1024 * 1024;
const MAX_METADATA: usize = 1024 * 1024;
const SUMS_NAME: &str = "SHA256SUMS";
/// Automatic checks happen at most this often.
pub const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 3600);

/// Which client is asking. They ship differently on Linux: the desktop client
/// only as an AppImage, the terminal client also as a bare binary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frontend {
    Gui,
    Tui,
}

/// How this copy of Murk was installed, which decides what to download and how
/// to put it in place.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// Run the Forge installer; it owns Program Files.
    WindowsInstaller { exe: PathBuf },
    /// Replace the `.app` bundle that contains the running executable.
    MacBundle { bundle: PathBuf, exe: PathBuf },
    /// Replace the AppImage file named by `$APPIMAGE`.
    AppImage { file: PathBuf },
    /// Replace the running executable itself.
    LinuxBinary { exe: PathBuf },
}

impl Target {
    /// Work out the target for the running process.
    pub fn detect(frontend: Frontend) -> Result<Target, String> {
        let exe =
            std::env::current_exe().map_err(|e| format!("Cannot find the running program: {e}"))?;
        let exe = std::fs::canonicalize(&exe).unwrap_or(exe);
        let appimage = std::env::var_os("APPIMAGE").map(PathBuf::from);
        detect_from(
            std::env::consts::OS,
            std::env::consts::ARCH,
            &exe,
            appimage,
            frontend,
        )
    }

    /// The release asset file name this target installs from.
    pub fn asset_name(&self, version: &Version) -> String {
        match self {
            Target::WindowsInstaller { .. } => format!("Murk-{version}-Setup.exe"),
            Target::MacBundle { .. } => format!("Murk-{version}-macos-arm64.dmg"),
            Target::AppImage { .. } => format!("Murk-{version}-x86_64.AppImage"),
            Target::LinuxBinary { .. } => format!("murk-{version}-linux-x86_64"),
        }
    }
}

fn detect_from(
    os: &str,
    arch: &str,
    exe: &Path,
    appimage: Option<PathBuf>,
    frontend: Frontend,
) -> Result<Target, String> {
    // A build out of cargo's target directory updates with cargo. Installing a
    // release over the developer's checkout would be a surprise.
    let parent = exe
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str());
    let grand = exe
        .parent()
        .and_then(Path::parent)
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str());
    if matches!(parent, Some("debug" | "release")) && grand == Some("target") {
        return Err("This is a development build. Update it with git and cargo.".into());
    }
    match os {
        "windows" => Ok(Target::WindowsInstaller { exe: exe.into() }),
        "macos" => {
            if arch != "aarch64" {
                return Err("Murk releases for macOS are Apple Silicon only.".into());
            }
            let bundle = exe
                .ancestors()
                .find(|p| p.extension().is_some_and(|e| e == "app"))
                .ok_or("Murk is not running from an app bundle, so there is nothing to replace.")?;
            Ok(Target::MacBundle {
                bundle: bundle.into(),
                exe: exe.into(),
            })
        }
        "linux" => {
            if arch != "x86_64" {
                return Err(format!("There is no Murk release for Linux on {arch}."));
            }
            if let Some(file) = appimage.filter(|p| !p.as_os_str().is_empty()) {
                return Ok(Target::AppImage { file });
            }
            match frontend {
                Frontend::Tui => Ok(Target::LinuxBinary { exe: exe.into() }),
                Frontend::Gui => Err(
                    "The desktop client updates itself only when run as the AppImage. Update this copy the way you installed it."
                        .into(),
                ),
            }
        }
        other => Err(format!("Self-update is not available on {other}.")),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
}

/// A newer release with an asset for this target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Update {
    pub version: Version,
    pub page: String,
    pub asset: Asset,
    pub sums: Asset,
}

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    assets: Vec<GhAsset>,
}

#[derive(Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
    size: u64,
}

pub fn current_version() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).expect("the crate version is semver")
}

/// `v1.2.3` or `1.2.3`.
pub fn parse_tag(tag: &str) -> Option<Version> {
    Version::parse(tag.trim().strip_prefix('v').unwrap_or(tag.trim())).ok()
}

/// Read a releases/latest response. `Ok(None)` means up to date, or nothing
/// installable for this target.
pub fn evaluate(json: &str, current: &Version, target: &Target) -> Result<Option<Update>, String> {
    let rel: GhRelease =
        serde_json::from_str(json).map_err(|e| format!("Unexpected answer from GitHub: {e}"))?;
    if rel.draft || rel.prerelease {
        return Ok(None);
    }
    let Some(version) = parse_tag(&rel.tag_name) else {
        return Ok(None);
    };
    if !version.pre.is_empty() || version <= *current {
        return Ok(None);
    }
    let pick = |name: &str| {
        rel.assets.iter().find(|a| a.name == name).map(|a| Asset {
            name: a.name.clone(),
            url: a.browser_download_url.clone(),
            size: a.size,
        })
    };
    let Some(asset) = pick(&target.asset_name(&version)) else {
        return Ok(None);
    };
    let sums = pick(SUMS_NAME).ok_or_else(|| {
        format!("Murk {version} was published without checksums, so it cannot be verified. Update by hand from {RELEASES_PAGE}.")
    })?;
    if !asset.url.starts_with("https://") || !sums.url.starts_with("https://") {
        return Err("The release links are not https; refusing them.".into());
    }
    if asset.size == 0 || asset.size > MAX_DOWNLOAD {
        return Err(format!(
            "{} reports an implausible size ({} bytes).",
            asset.name, asset.size
        ));
    }
    Ok(Some(Update {
        version,
        page: rel.html_url,
        asset,
        sums,
    }))
}

/// Parse `sha256sum` output: `<64 hex>  <name>`, or `*<name>` in binary mode.
pub fn parse_sums(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let (hash, rest) = line.trim_end().split_once(char::is_whitespace)?;
            let name = rest.trim_start().trim_start_matches('*');
            let ok = hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit());
            (ok && !name.is_empty()).then(|| (hash.to_ascii_lowercase(), name.to_string()))
        })
        .collect()
}

pub fn sum_for<'a>(sums: &'a [(String, String)], name: &str) -> Option<&'a str> {
    sums.iter()
        .find(|(_, n)| n == name)
        .map(|(h, _)| h.as_str())
}

fn client() -> Result<reqwest::Client, String> {
    static CRYPTO: OnceLock<()> = OnceLock::new();
    CRYPTO.get_or_init(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
    // No total timeout: a large download on a slow link is fine as long as it moves.
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(60))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .map_err(|e| e.to_string())
}

async fn get_small(client: &reqwest::Client, url: &str, accept: &str) -> Result<String, String> {
    let resp = client
        .get(url)
        .header(reqwest::header::ACCEPT, accept)
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| format!("Could not reach GitHub: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("GitHub answered {}", resp.status()));
    }
    let mut body = Vec::new();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        body.extend_from_slice(&chunk.map_err(|e| e.to_string())?);
        if body.len() > MAX_METADATA {
            return Err("GitHub's answer was unexpectedly large.".into());
        }
    }
    String::from_utf8(body).map_err(|_| "GitHub's answer was not text.".into())
}

/// Ask GitHub whether there is something newer for `target`.
pub async fn check(target: &Target) -> Result<Option<Update>, String> {
    let client = client()?;
    let json = get_small(&client, LATEST_URL, "application/vnd.github+json").await?;
    evaluate(&json, &current_version(), target)
}

/// Where downloads go. Per user, and emptied before each download.
pub fn download_dir() -> PathBuf {
    std::env::temp_dir().join("murk-update")
}

/// Download the asset into `dir` and verify size and SHA-256. `progress` gets
/// (bytes so far, total).
pub async fn download(
    update: &Update,
    dir: &Path,
    mut progress: impl FnMut(u64, u64),
) -> Result<PathBuf, String> {
    let client = client()?;
    let sums = get_small(&client, &update.sums.url, "application/octet-stream").await?;
    let want = sum_for(&parse_sums(&sums), &update.asset.name)
        .ok_or_else(|| format!("{} is not listed in {SUMS_NAME}.", update.asset.name))?
        .to_string();

    // The name came over the network; it is only ever a bare file name we built.
    if update.asset.name.contains(['/', '\\']) || update.asset.name.starts_with('.') {
        return Err("Refusing an asset name with a path in it.".into());
    }
    let _ = tokio::fs::remove_dir_all(dir).await;
    tokio::fs::create_dir_all(dir)
        .await
        .map_err(|e| format!("Cannot create {}: {e}", dir.display()))?;
    let part = dir.join(format!("{}.part", update.asset.name));
    let done = dir.join(&update.asset.name);

    let resp = client
        .get(&update.asset.url)
        .header(reqwest::header::ACCEPT, "application/octet-stream")
        .send()
        .await
        .map_err(|e| format!("Download failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("Download failed: {}", resp.status()));
    }
    let total = update.asset.size;
    if resp.content_length().is_some_and(|n| n != total) {
        return Err("The download is not the size the release says it is.".into());
    }
    let mut file = tokio::fs::File::create(&part)
        .await
        .map_err(|e| format!("Cannot write {}: {e}", part.display()))?;
    let mut hasher = Sha256::new();
    let mut got: u64 = 0;
    let mut stream = resp.bytes_stream();
    progress(0, total);
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Download interrupted: {e}"))?;
        got += chunk.len() as u64;
        if got > total {
            return Err("The download ran past the size the release says it is.".into());
        }
        hasher.update(&chunk);
        file.write_all(&chunk).await.map_err(|e| e.to_string())?;
        progress(got, total);
    }
    file.flush().await.map_err(|e| e.to_string())?;
    drop(file);
    if got != total {
        return Err(format!(
            "The download stopped short ({got} of {total} bytes)."
        ));
    }
    let have = hex(&hasher.finalize());
    if have != want {
        let _ = tokio::fs::remove_file(&part).await;
        return Err(format!(
            "{} failed verification: its SHA-256 does not match the release.",
            update.asset.name
        ));
    }
    tokio::fs::rename(&part, &done)
        .await
        .map_err(|e| e.to_string())?;
    Ok(done)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// What the frontend does once [`install`] returns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum After {
    /// Quit now. On Windows the installer is waiting for this process to end.
    Exit,
    /// The new version is in place: quit and start this executable.
    Relaunch(PathBuf),
}

/// Put the verified download in place. For Windows that means handing over to
/// the installer, which needs this process gone.
pub fn install(target: &Target, file: &Path, frontend: Frontend) -> Result<After, String> {
    match target {
        Target::WindowsInstaller { exe } => install_windows(file, exe, frontend),
        Target::MacBundle { bundle, exe } => {
            install_mac(file, bundle)?;
            Ok(After::Relaunch(exe.clone()))
        }
        Target::AppImage { file: dest } => {
            replace_file(file, dest)?;
            Ok(After::Relaunch(dest.clone()))
        }
        Target::LinuxBinary { exe } => {
            replace_file(file, exe)?;
            Ok(After::Relaunch(exe.clone()))
        }
    }
}

/// Start `exe` detached. For a GUI relaunch after [`After::Relaunch`].
pub fn spawn(exe: &Path) -> Result<(), String> {
    std::process::Command::new(exe)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Could not start {}: {e}", exe.display()))
}

fn not_writable(path: &Path, e: &std::io::Error) -> String {
    if e.kind() == std::io::ErrorKind::PermissionDenied {
        format!(
            "{} is not writable by you, so Murk cannot replace itself. Update it the way it was installed, or download it from {RELEASES_PAGE}.",
            path.display()
        )
    } else {
        format!("Cannot write beside {}: {e}", path.display())
    }
}

/// Copy beside `dest`, mark executable, and rename over it. The rename is the
/// only step that touches `dest`, so a failure leaves the old copy working.
fn replace_file(src: &Path, dest: &Path) -> Result<(), String> {
    let dir = dest
        .parent()
        .ok_or("The program has no parent directory.")?;
    let name = dest.file_name().ok_or("The program has no file name.")?;
    let staged = dir.join(format!(
        ".{}.new-{}",
        name.to_string_lossy(),
        std::process::id()
    ));
    std::fs::copy(src, &staged).map_err(|e| not_writable(dest, &e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(e) = std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755)) {
            let _ = std::fs::remove_file(&staged);
            return Err(format!("Cannot mark the new copy executable: {e}"));
        }
    }
    std::fs::rename(&staged, dest).map_err(|e| {
        let _ = std::fs::remove_file(&staged);
        not_writable(dest, &e)
    })
}

fn run(cmd: &mut std::process::Command) -> Result<(), String> {
    let name = cmd.get_program().to_string_lossy().into_owned();
    let out = cmd
        .output()
        .map_err(|e| format!("Could not run {name}: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{name} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

/// Mount the image, copy the bundle beside the running one, swap them, unmount.
fn install_mac(dmg: &Path, bundle: &Path) -> Result<(), String> {
    let parent = bundle
        .parent()
        .ok_or("The app bundle has no parent directory.")?;
    let name = bundle
        .file_name()
        .ok_or("The app bundle has no name.")?
        .to_string_lossy()
        .into_owned();
    let pid = std::process::id();
    let staged = parent.join(format!(".{name}.new-{pid}"));
    let old = parent.join(format!(".{name}.old-{pid}"));
    // Probe first so an unwritable /Applications is reported plainly, before mounting anything.
    std::fs::create_dir(&staged).map_err(|e| not_writable(bundle, &e))?;
    let _ = std::fs::remove_dir(&staged);

    let mnt = dmg.with_extension("mnt");
    let _ = std::fs::create_dir_all(&mnt);
    run(std::process::Command::new("/usr/bin/hdiutil")
        .args([
            "attach",
            "-nobrowse",
            "-readonly",
            "-noautoopen",
            "-mountpoint",
        ])
        .arg(&mnt)
        .arg(dmg))?;
    let result = (|| {
        let src = mnt.join("Murk.app");
        if !src.is_dir() {
            return Err("The disk image does not contain Murk.app.".to_string());
        }
        // ditto keeps the signature, extended attributes and symlinks intact.
        run(std::process::Command::new("/usr/bin/ditto")
            .arg(&src)
            .arg(&staged))?;
        std::fs::rename(bundle, &old).map_err(|e| not_writable(bundle, &e))?;
        if let Err(e) = std::fs::rename(&staged, bundle) {
            let _ = std::fs::rename(&old, bundle);
            return Err(not_writable(bundle, &e));
        }
        let _ = std::fs::remove_dir_all(&old);
        Ok(())
    })();
    let _ = run(std::process::Command::new("/usr/bin/hdiutil")
        .args(["detach", "-quiet"])
        .arg(&mnt));
    let _ = std::fs::remove_dir(&mnt);
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&staged);
    }
    result
}

/// Quote for a PowerShell single-quoted string literal.
fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// The script the detached helper runs: wait for us to exit (the installer
/// refuses to run while murk-gui.exe is up), install silently, and for the
/// desktop client start it again whatever the result, so a declined UAC prompt
/// does not leave the user with Murk simply gone.
fn windows_script(pid: u32, setup: &Path, relaunch: Option<&Path>) -> String {
    let mut s = format!(
        "$ErrorActionPreference = 'SilentlyContinue'; Wait-Process -Id {pid} -Timeout 120; \
         Start-Process -FilePath {} -ArgumentList '/S' -Wait",
        ps_quote(&setup.to_string_lossy())
    );
    if let Some(exe) = relaunch {
        s.push_str(&format!(
            "; Start-Process -FilePath {}",
            ps_quote(&exe.to_string_lossy())
        ));
    }
    s
}

fn install_windows(setup: &Path, exe: &Path, frontend: Frontend) -> Result<After, String> {
    let relaunch = (frontend == Frontend::Gui).then_some(exe);
    let script = windows_script(std::process::id(), setup, relaunch);
    // Full path, so a powershell.exe earlier on PATH is never the one run.
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    let ps = PathBuf::from(root).join("System32\\WindowsPowerShell\\v1.0\\powershell.exe");
    let mut cmd = std::process::Command::new(ps);
    cmd.args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-WindowStyle",
        "Hidden",
        "-Command",
    ])
    .arg(script);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // No console window, and its own process group so closing the terminal
        // the TUI ran in does not take the helper with it.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP);
    }
    cmd.spawn()
        .map_err(|e| format!("Could not start the installer: {e}"))?;
    Ok(After::Exit)
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Whether an automatic check is due. A last-check time in the future (a clock
/// that was wrong) counts as due rather than postponing checks indefinitely.
pub fn due(last: Option<u64>, now: u64) -> bool {
    match last {
        None => true,
        Some(t) if t > now => true,
        Some(t) => now - t >= CHECK_INTERVAL.as_secs(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    fn win() -> Target {
        Target::WindowsInstaller {
            exe: "C:\\Program Files\\Murk\\murk-gui.exe".into(),
        }
    }

    fn release(tag: &str, draft: bool, pre: bool, names: &[&str]) -> String {
        let assets: Vec<_> = names
            .iter()
            .map(|n| {
                serde_json::json!({
                    "name": n,
                    "browser_download_url": format!("https://github.com/Locke-Werks/murk/releases/download/{tag}/{n}"),
                    "size": 1234,
                })
            })
            .collect();
        serde_json::json!({
            "tag_name": tag, "draft": draft, "prerelease": pre,
            "html_url": "https://github.com/Locke-Werks/murk/releases/tag/x",
            "assets": assets,
        })
        .to_string()
    }

    #[test]
    fn tags_parse_with_or_without_v() {
        assert_eq!(parse_tag("v0.4.0"), Some(v("0.4.0")));
        assert_eq!(parse_tag("0.10.2"), Some(v("0.10.2")));
        assert_eq!(parse_tag("nightly"), None);
    }

    #[test]
    fn comparison_is_semantic_not_lexical() {
        assert!(v("0.10.0") > v("0.9.9"));
        let json = release(
            "v0.10.0",
            false,
            false,
            &["Murk-0.10.0-Setup.exe", "SHA256SUMS"],
        );
        let u = evaluate(&json, &v("0.9.9"), &win()).unwrap().unwrap();
        assert_eq!(u.version, v("0.10.0"));
        assert_eq!(u.asset.name, "Murk-0.10.0-Setup.exe");
    }

    #[test]
    fn same_or_older_is_no_update() {
        let json = release(
            "v0.3.0",
            false,
            false,
            &["Murk-0.3.0-Setup.exe", "SHA256SUMS"],
        );
        assert_eq!(evaluate(&json, &v("0.3.0"), &win()).unwrap(), None);
        assert_eq!(evaluate(&json, &v("0.3.1"), &win()).unwrap(), None);
    }

    #[test]
    fn drafts_and_prereleases_are_skipped() {
        let names = ["Murk-0.4.0-Setup.exe", "SHA256SUMS"];
        let cur = v("0.3.0");
        assert_eq!(
            evaluate(&release("v0.4.0", true, false, &names), &cur, &win()).unwrap(),
            None
        );
        assert_eq!(
            evaluate(&release("v0.4.0", false, true, &names), &cur, &win()).unwrap(),
            None
        );
        let rc = release(
            "v0.4.0-rc.1",
            false,
            false,
            &["Murk-0.4.0-rc.1-Setup.exe", "SHA256SUMS"],
        );
        assert_eq!(evaluate(&rc, &cur, &win()).unwrap(), None);
    }

    #[test]
    fn missing_checksums_is_an_error_not_an_unverified_update() {
        let json = release("v0.4.0", false, false, &["Murk-0.4.0-Setup.exe"]);
        assert!(evaluate(&json, &v("0.3.0"), &win()).is_err());
    }

    #[test]
    fn asset_per_platform() {
        let all = [
            "Murk-0.4.0-Setup.exe",
            "Murk-0.4.0-macos-arm64.dmg",
            "Murk-0.4.0-x86_64.AppImage",
            "murk-0.4.0-linux-x86_64",
            "SHA256SUMS",
        ];
        let json = release("v0.4.0", false, false, &all);
        let cur = v("0.3.0");
        let cases = [
            (win(), "Murk-0.4.0-Setup.exe"),
            (
                Target::MacBundle {
                    bundle: "/Applications/Murk.app".into(),
                    exe: "/Applications/Murk.app/Contents/MacOS/murk-gui".into(),
                },
                "Murk-0.4.0-macos-arm64.dmg",
            ),
            (
                Target::AppImage {
                    file: "/home/a/Murk.AppImage".into(),
                },
                "Murk-0.4.0-x86_64.AppImage",
            ),
            (
                Target::LinuxBinary {
                    exe: "/usr/local/bin/murk".into(),
                },
                "murk-0.4.0-linux-x86_64",
            ),
        ];
        for (t, want) in cases {
            let u = evaluate(&json, &cur, &t).unwrap().unwrap();
            assert_eq!(u.asset.name, want);
            assert_eq!(u.sums.name, "SHA256SUMS");
        }
        // A release missing this platform's asset is not an update for it.
        let partial = release(
            "v0.4.0",
            false,
            false,
            &["Murk-0.4.0-Setup.exe", "SHA256SUMS"],
        );
        let t = Target::LinuxBinary {
            exe: "/usr/local/bin/murk".into(),
        };
        assert_eq!(evaluate(&partial, &cur, &t).unwrap(), None);
    }

    #[test]
    fn detection() {
        let exe = Path::new("/Applications/Murk.app/Contents/MacOS/murk-gui");
        assert_eq!(
            detect_from("macos", "aarch64", exe, None, Frontend::Gui).unwrap(),
            Target::MacBundle {
                bundle: "/Applications/Murk.app".into(),
                exe: exe.into()
            }
        );
        assert!(detect_from("macos", "x86_64", exe, None, Frontend::Gui).is_err());
        assert!(
            detect_from(
                "macos",
                "aarch64",
                Path::new("/usr/local/bin/murk"),
                None,
                Frontend::Tui
            )
            .is_err()
        );

        let bin = Path::new("/usr/local/bin/murk");
        let img = PathBuf::from("/home/a/Apps/Murk-0.3.0-x86_64.AppImage");
        assert_eq!(
            detect_from(
                "linux",
                "x86_64",
                Path::new("/tmp/.mount_x/usr/bin/murk-gui"),
                Some(img.clone()),
                Frontend::Gui
            )
            .unwrap(),
            Target::AppImage { file: img }
        );
        assert_eq!(
            detect_from("linux", "x86_64", bin, None, Frontend::Tui).unwrap(),
            Target::LinuxBinary { exe: bin.into() }
        );
        assert!(detect_from("linux", "x86_64", bin, None, Frontend::Gui).is_err());
        assert!(detect_from("linux", "aarch64", bin, None, Frontend::Tui).is_err());

        let dev = Path::new("C:/src/murk/target/release/murk-gui.exe");
        assert!(detect_from("windows", "x86_64", dev, None, Frontend::Gui).is_err());
        let installed = Path::new("C:/Program Files/Murk/murk-gui.exe");
        assert!(matches!(
            detect_from("windows", "x86_64", installed, None, Frontend::Gui),
            Ok(Target::WindowsInstaller { .. })
        ));
    }

    #[test]
    fn checksum_parsing() {
        let a = "a".repeat(64);
        let b = "B".repeat(64);
        let text = format!(
            "{a}  Murk-0.4.0-Setup.exe\n{b} *murk-0.4.0-linux-x86_64\nnot a line\n{}  short\n\n",
            "c".repeat(10)
        );
        let sums = parse_sums(&text);
        assert_eq!(sums.len(), 2);
        assert_eq!(sum_for(&sums, "Murk-0.4.0-Setup.exe"), Some(a.as_str()));
        // Upper-case hex is normalised, and the binary-mode marker is not part of the name.
        assert_eq!(
            sum_for(&sums, "murk-0.4.0-linux-x86_64"),
            Some("b".repeat(64).as_str())
        );
        assert_eq!(sum_for(&sums, "short"), None);
    }

    #[test]
    fn hex_matches_sha256sum() {
        assert_eq!(
            hex(&Sha256::digest(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn once_a_day() {
        let now = 1_000_000;
        assert!(due(None, now));
        assert!(!due(Some(now - 3600), now));
        assert!(due(Some(now - 24 * 3600), now));
        assert!(due(Some(now + 3600), now));
    }

    #[test]
    fn powershell_quoting() {
        let s = windows_script(
            42,
            Path::new("C:\\Users\\o'brien\\Temp\\Murk-0.4.0-Setup.exe"),
            None,
        );
        assert!(s.contains("Wait-Process -Id 42"));
        assert!(s.contains("'C:\\Users\\o''brien\\Temp\\Murk-0.4.0-Setup.exe'"));
        assert!(s.contains("'/S'"));
        assert!(!s.contains("murk-gui"));
        let g = windows_script(
            1,
            Path::new("C:\\t\\s.exe"),
            Some(Path::new("C:\\Program Files\\Murk\\murk-gui.exe")),
        );
        assert!(g.ends_with("Start-Process -FilePath 'C:\\Program Files\\Murk\\murk-gui.exe'"));
    }

    #[test]
    fn replace_is_a_rename_over_the_old_file() {
        let dir = tempfile::tempdir().unwrap();
        let new = dir.path().join("download");
        let dest = dir.path().join("murk");
        std::fs::write(&new, b"new").unwrap();
        std::fs::write(&dest, b"old").unwrap();
        replace_file(&new, &dest).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"new");
        let left: Vec<_> = std::fs::read_dir(dir.path()).unwrap().collect();
        assert_eq!(left.len(), 2, "no staging file left behind");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&dest).unwrap().permissions().mode() & 0o777,
                0o755
            );
        }
    }
}
