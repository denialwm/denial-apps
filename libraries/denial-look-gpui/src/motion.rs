//! Animation for GPUI views, on `denial-look`'s timings, curves and springs.
//!
//! A view keeps its animations as plain values (`denial_look::Tween`,
//! `denial_look::Spring` and the types here) and draws from them with the
//! frame's `Instant`. At the end of `render`, `drive` asks the window for
//! the next frame only while one of them runs: a still view draws nothing.
//! Denial's duration scale applies throughout; with reduced motion each
//! animation is finished as it starts.
//!
//! GPUI moves, fades and resizes elements but scales only images and what a
//! canvas paints. Things that grow or shrink, such as `Presence`'s items,
//! are painted; whole elements slide and fade.

use std::time::{Duration, Instant};

use denial_look::motion::{
    APPEAR, APPEAR_FROM, BLINK, SHAKE, SHAKE_DISTANCE, VANISH, VANISH_STAGGER, VANISH_STEP,
};
use denial_look::{Look, Tween, ease_in, ease_out_back};
use gpui::{Context, Task, Window};

/// Asks for the next frame while `running`. Call it once a render, with
/// every animation the view has.
pub fn drive(window: &mut Window, running: bool) {
    if running {
        window.request_animation_frame();
    }
}

/// The items of a row that grow and shrink at its end, such as a password's
/// dots or a row of chips: when each appeared, and which are leaving.
#[derive(Clone, Default)]
pub struct Presence {
    born: Vec<Tween>,
    /// Index and shrink of the items that left.
    leaving: Vec<(usize, Tween)>,
}

impl Presence {
    /// `count` items already in place.
    pub fn settled(count: usize, look: &Look) -> Self {
        Self {
            born: vec![look.tween(0); count],
            leaving: Vec::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.born.len()
    }

    pub fn is_empty(&self) -> bool {
        self.born.is_empty()
    }

    /// Follows the row to `count` items: new ones appear, the ones past it
    /// leave, last first.
    pub fn sync(&mut self, count: usize, look: &Look) {
        let now = Instant::now();
        self.leaving.retain(|(_, tween)| tween.running(now));
        let had = self.born.len();
        if count > had {
            self.born.resize(count, look.tween(APPEAR));
        } else if count < had {
            for (order, index) in (count..had).rev().enumerate() {
                let delay = (VANISH_STEP * order as u64).min(VANISH_STAGGER);
                self.leaving
                    .push((index, look.tween(0).segment(delay, VANISH)));
            }
            self.born.truncate(count);
        }
    }

    /// The size of item `index`, from `APPEAR_FROM` through a small
    /// overshoot to 1.
    pub fn size(&self, index: usize, now: Instant) -> f32 {
        let t = self
            .born
            .get(index)
            .map_or(1.0, |tween| tween.progress(now));
        APPEAR_FROM + (1.0 - APPEAR_FROM) * ease_out_back(t)
    }

    /// The items that left and are still shrinking: index and size.
    pub fn leaving(&self, now: Instant) -> impl Iterator<Item = (usize, f32)> + '_ {
        self.leaving.iter().filter_map(move |(index, tween)| {
            let t = tween.progress(now);
            (t < 1.0).then(|| (*index, 1.0 - ease_in(t)))
        })
    }

    pub fn running(&self, now: Instant) -> bool {
        self.born.iter().any(|tween| tween.running(now))
            || self.leaving.iter().any(|(_, tween)| tween.running(now))
    }
}

/// A refusal's sideways shake, such as a wrong password's.
#[derive(Clone, Copy, Default)]
pub struct Shake(Option<Tween>);

impl Shake {
    pub fn start(&mut self, look: &Look) {
        self.0 = Some(look.tween(SHAKE));
    }

    /// How far to move the shaken thing sideways, in logical pixels.
    pub fn offset(&self, now: Instant) -> f32 {
        self.0
            .filter(|tween| tween.running(now))
            .map_or(0.0, |tween| {
                SHAKE_DISTANCE * denial_look::motion::shake(tween.progress(now))
            })
    }

    pub fn running(&self, now: Instant) -> bool {
        self.0.is_some_and(|tween| tween.running(now))
    }
}

/// A wave along a row of items while something is awaited, such as a
/// password being checked. It runs until stopped.
#[derive(Clone, Copy, Default)]
pub struct Pulse {
    since: Option<Instant>,
    scale: f32,
}

