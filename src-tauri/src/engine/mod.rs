pub mod category;
pub mod error;
pub mod limiter;
pub mod model;
pub mod naming;
pub mod probe;
pub mod segments;
mod store;
mod worker;

use reqwest::Client;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::runtime::Handle;
use tokio::sync::watch;

use category::Category;
use error::EngineError;
use limiter::RateLimiter;
use model::{
    AddRequest, Download, EngineEvent, ProbeInfo, ProgressItem, Segment, Settings, Snapshot,
    Status, now_ms,
};
use probe::RequestOptions;
use segments::LiveSegment;
use store::Store;

pub trait EventSink: Send + Sync + 'static {
    fn emit(&self, event: EngineEvent);
    fn completed(&self, _download: &Download) {}
    fn activity(&self, _active: usize, _total_speed: u64) {}
}

type LiveSegments = Arc<Mutex<Vec<Arc<LiveSegment>>>>;

const TICK: Duration = Duration::from_millis(500);
const SAVE_EVERY_TICKS: u32 = 4;
const MAX_CONNECTIONS: u32 = 32;

#[derive(Clone)]
pub struct Manager {
    inner: Arc<Inner>,
}

struct Inner {
    state: Mutex<State>,
    client: Client,
    sink: Arc<dyn EventSink>,
    limiter: Arc<RateLimiter>,
    store: Store,
    generation: AtomicU64,
    runtime: Handle,
}

struct State {
    downloads: Vec<Download>,
    settings: Settings,
    jobs: HashMap<String, Job>,
    dirty: bool,
    shutting_down: bool,
}

struct Job {
    cancel: watch::Sender<bool>,
    segments: LiveSegments,
    stopping: bool,
    /// Files to delete once the job has stopped writing (set by remove).
    delete_after: Vec<PathBuf>,
    speed: SpeedMeter,
}

impl Job {
    fn live_segments(&self) -> Vec<Segment> {
        let mut segs: Vec<Segment> = self
            .segments
            .lock()
            .unwrap()
            .iter()
            .map(|s| s.snapshot())
            .collect();
        segs.sort_by_key(|s| s.start);
        segs
    }

    fn stop(&mut self) {
        self.stopping = true;
        let _ = self.cancel.send(true);
    }
}

struct SpeedMeter {
    samples: VecDeque<(Instant, u64)>,
}

impl SpeedMeter {
    const WINDOW: Duration = Duration::from_secs(3);

    fn new() -> Self {
        SpeedMeter {
            samples: VecDeque::new(),
        }
    }

    fn sample(&mut self, downloaded: u64) -> u64 {
        let now = Instant::now();
        self.samples.push_back((now, downloaded));
        while self.samples.len() > 2 && now.duration_since(self.samples[0].0) > Self::WINDOW {
            self.samples.pop_front();
        }
        let (t0, d0) = self.samples[0];
        let dt = now.duration_since(t0).as_secs_f64();
        if dt < 0.2 {
            return 0;
        }
        (downloaded.saturating_sub(d0) as f64 / dt) as u64
    }
}

fn validate_url(url: &str) -> Result<url::Url, EngineError> {
    let parsed =
        url::Url::parse(url.trim()).map_err(|_| EngineError::InvalidUrl(url.to_string()))?;
    match parsed.scheme() {
        "http" | "https" => Ok(parsed),
        _ => Err(EngineError::InvalidUrl(url.to_string())),
    }
}

impl Manager {
    /// Loads saved state. Must be called from within a Tokio runtime.
    pub fn new(
        data_dir: PathBuf,
        default_download_dir: PathBuf,
        sink: Arc<dyn EventSink>,
    ) -> Manager {
        let store = Store::new(data_dir);
        let (mut downloads, mut settings) = store.load();
        if settings.download_dir.as_os_str().is_empty() {
            settings.download_dir = default_download_dir;
        }
        for d in &mut downloads {
            // Downloads interrupted by a crash or quit pick up where they left off.
            if d.status.is_active() {
                d.status = Status::Queued;
            }
        }
        // HTTP/2 would multiplex every segment over one TCP connection, which
        // defeats the point of downloading with several connections.
        let client = Client::builder()
            .http1_only()
            .connect_timeout(Duration::from_secs(20))
            .read_timeout(Duration::from_secs(30))
            .build()
            .expect("HTTP client configuration is static and valid");
        Manager {
            inner: Arc::new(Inner {
                limiter: Arc::new(RateLimiter::new(settings.speed_limit)),
                state: Mutex::new(State {
                    downloads,
                    settings,
                    jobs: HashMap::new(),
                    dirty: false,
                    shutting_down: false,
                }),
                client,
                sink,
                store,
                generation: AtomicU64::new(0),
                runtime: Handle::current(),
            }),
        }
    }

