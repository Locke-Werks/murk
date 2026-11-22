//! `/update` and the once-a-day background check, for the terminal client.
//! Network work runs on tokio tasks and reports back over a channel, so the
//! UI loop never waits on GitHub.

use murk_client::App;
use murk_client::app::UpdateRequest;
use murk_update::{After, Frontend, Target, Update};
use std::path::PathBuf;
use tokio::sync::mpsc;

pub enum Msg {
    Checked {
        manual: bool,
        install: bool,
        result: Result<Option<Update>, String>,
    },
    Progress(u64, u64),
    Installed(Result<After, String>),
}

pub struct Updater {
    tx: mpsc::UnboundedSender<Msg>,
    pub rx: mpsc::UnboundedReceiver<Msg>,
    target: Result<Target, String>,
    available: Option<Update>,
    busy: bool,
    /// Shown on the status line until something replaces it.
    pub notice: Option<String>,
    /// Set once an install finished: the caller exits and acts on it.
    pub after: Option<After>,
}

impl Updater {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Updater {
            tx,
            rx,
            target: Target::detect(Frontend::Tui),
            available: None,
            busy: false,
            notice: None,
            after: None,
        }
    }

    /// The automatic check: only if enabled, due, and this install can update itself.
    pub fn startup(&mut self, app: &mut App) {
        let now = murk_update::now_secs();
        if !app.config.update.check || !murk_update::due(app.config.update.last_check, now) {
            return;
        }
        if self.target.is_err() {
            return;
        }
        app.config.update.last_check = Some(now);
        if let Some(p) = &app.config_path {
            let _ = app.config.save(p);
        }
        self.check(false, false);
    }

    pub fn request(&mut self, app: &mut App, req: UpdateRequest) {
        if let Err(e) = &self.target {
            app.set_status(e.clone());
            return;
        }
        if self.busy {
            app.set_status("An update is already in progress");
            return;
        }
        match (req, self.available.clone()) {
            (UpdateRequest::Install, Some(u)) => self.install(u),
            (UpdateRequest::Install, None) => self.check(true, true),
            (UpdateRequest::Check, _) => self.check(true, false),
        }
        self.notice = Some("Checking for updates...".into());
    }

    fn check(&mut self, manual: bool, install: bool) {
        let Ok(target) = self.target.clone() else {
            return;
        };
        self.busy = true;
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = murk_update::check(&target).await;
            let _ = tx.send(Msg::Checked {
                manual,
                install,
                result,
            });
        });
    }

    fn install(&mut self, update: Update) {
        let Ok(target) = self.target.clone() else {
            return;
        };
        self.busy = true;
        self.notice = Some(format!("Downloading Murk {}...", update.version));
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let dir = murk_update::download_dir();
            let ptx = tx.clone();
            let mut last_pct = u64::MAX;
            let file: Result<PathBuf, String> =
                murk_update::download(&update, &dir, move |got, total| {
                    // One message per percent is plenty for a status line.
                    let pct = got * 100 / total.max(1);
                    if pct != last_pct {
                        last_pct = pct;
                        let _ = ptx.send(Msg::Progress(got, total));
                    }
                })
                .await;
            let result = match file {
                Ok(file) => tokio::task::spawn_blocking(move || {
                    murk_update::install(&target, &file, Frontend::Tui)
                })
                .await
                .unwrap_or_else(|e| Err(e.to_string())),
                Err(e) => Err(e),
            };
            let _ = tx.send(Msg::Installed(result));
        });
    }

    pub fn handle(&mut self, app: &mut App, msg: Msg) {
        match msg {
            Msg::Checked {
                manual,
                install,
                result,
            } => {
                self.busy = false;
                match result {
                    Ok(Some(u)) => {
                        let skipped =
                            app.config.update.skip.as_deref() == Some(&u.version.to_string());
                        if skipped && !manual {
                            self.notice = None;
                            return;
                        }
                        self.notice =
                            Some(format!("Murk {} is available: /update install", u.version));
                        self.available = Some(u.clone());
                        if install {
                            self.install(u);
                        }
                    }
                    Ok(None) => {
                        self.notice = None;
                        if manual {
                            app.set_status(format!(
                                "Murk {} is up to date",
                                murk_update::current_version()
                            ));
                        }
                    }
                    Err(e) => {
                        self.notice = None;
                        if manual {
                            app.set_status(format!("Update check failed: {e}"));
                        }
                    }
                }
            }
            Msg::Progress(got, total) => {
                if let Some(u) = &self.available {
                    self.notice = Some(format!(
                        "Downloading Murk {}: {}%",
                        u.version,
                        got * 100 / total.max(1)
                    ));
                }
            }
            Msg::Installed(Ok(after)) => {
                self.notice = Some("Update installed, restarting...".into());
                self.after = Some(after);
                app.submit_quit();
            }
            Msg::Installed(Err(e)) => {
                self.busy = false;
                self.notice = None;
                app.set_status(format!("Update failed: {e}"));
            }
        }
    }

    /// After the terminal is restored: say what happened and, where it is
    /// meaningful, start the new version in this same terminal.
    pub fn finish(self) {
        match self.after {
            None => {}
            Some(After::Exit) => {
                println!(
                    "murk: the installer is running and will finish once this window's murk has exited. Start murk again afterwards."
                );
            }
            Some(After::Relaunch(exe)) => relaunch(&exe),
        }
    }
}

#[cfg(unix)]
fn relaunch(exe: &std::path::Path) {
    use std::os::unix::process::CommandExt;
    // exec only returns on failure; on success the new version owns the terminal.
    let err = std::process::Command::new(exe)
        .args(std::env::args_os().skip(1))
        .exec();
    eprintln!("murk: updated, but could not restart ({err}). Start murk again.");
}

#[cfg(not(unix))]
fn relaunch(_exe: &std::path::Path) {
    println!("murk: updated. Start murk again to use the new version.");
}
