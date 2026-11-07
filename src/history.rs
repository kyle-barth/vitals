//! Fixed-capacity history of samples. It never allocates after construction,
//! so memory use stays flat however long the app runs.

/// Two minutes at one sample per second.
pub const CAPACITY: usize = 120;

#[derive(Clone)]
pub struct Series {
    samples: [f32; CAPACITY],
    next: usize,
    len: usize,
}

impl Default for Series {
    fn default() -> Self {
        Self {
            samples: [f32::NAN; CAPACITY],
            next: 0,
            len: 0,
        }
    }
}

impl Series {
    /// Records a sample. `None` is stored as a gap so a dropped sensor shows as
    /// a break in the line rather than a false value.
    pub fn push(&mut self, value: Option<f32>) {
        self.samples[self.next] = value.unwrap_or(f32::NAN);
        self.next = (self.next + 1) % CAPACITY;
        self.len = (self.len + 1).min(CAPACITY);
    }

    pub fn len(&self) -> usize {
        self.len
    }

    /// Samples from oldest to newest. Gaps are `NaN`.
    pub fn iter(&self) -> impl Iterator<Item = f32> + '_ {
        let start = (self.next + CAPACITY - self.len) % CAPACITY;
        (0..self.len).map(move |i| self.samples[(start + i) % CAPACITY])
    }

    /// The mean of the values held, ignoring gaps.
    pub fn average(&self) -> Option<f32> {
        let (sum, count) = self
            .iter()
            .filter(|v| !v.is_nan())
            .fold((0.0, 0u32), |(sum, count), v| (sum + v, count + 1));
        (count > 0).then(|| sum / count as f32)
    }

    /// Lowest and highest values, ignoring gaps.
    pub fn bounds(&self) -> Option<(f32, f32)> {
        self.iter()
            .filter(|v| !v.is_nan())
            .fold(None, |acc, v| match acc {
                None => Some((v, v)),
                Some((lo, hi)) => Some((lo.min(v), hi.max(v))),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_average_skips_gaps() {
        let mut s = Series::default();
        assert_eq!(s.average(), None);
        s.push(Some(10.0));
        s.push(None);
        s.push(Some(20.0));
        assert_eq!(s.average(), Some(15.0));
    }

    #[test]
    fn iterates_oldest_first_after_wrapping() {
        let mut s = Series::default();
        for i in 0..CAPACITY + 5 {
            s.push(Some(i as f32));
        }
        assert_eq!(s.len(), CAPACITY);
        let values: Vec<f32> = s.iter().collect();
        assert_eq!(values[0], 5.0);
        assert_eq!(values[CAPACITY - 1], (CAPACITY + 4) as f32);
    }

    #[test]
    fn bounds_skip_gaps() {
        let mut s = Series::default();
        s.push(Some(5.0));
        s.push(None);
        s.push(Some(9.0));
        assert_eq!(s.bounds(), Some((5.0, 9.0)));
    }

    #[test]
    fn empty_series_has_no_bounds() {
        assert_eq!(Series::default().bounds(), None);
    }
}