    /// Starts the progress ticker and any queued downloads.
    pub fn start(&self) {
        let inner = self.inner.clone();
        self.inner.runtime.spawn(async move { ticker(inner).await });
        schedule(&self.inner);
    }

    pub fn snapshot(&self) -> Snapshot {
        let st = self.inner.state.lock().unwrap();
        Snapshot {
            downloads: st.downloads.clone(),
            settings: st.settings.clone(),
        }
    }

    pub fn settings(&self) -> Settings {
        self.inner.state.lock().unwrap().settings.clone()
    }

    pub async fn probe(&self, url: &str, referer: Option<&str>) -> Result<ProbeInfo, EngineError> {
        let url = validate_url(url)?;
        let ua = self.settings().user_agent;
        probe::probe(
            &self.inner.client,
            url.as_str(),
            &RequestOptions {
                user_agent: &ua,
                referer,
            },
        )
        .await
    }

    pub fn add(&self, req: AddRequest) -> Result<Download, EngineError> {
        let url = validate_url(&req.url)?.to_string();
        let download = {
            let mut st = self.inner.state.lock().unwrap();
            let explicit = req
                .file_name
                .as_deref()
                .map(str::trim)
                .filter(|n| !n.is_empty());
            let auto_name = explicit.is_none();
            let name = naming::sanitize(
                &explicit
                    .map(str::to_string)
                    .or_else(|| naming::from_url(&url))
                    .unwrap_or_else(|| "download".into()),
            );
            let category = Category::from_file_name(&name);
            let auto_dir = req.save_dir.is_none() && st.settings.category_folders;
            let save_dir = req
                .save_dir
                .clone()
                .unwrap_or_else(|| default_dir(&st.settings, category));
            let file_name = naming::unique_name(
                &save_dir,
                &name,
                &claimed_names(&st.downloads, &save_dir, None),
            );
            let queue_position = st
                .downloads
                .iter()
                .map(|d| d.queue_position)
                .max()
                .map_or(0, |p| p + 1);
            let d = Download {
                id: uuid::Uuid::new_v4().to_string(),
                url,
                final_url: None,
                file_name,
                auto_name,
                save_dir,
                auto_dir,
                total_size: None,
                downloaded: 0,
                resumable: false,
                status: if req.start {
                    Status::Queued
                } else {
                    Status::Paused
                },
                error: None,
                segments: Vec::new(),
                connections: req
                    .connections
                    .unwrap_or(st.settings.connections)
                    .clamp(1, MAX_CONNECTIONS),
                category,
                referer: req.referer.clone(),
                content_type: None,
                etag: None,
                last_modified: None,
                created_at: now_ms(),
                completed_at: None,
                queue_position,
            };
            st.downloads.push(d.clone());
            st.dirty = true;
            d
        };
        self.inner.sink.emit(EngineEvent::Upsert {
            download: download.clone(),
        });
        schedule(&self.inner);
        Ok(download)
    }

    pub fn pause(&self, ids: &[String]) {
        let updated = {
            let mut st = self.inner.state.lock().unwrap();
            let State {
                downloads, jobs, ..
            } = &mut *st;
            let mut updated = Vec::new();
            for d in downloads.iter_mut().filter(|d| ids.contains(&d.id)) {
                if matches!(
                    d.status,
                    Status::Queued | Status::Connecting | Status::Downloading
                ) {
                    d.status = Status::Paused;
                    if let Some(job) = jobs.get_mut(&d.id) {
                        job.stop();
                    }
                    updated.push(d.clone());
                }
            }
            st.dirty |= !updated.is_empty();
            updated
        };
        for d in updated {
            self.inner.sink.emit(EngineEvent::Upsert { download: d });
        }
    }

