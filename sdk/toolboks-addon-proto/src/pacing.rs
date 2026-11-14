//! Frame pacing and liveness, host side. All types take the current time as an argument,
//! so they never sleep and tests drive them with made-up instants.
//!
//! Rules:
//! - One frame in flight. The host sends the next [`crate::InputBatch`] only after the
//!   [`crate::Frame`] that acknowledges the previous one. Input that arrives meanwhile is
//!   queued in [`FramePacer`] and merged (consecutive pointer moves collapse into one).
//! - The host sends at most one batch per `min_interval` (60 fps by default).
//! - Add-on-initiated repaints ([`crate::AddonMessage::RequestRepaint`] and the frame's
//!   `repaint_after_ms`) go through [`RepaintLimiter`].
//! - The host never blocks on the add-on: reads run on their own thread and
//!   [`HeartbeatMonitor`] and [`FramePacer::stalled`] decide when the add-on is stuck.

use crate::InputEvent;
use std::time::{Duration, Instant};

/// 60 frames per second.
pub const FRAME_INTERVAL: Duration = Duration::from_nanos(16_666_667);
/// How often the host pings an add-on that has sent nothing.
pub const PING_INTERVAL: Duration = Duration::from_secs(1);
/// How long a ping or a frame may stay unanswered before the add-on counts as stalled.
pub const STALL_TIMEOUT: Duration = Duration::from_secs(5);

pub struct FramePacer {
    min_interval: Duration,
    next_seq: u64,
    in_flight: Option<(u64, Instant)>,
    last_sent: Option<Instant>,
    pending: Vec<InputEvent>,
    wanted: bool,
}

impl Default for FramePacer {
    fn default() -> Self {
        Self::new(FRAME_INTERVAL)
    }
}

impl FramePacer {
    pub fn new(min_interval: Duration) -> Self {
        Self {
            min_interval,
            next_seq: 1,
            in_flight: None,
            last_sent: None,
            pending: Vec::new(),
            wanted: true,
        }
    }

    /// Queues input for the next batch and marks a frame as wanted.
    pub fn push(&mut self, events: impl IntoIterator<Item = InputEvent>) {
        for event in events {
            if let (InputEvent::PointerMoved { .. }, Some(InputEvent::PointerMoved { pos })) =
                (&event, self.pending.last_mut())
            {
                if let InputEvent::PointerMoved { pos: new } = event {
                    *pos = new;
                }
                continue;
            }
            self.pending.push(event);
        }
        self.wanted = true;
    }

    /// Marks a frame as wanted without input (resize, theme change, repaint request).
    pub fn request(&mut self) {
        self.wanted = true;
    }

    /// The earliest time the next batch may go out, or `None` while waiting for an ack or
    /// when no frame is wanted.
    pub fn next_send_at(&self) -> Option<Instant> {
        if self.in_flight.is_some() || !self.wanted {
            return None;
        }
        Some(match self.last_sent {
            Some(last) => last + self.min_interval,
            None => Instant::now(),
        })
    }

    pub fn ready(&self, now: Instant) -> bool {
        self.in_flight.is_none()
            && self.wanted
            && self
                .last_sent
                .is_none_or(|last| now.saturating_duration_since(last) >= self.min_interval)
    }

    /// Takes the next batch when [`Self::ready`]: its sequence number and merged events.
    pub fn take(&mut self, now: Instant) -> Option<(u64, Vec<InputEvent>)> {
        if !self.ready(now) {
            return None;
        }
        let seq = self.next_seq;
        self.next_seq += 1;
        self.in_flight = Some((seq, now));
        self.last_sent = Some(now);
        self.wanted = false;
        Some((seq, std::mem::take(&mut self.pending)))
    }

    /// Records the frame that acknowledges `seq`. Returns false for a stale or unknown seq.
    pub fn ack(&mut self, seq: u64) -> bool {
        if self.in_flight.is_some_and(|(sent, _)| sent == seq) {
            self.in_flight = None;
            true
        } else {
            false
        }
    }

