//! The measured history behind the Home graphs: one value every two seconds for up to a day,
//! thinned to at most 360 points when a graph asks for a range. Lives in memory only.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Seconds between two stored points.
pub const STEP_SECS: u64 = 2;
/// A day of points.
const CAPACITY: usize = 24 * 3600 / STEP_SECS as usize;
/// A graph never shows more points than this, so a day is averaged down.
const MAX_POINTS: usize = 360;
/// A gap longer than this (the window was hidden) is filled with at most this many points.
const MAX_FILL: usize = 900;

/// How far back a graph looks.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Range {
    #[default]
    Minute,
    FiveMinutes,
    Hour,
    FourHours,
    Day,
}

impl Range {
    pub const ALL: [Range; 5] = [
        Range::Minute,
        Range::FiveMinutes,
        Range::Hour,
        Range::FourHours,
        Range::Day,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Range::Minute => "1 min",
            Range::FiveMinutes => "5 min",
            Range::Hour => "1 h",
            Range::FourHours => "4 h",
            Range::Day => "24 h",
        }
    }

    /// The text under the graph.
    pub fn caption(self) -> &'static str {
        match self {
            Range::Minute => "Last minute",
            Range::FiveMinutes => "Last 5 minutes",
            Range::Hour => "Last hour",
            Range::FourHours => "Last 4 hours",
            Range::Day => "Last 24 hours",
        }
    }

    fn secs(self) -> u64 {
        match self {
            Range::Minute => 60,
            Range::FiveMinutes => 300,
            Range::Hour => 3600,
            Range::FourHours => 4 * 3600,
            Range::Day => 24 * 3600,
        }
    }
}

/// What a graph draws: the points, how many a full graph holds and the seconds between points.
#[derive(Clone, Debug, PartialEq)]
pub struct View {
    pub values: Vec<f32>,
    pub capacity: usize,
    pub step_secs: f32,
}

#[derive(Default)]
pub struct Series {
    points: VecDeque<f32>,
}

impl Series {
    pub fn push(&mut self, value: f32) {
        self.points.push_back(value);
        if self.points.len() > CAPACITY {
            self.points.pop_front();
        }
    }

    pub fn last(&self) -> Option<f32> {
        self.points.back().copied()
    }

    /// The newest points for `range`, averaged in equal chunks (aligned to the newest point) so
    /// that no more than `MAX_POINTS` remain.
    pub fn view(&self, range: Range) -> View {
        let want = (range.secs() / STEP_SECS) as usize;
        let chunk = want.div_ceil(MAX_POINTS).max(1);
        let capacity = want / chunk;
        let skip = self.points.len().saturating_sub(want);
        let recent: Vec<f32> = self.points.iter().skip(skip).copied().collect();
        let values = if chunk == 1 {
            recent
        } else {
            // Chunks end at the newest point; a short first chunk is averaged on its own.
            let mut out: Vec<f32> = recent
                .rchunks(chunk)
                .map(|c| c.iter().sum::<f32>() / c.len() as f32)
                .collect();
            out.reverse();
            out
        };
        View {
            values,
            capacity,
            step_secs: (chunk as u64 * STEP_SECS) as f32,
        }
    }
}

/// The four series of the Home page and the clock that feeds them.
pub struct History {
    pub used_percent: Series,
    pub commit_percent: Series,
    /// Cache in GB.
    pub cache_gb: Series,
    /// Disk reads for memory per second.
    pub disk_reads: Series,
    last: Instant,
}

impl History {
    pub fn new() -> Self {
        Self {
            used_percent: Series::default(),
            commit_percent: Series::default(),
            cache_gb: Series::default(),
            disk_reads: Series::default(),
            last: Instant::now(),
        }
    }

    /// How many points are due now: one per two seconds since the last call, none if it is too
    /// early. The first call always owes one.
    pub fn due(&mut self, now: Instant, first: bool) -> usize {
        let elapsed = now.saturating_duration_since(self.last);
        if !first && elapsed < Duration::from_secs(STEP_SECS) {
            return 0;
        }
        self.last = now;
        ((elapsed.as_secs() / STEP_SECS) as usize).clamp(1, MAX_FILL)
    }
}

impl Default for History {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filled(n: usize) -> Series {
        let mut s = Series::default();
        for i in 0..n {
            s.push(i as f32);
        }
        s
    }

    #[test]
    fn short_ranges_show_every_point() {
        let v = filled(100).view(Range::Minute);
        assert_eq!(v.values.len(), 30);
        assert_eq!(v.capacity, 30);
        assert_eq!(v.step_secs, 2.0);
        assert_eq!(v.values.last(), Some(&99.0));
        assert_eq!(v.values.first(), Some(&70.0));
    }

    #[test]
    fn long_ranges_are_averaged_to_at_most_360_points() {
        let v = filled(CAPACITY).view(Range::Day);
        assert_eq!(v.capacity, 360);
        assert_eq!(v.step_secs, 240.0);
        assert!(v.values.len() <= 360);
        // The newest chunk is the mean of the newest 120 points.
        let newest = CAPACITY as f32 - 1.0 - 59.5;
        assert!((v.values.last().unwrap() - newest).abs() < 1e-3);
    }

    #[test]
    fn the_hour_is_averaged_in_fives() {
        let v = filled(10).view(Range::Hour);
        assert_eq!(v.step_secs, 10.0);
        assert_eq!(v.values, vec![2.0, 7.0]);
    }

    #[test]
    fn storage_is_bounded_to_a_day() {
        let s = filled(CAPACITY + 500);
        assert_eq!(s.points.len(), CAPACITY);
        assert_eq!(s.last(), Some((CAPACITY + 499) as f32));
    }

    #[test]
    fn points_are_due_every_two_seconds_and_gaps_are_filled() {
        let mut h = History::new();
        let t0 = h.last;
        assert_eq!(h.due(t0 + Duration::from_millis(1500), false), 0);
        assert_eq!(h.due(t0 + Duration::from_secs(2), false), 1);
        assert_eq!(h.due(t0 + Duration::from_secs(12), false), 5);
        assert_eq!(h.due(t0 + Duration::from_secs(12), true), 1);
    }
}
