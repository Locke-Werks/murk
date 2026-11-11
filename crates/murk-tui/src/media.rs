//! Inline images and video in the terminal.
//!
//! Pictures draw with whatever the terminal supports, detected at startup:
//! Sixel, the Kitty graphics protocol or iTerm2's, falling back to Unicode
//! half blocks so every terminal shows something. Images come through the
//! same cache as the desktop client's previews. Video gets a poster frame and
//! can play in place (`/play`), both decoded by `ffmpeg` when it is installed.

use image::DynamicImage;
use ratatui::layout::Size;
use ratatui_image::Resize;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::Protocol;
use std::collections::HashMap;
use std::process::Stdio;
use tokio::io::AsyncReadExt;
use tokio::sync::mpsc;

/// The largest a picture may be, in cells. Small enough to keep a chat readable.
pub const MAX_COLS: u16 = 56;
pub const MAX_ROWS: u16 = 14;
/// Frames per second for in-place playback. Sixel redraws a whole picture per
/// frame, so this stays modest.
const PLAY_FPS: u32 = 10;

pub enum Entry {
    Loading,
    /// A picture ready to draw. `video` marks a poster or a playing frame.
    Ready {
        proto: Protocol,
        video: bool,
    },
    /// Video with no poster (no ffmpeg, or it failed): a text placeholder.
    VideoNoPoster,
    /// Nothing to show: not media, or it failed.
    Nothing,
}

pub enum Update {
    Loaded(String, Entry),
    /// One frame of a video playing in place, or the end of playback.
    Frame(String, Option<Protocol>),
}

pub struct Media {
    picker: Picker,
    previewer: Option<murk_media::Previewer>,
    entries: HashMap<String, Entry>,
    tx: mpsc::UnboundedSender<Update>,
    pub rx: mpsc::UnboundedReceiver<Update>,
    ffmpeg: bool,
    playing: Option<(String, tokio::task::JoinHandle<()>)>,
    /// What a playing video drew over its poster, restored when it stops.
    posters: HashMap<String, Protocol>,
    /// Where pictures went on the last frame. Graphics are not cells, so when
    /// these move the screen needs a full repaint to clear the old ones.
    pub drawn: Vec<(String, ratatui::layout::Rect)>,
}

impl Media {
    /// Ask the terminal what it can draw. Call after entering raw mode and before
    /// anything else reads stdin.
    pub fn detect() -> Self {
        let picker = Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks());
        let (tx, rx) = mpsc::unbounded_channel();
        let dir = murk_config::data_dir().join("media");
        Media {
            picker,
            previewer: murk_media::Previewer::new(dir).ok(),
            entries: HashMap::new(),
            tx,
            rx,
            ffmpeg: which("ffmpeg"),
            playing: None,
            posters: HashMap::new(),
            drawn: Vec::new(),
        }
    }

    /// The protocol in use, for the status line: "sixel", "kitty", "halfblocks".
    pub fn protocol_name(&self) -> String {
        format!("{:?}", self.picker.protocol_type()).to_lowercase()
    }

    pub fn get(&self, url: &str) -> Option<&Entry> {
        self.entries.get(url)
    }

    /// The newest video link among `texts` (newest first) that has loaded as video.
    pub fn newest_video<'a>(&self, texts: impl Iterator<Item = &'a str>) -> Option<String> {
        texts.flat_map(links).find(|u| {
            matches!(
                self.entries.get(u),
                Some(Entry::Ready { video: true, .. }) | Some(Entry::VideoNoPoster)
            )
        })
    }

    pub fn is_playing(&self, url: &str) -> bool {
        self.playing.as_ref().is_some_and(|(u, _)| u == url)
    }

    /// Start loading `url` once; later calls are free.
    pub fn request(&mut self, url: &str) {
        if self.entries.contains_key(url) {
            return;
        }
        let Some(previewer) = self.previewer.clone() else {
            self.entries.insert(url.to_string(), Entry::Nothing);
            return;
        };
        self.entries.insert(url.to_string(), Entry::Loading);
        let (url, tx, picker, ffmpeg) = (
            url.to_string(),
            self.tx.clone(),
            self.picker.clone(),
            self.ffmpeg,
        );
        tokio::spawn(async move {
            let entry = match previewer.preview(&url).await {
                Some(p) if p.kind == "image" => match p.image {
                    Some(path) => {
                        let picker = picker.clone();
                        tokio::task::spawn_blocking(move || {
                            // The cache names files `.img`, so the format comes from the bytes.
                            image::ImageReader::open(&path)
                                .ok()
                                .and_then(|r| r.with_guessed_format().ok())
                                .and_then(|r| r.decode().ok())
                                .and_then(|img| fit(&picker, img))
                                .map(|proto| Entry::Ready {
                                    proto,
                                    video: false,
                                })
                                .unwrap_or(Entry::Nothing)
                        })
                        .await
                        .unwrap_or(Entry::Nothing)
                    }
                    None => Entry::Nothing,
                },
                Some(p) if p.kind == "movie" => {
                    if ffmpeg {
                        match poster(&url).await {
                            Some(img) => {
                                let picker = picker.clone();
                                tokio::task::spawn_blocking(move || fit(&picker, img))
                                    .await
                                    .ok()
                                    .flatten()
                                    .map(|proto| Entry::Ready { proto, video: true })
                                    .unwrap_or(Entry::VideoNoPoster)
                            }
                            None => Entry::VideoNoPoster,
                        }
                    } else {
                        Entry::VideoNoPoster
                    }
                }
                _ => Entry::Nothing,
            };
            let _ = tx.send(Update::Loaded(url, entry));
        });
    }

    /// Apply a finished load or a playback frame. Returns true when a redraw is due.
    pub fn apply(&mut self, update: Update) -> bool {
        match update {
            Update::Loaded(url, entry) => {
                self.entries.insert(url, entry);
            }
            Update::Frame(url, Some(proto)) => {
                if self.is_playing(&url) {
                    let old = self
                        .entries
                        .insert(url.clone(), Entry::Ready { proto, video: true });
                    // Keep the poster the first time a frame replaces it.
                    if let Some(Entry::Ready { proto: p, .. }) = old {
                        self.posters.entry(url).or_insert(p);
                    }
                }
            }
            Update::Frame(url, None) => {
                if self.is_playing(&url) {
                    self.playing = None;
                }
                if let Some(p) = self.posters.remove(&url) {
                    self.entries.insert(
                        url,
                        Entry::Ready {
                            proto: p,
                            video: true,
                        },
                    );
                }
            }
        }
        true
    }

    /// Play `url` in place, or stop it if it is already playing. Returns a
    /// sentence for the status line.
    pub fn toggle_play(&mut self, url: &str) -> String {
        if let Some((playing, task)) = self.playing.take() {
            task.abort();
            let _ = self.tx.send(Update::Frame(playing.clone(), None));
            if playing == url {
                return "Stopped".into();
            }
        }
        if !self.ffmpeg {
            return "Playing video here needs ffmpeg on PATH".into();
        }
        // Play at the size the poster took, so the layout does not jump.
        let size = match self.entries.get(url) {
            Some(Entry::Ready { proto, .. }) => proto.size(),
            _ => Size::new(MAX_COLS, MAX_ROWS),
        };
        let font = self.picker.font_size();
        let (pw, ph) = (
            u32::from(size.width) * u32::from(font.width),
            u32::from(size.height) * u32::from(font.height),
        );
        let (url_s, tx, picker) = (url.to_string(), self.tx.clone(), self.picker.clone());
        let task = tokio::spawn(async move {
            play(url_s.clone(), pw, ph, size, picker, tx.clone()).await;
            let _ = tx.send(Update::Frame(url_s, None));
        });
        self.playing = Some((url.to_string(), task));
        "Playing. /play again stops it".into()
    }
}

