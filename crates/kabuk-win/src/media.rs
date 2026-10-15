//! The current media session (what is playing) through
//! `GlobalSystemMediaTransportControlsSessionManager`. WinRT work happens on a
//! dedicated thread that sleeps on a channel; session events only queue commands, and
//! every change is posted to a window. The media card (DECISIONS D66) reads the item's
//! artist, album and cover too, and steps to the previous or next item.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use windows::Foundation::TypedEventHandler;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session, GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
};

use crate::audio::Notify;
use crate::msgloop::{post_message, Msg};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MediaInfo {
    /// Title of the current session's item; `None` when nothing is open.
    pub title: Option<String>,
    pub playing: bool,
    pub artist: String,
    pub album: String,
    /// The program playing it (its app id).
    pub source: String,
    /// The session takes "previous" and "next" now.
    pub can_previous: bool,
    pub can_next: bool,
    pub cover: Option<Cover>,
}

/// The item's cover as [`COVER_SIDE`]-square premultiplied BGRA pixels; two covers are
/// the same when their `id` (the item's title, artist and album) is.
#[derive(Clone, Debug)]
pub struct Cover {
    pub id: u64,
    pub pixels: Arc<Vec<u8>>,
}

impl PartialEq for Cover {
    fn eq(&self, other: &Cover) -> bool {
        self.id == other.id
    }
}

impl Eq for Cover {}

/// The side the cover is decoded at (pixels): the card shows it at most this large.
pub const COVER_SIDE: u32 = 192;

enum Command {
    Refresh,
    SessionChanged,
    Toggle,
    Previous,
    Next,
    Quit,
}

pub struct Media {
    info: Arc<Mutex<MediaInfo>>,
    tx: Sender<Command>,
    thread: Option<JoinHandle<()>>,
}

impl Media {
    /// Starts watching. `notify` receives a message (`wparam` 0) after every change.
    pub fn start(notify: Notify) -> Media {
        let info = Arc::new(Mutex::new(MediaInfo::default()));
        let (tx, rx) = channel();
        let shared = info.clone();
        let tx2 = tx.clone();
        let thread = std::thread::Builder::new()
            .name("kabuk-media".into())
            .spawn(move || run(rx, tx2, shared, notify))
            .ok();
        Media { info, tx, thread }
    }

    pub fn info(&self) -> MediaInfo {
        self.info.lock().map(|i| i.clone()).unwrap_or_default()
    }

    /// Play/pause the current session.
    pub fn toggle(&self) {
        let _ = self.tx.send(Command::Toggle);
    }

    /// The current session's previous item.
    pub fn previous(&self) {
        let _ = self.tx.send(Command::Previous);
    }

    /// The current session's next item.
    pub fn next(&self) {
        let _ = self.tx.send(Command::Next);
    }
}

