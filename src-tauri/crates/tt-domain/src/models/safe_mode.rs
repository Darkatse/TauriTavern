use serde::{Deserialize, Serialize};

pub const LAUNCH_WINDOW_MS: i64 = 3 * 60 * 1000;
pub const OFFER_LAUNCH_COUNT: usize = 5;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchRecord {
    #[serde(default)]
    launches: Vec<i64>,
    #[serde(default)]
    requested: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchDecision {
    Normal,
    Offer,
    Rescue,
}

impl LaunchRecord {
    /// `now` is in Unix milliseconds. The request is consumed before the rescue runs, so one
    /// request rescues at most once.
    pub fn register(&mut self, now: i64) -> LaunchDecision {
        if std::mem::take(&mut self.requested) {
            self.launches.clear();
            return LaunchDecision::Rescue;
        }
        self.launches
            .retain(|&launch| launch <= now && now - launch < LAUNCH_WINDOW_MS);
        self.launches.push(now);
        if self.launches.len() >= OFFER_LAUNCH_COUNT {
            LaunchDecision::Offer
        } else {
            LaunchDecision::Normal
        }
    }

    pub fn request(&mut self) {
        self.requested = true;
        self.launches.clear();
    }

    pub fn dismiss_offer(&mut self) {
        self.launches.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launches_offer_safe_mode_and_a_request_rescues_once() {
        const SECOND: i64 = 1000;
        let mut record = LaunchRecord::default();
        let decisions: Vec<_> = [0, 30, 60, 90, 120]
            .iter()
            .map(|seconds| record.register(seconds * SECOND))
            .collect();
        assert_eq!(decisions[..4], [LaunchDecision::Normal; 4]);
        assert_eq!(decisions[4], LaunchDecision::Offer);

        // An unanswered offer stays until its launches leave the window.
        assert_eq!(record.register(150 * SECOND), LaunchDecision::Offer);
        assert_eq!(record.register(500 * SECOND), LaunchDecision::Normal);

        // A clock set back drops the launches now in the future.
        let mut record = LaunchRecord::default();
        for seconds in [100, 110, 120, 130] {
            record.register(seconds * SECOND);
        }
        assert_eq!(record.register(10 * SECOND), LaunchDecision::Normal);

        record.request();
        assert_eq!(record.register(20 * SECOND), LaunchDecision::Rescue);
        assert_eq!(record.register(21 * SECOND), LaunchDecision::Normal);

        record.dismiss_offer();
        assert_eq!(record, LaunchRecord::default());
    }
}
