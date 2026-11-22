use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use super::model::{Download, Settings};

const STATE_FILE: &str = "downloads.json";
const SETTINGS_FILE: &str = "settings.json";

#[derive(Serialize, Deserialize, Default)]
struct Persisted {
    version: u32,
    downloads: Vec<Download>,
}

pub struct Store {
    dir: PathBuf,
    /// Saves run on a blocking thread; a slow older save must not overwrite a newer one.
    written: Mutex<u64>,
}

impl Store {
    pub fn new(dir: PathBuf) -> Self {
        Store {
            dir,
            written: Mutex::new(0),
        }
    }

    pub fn load(&self) -> (Vec<Download>, Settings) {
        let downloads = read_json::<Persisted>(&self.dir.join(STATE_FILE))
            .map(|p| p.downloads)
            .unwrap_or_default();
        let settings = read_json::<Settings>(&self.dir.join(SETTINGS_FILE)).unwrap_or_default();
        (downloads, settings)
    }

    pub fn save_downloads(&self, generation: u64, downloads: &[Download]) -> io::Result<()> {
        let mut written = self.written.lock().unwrap();
        if generation <= *written {
            return Ok(());
        }
        let data = Persisted {
            version: 1,
            downloads: downloads.to_vec(),
        };
        write_atomic(&self.dir.join(STATE_FILE), &serde_json::to_vec(&data)?)?;
        *written = generation;
        Ok(())
    }

    pub fn save_settings(&self, settings: &Settings) -> io::Result<()> {
        write_atomic(
            &self.dir.join(SETTINGS_FILE),
            &serde_json::to_vec_pretty(settings)?,
        )
    }
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    let bytes = std::fs::read(path).ok()?;
    match serde_json::from_slice(&bytes) {
        Ok(v) => Some(v),
        Err(e) => {
            // Keep the unreadable file for recovery instead of silently overwriting it.
            let backup = path.with_extension("json.corrupt");
            let _ = std::fs::rename(path, &backup);
            eprintln!(
                "downpour: could not parse {}: {e}; moved to {}",
                path.display(),
                backup.display()
            );
            None
        }
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}
