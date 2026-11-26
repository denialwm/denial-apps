//! Whether a file is there, for checks render makes on every frame.

use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long an answer holds: a package installed while Settings is open
/// shows within this.
const HOLD: Duration = Duration::from_secs(1);

/// A file whose presence is looked up at most once per [`HOLD`].
pub struct Presence {
    path: &'static str,
    seen: Mutex<Option<(Instant, bool)>>,
}

impl Presence {
    pub const fn new(path: &'static str) -> Self {
        Self {
            path,
            seen: Mutex::new(None),
        }
    }

    pub fn get(&self) -> bool {
        let mut seen = self
            .seen
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let now = Instant::now();
        match *seen {
            Some((at, there)) if now.duration_since(at) < HOLD => there,
            _ => {
                let there = std::path::Path::new(self.path).exists();
                *seen = Some((now, there));
                there
            }
        }
    }
}
