//! The measured history of one value, kept in three resolutions so a graph can show anything from
//! the last minute to the last day. Lives in memory only.
//!
//! Every sample is one second apart. Tier 0 keeps the last 5 minutes as they came in, tier 1 the
//! last hour as 10 second averages and tier 2 the last day as 1 minute averages.

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

    /// How many points a full graph shows.
    pub fn points(self) -> usize {
        match self {
            Range::Minute => 60,
            Range::FiveMinutes => 300,
            Range::Hour => 360,
            Range::FourHours => 240,
            Range::Day => 1440,
        }
    }

    /// Seconds between two points.
    pub fn step_secs(self) -> f32 {
        match self {
            Range::Minute | Range::FiveMinutes => 1.0,
            Range::Hour => 10.0,
            Range::FourHours | Range::Day => 60.0,
        }
    }

    /// Which tier the range reads from.
    fn tier(self) -> usize {
        match self {
            Range::Minute | Range::FiveMinutes => 0,
            Range::Hour => 1,
            Range::FourHours | Range::Day => 2,
        }
    }
}

/// Samples averaged into one point of the next tier.
const TIER_RATIO: [usize; 2] = [10, 6];
/// Points kept per tier.
const TIER_CAPACITY: [usize; 3] = [300, 360, 1440];

pub struct History {
    tiers: [Vec<f32>; 3],
    /// Running sum and count of the samples not yet folded into the next tier.
    pending: [(f32, usize); 2],
}

impl History {
    pub fn new() -> Self {
        Self {
            tiers: [Vec::new(), Vec::new(), Vec::new()],
            pending: [(0.0, 0); 2],
        }
    }

    /// Adds one sample; older points fall off the end of each tier, and every few samples one
    /// average is passed on to the next tier.
    pub fn push(&mut self, value: f32) {
        self.push_tier(0, value);
    }

    fn push_tier(&mut self, tier: usize, value: f32) {
        let t = &mut self.tiers[tier];
        t.push(value);
        if t.len() > TIER_CAPACITY[tier] {
            t.remove(0);
        }
        if tier < TIER_RATIO.len() {
            let (sum, count) = &mut self.pending[tier];
            *sum += value;
            *count += 1;
            if *count == TIER_RATIO[tier] {
                let mean = *sum / *count as f32;
                *sum = 0.0;
                *count = 0;
                self.push_tier(tier + 1, mean);
            }
        }
    }

    /// The values a graph of `range` shows, oldest first.
    pub fn view(&self, range: Range) -> &[f32] {
        let t = &self.tiers[range.tier()];
        &t[t.len().saturating_sub(range.points())..]
    }

    /// The newest sample.
    pub fn last(&self) -> Option<f32> {
        self.tiers[0].last().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fine_tier_keeps_the_newest_five_minutes() {
        let mut h = History::new();
        for v in 0..400 {
            h.push(v as f32);
        }
        assert_eq!(h.tiers[0].len(), 300);
        assert_eq!(h.last(), Some(399.0));
        assert_eq!(h.view(Range::Minute).len(), 60);
        assert_eq!(h.view(Range::Minute).first(), Some(&340.0));
        assert_eq!(h.view(Range::FiveMinutes).len(), 300);
    }

    #[test]
    fn ten_samples_become_one_average() {
        let mut h = History::new();
        for v in 1..=10 {
            h.push(v as f32);
        }
        assert_eq!(h.view(Range::Hour), &[5.5]);
        // Nine more samples are not enough for another point.
        for _ in 0..9 {
            h.push(0.0);
        }
        assert_eq!(h.view(Range::Hour).len(), 1);
        h.push(0.0);
        assert_eq!(h.view(Range::Hour), &[5.5, 0.0]);
    }

    #[test]
    fn sixty_samples_become_one_minute_point() {
        let mut h = History::new();
        for _ in 0..60 {
            h.push(10.0);
        }
        assert_eq!(h.view(Range::Day), &[10.0]);
        assert_eq!(h.view(Range::FourHours), &[10.0]);
        assert_eq!(h.view(Range::Hour).len(), 6);
    }

    #[test]
    fn long_tiers_are_capped() {
        let mut h = History::new();
        // A bit over 25 hours.
        for _ in 0..(25 * 3600) {
            h.push(1.0);
        }
        assert_eq!(h.tiers[1].len(), 360);
        assert_eq!(h.tiers[2].len(), 1440);
        assert_eq!(h.view(Range::FourHours).len(), 240);
        assert_eq!(h.view(Range::Day).len(), 1440);
    }

    #[test]
    fn starts_empty() {
        let h = History::new();
        assert!(h.view(Range::Day).is_empty());
        assert_eq!(h.last(), None);
    }

    #[test]
    fn range_steps_match_tiers() {
        for r in Range::ALL {
            let span = r.points() as f32 * r.step_secs();
            let expected = match r {
                Range::Minute => 60.0,
                Range::FiveMinutes => 300.0,
                Range::Hour => 3600.0,
                Range::FourHours => 14400.0,
                Range::Day => 86400.0,
            };
            assert_eq!(span, expected, "{}", r.label());
        }
    }
}