    pub fn pause_all(&self) {
        let ids: Vec<String> = self
            .inner
            .state
            .lock()
            .unwrap()
            .downloads
            .iter()
            .map(|d| d.id.clone())
            .collect();
        self.pause(&ids);
    }

    pub fn resume(&self, ids: &[String]) {
        let updated = {
            let mut st = self.inner.state.lock().unwrap();
            let mut updated = Vec::new();
            for d in st.downloads.iter_mut().filter(|d| ids.contains(&d.id)) {
                if matches!(d.status, Status::Paused | Status::Failed) {
                    d.status = Status::Queued;
                    d.error = None;
                    updated.push(d.clone());
                }
            }
            st.dirty |= !updated.is_empty();
            updated
        };
        for d in updated {
            self.inner.sink.emit(EngineEvent::Upsert { download: d });
        }
        schedule(&self.inner);
    }

    pub fn resume_all(&self) {
        let ids: Vec<String> = self
            .inner
            .state
            .lock()
            .unwrap()
            .downloads
            .iter()
            .map(|d| d.id.clone())
            .collect();
        self.resume(&ids);
    }

    /// Removes downloads from the list; with `delete_files`, also deletes the
    /// partial file and, for completed downloads, the finished file.
    pub fn remove(&self, ids: &[String], delete_files: bool) {
        let removed: Vec<String> = {
            let mut st = self.inner.state.lock().unwrap();
            let State {
                downloads, jobs, ..
            } = &mut *st;
            let mut removed = Vec::new();
            downloads.retain(|d| {
                if !ids.contains(&d.id) {
                    return true;
                }
                let mut paths = vec![d.part_path()];
                if delete_files && d.status == Status::Completed {
                    paths.push(d.file_path());
                }
                match jobs.get_mut(&d.id) {
                    Some(job) => {
                        job.stop();
                        if delete_files {
                            job.delete_after = paths;
                        }
                    }
                    None if delete_files => {
                        for p in paths {
                            let _ = std::fs::remove_file(p);
                        }
                    }
                    None => {}
                }
                removed.push(d.id.clone());
                false
            });
            st.dirty |= !removed.is_empty();
            removed
        };
        if !removed.is_empty() {
            self.inner.sink.emit(EngineEvent::Removed { ids: removed });
        }
    }

    /// Moves a download to just before `before_id` in the queue, or to the end.
    pub fn reorder(&self, id: &str, before_id: Option<&str>) {
        let updated = {
            let mut st = self.inner.state.lock().unwrap();
            let mut order: Vec<String> = {
                let mut ds: Vec<&Download> = st.downloads.iter().collect();
                ds.sort_by_key(|d| d.queue_position);
                ds.into_iter()
                    .map(|d| d.id.clone())
                    .filter(|d| d != id)
                    .collect()
            };
            let at = before_id
                .and_then(|b| order.iter().position(|x| x == b))
                .unwrap_or(order.len());
            order.insert(at, id.to_string());
            let mut updated = Vec::new();
            for d in st.downloads.iter_mut() {
                let pos = order.iter().position(|x| *x == d.id).unwrap_or(0) as u64;
                if d.queue_position != pos {
                    d.queue_position = pos;
                    updated.push(d.clone());
                }
            }
            st.dirty = true;
            updated
        };
        for d in updated {
            self.inner.sink.emit(EngineEvent::Upsert { download: d });
        }
    }

    pub fn update_settings(&self, mut settings: Settings) -> Result<Settings, EngineError> {
        settings.max_concurrent = settings.max_concurrent.clamp(1, 20);
        settings.connections = settings.connections.clamp(1, MAX_CONNECTIONS);
        settings.max_retries = settings.max_retries.min(50);
        if settings.user_agent.trim().is_empty() {
            settings.user_agent = model::DEFAULT_USER_AGENT.to_string();
        }
        self.inner.limiter.set_limit(settings.speed_limit);
        self.inner.store.save_settings(&settings)?;
        self.inner.state.lock().unwrap().settings = settings.clone();
        self.inner.sink.emit(EngineEvent::Settings {
            settings: settings.clone(),
        });
        schedule(&self.inner);
        Ok(settings)
    }

    /// The finished file, or the partial file while still downloading.
    pub fn path_of(&self, id: &str) -> Option<PathBuf> {
        let st = self.inner.state.lock().unwrap();
        let d = st.downloads.iter().find(|d| d.id == id)?;
        Some(if d.status == Status::Completed {
            d.file_path()
        } else {
            d.part_path()
        })
    }

