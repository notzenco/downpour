use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

use super::category::Category;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Queued,
    Connecting,
    Downloading,
    Paused,
    Completed,
    Failed,
}

impl Status {
    pub fn is_active(self) -> bool {
        matches!(self, Status::Connecting | Status::Downloading)
    }
}

/// A byte range `[start, end)` of the target file and how much of it is on disk.
/// `end == u64::MAX` means the total size is unknown and the segment runs to EOF.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Segment {
    pub start: u64,
    pub end: u64,
    pub downloaded: u64,
}

impl Segment {
    fn len(&self) -> u64 {
        self.end.saturating_sub(self.start)
    }

    /// Bytes confirmed on disk. Overlap after a split can make `downloaded`
    /// exceed the segment length, so it is clamped.
    pub fn done(&self) -> u64 {
        self.downloaded.min(self.len())
    }

    pub fn is_complete(&self) -> bool {
        self.end != u64::MAX && self.start + self.downloaded >= self.end
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Download {
    pub id: String,
    pub url: String,
    #[serde(default)]
    pub final_url: Option<String>,
    pub file_name: String,
    /// True until the user or a server response has settled the file name.
    #[serde(default)]
    pub auto_name: bool,
    pub save_dir: PathBuf,
    /// True when `save_dir` was derived from the category, so a rename that
    /// changes the category should also move it.
    #[serde(default)]
    pub auto_dir: bool,
    pub total_size: Option<u64>,
    pub downloaded: u64,
    pub resumable: bool,
    pub status: Status,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub segments: Vec<Segment>,
    pub connections: u32,
    pub category: Category,
    #[serde(default)]
    pub referer: Option<String>,
    /// Extra request headers, such as the browser's cookies for a signed-in download.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub content_type: Option<String>,
    #[serde(default)]
    pub etag: Option<String>,
    #[serde(default)]
    pub last_modified: Option<String>,
    pub created_at: u64,
    #[serde(default)]
    pub completed_at: Option<u64>,
    pub queue_position: u64,
}

impl Download {
    pub fn file_path(&self) -> PathBuf {
        self.save_dir.join(&self.file_name)
    }

    pub fn part_path(&self) -> PathBuf {
        self.save_dir.join(format!("{}.part", self.file_name))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub download_dir: PathBuf,
    /// Sort new downloads into per-category subfolders (Video, Compressed, ...).
    pub category_folders: bool,
    pub max_concurrent: u32,
    pub connections: u32,
    /// Global limit in bytes per second; 0 means unlimited.
    pub speed_limit: u64,
    pub max_retries: u32,
    pub user_agent: String,
    pub notify_on_complete: bool,
    pub close_to_tray: bool,
    pub watch_clipboard: bool,
    /// Show the add dialog for links sent by the browser extension instead of starting them.
    pub confirm_browser_downloads: bool,
    pub theme: String,
}

pub const DEFAULT_USER_AGENT: &str = concat!("Downpour/", env!("CARGO_PKG_VERSION"));

impl Default for Settings {
    fn default() -> Self {
        Settings {
            download_dir: PathBuf::new(),
            category_folders: true,
            max_concurrent: 3,
            connections: 8,
            speed_limit: 0,
            max_retries: 5,
            user_agent: DEFAULT_USER_AGENT.to_string(),
            notify_on_complete: true,
            close_to_tray: true,
            watch_clipboard: true,
            confirm_browser_downloads: true,
            theme: "system".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeInfo {
    pub url: String,
    pub final_url: String,
    pub file_name: String,
    pub total_size: Option<u64>,
    pub resumable: bool,
    pub content_type: Option<String>,
    pub category: Category,
    #[serde(skip)]
    pub etag: Option<String>,
    #[serde(skip)]
    pub last_modified: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddRequest {
    pub url: String,
    #[serde(default)]
    pub file_name: Option<String>,
    #[serde(default)]
    pub save_dir: Option<PathBuf>,
    #[serde(default)]
    pub connections: Option<u32>,
    #[serde(default)]
    pub referer: Option<String>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default = "default_true")]
    pub start: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressItem {
    pub id: String,
    pub downloaded: u64,
    pub total_size: Option<u64>,
    pub speed: u64,
    pub eta: Option<u64>,
    pub active_connections: u32,
    /// `[start, end, done]` per segment; only sent when the total size is known.
    pub segments: Vec<[u64; 3]>,
}

// Events are short-lived and serialized immediately, so boxing the large variant buys nothing.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum EngineEvent {
    Upsert {
        download: Download,
    },
    Removed {
        ids: Vec<String>,
    },
    Progress {
        items: Vec<ProgressItem>,
        total_speed: u64,
    },
    Settings {
        settings: Settings,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub downloads: Vec<Download>,
    pub settings: Settings,
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_use_camel_case_fields_for_the_frontend() {
        let json = serde_json::to_value(EngineEvent::Progress {
            items: Vec::new(),
            total_speed: 42,
        })
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "type": "progress", "items": [], "totalSpeed": 42 })
        );
    }
}