impl Pulse {
    /// Starts the wave, or stops it. Reduced motion never starts it.
    pub fn set(&mut self, on: bool, look: &Look) {
        if !on || look.reduced_motion() {
            self.since = None;
        } else if self.since.is_none() {
            self.since = Some(Instant::now());
            self.scale = look.duration_scale;
        }
    }

    /// The size of item `index`, 1 while the wave is off.
    pub fn size(&self, index: usize, now: Instant) -> f32 {
        self.since.map_or(1.0, |since| {
            let seconds = now.saturating_duration_since(since).as_secs_f32() / self.scale;
            denial_look::motion::pulse(seconds, index)
        })
    }

    pub fn running(&self) -> bool {
        self.since.is_some()
    }
}

/// A text cursor's blink: shown, then hidden, `BLINK` milliseconds each,
/// from when the text last changed. It costs two frames a second, which
/// `keep_blinking` asks for; `drive` needs not know about it.
#[derive(Clone, Copy)]
pub struct Blink {
    since: Instant,
}

impl Default for Blink {
    fn default() -> Self {
        Self {
            since: Instant::now(),
        }
    }
}

impl Blink {
    /// Shows the cursor from now on: typing keeps it lit.
    pub fn restart(&mut self) {
        self.since = Instant::now();
    }

    pub fn visible(&self, now: Instant) -> bool {
        (self.elapsed(now) / BLINK).is_multiple_of(2)
    }

    /// How long until it next shows or hides.
    pub fn until_change(&self, now: Instant) -> Duration {
        Duration::from_millis(BLINK - self.elapsed(now) % BLINK)
    }

    fn elapsed(&self, now: Instant) -> u64 {
        now.saturating_duration_since(self.since).as_millis() as u64
    }
}

/// Redraws the view each time its cursor shows or hides, while `blink`
/// gives a blink: none while it shows no cursor. Keep the task while the
/// view lives.
pub fn keep_blinking<V: 'static>(cx: &mut Context<V>, blink: fn(&V) -> Option<Blink>) -> Task<()> {
    cx.spawn(async move |this, cx| {
        loop {
            let Ok(wait) = this.read_with(cx, |view, _| {
                blink(view).map_or(Duration::from_millis(BLINK), |blink| {
                    blink.until_change(Instant::now())
                })
            }) else {
                break;
            };
            cx.background_executor().timer(wait).await;
            let shown = this.update(cx, |view, cx| {
                if blink(view).is_some() {
                    cx.notify();
                }
            });
            if shown.is_err() {
                break;
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn look(scale: f32) -> Look {
        let mut look = Look::from_settings(&Default::default());
        look.duration_scale = scale;
        look
    }

    #[test]
    fn items_appear_and_leave_last_first() {
        let look = look(1.0);
        let mut presence = Presence::settled(2, &look);
        let now = Instant::now();
        assert!(!presence.running(now));
        presence.sync(5, &look);
        assert!(presence.running(Instant::now()));
        assert!(presence.size(4, Instant::now()) < 0.5);
        presence.sync(1, &look);
        let leaving: Vec<usize> = presence
            .leaving(Instant::now())
            .map(|(index, _)| index)
            .collect();
        assert_eq!(leaving, [4, 3, 2, 1]);
        assert_eq!(presence.len(), 1);
    }

    #[test]
    fn reduced_motion_finishes_at_once() {
        let look = look(0.0);
        let mut presence = Presence::default();
        presence.sync(3, &look);
        presence.sync(0, &look);
        let now = Instant::now();
        assert!(!presence.running(now));
        assert_eq!(presence.leaving(now).count(), 0);
        let mut shake = Shake::default();
        shake.start(&look);
        assert_eq!(shake.offset(now), 0.0);
        let mut pulse = Pulse::default();
        pulse.set(true, &look);
        assert!(!pulse.running());
    }

    #[test]
    fn a_cursor_blinks_from_the_last_change() {
        let blink = Blink::default();
        let start = blink.since;
        assert!(blink.visible(start));
        assert!(!blink.visible(start + Duration::from_millis(BLINK + 1)));
        assert_eq!(
            blink.until_change(start + Duration::from_millis(100)),
            Duration::from_millis(BLINK - 100)
        );
    }
}
