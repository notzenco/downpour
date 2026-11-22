use futures_util::StreamExt;
use reqwest::{Client, StatusCode, header};
use std::collections::VecDeque;
use std::io::SeekFrom;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::fs::OpenOptions;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tokio::sync::watch;
use tokio::task::JoinSet;
use tokio::time::MissedTickBehavior;

use super::error::EngineError;
use super::limiter::RateLimiter;
use super::probe::{self, RequestOptions};
use super::segments::{self, LiveSegment};

/// Bytes are buffered and written in batches; `downloaded` only advances after
/// a batch is flushed, so persisted progress never claims bytes that are not on disk.
const WRITE_BATCH: usize = 256 * 1024;
const FLUSH_INTERVAL: Duration = Duration::from_millis(200);

pub struct JobCtx {
    pub client: Client,
    pub url: String,
    pub user_agent: String,
    pub referer: Option<String>,
    pub part_path: PathBuf,
    /// The server honours byte ranges, so segments can run in parallel and resume.
    pub ranged: bool,
    pub connections: u32,
    pub max_retries: u32,
    pub limiter: Arc<RateLimiter>,
    pub cancel: watch::Receiver<bool>,
    pub segments: Arc<Mutex<Vec<Arc<LiveSegment>>>>,
    /// Connections currently running for this job.
    pub active: AtomicUsize,
}

/// Connections open one at a time at this interval; opening them all at once
/// trips the burst limits many servers apply per client.
const RAMP_STEP: Duration = Duration::from_millis(120);

/// Runs connections over the job's segments until all are complete. When a
/// connection frees up and nothing is pending, it takes over half of the
/// largest remaining segment, which keeps every connection busy to the end.
///
/// If the server refuses an extra connection while others are working, the
/// range goes back to the queue and the job continues with fewer connections,
/// so a per-client connection cap slows the download instead of failing it.
pub async fn download_segments(ctx: Arc<JobCtx>) -> Result<(), EngineError> {
    let mut pending: VecDeque<Arc<LiveSegment>> = ctx
        .segments
        .lock()
        .unwrap()
        .iter()
        .filter(|s| !s.snapshot().is_complete())
        .cloned()
        .collect();
    let mut limit = if ctx.ranged {
        ctx.connections.max(1) as usize
    } else {
        1
    };

    let mut set = JoinSet::new();
    let spawn = |set: &mut JoinSet<_>, seg: Arc<LiveSegment>| {
        seg.active.store(true, Ordering::Release);
        ctx.active.fetch_add(1, Ordering::AcqRel);
        let ctx = ctx.clone();
        set.spawn(async move {
            let result = run_segment(&ctx, &seg).await;
            seg.active.store(false, Ordering::Release);
            ctx.active.fetch_sub(1, Ordering::AcqRel);
            (seg, result)
        });
    };
    let next_segment = |pending: &mut VecDeque<Arc<LiveSegment>>| {
        pending.pop_front().or_else(|| {
            if !ctx.ranged {
                return None;
            }
            let mut segs = ctx.segments.lock().unwrap();
            let tail = segments::split_largest(&segs)?;
            let live = LiveSegment::new(&tail);
            segs.push(live.clone());
            Some(live)
        })
    };

    let mut ramp = tokio::time::interval(RAMP_STEP);
    ramp.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut failure = None;
    loop {
        let stopping = failure.is_some() || *ctx.cancel.borrow();
        let can_grow = !stopping && set.len() < limit && !pending.is_empty();
        tokio::select! {
            _ = ramp.tick(), if can_grow => {
                if let Some(seg) = pending.pop_front() {
                    spawn(&mut set, seg);
                }
            }
            Some(joined) = set.join_next(), if !set.is_empty() => {
                let (seg, result) = match joined {
                    Ok(r) => r,
                    Err(e) if e.is_cancelled() => continue,
                    Err(e) => {
                        failure.get_or_insert(EngineError::Network(format!("connection task failed: {e}")));
                        set.abort_all();
                        continue;
                    }
                };
                match result {
                    Ok(()) if !stopping && set.len() < limit => {
                        if let Some(next) = next_segment(&mut pending) {
                            spawn(&mut set, next);
                        }
                    }
                    Err(EngineError::Refused) => {
                        pending.push_front(seg);
                        limit = set.len().max(1);
                    }
                    Ok(()) | Err(EngineError::Cancelled) => {}
                    Err(e) => {
                        if failure.is_none() {
                            failure = Some(e);
                            set.abort_all();
                        }
                    }
                }
            }
            else => break,
        }
    }

    if let Some(e) = failure {
        return Err(e);
    }
    if *ctx.cancel.borrow() {
        return Err(EngineError::Cancelled);
    }
    Ok(())
}

