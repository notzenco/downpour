use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use super::model::Segment;

/// Segments are never split below this size; the per-connection overhead
/// outweighs the gain for small ranges.
pub const MIN_SPLIT: u64 = 1024 * 1024;

/// Divides `total` bytes into at most `connections` contiguous segments.
pub fn plan(total: u64, connections: u32) -> Vec<Segment> {
    let wanted = u64::from(connections.max(1));
    let n = wanted.min((total / MIN_SPLIT).max(1));
    let size = total / n;
    (0..n)
        .map(|i| Segment {
            start: i * size,
            end: if i == n - 1 { total } else { (i + 1) * size },
            downloaded: 0,
        })
        .collect()
}

/// Segment state shared between a connection task and the manager. The task
/// re-reads `end` before every write, so the manager can shorten a running
/// segment to hand its tail to a new connection.
#[derive(Debug)]
pub struct LiveSegment {
    pub start: u64,
    pub end: AtomicU64,
    pub downloaded: AtomicU64,
    pub active: AtomicBool,
}

impl LiveSegment {
    pub fn new(s: &Segment) -> Arc<Self> {
        Arc::new(LiveSegment {
            start: s.start,
            end: AtomicU64::new(s.end),
            downloaded: AtomicU64::new(s.downloaded),
            active: AtomicBool::new(false),
        })
    }

    pub fn snapshot(&self) -> Segment {
        Segment {
            start: self.start,
            end: self.end.load(Ordering::Acquire),
            downloaded: self.downloaded.load(Ordering::Acquire),
        }
    }

    pub fn position(&self) -> u64 {
        self.start + self.downloaded.load(Ordering::Acquire)
    }
}

/// Picks the active segment with the most bytes left and, if it is large
/// enough, shortens it to its midpoint and returns the new tail segment.
///
/// A connection that read the old `end` just before the store may write a
/// little past the midpoint; the new tail rewrites identical bytes there, and
/// `Segment::done` clamps the double count, so the race is harmless.
pub fn split_largest(segments: &[Arc<LiveSegment>]) -> Option<Segment> {
    let (seg, remaining) = segments
        .iter()
        .filter(|s| s.active.load(Ordering::Acquire))
        .filter_map(|s| {
            let end = s.end.load(Ordering::Acquire);
            (end != u64::MAX).then(|| (s, end.saturating_sub(s.position())))
        })
        .max_by_key(|(_, r)| *r)?;
    if remaining < 2 * MIN_SPLIT {
        return None;
    }
    let old_end = seg.end.load(Ordering::Acquire);
    let mid = seg.position() + remaining / 2;
    seg.end.store(mid, Ordering::Release);
    Some(Segment {
        start: mid,
        end: old_end,
        downloaded: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn covers(segs: &[Segment], total: u64) -> bool {
        segs.first().map(|s| s.start) == Some(0)
            && segs.last().map(|s| s.end) == Some(total)
            && segs.windows(2).all(|w| w[0].end == w[1].start)
    }

    #[test]
    fn plan_covers_the_file_contiguously() {
        let total = 100 * MIN_SPLIT + 7;
        let segs = plan(total, 8);
        assert_eq!(segs.len(), 8);
        assert!(covers(&segs, total));
    }

    #[test]
    fn plan_uses_fewer_segments_for_small_files() {
        assert_eq!(plan(3 * MIN_SPLIT, 8).len(), 3);
        assert_eq!(plan(10, 8).len(), 1);
        assert!(covers(&plan(10, 8), 10));
        assert_eq!(
            plan(0, 8),
            vec![Segment {
                start: 0,
                end: 0,
                downloaded: 0
            }]
        );
    }

    #[test]
    fn split_halves_the_largest_active_remainder() {
        let a = LiveSegment::new(&Segment {
            start: 0,
            end: 4 * MIN_SPLIT,
            downloaded: 3 * MIN_SPLIT,
        });
        let b = LiveSegment::new(&Segment {
            start: 4 * MIN_SPLIT,
            end: 10 * MIN_SPLIT,
            downloaded: 0,
        });
        a.active.store(true, Ordering::Release);
        b.active.store(true, Ordering::Release);
        let tail = split_largest(&[a.clone(), b.clone()]).unwrap();
        assert_eq!(
            tail,
            Segment {
                start: 7 * MIN_SPLIT,
                end: 10 * MIN_SPLIT,
                downloaded: 0
            }
        );
        assert_eq!(b.end.load(Ordering::Acquire), 7 * MIN_SPLIT);
    }

    #[test]
    fn split_refuses_small_or_inactive_remainders() {
        let a = LiveSegment::new(&Segment {
            start: 0,
            end: MIN_SPLIT,
            downloaded: 0,
        });
        a.active.store(true, Ordering::Release);
        let idle = LiveSegment::new(&Segment {
            start: MIN_SPLIT,
            end: 50 * MIN_SPLIT,
            downloaded: 0,
        });
        assert!(split_largest(&[a, idle]).is_none());
    }
}
