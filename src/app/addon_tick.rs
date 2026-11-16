//! When the add-on work runs: the supervisor tick, the hide rule, starting background add-ons
//! and the add-on list timer (#176).
//!
//! eframe calls `App::logic` on every wake-up but `App::ui` only when it thinks the window can be
//! seen. A minimized window gets no `ui` pass at all; a window hidden in the tray gets one on
//! Windows today (eframe 0.37 cannot tell it is hidden), but nothing promises that. So the add-on
//! work runs from `ui` while `ui` runs, as before, and from `logic` once no tick has run for
//! [`EVERY`]. One pass never runs it twice: when `logic` ticked, the `ui` of that pass skips it.
//! While the window is not drawn the app asks to be woken for the next tick, and for nothing
//! else, so the tray state stays at about one short wake-up per second.

use std::time::{Duration, Instant};

/// How often the add-ons are ticked while the window is not drawn. Equal to the heartbeat
/// interval, so a page add-on in a minimized window is still pinged on time.
pub const EVERY: Duration = Duration::from_secs(1);

#[derive(Debug, Default)]
pub struct AddonTick {
    /// When the add-on work last ran, from either path.
    last: Option<Instant>,
    /// The `logic` of the current pass ran it.
    in_logic: bool,
}

impl AddonTick {
    /// Called once at the top of every `logic`: true when the add-on work should run now, which is
    /// when it has not run for [`EVERY`] (the window is not drawn, or nothing woke it).
    pub fn logic_due(&mut self, now: Instant) -> bool {
        self.in_logic = self
            .last
            .is_none_or(|at| now.saturating_duration_since(at) >= EVERY);
        if self.in_logic {
            self.last = Some(now);
        }
        self.in_logic
    }

    /// Called from `ui`, after the `logic` of the same pass: true unless that `logic` already ran
    /// the add-on work.
    pub fn ui_due(&mut self, now: Instant) -> bool {
        if std::mem::take(&mut self.in_logic) {
            return false;
        }
        self.last = Some(now);
        true
    }

    /// How long until `logic` ticks again. Never zero, so an early wake-up does not spin.
    pub fn wait(&self, now: Instant) -> Duration {
        const MIN: Duration = Duration::from_millis(10);
        self.last.map_or(MIN, |at| {
            EVERY
                .saturating_sub(now.saturating_duration_since(at))
                .max(MIN)
        })
    }
}

/// Whether `logic` must ask to be woken for the next tick: only while the window is not drawn.
/// A drawn window is woken by its own repaints, and asking there would redraw it every second.
pub fn wake_for_tick(hidden: bool, minimized: bool) -> bool {
    hidden || minimized
}

/// What the add-on list timer does in this tick.
#[derive(Debug, PartialEq, Eq)]
pub enum ListFetch {
    /// The list was never fetched: fetch it (this shows "Checking for add-ons...").
    First,
    /// The periodic quiet fetch is due.
    Quiet,
    Nothing,
}

/// The add-on list timer. The periodic fetch runs only while the window is shown (a fetch due in
/// the tray waits until the window comes back, then runs at once), and never while an install,
/// an update or an install review is open, as before.
pub fn list_fetch(checked: bool, hidden: bool, due: bool, busy: bool) -> ListFetch {
    if busy {
        ListFetch::Nothing
    } else if !checked {
        ListFetch::First
    } else if !hidden && due {
        ListFetch::Quiet
    } else {
        ListFetch::Nothing
    }
}