    pub fn active_count(&self) -> usize {
        self.inner.state.lock().unwrap().jobs.len()
    }

    /// Stops all connections, waits briefly for buffers to flush, and saves state.
    pub fn shutdown(&self, wait: Duration) {
        {
            let mut st = self.inner.state.lock().unwrap();
            st.shutting_down = true;
            for job in st.jobs.values_mut() {
                job.stop();
            }
        }
        let deadline = Instant::now() + wait;
        while self.active_count() > 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(25));
        }
        let downloads = {
            let mut st = self.inner.state.lock().unwrap();
            let State {
                downloads, jobs, ..
            } = &mut *st;
            for d in downloads.iter_mut() {
                if let Some(job) = jobs.get(&d.id) {
                    apply_segments(d, job.live_segments());
                    d.status = Status::Queued;
                }
            }
            downloads.clone()
        };
        let generation = self.inner.generation.fetch_add(1, Ordering::AcqRel) + 1;
        if let Err(e) = self.inner.store.save_downloads(generation, &downloads) {
            eprintln!("downpour: failed to save downloads on exit: {e}");
        }
    }
}

fn default_dir(settings: &Settings, category: Category) -> PathBuf {
    if settings.category_folders {
        settings.download_dir.join(category.folder_name())
    } else {
        settings.download_dir.clone()
    }
}

fn claimed_names(
    downloads: &[Download],
    dir: &std::path::Path,
    except: Option<&str>,
) -> HashSet<String> {
    downloads
        .iter()
        .filter(|d| {
            d.save_dir == dir && d.status != Status::Completed && Some(d.id.as_str()) != except
        })
        .map(|d| d.file_name.clone())
        .collect()
}

fn apply_segments(d: &mut Download, segs: Vec<Segment>) {
    if segs.is_empty() {
        return;
    }
    d.downloaded = segs.iter().map(Segment::done).sum();
    d.segments = segs;
}

/// Starts queued downloads in queue order until the concurrency limit is reached.
fn schedule(inner: &Arc<Inner>) {
    let started: Vec<(Download, watch::Receiver<bool>, LiveSegments)> = {
        let mut st = inner.state.lock().unwrap();
        if st.shutting_down {
            return;
        }
        let running = st.jobs.values().filter(|j| !j.stopping).count();
        let slots = (st.settings.max_concurrent as usize).saturating_sub(running);
        let mut queued: Vec<(u64, String)> = st
            .downloads
            .iter()
            .filter(|d| d.status == Status::Queued && !st.jobs.contains_key(&d.id))
            .map(|d| (d.queue_position, d.id.clone()))
            .collect();
        queued.sort();
        let mut started = Vec::new();
        for (_, id) in queued.into_iter().take(slots) {
            let (tx, rx) = watch::channel(false);
            let segments = Arc::new(Mutex::new(Vec::new()));
            st.jobs.insert(
                id.clone(),
                Job {
                    cancel: tx,
                    segments: segments.clone(),
                    stopping: false,
                    delete_after: Vec::new(),
                    speed: SpeedMeter::new(),
                },
            );
            if let Some(d) = st.downloads.iter_mut().find(|d| d.id == id) {
                d.status = Status::Connecting;
                d.error = None;
                started.push((d.clone(), rx, segments));
            }
        }
        st.dirty |= !started.is_empty();
        started
    };
    for (d, rx, segments) in started {
        inner.sink.emit(EngineEvent::Upsert {
            download: d.clone(),
        });
        let inner2 = inner.clone();
        inner.runtime.spawn(async move {
            let result = run_job(&inner2, &d.id, rx, segments).await;
            finish(&inner2, &d.id, result);
        });
    }
}

