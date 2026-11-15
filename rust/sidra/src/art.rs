//! Artwork bytes for egui's image loaders, kept on disk so a page opened before
//! draws its covers without touching the network.

use egui::load::{Bytes, BytesLoadResult, BytesLoader, BytesPoll, LoadError};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

const WORKERS: usize = 6;
/// Covers unused for this long are deleted at start-up.
const KEEP: Duration = Duration::from_secs(30 * 24 * 3600);

type Slot = Option<Result<Arc<[u8]>, String>>;

pub struct ArtLoader {
    slots: Arc<Mutex<HashMap<String, Slot>>>,
    jobs: Mutex<Sender<(String, egui::Context)>>,
}

fn file_name(uri: &str) -> String {
    // FNV-1a: stable across runs and builds, unlike std's hasher.
    let h = uri.bytes().fold(0xcbf29ce484222325u64, |h, b| {
        (h ^ b as u64).wrapping_mul(0x100000001b3)
    });
    format!("{h:016x}.img")
}

fn fetch(dir: &Path, uri: &str) -> Result<Arc<[u8]>, String> {
    let path = dir.join(file_name(uri));
    if let Ok(bytes) = std::fs::read(&path) {
        return Ok(bytes.into());
    }
    let res = ehttp::fetch_blocking(&ehttp::Request::get(uri))?;
    if !res.ok {
        return Err(format!("HTTP {}", res.status));
    }
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, &res.bytes).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
    Ok(res.bytes.into())
}

impl ArtLoader {
    pub fn new(dir: PathBuf) -> Self {
        let _ = std::fs::create_dir_all(&dir);
        sweep(&dir);
        let slots: Arc<Mutex<HashMap<String, Slot>>> = Arc::default();
        let (tx, rx) = channel::<(String, egui::Context)>();
        let rx = Arc::new(Mutex::new(rx));
        for _ in 0..WORKERS {
            let (rx, slots, dir) = (rx.clone(), slots.clone(), dir.clone());
            std::thread::spawn(move || {
                loop {
                    let job = match rx.lock() {
                        Ok(r) => r.recv(),
                        Err(_) => return,
                    };
                    let Ok((uri, ctx)) = job else { return };
                    let result = fetch(&dir, &uri);
                    if let Ok(mut s) = slots.lock() {
                        s.insert(uri, Some(result));
                    }
                    ctx.request_repaint();
                }
            });
        }
        Self {
            slots,
            jobs: Mutex::new(tx),
        }
    }
}

fn sweep(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let now = SystemTime::now();
    for e in entries.flatten() {
        let old = e
            .metadata()
            .and_then(|m| m.modified())
            .is_ok_and(|t| now.duration_since(t).unwrap_or_default() > KEEP);
        if old {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

impl BytesLoader for ArtLoader {
    fn id(&self) -> &str {
        "sidra::ArtLoader"
    }

    fn load(&self, ctx: &egui::Context, uri: &str) -> BytesLoadResult {
        if !uri.starts_with("https://") {
            return Err(LoadError::NotSupported);
        }
        let mut slots = self
            .slots
            .lock()
            .map_err(|e| LoadError::Loading(e.to_string()))?;
        match slots.get(uri) {
            Some(Some(Ok(bytes))) => Ok(BytesPoll::Ready {
                size: None,
                bytes: Bytes::Shared(bytes.clone()),
                // Apple serves JPEG covers; the image loader sniffs the format anyway.
                mime: None,
            }),
            Some(Some(Err(e))) => Err(LoadError::Loading(e.clone())),
            Some(None) => Ok(BytesPoll::Pending { size: None }),
            None => {
                slots.insert(uri.to_owned(), None);
                if let Ok(jobs) = self.jobs.lock() {
                    let _ = jobs.send((uri.to_owned(), ctx.clone()));
                }
                Ok(BytesPoll::Pending { size: None })
            }
        }
    }

    fn forget(&self, uri: &str) {
        if let Ok(mut s) = self.slots.lock() {
            s.remove(uri);
        }
    }

    fn forget_all(&self) {
        if let Ok(mut s) = self.slots.lock() {
            s.clear();
        }
    }

    fn byte_size(&self) -> usize {
        self.slots.lock().map_or(0, |s| {
            s.values()
                .map(|v| match v {
                    Some(Ok(b)) => b.len(),
                    _ => 0,
                })
                .sum()
        })
    }
}