    pub fn in_flight(&self) -> Option<u64> {
        self.in_flight.map(|(seq, _)| seq)
    }

    /// True when a batch has waited for its frame for `timeout` or longer.
    pub fn stalled(&self, now: Instant, timeout: Duration) -> bool {
        self.in_flight
            .is_some_and(|(_, sent)| now.saturating_duration_since(sent) >= timeout)
    }
}

/// Caps add-on-initiated repaints: whatever the add-on asks for, two of them are at least
/// `min_interval` apart.
pub struct RepaintLimiter {
    min_interval: Duration,
    last: Option<Instant>,
}

impl Default for RepaintLimiter {
    fn default() -> Self {
        Self::new(FRAME_INTERVAL)
    }
}

impl RepaintLimiter {
    pub fn new(min_interval: Duration) -> Self {
        Self {
            min_interval,
            last: None,
        }
    }

    /// When the requested repaint may run: `now + after`, but not before `min_interval`
    /// after the last repaint.
    pub fn deadline(&self, now: Instant, after: Duration) -> Instant {
        let wanted = now + after;
        match self.last {
            Some(last) => wanted.max(last + self.min_interval),
            None => wanted,
        }
    }

    /// Records that a repaint ran at `now`.
    pub fn mark(&mut self, now: Instant) {
        self.last = Some(now);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Heartbeat {
    /// Nothing to do.
    Alive,
    /// Send `HostMessage::Ping` with this nonce.
    SendPing(u64),
    /// The add-on has not answered a ping within the timeout. End it.
    Stalled { silent_for: Duration },
}

/// Detects a stalled add-on. Any message from the add-on proves its main loop is alive;
/// after `interval` of silence the host pings, and an unanswered ping past `timeout`
/// means stalled.
pub struct HeartbeatMonitor {
    interval: Duration,
    timeout: Duration,
    last_alive: Instant,
    outstanding: Option<(u64, Instant)>,
    next_nonce: u64,
}

impl HeartbeatMonitor {
    pub fn new(now: Instant) -> Self {
        Self::with_timing(now, PING_INTERVAL, STALL_TIMEOUT)
    }

    pub fn with_timing(now: Instant, interval: Duration, timeout: Duration) -> Self {
        Self {
            interval,
            timeout,
            last_alive: now,
            outstanding: None,
            next_nonce: 1,
        }
    }

    /// Any message from the add-on. A [`crate::AddonMessage::Pong`] clears the ping only when
    /// its nonce matches; call [`Self::on_pong`] for those.
    pub fn on_message(&mut self, now: Instant) {
        self.last_alive = now;
        self.outstanding = None;
    }

    pub fn on_pong(&mut self, nonce: u64, now: Instant) {
        if self.outstanding.is_some_and(|(sent, _)| sent == nonce) {
            self.on_message(now);
        }
    }

    pub fn poll(&mut self, now: Instant) -> Heartbeat {
        if let Some((_, sent)) = self.outstanding {
            let waited = now.saturating_duration_since(sent);
            if waited >= self.timeout {
                return Heartbeat::Stalled {
                    silent_for: now.saturating_duration_since(self.last_alive),
                };
            }
            return Heartbeat::Alive;
        }
        if now.saturating_duration_since(self.last_alive) >= self.interval {
            let nonce = self.next_nonce;
            self.next_nonce += 1;
            self.outstanding = Some((nonce, now));
            return Heartbeat::SendPing(nonce);
        }
        Heartbeat::Alive
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: Duration = Duration::from_millis(1);

    #[test]
    fn one_frame_in_flight_and_events_merge() {
        let t0 = Instant::now();
        let mut pacer = FramePacer::new(16 * MS);
        let (seq, events) = pacer.take(t0).unwrap();
        assert!(events.is_empty());
        pacer.push([InputEvent::PointerMoved { pos: [1.0, 1.0] }]);
        pacer.push([
            InputEvent::PointerMoved { pos: [2.0, 2.0] },
            InputEvent::PointerMoved { pos: [3.0, 3.0] },
            InputEvent::Text("a".into()),
        ]);
        assert!(pacer.take(t0 + 100 * MS).is_none(), "waits for the ack");
        assert!(!pacer.ack(seq + 7));
        assert!(pacer.ack(seq));
        let (next, events) = pacer.take(t0 + 100 * MS).unwrap();
        assert_eq!(next, seq + 1);
        assert_eq!(
            events,
            vec![
                InputEvent::PointerMoved { pos: [3.0, 3.0] },
                InputEvent::Text("a".into())
            ]
        );
    }

    #[test]
    fn sixty_fps_cap() {
        let t0 = Instant::now();
        let mut pacer = FramePacer::new(16 * MS);
        let (seq, _) = pacer.take(t0).unwrap();
        pacer.ack(seq);
        pacer.request();
        assert!(!pacer.ready(t0 + 10 * MS));
        assert!(pacer.ready(t0 + 16 * MS));
        pacer.take(t0 + 16 * MS).unwrap();
        assert!(!pacer.ready(t0 + 40 * MS), "nothing wanted and in flight");
    }

    #[test]
    fn in_flight_frame_stalls_after_timeout() {
        let t0 = Instant::now();
        let mut pacer = FramePacer::default();
        pacer.take(t0).unwrap();
        assert!(!pacer.stalled(t0 + 4999 * MS, STALL_TIMEOUT));
        assert!(pacer.stalled(t0 + STALL_TIMEOUT, STALL_TIMEOUT));
    }

    #[test]
    fn repaint_limiter_spaces_bursts() {
        let t0 = Instant::now();
        let mut limiter = RepaintLimiter::new(16 * MS);
        assert_eq!(limiter.deadline(t0, Duration::ZERO), t0);
        limiter.mark(t0);
        assert_eq!(limiter.deadline(t0 + MS, Duration::ZERO), t0 + 16 * MS);
        assert_eq!(limiter.deadline(t0 + MS, 100 * MS), t0 + 101 * MS);
    }

    #[test]
    fn heartbeat_detects_a_stalled_addon() {
        let t0 = Instant::now();
        let mut monitor = HeartbeatMonitor::with_timing(t0, 1000 * MS, 5000 * MS);
        assert_eq!(monitor.poll(t0 + 500 * MS), Heartbeat::Alive);
        let Heartbeat::SendPing(nonce) = monitor.poll(t0 + 1000 * MS) else {
            panic!("expected a ping")
        };
        assert_eq!(monitor.poll(t0 + 2000 * MS), Heartbeat::Alive);
        monitor.on_pong(nonce + 1, t0 + 2500 * MS);
        assert_eq!(
            monitor.poll(t0 + 6000 * MS),
            Heartbeat::Stalled {
                silent_for: 6000 * MS
            }
        );
    }

    #[test]
    fn heartbeat_pong_and_messages_keep_it_alive() {
        let t0 = Instant::now();
        let mut monitor = HeartbeatMonitor::with_timing(t0, 1000 * MS, 5000 * MS);
        let Heartbeat::SendPing(nonce) = monitor.poll(t0 + 1000 * MS) else {
            panic!("expected a ping")
        };
        monitor.on_pong(nonce, t0 + 1200 * MS);
        assert_eq!(monitor.poll(t0 + 2100 * MS), Heartbeat::Alive);
        monitor.on_message(t0 + 2150 * MS);
        assert_eq!(monitor.poll(t0 + 3000 * MS), Heartbeat::Alive);
        assert!(matches!(
            monitor.poll(t0 + 3200 * MS),
            Heartbeat::SendPing(_)
        ));
    }
}
