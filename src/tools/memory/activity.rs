//! What the safe actions engine needs to know about how busy each app is: how long ago it was
//! used, and whether a full-screen app is in front. The decisions are pure (and tested); the
//! few Windows reads at the bottom are thin.
//!
//! Windows has no per-app "last input" value, so activity is inferred: an app counts as used
//! while any of its processes owns the foreground window. For the app in front, the time since
//! the last keyboard or mouse input (`GetLastInputInfo`) is used instead, so an app left in
//! front while the user is away becomes idle too. An app never seen in front since this tool
//! started is only known to be idle for as long as the tool has been watching.

use std::collections::HashMap;
use std::time::Duration;

use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::System::SystemInformation::GetTickCount;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowRect};

/// Remembers when each app (lowercase process name) was last in front.
#[derive(Debug)]
pub struct Activity {
    started_ms: u64,
    last_in_front: HashMap<String, u64>,
}

impl Activity {
    pub fn new(now_ms: u64) -> Self {
        Self {
            started_ms: now_ms,
            last_in_front: HashMap::new(),
        }
    }

    /// Call on every evaluation with the name of the app in front, if any.
    pub fn note_foreground(&mut self, now_ms: u64, name: Option<&str>) {
        if let Some(name) = name {
            self.last_in_front.insert(name.to_lowercase(), now_ms);
        }
    }

    /// How long the app `name` has been idle. `foreground_name` is the app in front now and
    /// `system_idle` the time since the last keyboard or mouse input; both may be unknown.
    pub fn idle_for(
        &self,
        name: &str,
        now_ms: u64,
        foreground_name: Option<&str>,
        system_idle: Option<Duration>,
    ) -> Option<Duration> {
        let lower = name.to_lowercase();
        if foreground_name.is_some_and(|f| f.eq_ignore_ascii_case(&lower)) {
            return system_idle;
        }
        let since = self
            .last_in_front
            .get(&lower)
            .copied()
            .unwrap_or(self.started_ms);
        Some(Duration::from_millis(now_ms.saturating_sub(since)))
    }
}

/// A window that covers its whole monitor counts as full screen (games, video, presentations).
pub fn covers_monitor(window: RECT, monitor: RECT) -> bool {
    window.left <= monitor.left
        && window.top <= monitor.top
        && window.right >= monitor.right
        && window.bottom >= monitor.bottom
}

/// Whether the foreground window covers its monitor. `false` if there is none or it is unknown.
pub fn foreground_is_fullscreen() -> bool {
    // SAFETY: plain FFI calls; every out-parameter is a valid struct for the call's duration and
    // `MONITORINFO::cbSize` is set before the call.
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return false;
        }
        let mut window = RECT::default();
        if GetWindowRect(hwnd, &mut window).is_err() {
            return false;
        }
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return false;
        }
        covers_monitor(window, info.rcMonitor)
    }
}

/// Time since the last keyboard or mouse input, or `None` if Windows does not say.
pub fn system_idle() -> Option<Duration> {
    let mut info = LASTINPUTINFO {
        cbSize: size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    // SAFETY: `info` is a valid LASTINPUTINFO with `cbSize` set.
    if !unsafe { GetLastInputInfo(&mut info) }.as_bool() {
        return None;
    }
    // SAFETY: plain FFI call without arguments.
    let now = unsafe { GetTickCount() };
    Some(Duration::from_millis(u64::from(
        now.wrapping_sub(info.dwTime),
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(l: i32, t: i32, r: i32, b: i32) -> RECT {
        RECT {
            left: l,
            top: t,
            right: r,
            bottom: b,
        }
    }

    #[test]
    fn app_in_front_uses_the_input_idle_time() {
        let a = Activity::new(0);
        let idle = a.idle_for(
            "Chrome.exe",
            600_000,
            Some("chrome.exe"),
            Some(Duration::from_secs(90)),
        );
        assert_eq!(idle, Some(Duration::from_secs(90)));
        // Unknown input time stays unknown (so the app is not eligible).
        assert_eq!(
            a.idle_for("chrome.exe", 600_000, Some("chrome.exe"), None),
            None
        );
    }

    #[test]
    fn background_app_counts_from_when_it_was_last_in_front() {
        let mut a = Activity::new(0);
        a.note_foreground(100_000, Some("Slack.exe"));
        let idle = a.idle_for("slack.exe", 400_000, Some("code.exe"), Some(Duration::ZERO));
        assert_eq!(idle, Some(Duration::from_secs(300)));
    }

    #[test]
    fn never_seen_app_is_only_idle_since_watching_began() {
        let a = Activity::new(50_000);
        let idle = a.idle_for("old.exe", 110_000, None, None);
        assert_eq!(idle, Some(Duration::from_secs(60)));
        // A clock that went back never underflows.
        assert_eq!(a.idle_for("old.exe", 10, None, None), Some(Duration::ZERO));
    }

    #[test]
    fn nothing_in_front_is_not_recorded() {
        let mut a = Activity::new(0);
        a.note_foreground(5_000, None);
        assert!(a.last_in_front.is_empty());
    }

    #[test]
    fn fullscreen_means_covering_the_monitor() {
        let monitor = rect(0, 0, 1920, 1080);
        assert!(covers_monitor(rect(0, 0, 1920, 1080), monitor));
        assert!(covers_monitor(rect(-8, -8, 1928, 1088), monitor));
        assert!(!covers_monitor(rect(0, 0, 1920, 1040), monitor), "taskbar");
        assert!(!covers_monitor(rect(100, 100, 900, 700), monitor));
    }

    #[test]
    fn real_readers_do_not_panic() {
        let _ = foreground_is_fullscreen();
        assert!(system_idle().is_some());
    }
}