async fn run_job(
    inner: &Arc<Inner>,
    id: &str,
    cancel: watch::Receiver<bool>,
    live: LiveSegments,
) -> Result<(), EngineError> {
    let (dl, settings) = {
        let st = inner.state.lock().unwrap();
        let d = st
            .downloads
            .iter()
            .find(|d| d.id == id)
            .ok_or(EngineError::NotFound)?;
        (d.clone(), st.settings.clone())
    };
    let opts = RequestOptions {
        user_agent: &settings.user_agent,
        referer: dl.referer.as_deref(),
    };
    let mut probe_cancel = cancel.clone();
    let info = tokio::select! {
        r = probe::probe(&inner.client, &dl.url, &opts) => r?,
        _ = probe_cancel.changed() => return Err(EngineError::Cancelled),
    };

    let changed_remotely = !dl.segments.is_empty()
        && (info.total_size != dl.total_size
            || matches!((&dl.etag, &info.etag), (Some(a), Some(b)) if a != b)
            || matches!((&dl.last_modified, &info.last_modified), (Some(a), Some(b)) if a != b));
    let can_continue =
        !dl.segments.is_empty() && info.resumable && !changed_remotely && dl.part_path().exists();

    // Update the record from the server's answer and decide where bytes go.
    let dl = {
        let mut st = inner.state.lock().unwrap();
        let State {
            downloads,
            settings,
            ..
        } = &mut *st;
        let snapshot = downloads.clone();
        let d = downloads
            .iter_mut()
            .find(|d| d.id == id)
            .ok_or(EngineError::NotFound)?;
        d.final_url = Some(info.final_url.clone());
        d.total_size = info.total_size;
        d.resumable = info.resumable;
        d.content_type = info.content_type.clone();
        d.etag = info.etag.clone();
        d.last_modified = info.last_modified.clone();
        if !can_continue {
            if d.auto_name && d.segments.is_empty() {
                let category = Category::from_file_name(&info.file_name);
                if d.auto_dir {
                    d.save_dir = default_dir(settings, category);
                }
                d.file_name = naming::unique_name(
                    &d.save_dir,
                    &info.file_name,
                    &claimed_names(&snapshot, &d.save_dir, Some(id)),
                );
                d.category = category;
            }
            d.auto_name = false;
            d.segments = match (info.total_size, info.resumable) {
                (Some(total), true) => segments::plan(total, d.connections),
                (Some(total), false) => vec![Segment {
                    start: 0,
                    end: total,
                    downloaded: 0,
                }],
                (None, _) => vec![Segment {
                    start: 0,
                    end: u64::MAX,
                    downloaded: 0,
                }],
            };
            d.downloaded = 0;
        }
        d.status = Status::Downloading;
        st.dirty = true;
        st.downloads
            .iter()
            .find(|d| d.id == id)
            .cloned()
            .ok_or(EngineError::NotFound)?
    };
    inner.sink.emit(EngineEvent::Upsert {
        download: dl.clone(),
    });

    tokio::fs::create_dir_all(&dl.save_dir).await?;
    let part = dl.part_path();
    if !can_continue {
        let file = tokio::fs::File::create(&part).await?;
        if let (Some(total), true) = (dl.total_size, dl.resumable) {
            file.set_len(total).await?;
        }
    }
    *live.lock().unwrap() = dl.segments.iter().map(LiveSegment::new).collect();

    let ctx = Arc::new(worker::JobCtx {
        client: inner.client.clone(),
        url: info.final_url.clone(),
        user_agent: settings.user_agent.clone(),
        referer: dl.referer.clone(),
        part_path: part.clone(),
        ranged: dl.resumable,
        connections: dl.connections,
        max_retries: settings.max_retries,
        limiter: inner.limiter.clone(),
        cancel,
        segments: live.clone(),
        active: AtomicUsize::new(0),
    });
    worker::download_segments(ctx).await?;

    let segs: Vec<Segment> = live.lock().unwrap().iter().map(|s| s.snapshot()).collect();
    let written: u64 = segs.iter().map(Segment::done).sum();
    if let Some(total) = dl.total_size
        && written < total
    {
        return Err(EngineError::Network(format!(
            "incomplete: {written} of {total} bytes"
        )));
    }
    let file = tokio::fs::OpenOptions::new()
        .write(true)
        .open(&part)
        .await?;
    if dl.total_size.is_none() {
        file.set_len(written).await?;
    }
    file.sync_all().await?;
    drop(file);

    let final_name = {
        let st = inner.state.lock().unwrap();
        if dl.file_path().exists() {
            naming::unique_name(
                &dl.save_dir,
                &dl.file_name,
                &claimed_names(&st.downloads, &dl.save_dir, Some(id)),
            )
        } else {
            dl.file_name.clone()
        }
    };
    tokio::fs::rename(&part, dl.save_dir.join(&final_name)).await?;
    let mut st = inner.state.lock().unwrap();
    if let Some(d) = st.downloads.iter_mut().find(|d| d.id == id) {
        d.file_name = final_name;
        d.total_size = Some(dl.total_size.unwrap_or(written));
    }
    Ok(())
}