async fn run_segment(ctx: &JobCtx, seg: &LiveSegment) -> Result<(), EngineError> {
    let mut attempts = 0;
    loop {
        if *ctx.cancel.borrow() {
            return Err(EngineError::Cancelled);
        }
        let before = seg.position();
        match fetch(ctx, seg).await {
            Ok(()) => return Ok(()),
            Err(EngineError::Cancelled) => return Err(EngineError::Cancelled),
            Err(e) if e.is_retryable() => {
                let progressed = seg.position() > before;
                if !progressed && ctx.active.load(Ordering::Acquire) > 1 {
                    // Other connections work, so the server is limiting us rather than failing.
                    return Err(EngineError::Refused);
                }
                if progressed {
                    attempts = 0;
                }
                if attempts >= ctx.max_retries {
                    return Err(e);
                }
                attempts += 1;
                let backoff = Duration::from_millis(500 * (1u64 << attempts.min(6)));
                let mut cancel = ctx.cancel.clone();
                tokio::select! {
                    _ = tokio::time::sleep(backoff) => {}
                    _ = cancel.changed() => return Err(EngineError::Cancelled),
                }
            }
            Err(e) => return Err(e),
        }
    }
}

async fn fetch(ctx: &JobCtx, seg: &LiveSegment) -> Result<(), EngineError> {
    let mut cancel = ctx.cancel.clone();
    if !ctx.ranged && seg.downloaded.load(Ordering::Acquire) > 0 {
        // Without range support a broken stream can only restart from zero.
        seg.downloaded.store(0, Ordering::Release);
    }
    let pos = seg.position();
    let end = seg.end.load(Ordering::Acquire);
    if end != u64::MAX && pos >= end {
        return Ok(());
    }

    let opts = RequestOptions {
        user_agent: &ctx.user_agent,
        referer: ctx.referer.as_deref(),
    };
    let mut req = probe::request(&ctx.client, &ctx.url, &opts);
    if ctx.ranged {
        let range = if end == u64::MAX {
            format!("bytes={pos}-")
        } else {
            format!("bytes={pos}-{}", end - 1)
        };
        req = req.header(header::RANGE, range);
    }
    let resp = tokio::select! {
        r = req.send() => r?,
        _ = cancel.changed() => return Err(EngineError::Cancelled),
    };
    let status = resp.status();
    if !status.is_success() {
        return Err(EngineError::Http(status.as_u16()));
    }
    if ctx.ranged && status != StatusCode::PARTIAL_CONTENT && pos != 0 {
        return Err(EngineError::RangeUnsupported);
    }

    let mut file = OpenOptions::new().write(true).open(&ctx.part_path).await?;
    file.seek(SeekFrom::Start(pos)).await?;
    let mut stream = resp.bytes_stream();
    let mut buf: Vec<u8> = Vec::with_capacity(WRITE_BATCH);
    let mut last_flush = Instant::now();

    loop {
        let next = tokio::select! {
            biased;
            _ = cancel.changed() => {
                flush(&mut file, &mut buf, seg).await?;
                return Err(EngineError::Cancelled);
            }
            n = stream.next() => n,
        };
        let bytes = match next {
            Some(Ok(b)) => b,
            Some(Err(e)) => {
                flush(&mut file, &mut buf, seg).await?;
                return Err(e.into());
            }
            None => {
                flush(&mut file, &mut buf, seg).await?;
                let end = seg.end.load(Ordering::Acquire);
                if end == u64::MAX {
                    seg.end.store(seg.position(), Ordering::Release);
                    return Ok(());
                }
                if seg.position() >= end {
                    return Ok(());
                }
                return Err(EngineError::Network(
                    "connection closed before the range finished".into(),
                ));
            }
        };

        let end = seg.end.load(Ordering::Acquire);
        let cursor = seg.position() + buf.len() as u64;
        let take = if end == u64::MAX {
            bytes.len()
        } else {
            end.saturating_sub(cursor).min(bytes.len() as u64) as usize
        };

        let wait = ctx.limiter.reserve(take as u64);
        if !wait.is_zero() {
            tokio::select! {
                _ = tokio::time::sleep(wait) => {}
                _ = cancel.changed() => {
                    flush(&mut file, &mut buf, seg).await?;
                    return Err(EngineError::Cancelled);
                }
            }
        }

        buf.extend_from_slice(&bytes[..take]);
        let reached_end = end != u64::MAX && cursor + take as u64 >= end;
        if reached_end || buf.len() >= WRITE_BATCH || last_flush.elapsed() >= FLUSH_INTERVAL {
            flush(&mut file, &mut buf, seg).await?;
            last_flush = Instant::now();
        }
        if reached_end {
            return Ok(());
        }
    }
}

async fn flush(
    file: &mut tokio::fs::File,
    buf: &mut Vec<u8>,
    seg: &LiveSegment,
) -> Result<(), EngineError> {
    if buf.is_empty() {
        return Ok(());
    }
    file.write_all(buf).await?;
    file.flush().await?;
    seg.downloaded.fetch_add(buf.len() as u64, Ordering::AcqRel);
    buf.clear();
    Ok(())
}
