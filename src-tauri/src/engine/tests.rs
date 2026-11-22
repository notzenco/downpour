//! End-to-end engine tests against a minimal in-process HTTP/1.1 server.

use super::*;
use std::sync::atomic::AtomicUsize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

fn content(len: usize) -> Arc<Vec<u8>> {
    Arc::new((0..len).map(|i| ((i * 31 + i / 7) % 251) as u8).collect())
}

#[derive(Default)]
struct ServerLog {
    requests: AtomicUsize,
    /// Start offsets of every ranged request, in arrival order.
    range_starts: Mutex<Vec<u64>>,
    /// Transfers currently being served on `/limited`.
    in_flight: AtomicUsize,
    throttled: AtomicUsize,
}

/// `/limited` refuses a third simultaneous transfer, like servers that cap connections per client.
const LIMITED_CONNECTIONS: usize = 2;

/// Serves `/file` (ranges), `/limited` (ranges, slow, at most two at once),
/// `/norange` (ignores ranges), `/chunked` (no length) and 404 for anything else. Each response closes its connection.
async fn serve(body: Arc<Vec<u8>>) -> (String, Arc<ServerLog>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let log = Arc::new(ServerLog::default());
    let log2 = log.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            let body = body.clone();
            let log = log2.clone();
            tokio::spawn(async move {
                let mut req = Vec::new();
                let mut buf = [0u8; 1024];
                while !req.windows(4).any(|w| w == b"\r\n\r\n") {
                    match sock.read(&mut buf).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => req.extend_from_slice(&buf[..n]),
                    }
                }
                log.requests.fetch_add(1, Ordering::SeqCst);
                let text = String::from_utf8_lossy(&req).to_string();
                let path = text.split_whitespace().nth(1).unwrap_or("/").to_string();
                let range = text.lines().find_map(|l| {
                    let (k, v) = l.split_once(':')?;
                    k.eq_ignore_ascii_case("range")
                        .then(|| v.trim().to_string())
                });
                let total = body.len() as u64;
                let (head, slice): (String, &[u8]) = match (path.as_str(), range) {
                    (p @ ("/file" | "/limited"), Some(r)) => {
                        if p == "/limited"
                            && log.in_flight.fetch_add(1, Ordering::SeqCst) >= LIMITED_CONNECTIONS
                        {
                            log.in_flight.fetch_sub(1, Ordering::SeqCst);
                            log.throttled.fetch_add(1, Ordering::SeqCst);
                            let _ = sock
                                .write_all(b"HTTP/1.1 429 Too Many Requests\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                                .await;
                            return;
                        }
                        let spec = r.trim_start_matches("bytes=");
                        let (a, b) = spec.split_once('-').unwrap();
                        let start: u64 = a.parse().unwrap();
                        let end: u64 = if b.is_empty() {
                            total - 1
                        } else {
                            b.parse::<u64>().unwrap().min(total - 1)
                        };
                        log.range_starts.lock().unwrap().push(start);
                        let s = &body[start as usize..=end as usize];
                        (
                            format!(
                                "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{end}/{total}\r\nAccept-Ranges: bytes\r\nContent-Disposition: attachment; filename=\"served.bin\"\r\n",
                                s.len()
                            ),
                            s,
                        )
                    }
                    ("/file", None) | ("/norange", _) => (
                        format!("HTTP/1.1 200 OK\r\nContent-Length: {total}\r\n"),
                        &body[..],
                    ),
                    ("/chunked", _) => {
                        let mut out = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n".to_vec();
                        for chunk in body.chunks(10_000) {
                            out.extend_from_slice(format!("{:x}\r\n", chunk.len()).as_bytes());
                            out.extend_from_slice(chunk);
                            out.extend_from_slice(b"\r\n");
                        }
                        out.extend_from_slice(b"0\r\n\r\n");
                        let _ = sock.write_all(&out).await;
                        return;
                    }
                    _ => (
                        "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n".to_string(),
                        &[][..],
                    ),
                };
                let _ = sock
                    .write_all(format!("{head}Connection: close\r\n\r\n").as_bytes())
                    .await;
                if path == "/limited" {
                    for chunk in slice.chunks(32 * 1024) {
                        if sock.write_all(chunk).await.is_err() {
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(2)).await;
                    }
                    log.in_flight.fetch_sub(1, Ordering::SeqCst);
                } else {
                    let _ = sock.write_all(slice).await;
                }
            });
        }
    });
    (base, log)
}