impl Drop for Media {
    fn drop(&mut self) {
        let _ = self.tx.send(Command::Quit);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// The id of an item's cover: its title, artist and album.
fn cover_id(title: &str, artist: &str, album: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (title, artist, album).hash(&mut h);
    h.finish()
}

/// The item's cover, read and decoded (only when it is another item than `known`'s).
fn read_cover(
    props: &windows::Media::Control::GlobalSystemMediaTransportControlsSessionMediaProperties,
    id: u64,
    known: Option<&Cover>,
) -> Option<Cover> {
    if let Some(k) = known.filter(|k| k.id == id) {
        return Some(k.clone());
    }
    let stream = props.Thumbnail().ok()?.OpenReadAsync().ok()?.join().ok()?;
    let size = u32::try_from(stream.Size().ok()?).ok()?;
    let reader = windows::Storage::Streams::DataReader::CreateDataReader(&stream.GetInputStreamAt(0).ok()?).ok()?;
    reader.LoadAsync(size).ok()?.join().ok()?;
    let mut bytes = vec![0u8; size as usize];
    reader.ReadBytes(&mut bytes).ok()?;
    let pixels = crate::icon::png_pixels(&bytes, COVER_SIDE)?;
    Some(Cover {
        id,
        pixels: Arc::new(pixels),
    })
}

fn read(session: &Session, known: Option<&Cover>) -> windows::core::Result<MediaInfo> {
    let props = session.TryGetMediaPropertiesAsync()?.join()?;
    let title = props.Title()?.to_string();
    let artist = props.Artist().map(|a| a.to_string()).unwrap_or_default();
    let album = props.AlbumTitle().map(|a| a.to_string()).unwrap_or_default();
    let playback = session.GetPlaybackInfo()?;
    let playing = playback.PlaybackStatus()? == Status::Playing;
    let controls = playback.Controls()?;
    let cover = read_cover(&props, cover_id(&title, &artist, &album), known);
    Ok(MediaInfo {
        title: (!title.trim().is_empty()).then_some(title),
        playing,
        artist,
        album,
        source: session
            .SourceAppUserModelId()
            .map(|s| s.to_string())
            .unwrap_or_default(),
        can_previous: controls.IsPreviousEnabled().unwrap_or(false),
        can_next: controls.IsNextEnabled().unwrap_or(false),
        cover,
    })
}

fn run(rx: Receiver<Command>, tx: Sender<Command>, info: Arc<Mutex<MediaInfo>>, notify: Notify) {
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
    // SAFETY: initialises COM for this dedicated thread only.
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    let manager = match Manager::RequestAsync().and_then(|op| op.join()) {
        Ok(m) => m,
        Err(e) => {
            tracing::info!(error = %e, "media sessions unavailable");
            return;
        }
    };
    let changed = tx.clone();
    let _ = manager.CurrentSessionChanged(&TypedEventHandler::new(move |_, _| {
        let _ = changed.send(Command::SessionChanged);
        Ok(())
    }));
    let mut current: Option<(Session, i64, i64)> = None;
    let attach = |current: &mut Option<(Session, i64, i64)>| {
        if let Some((s, a, b)) = current.take() {
            let _ = s.RemoveMediaPropertiesChanged(a);
            let _ = s.RemovePlaybackInfoChanged(b);
        }
        if let Ok(s) = manager.GetCurrentSession() {
            let t1 = tx.clone();
            let t2 = tx.clone();
            let a = s.MediaPropertiesChanged(&TypedEventHandler::new(move |_, _| {
                let _ = t1.send(Command::Refresh);
                Ok(())
            }));
            let b = s.PlaybackInfoChanged(&TypedEventHandler::new(move |_, _| {
                let _ = t2.send(Command::Refresh);
                Ok(())
            }));
            if let (Ok(a), Ok(b)) = (a, b) {
                *current = Some((s, a, b));
            }
        }
    };
    attach(&mut current);
    let _ = tx.send(Command::Refresh);
    while let Ok(cmd) = rx.recv() {
        match cmd {
            Command::Quit => break,
            Command::SessionChanged => {
                attach(&mut current);
                let _ = tx.send(Command::Refresh);
            }
            Command::Toggle => {
                if let Some((s, _, _)) = &current {
                    let _ = s.TryTogglePlayPauseAsync().and_then(|op| op.join());
                }
            }
            Command::Previous => {
                if let Some((s, _, _)) = &current {
                    let _ = s.TrySkipPreviousAsync().and_then(|op| op.join());
                }
            }
            Command::Next => {
                if let Some((s, _, _)) = &current {
                    let _ = s.TrySkipNextAsync().and_then(|op| op.join());
                }
            }
            Command::Refresh => {
                let known = info.lock().ok().and_then(|i| i.cover.clone());
                let next = current
                    .as_ref()
                    .and_then(|(s, _, _)| read(s, known.as_ref()).ok())
                    .unwrap_or_default();
                let changed = info
                    .lock()
                    .map(|mut i| std::mem::replace(&mut *i, next.clone()) != next)
                    .unwrap_or(false);
                if changed {
                    let _ = post_message(
                        notify.window,
                        Msg {
                            id: notify.message,
                            wparam: 0,
                            lparam: 0,
                        },
                    );
                }
            }
        }
    }
    if let Some((s, a, b)) = current.take() {
        let _ = s.RemoveMediaPropertiesChanged(a);
        let _ = s.RemovePlaybackInfoChanged(b);
    }
}

#[cfg(test)]
mod tests {
    //! Starting and stopping never blocks, whether or not something is playing.
    use super::*;
    use crate::window::Hwnd;

    #[test]
    fn starts_and_stops_cleanly() {
        let m = Media::start(Notify {
            window: Hwnd(0),
            message: 0,
        });
        let _ = m.info();
        drop(m);
    }
}
