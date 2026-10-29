//! A fixed-size history of one measured value, oldest first. Lives in memory only.

pub struct History {
    values: Vec<f32>,
    capacity: usize,
}

impl History {
    pub fn new(capacity: usize) -> Self {
        Self {
            values: Vec::with_capacity(capacity + 1),
            capacity: capacity.max(1),
        }
    }

    /// Adds a value; the oldest one is dropped once the history is full.
    pub fn push(&mut self, value: f32) {
        self.values.push(value);
        if self.values.len() > self.capacity {
            self.values.remove(0);
        }
    }

    pub fn values(&self) -> &[f32] {
        &self.values
    }

    /// How many points a full graph shows.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn last(&self) -> Option<f32> {
        self.values.last().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_newest_values_up_to_capacity() {
        let mut h = History::new(3);
        for v in [1.0, 2.0, 3.0, 4.0, 5.0] {
            h.push(v);
        }
        assert_eq!(h.values(), &[3.0, 4.0, 5.0]);
        assert_eq!(h.capacity(), 3);
        assert_eq!(h.last(), Some(5.0));
    }

    #[test]
    fn starts_empty() {
        let h = History::new(60);
        assert!(h.values().is_empty());
        assert_eq!(h.last(), None);
    }

    #[test]
    fn zero_capacity_still_holds_one_point() {
        let mut h = History::new(0);
        h.push(7.0);
        h.push(8.0);
        assert_eq!(h.values(), &[8.0]);
    }
}