/// Scale a picture to fit the chat's maximum size.
fn fit(picker: &Picker, img: DynamicImage) -> Option<Protocol> {
    picker
        .new_protocol(img, Size::new(MAX_COLS, MAX_ROWS), Resize::Fit(None))
        .ok()
}

fn which(cmd: &str) -> bool {
    std::process::Command::new(cmd)
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// One frame from early in the video, as a picture.
async fn poster(url: &str) -> Option<DynamicImage> {
    let out = tokio::time::timeout(
        std::time::Duration::from_secs(20),
        tokio::process::Command::new("ffmpeg")
            .args([
                "-v",
                "error",
                "-ss",
                "0.5",
                "-i",
                url,
                "-frames:v",
                "1",
                "-f",
                "image2pipe",
                "-vcodec",
                "png",
                "-",
            ])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .ok()?
    .ok()?;
    image::load_from_memory(&out.stdout).ok()
}

/// Decode `url` at `pw`x`ph` pixels and send a frame at a time, paced by ffmpeg.
async fn play(
    url: String,
    pw: u32,
    ph: u32,
    size: Size,
    picker: Picker,
    tx: mpsc::UnboundedSender<Update>,
) {
    let filter = format!(
        "fps={PLAY_FPS},scale={pw}:{ph}:force_original_aspect_ratio=decrease,pad={pw}:{ph}:(ow-iw)/2:(oh-ih)/2"
    );
    let Ok(mut child) = tokio::process::Command::new("ffmpeg")
        .args([
            "-v", "error", "-re", "-i", &url, "-an", "-vf", &filter, "-f", "rawvideo", "-pix_fmt",
            "rgb24", "-",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
    else {
        return;
    };
    let Some(mut out) = child.stdout.take() else {
        return;
    };
    let frame_len = (pw * ph * 3) as usize;
    let mut buf = vec![0u8; frame_len];
    loop {
        if out.read_exact(&mut buf).await.is_err() {
            break;
        }
        let Some(rgb) = image::RgbImage::from_raw(pw, ph, buf.clone()) else {
            break;
        };
        let picker = picker.clone();
        let proto = tokio::task::spawn_blocking(move || {
            picker
                .new_protocol(DynamicImage::ImageRgb8(rgb), size, Resize::Fit(None))
                .ok()
        })
        .await
        .ok()
        .flatten();
        let Some(proto) = proto else { break };
        if tx.send(Update::Frame(url.clone(), Some(proto))).is_err() {
            break;
        }
    }
    let _ = child.kill().await;
}

/// The first two links in a message, as the desktop client finds them.
pub fn links(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for word in text.split(|c: char| c.is_whitespace() || c == '<' || c == '>' || c == '"') {
        let Some(start) = word.find("http://").or_else(|| word.find("https://")) else {
            continue;
        };
        let url = word[start..].trim_end_matches(['.', ',', ')', ';', ':', '!', '?', '\'']);
        if url.len() > "https://".len() && !out.iter().any(|u| u == url) {
            out.push(url.to_string());
            if out.len() == 2 {
                break;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::links;

    #[test]
    fn finds_links_like_the_gui() {
        assert_eq!(
            links("look (https://a.example/x.png). and http://b.example/y.mp4, ok"),
            vec!["https://a.example/x.png", "http://b.example/y.mp4"]
        );
        assert!(links("no links here").is_empty());
        assert_eq!(
            links("https://a.io https://a.io https://b.io https://c.io").len(),
            2
        );
    }
}
