//! Temperature severity, with hysteresis so a value hovering on a threshold
//! does not flicker between two states.

/// How far a value must fall below a threshold before its severity steps down.
pub const HYSTERESIS: f32 = 3.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    #[default]
    Normal,
    Warm,
    Hot,
    Critical,
}

impl Severity {
    pub fn label(self) -> &'static str {
        match self {
            Severity::Normal => "",
            Severity::Warm => "Warm",
            Severity::Hot => "Hot",
            Severity::Critical => "Critical",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub warm: f32,
    pub hot: f32,
    pub critical: f32,
}

impl Limits {
    fn level(&self, value: f32) -> Severity {
        if value >= self.critical {
            Severity::Critical
        } else if value >= self.hot {
            Severity::Hot
        } else if value >= self.warm {
            Severity::Warm
        } else {
            Severity::Normal
        }
    }

    /// Classifies `value`, stepping up immediately but stepping down only once
    /// it has cleared the threshold by [`HYSTERESIS`].
    pub fn classify(&self, value: f32, previous: Severity) -> Severity {
        let now = self.level(value);
        now.max(previous.min(self.level(value + HYSTERESIS)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const L: Limits = Limits {
        warm: 80.0,
        hot: 90.0,
        critical: 95.0,
    };

    #[test]
    fn steps_up_immediately() {
        assert_eq!(L.classify(79.9, Severity::Normal), Severity::Normal);
        assert_eq!(L.classify(80.0, Severity::Normal), Severity::Warm);
        assert_eq!(L.classify(95.0, Severity::Normal), Severity::Critical);
    }

    #[test]
    fn holds_within_hysteresis_band() {
        assert_eq!(L.classify(88.0, Severity::Hot), Severity::Hot);
        assert_eq!(L.classify(93.0, Severity::Critical), Severity::Critical);
    }

    #[test]
    fn steps_down_once_clear_of_the_band() {
        assert_eq!(L.classify(86.9, Severity::Hot), Severity::Warm);
        assert_eq!(L.classify(60.0, Severity::Critical), Severity::Normal);
    }

    #[test]
    fn never_raises_severity_because_of_hysteresis() {
        assert_eq!(L.classify(78.0, Severity::Normal), Severity::Normal);
    }
}