struct NullSink;
impl EventSink for NullSink {
    fn emit(&self, _event: EngineEvent) {}
}

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("downpour-test-{tag}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn manager(root: &std::path::Path) -> Manager {
    let m = Manager::new(
        root.join("data"),
        root.join("downloads"),
        Arc::new(NullSink),
    );
    let mut s = m.settings();
    s.category_folders = false;
    s.max_retries = 1;
    m.update_settings(s).unwrap();
    m.start();
    m
}

async fn wait_for(m: &Manager, id: &str, pred: impl Fn(&Download) -> bool) -> Download {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let d = m
            .snapshot()
            .downloads
            .into_iter()
            .find(|d| d.id == id)
            .expect("download exists");
        if pred(&d) {
            return d;
        }
        assert!(Instant::now() < deadline, "timed out; last state: {d:?}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn add(m: &Manager, url: String) -> Download {
    m.add(AddRequest {
        url,
        file_name: None,
        save_dir: None,
        connections: Some(4),
        referer: None,
        start: true,
    })
    .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn segmented_download_matches_source_and_uses_server_name() {
    let body = content(9 * 1024 * 1024 + 123);
    let (base, log) = serve(body.clone()).await;
    let root = temp_dir("segmented");
    let m = manager(&root);

    let d = add(&m, format!("{base}/file"));
    let done = wait_for(&m, &d.id, |d| {
        matches!(d.status, Status::Completed | Status::Failed)
    })
    .await;

    assert_eq!(done.status, Status::Completed, "{:?}", done.error);
    assert_eq!(done.file_name, "served.bin");
    assert!(done.resumable);
    assert_eq!(std::fs::read(done.file_path()).unwrap(), *body);
    assert!(!done.part_path().exists());
    let starts = log.range_starts.lock().unwrap().clone();
    assert!(
        starts.iter().filter(|s| **s > 0).count() >= 3,
        "expected parallel ranges, got {starts:?}"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn server_without_ranges_downloads_on_one_connection() {
    let body = content(3 * 1024 * 1024);
    let (base, log) = serve(body.clone()).await;
    let root = temp_dir("norange");
    let m = manager(&root);

    let d = add(&m, format!("{base}/norange"));
    let done = wait_for(&m, &d.id, |d| {
        matches!(d.status, Status::Completed | Status::Failed)
    })
    .await;

    assert_eq!(done.status, Status::Completed, "{:?}", done.error);
    assert!(!done.resumable);
    assert_eq!(std::fs::read(done.file_path()).unwrap(), *body);
    // One probe plus one transfer.
    assert_eq!(log.requests.load(Ordering::SeqCst), 2);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn unknown_length_stream_completes_with_measured_size() {
    let body = content(777_777);
    let (base, _log) = serve(body.clone()).await;
    let root = temp_dir("chunked");
    let m = manager(&root);

    let d = add(&m, format!("{base}/chunked"));
    let done = wait_for(&m, &d.id, |d| {
        matches!(d.status, Status::Completed | Status::Failed)
    })
    .await;

    assert_eq!(done.status, Status::Completed, "{:?}", done.error);
    assert_eq!(done.total_size, Some(body.len() as u64));
    assert_eq!(std::fs::read(done.file_path()).unwrap(), *body);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn missing_file_fails_without_retrying() {
    let (base, log) = serve(content(10)).await;
    let root = temp_dir("404");
    let m = manager(&root);

    let d = add(&m, format!("{base}/missing.zip"));
    let failed = wait_for(&m, &d.id, |d| {
        matches!(d.status, Status::Completed | Status::Failed)
    })
    .await;

    assert_eq!(failed.status, Status::Failed);
    assert!(failed.error.unwrap().contains("404"));
    assert_eq!(log.requests.load(Ordering::SeqCst), 1);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn pause_and_resume_continue_from_saved_offsets() {
    let body = content(4 * 1024 * 1024);
    let (base, log) = serve(body.clone()).await;
    let root = temp_dir("resume");
    let m = manager(&root);
    let mut s = m.settings();
    s.speed_limit = 2 * 1024 * 1024;
    m.update_settings(s).unwrap();

    let d = add(&m, format!("{base}/file"));
    wait_for(&m, &d.id, |d| d.downloaded > 512 * 1024).await;
    m.pause(std::slice::from_ref(&d.id));
    let paused = wait_for(&m, &d.id, |d| d.status == Status::Paused).await;
    while m.active_count() > 0 {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let paused_at = m.snapshot().downloads[0].downloaded;
    assert!(
        paused_at > 0 && paused_at < body.len() as u64,
        "paused at {paused_at}"
    );
    assert!(paused.part_path().exists());
    let requests_before_resume = log.range_starts.lock().unwrap().len();

    let mut s = m.settings();
    s.speed_limit = 0;
    m.update_settings(s).unwrap();
    m.resume(std::slice::from_ref(&d.id));
    let done = wait_for(&m, &d.id, |d| {
        matches!(d.status, Status::Completed | Status::Failed)
    })
    .await;

    assert_eq!(done.status, Status::Completed, "{:?}", done.error);
    assert_eq!(std::fs::read(done.file_path()).unwrap(), *body);
    let after: Vec<u64> = log.range_starts.lock().unwrap()[requests_before_resume..].to_vec();
    // The resume probe asks from 0; every transfer after it starts mid-file.
    assert!(
        after.iter().skip(1).all(|s| *s > 0),
        "resume restarted from zero: {after:?}"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn interrupted_download_is_queued_again_after_restart() {
    let body = content(4 * 1024 * 1024);
    let (base, _log) = serve(body.clone()).await;
    let root = temp_dir("restart");

    let id = {
        let m = manager(&root);
        let mut s = m.settings();
        s.speed_limit = 1024 * 1024;
        m.update_settings(s).unwrap();
        let d = add(&m, format!("{base}/file"));
        wait_for(&m, &d.id, |d| d.downloaded > 256 * 1024).await;
        let m2 = m.clone();
        tokio::task::spawn_blocking(move || m2.shutdown(Duration::from_secs(2)))
            .await
            .unwrap();
        d.id
    };

    let m = Manager::new(
        root.join("data"),
        root.join("downloads"),
        Arc::new(NullSink),
    );
    let restored = m
        .snapshot()
        .downloads
        .into_iter()
        .find(|d| d.id == id)
        .unwrap();
    assert_eq!(restored.status, Status::Queued);
    assert!(restored.downloaded > 0);
    let mut s = m.settings();
    s.speed_limit = 0;
    m.update_settings(s).unwrap();
    m.start();
    let done = wait_for(&m, &id, |d| {
        matches!(d.status, Status::Completed | Status::Failed)
    })
    .await;
    assert_eq!(done.status, Status::Completed, "{:?}", done.error);
    assert_eq!(std::fs::read(done.file_path()).unwrap(), *body);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn queue_respects_max_concurrent() {
    let body = content(2 * 1024 * 1024);
    let (base, _log) = serve(body.clone()).await;
    let root = temp_dir("queue");
    let m = manager(&root);
    let mut s = m.settings();
    s.max_concurrent = 1;
    s.speed_limit = 4 * 1024 * 1024;
    m.update_settings(s).unwrap();

    let a = add(&m, format!("{base}/file"));
    let b = add(&m, format!("{base}/file"));
    wait_for(&m, &a.id, |d| d.status == Status::Downloading).await;
    let b_now = m
        .snapshot()
        .downloads
        .into_iter()
        .find(|d| d.id == b.id)
        .unwrap();
    assert_eq!(b_now.status, Status::Queued);
    assert_ne!(
        b_now.file_name, a.file_name,
        "second download must not claim the same name"
    );

    let a_done = wait_for(&m, &a.id, |d| d.status == Status::Completed).await;
    let b_done = wait_for(&m, &b.id, |d| d.status == Status::Completed).await;
    assert_ne!(a_done.file_path(), b_done.file_path());
    assert_eq!(std::fs::read(b_done.file_path()).unwrap(), *body);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn refused_extra_connections_fall_back_to_fewer() {
    let body = content(6 * 1024 * 1024);
    let (base, log) = serve(body.clone()).await;
    let root = temp_dir("limited");
    let m = manager(&root);
    // Refusals of extra connections must not count as failures, even with no retries allowed.
    let mut s = m.settings();
    s.max_retries = 0;
    m.update_settings(s).unwrap();

    let d = m
        .add(AddRequest {
            url: format!("{base}/limited"),
            file_name: None,
            save_dir: None,
            connections: Some(6),
            referer: None,
            start: true,
        })
        .unwrap();
    let done = wait_for(&m, &d.id, |d| {
        matches!(d.status, Status::Completed | Status::Failed)
    })
    .await;

    assert_eq!(done.status, Status::Completed, "{:?}", done.error);
    assert_eq!(std::fs::read(done.file_path()).unwrap(), *body);
    assert!(
        log.throttled.load(Ordering::SeqCst) > 0,
        "the server never refused a connection, so the test proved nothing"
    );
    std::fs::remove_dir_all(root).unwrap();
}

/// Real-network throughput check, skipped by default:
/// `DOWNPOUR_BENCH_URL=<url> cargo test --release --locked bench_real_url -- --ignored --nocapture`
#[tokio::test(flavor = "multi_thread")]
#[ignore = "downloads from the internet"]
async fn bench_real_url() {
    let url = std::env::var("DOWNPOUR_BENCH_URL").expect("set DOWNPOUR_BENCH_URL");
    let connections = std::env::var("DOWNPOUR_BENCH_CONNECTIONS")
        .ok()
        .and_then(|c| c.parse().ok())
        .unwrap_or(8);
    let root = temp_dir("bench");
    let m = manager(&root);
    let mut s = m.settings();
    s.max_retries = 5;
    m.update_settings(s).unwrap();
    let version = m
        .inner
        .client
        .get(&url)
        .header("range", "bytes=0-0")
        .send()
        .await
        .map(|r| r.version());

    let started = Instant::now();
    let d = m
        .add(AddRequest {
            url,
            file_name: None,
            save_dir: None,
            connections: Some(connections),
            referer: None,
            start: true,
        })
        .unwrap();
    let done = loop {
        let d = m
            .snapshot()
            .downloads
            .into_iter()
            .find(|x| x.id == d.id)
            .unwrap();
        if matches!(d.status, Status::Completed | Status::Failed) {
            break d;
        }
        assert!(
            started.elapsed() < Duration::from_secs(600),
            "bench timed out"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    let secs = started.elapsed().as_secs_f64();
    let size = done.total_size.unwrap_or(0) as f64;
    println!(
        "bench: {:?} {:?} {} connections, {:.1} MB in {secs:.2}s = {:.1} MB/s ({:?})",
        done.status,
        done.error,
        connections,
        size / 1048576.0,
        size / 1048576.0 / secs,
        version
    );
    std::fs::remove_dir_all(root).unwrap();
}