/// The hide rule for a page add-on: hidden (a page add-on stops, a background one is suspended)
/// unless its page is the one on screen in a window that is not hidden in the tray.
pub fn hide_addon(on_screen: bool, window_hidden: bool) -> bool {
    !on_screen || window_hidden
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::supervisor::tests::{fake, supervisor, ID};
    use crate::supervisor::{Event, Timing};

    const MS: Duration = Duration::from_millis(1);

    #[test]
    fn logic_ticks_first_and_then_once_a_second() {
        let t0 = Instant::now();
        let mut tick = AddonTick::default();
        assert!(tick.logic_due(t0), "the first pass ticks");
        assert!(!tick.logic_due(t0 + 500 * MS));
        assert!(!tick.logic_due(t0 + 999 * MS));
        assert!(tick.logic_due(t0 + EVERY));
        assert!(!tick.logic_due(t0 + EVERY + 10 * MS));
        assert!(
            tick.logic_due(t0 + 5 * EVERY),
            "a late wake-up ticks at once"
        );
    }

    #[test]
    fn no_double_tick_in_one_pass() {
        let t0 = Instant::now();
        let mut tick = AddonTick::default();
        // logic ticked in this pass: ui skips.
        assert!(tick.logic_due(t0));
        assert!(!tick.ui_due(t0));
        // The next drawn pass: logic does not tick, ui does.
        assert!(!tick.logic_due(t0 + 16 * MS));
        assert!(tick.ui_due(t0 + 16 * MS));
        // A window that keeps drawing never lets logic tick.
        for frame in 2..200 {
            let now = t0 + frame * 16 * MS;
            assert!(!tick.logic_due(now), "frame {frame}");
            assert!(tick.ui_due(now), "frame {frame}");
        }
    }

    #[test]
    fn a_logic_only_pass_does_not_leave_ui_skipping_later() {
        let t0 = Instant::now();
        let mut tick = AddonTick::default();
        // Hidden: logic-only passes, one of them ticks.
        assert!(tick.logic_due(t0));
        assert!(!tick.logic_due(t0 + 300 * MS));
        // Shown again: the first drawn pass after a non-ticking logic runs the work in ui.
        assert!(!tick.logic_due(t0 + 400 * MS));
        assert!(tick.ui_due(t0 + 400 * MS));
    }

    #[test]
    fn waits_until_the_next_tick_and_never_zero() {
        let t0 = Instant::now();
        let mut tick = AddonTick::default();
        assert_eq!(tick.wait(t0), 10 * MS);
        tick.logic_due(t0);
        assert_eq!(tick.wait(t0), EVERY);
        assert_eq!(tick.wait(t0 + 400 * MS), 600 * MS);
        assert_eq!(tick.wait(t0 + EVERY), 10 * MS);
        assert_eq!(tick.wait(t0 + 3 * EVERY), 10 * MS);
        // Ticks from ui count too: a drawn window's last tick sets the next hidden one.
        let mut tick = AddonTick::default();
        assert!(tick.logic_due(t0));
        assert!(!tick.logic_due(t0 + 500 * MS));
        assert!(tick.ui_due(t0 + 500 * MS));
        assert_eq!(tick.wait(t0 + 600 * MS), 900 * MS);
    }

    #[test]
    fn asks_for_wake_ups_only_while_the_window_is_not_drawn() {
        assert!(wake_for_tick(true, false));
        assert!(wake_for_tick(false, true));
        assert!(wake_for_tick(true, true));
        assert!(!wake_for_tick(false, false));
    }

    #[test]
    fn list_timer_rules() {
        use ListFetch::*;
        // Never fetched: fetch, hidden or not.
        assert_eq!(list_fetch(false, false, false, false), First);
        assert_eq!(list_fetch(false, true, false, false), First);
        // Due: only while shown.
        assert_eq!(list_fetch(true, false, true, false), Quiet);
        assert_eq!(list_fetch(true, true, true, false), Nothing);
        assert_eq!(list_fetch(true, false, false, false), Nothing);
        // An install, update or review open: nothing at all.
        assert_eq!(list_fetch(true, false, true, true), Nothing);
        assert_eq!(list_fetch(false, false, false, true), Nothing);
    }

    #[test]
    fn hide_rule() {
        assert!(!hide_addon(true, false));
        assert!(
            hide_addon(true, true),
            "its page was open when the window went to the tray"
        );
        assert!(hide_addon(false, false));
        assert!(hide_addon(false, true));
    }

    /// The hidden path as `App::logic` drives it: logic-only passes woken by repaint requests,
    /// the add-on work only when [`AddonTick::logic_due`] says so, and the hide rule applied with
    /// the window hidden. A background add-on that exits is restarted by such a tick.
    #[test]
    fn hidden_ticks_restart_a_background_addon_that_exits() {
        let (dir, mut spec) = fake("hidden-tick-background", "crash");
        spec.background = true;
        let mut supervisor = supervisor(Timing::default());
        let mut tick = AddonTick::default();
        let started = Instant::now();
        let mut events = Vec::new();
        let mut ticks = 0;
        while !events.iter().any(|e| matches!(e, Event::Restarting { .. })) {
            assert!(
                started.elapsed() < Duration::from_secs(30),
                "no restart; events: {events:?}"
            );
            let now = Instant::now();
            if tick.logic_due(now) {
                ticks += 1;
                events.extend(supervisor.tick(now));
                if hide_addon(false, true) {
                    supervisor.hide(ID);
                }
                supervisor.start_background(spec.clone(), now);
            }
            std::thread::sleep(tick.wait(Instant::now()).min(50 * MS));
        }
        assert!(
            ticks >= 2,
            "the restart came from a later tick, not the first"
        );
        // The restarted process really runs.
        let deadline = Instant::now() + Duration::from_secs(30);
        while std::fs::read_to_string(dir.join("starts.txt"))
            .map(|s| s.lines().count())
            .unwrap_or(0)
            < 2
        {
            assert!(Instant::now() < deadline, "the restarted add-on never ran");
            std::thread::sleep(20 * MS);
            if tick.logic_due(Instant::now()) {
                supervisor.tick(Instant::now());
            }
        }
    }

    /// A page add-on whose page was open when the window went to the tray is stopped by the hide
    /// rule of a hidden tick.
    #[test]
    fn hidden_ticks_stop_a_page_addon_that_was_on_screen() {
        let (_dir, spec) = fake("hidden-tick-page", "normal");
        let mut supervisor = supervisor(Timing::default());
        supervisor.open(spec, Instant::now());
        let started = Instant::now();
        while !supervisor
            .tick(Instant::now())
            .iter()
            .any(|e| matches!(e, Event::Connected { .. }))
        {
            assert!(
                started.elapsed() < Duration::from_secs(30),
                "never connected"
            );
            std::thread::sleep(10 * MS);
        }
        let mut tick = AddonTick::default();
        let mut events = Vec::new();
        let started = Instant::now();
        while !events.iter().any(|e| matches!(e, Event::Stopped { .. })) {
            assert!(
                started.elapsed() < Duration::from_secs(30),
                "never stopped; events: {events:?}"
            );
            let now = Instant::now();
            if tick.logic_due(now) {
                events.extend(supervisor.tick(now));
                if hide_addon(true, true) {
                    supervisor.hide(ID);
                }
            }
            std::thread::sleep(tick.wait(Instant::now()).min(50 * MS));
        }
        assert!(!supervisor.is_running(ID));
    }
}