fn finish(inner: &Arc<Inner>, id: &str, result: Result<(), EngineError>) {
    let (updated, completed, notify) = {
        let mut st = inner.state.lock().unwrap();
        let job = st.jobs.remove(id);
        let shutting_down = st.shutting_down;
        let notify = st.settings.notify_on_complete;
        let Some(d) = st.downloads.iter_mut().find(|d| d.id == id) else {
            // Removed while running: the job has stopped writing, so deletion is safe now.
            if let Some(job) = job {
                for p in job.delete_after {
                    let _ = std::fs::remove_file(p);
                }
            }
            return;
        };
        if let Some(job) = &job {
            apply_segments(d, job.live_segments());
        }
        let mut completed = false;
        match result {
            Ok(()) => {
                d.status = Status::Completed;
                d.completed_at = Some(now_ms());
                d.downloaded = d.total_size.unwrap_or(d.downloaded);
                d.segments.clear();
                d.error = None;
                completed = true;
            }
            Err(EngineError::Cancelled) => {
                if shutting_down {
                    d.status = Status::Queued;
                } else if d.status.is_active() {
                    d.status = Status::Paused;
                }
            }
            Err(e) => {
                d.status = Status::Failed;
                d.error = Some(e.to_string());
            }
        }
        let updated = d.clone();
        st.dirty = true;
        (updated, completed, notify)
    };
    if completed && notify {
        inner.sink.completed(&updated);
    }
    inner.sink.emit(EngineEvent::Upsert { download: updated });
    schedule(inner);
}

async fn ticker(inner: Arc<Inner>) {
    let mut interval = tokio::time::interval(TICK);
    let mut ticks: u32 = 0;
    let mut was_active = false;
    loop {
        interval.tick().await;
        ticks = ticks.wrapping_add(1);
        let (items, total_speed, active, to_save) = {
            let mut st = inner.state.lock().unwrap();
            let State {
                downloads, jobs, ..
            } = &mut *st;
            let mut items = Vec::new();
            let mut total_speed = 0;
            for (id, job) in jobs.iter_mut() {
                let segs = job.live_segments();
                if segs.is_empty() {
                    continue;
                }
                let Some(d) = downloads.iter_mut().find(|d| &d.id == id) else {
                    continue;
                };
                apply_segments(d, segs.clone());
                let speed = job.speed.sample(d.downloaded);
                total_speed += speed;
                let active_connections = job
                    .segments
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|s| s.active.load(Ordering::Acquire))
                    .count() as u32;
                items.push(ProgressItem {
                    id: id.clone(),
                    downloaded: d.downloaded,
                    total_size: d.total_size,
                    speed,
                    eta: d
                        .total_size
                        .filter(|_| speed > 0)
                        .map(|t| t.saturating_sub(d.downloaded) / speed.max(1)),
                    active_connections,
                    segments: if d.total_size.is_some() {
                        segs.iter().map(|s| [s.start, s.end, s.done()]).collect()
                    } else {
                        Vec::new()
                    },
                });
            }
            let active = jobs.len();
            if !items.is_empty() {
                st.dirty = true;
            }
            let to_save = (st.dirty && ticks.is_multiple_of(SAVE_EVERY_TICKS)).then(|| {
                st.dirty = false;
                (
                    inner.generation.fetch_add(1, Ordering::AcqRel) + 1,
                    st.downloads.clone(),
                )
            });
            (items, total_speed, active, to_save)
        };
        if !items.is_empty() || was_active {
            inner
                .sink
                .emit(EngineEvent::Progress { items, total_speed });
        }
        was_active = active > 0;
        inner.sink.activity(active, total_speed);
        if let Some((generation, downloads)) = to_save {
            let inner2 = inner.clone();
            let _ = tokio::task::spawn_blocking(move || {
                if let Err(e) = inner2.store.save_downloads(generation, &downloads) {
                    eprintln!("downpour: failed to save downloads: {e}");
                }
            })
            .await;
        }
    }
}

#[cfg(test)]
mod tests;
