//! The notification-area (tray) icon: a live "memory in use" percentage, a tooltip and a small
//! menu. Everything is in memory; nothing is written to disk or the registry.
//!
//! The icon is drawn by [`digits_to_rgba`] with a tiny built-in 3x5 digit font, so no image
//! files or font code are needed.

use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use eframe::egui;
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

/// The icon is drawn at 32x32; Windows scales it down to the tray size.
const SIZE: usize = 32;
/// The icon is refreshed at most this often, and only if the number changed.
const REFRESH: Duration = Duration::from_secs(2);

/// What the user asked for through the tray.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Left click on the icon.
    Toggle,
    Show,
    Quit,
}

pub struct Tray {
    icon: TrayIcon,
    rx: Receiver<Action>,
    shown_percent: Option<u32>,
    last_refresh: Option<Instant>,
}

impl Tray {
    /// Creates the icon (or `None` if Windows refuses). Wakes `ctx` when the user clicks it.
    pub fn new(ctx: &egui::Context) -> Option<Self> {
        let (tx, rx) = mpsc::channel();

        let show = MenuItem::new("Show toolboks", true, None);
        let quit = MenuItem::new("Quit toolboks", true, None);
        let menu = Menu::new();
        menu.append(&show).ok()?;
        menu.append(&quit).ok()?;
        let (show_id, quit_id): (MenuId, MenuId) = (show.id().clone(), quit.id().clone());

        let (tx_menu, ctx_menu) = (tx.clone(), ctx.clone());
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            let action = if event.id == show_id {
                Action::Show
            } else if event.id == quit_id {
                Action::Quit
            } else {
                return;
            };
            let _ = tx_menu.send(action);
            ctx_menu.request_repaint();
        }));
        let ctx_click = ctx.clone();
        TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let _ = tx.send(Action::Toggle);
                ctx_click.request_repaint();
            }
        }));

        let percent = memory_percent();
        let icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .with_icon(make_icon(percent)?)
            .with_tooltip(tooltip_text(percent))
            .build()
            .ok()?;
        Some(Self {
            icon,
            rx,
            shown_percent: Some(percent),
            last_refresh: Some(Instant::now()),
        })
    }

    /// Shows or hides the icon (the icon itself is kept for the process lifetime).
    pub fn set_visible(&self, visible: bool) {
        let _ = self.icon.set_visible(visible);
    }

    /// Clicks and menu choices since the last call.
    pub fn actions(&self) -> Vec<Action> {
        self.rx.try_iter().collect()
    }

    /// Redraws the icon if 2 s passed and the percentage changed.
    pub fn refresh(&mut self) {
        if self.last_refresh.is_some_and(|t| t.elapsed() < REFRESH) {
            return;
        }
        self.last_refresh = Some(Instant::now());
        let percent = memory_percent();
        if self.shown_percent == Some(percent) {
            return;
        }
        self.shown_percent = Some(percent);
        if let Some(icon) = make_icon(percent) {
            let _ = self.icon.set_icon(Some(icon));
        }
        let _ = self.icon.set_tooltip(Some(tooltip_text(percent)));
    }
}

fn make_icon(percent: u32) -> Option<Icon> {
    Icon::from_rgba(digits_to_rgba(percent), SIZE as u32, SIZE as u32).ok()
}

/// Memory in use, 0 to 100, straight from Windows.
fn memory_percent() -> u32 {
    let mut status = MEMORYSTATUSEX {
        dwLength: size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    // SAFETY: `status` is a valid MEMORYSTATUSEX with `dwLength` set.
    if unsafe { GlobalMemoryStatusEx(&mut status) }.is_ok() {
        status.dwMemoryLoad.min(100)
    } else {
        0
    }
}

pub fn tooltip_text(percent: u32) -> String {
    format!("toolboks - memory {percent}% in use")
}

/// Same thresholds as Memory Home `health()`: green below 70, amber below 85, red above.
pub fn color_for_percent(percent: u32) -> [u8; 3] {
    if percent < 70 {
        [46, 160, 67]
    } else if percent < 85 {
        [214, 150, 20]
    } else {
        [210, 50, 50]
    }
}

/// 3x5 digit font, one row per `u8`, bit 2 = left pixel.
const FONT: [[u8; 5]; 10] = [
    [0b111, 0b101, 0b101, 0b101, 0b111],
    [0b010, 0b110, 0b010, 0b010, 0b111],
    [0b111, 0b001, 0b111, 0b100, 0b111],
    [0b111, 0b001, 0b111, 0b001, 0b111],
    [0b101, 0b101, 0b111, 0b001, 0b001],
    [0b111, 0b100, 0b111, 0b001, 0b111],
    [0b111, 0b100, 0b111, 0b101, 0b111],
    [0b111, 0b001, 0b001, 0b001, 0b001],
    [0b111, 0b101, 0b111, 0b101, 0b111],
    [0b111, 0b101, 0b111, 0b001, 0b111],
];

/// A `SIZE` x `SIZE` RGBA image: a square in the health color with the two digits of
/// `percent` on it (100 and above show as 99).
pub fn digits_to_rgba(percent: u32) -> Vec<u8> {
    let [r, g, b] = color_for_percent(percent);
    // Dark digits on amber, white on the others, for contrast.
    let ink: [u8; 3] = if (70..85).contains(&percent) {
        [20, 20, 20]
    } else {
        [255, 255, 255]
    };
    let shown = percent.min(99) as usize;
    let digits = [shown / 10, shown % 10];

    const SCALE: usize = 4;
    // Two digits of 3 columns and a 1 column gap, centered.
    let width = (3 + 1 + 3) * SCALE;
    let height = 5 * SCALE;
    let (left, top) = ((SIZE - width) / 2, (SIZE - height) / 2);

    let mut rgba = vec![0u8; SIZE * SIZE * 4];
    for (i, px) in rgba.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let (x, y) = (i % SIZE, i / SIZE);
        let mut c = [r, g, b];
        if x >= left && x < left + width && y >= top && y < top + height {
            let (cx, cy) = ((x - left) / SCALE, (y - top) / SCALE);
            // Column 3 is the gap between the digits.
            let on = match cx {
                0..=2 => (FONT[digits[0]][cy] >> (2 - cx)) & 1 == 1,
                4..=6 => (FONT[digits[1]][cy] >> (6 - cx)) & 1 == 1,
                _ => false,
            };
            if on {
                c = ink;
            }
        }
        px.copy_from_slice(&[c[0], c[1], c[2], 255]);
    }
    rgba
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_follow_the_health_thresholds() {
        assert_eq!(color_for_percent(0), color_for_percent(69));
        assert_ne!(color_for_percent(69), color_for_percent(70));
        assert_eq!(color_for_percent(70), color_for_percent(84));
        assert_ne!(color_for_percent(84), color_for_percent(85));
        assert_eq!(color_for_percent(85), color_for_percent(100));
    }

    #[test]
    fn tooltip_is_plain() {
        assert_eq!(tooltip_text(58), "toolboks - memory 58% in use");
    }

    #[test]
    fn image_has_the_right_size_and_is_opaque() {
        let img = digits_to_rgba(58);
        assert_eq!(img.len(), SIZE * SIZE * 4);
        assert!(img.as_chunks::<4>().0.iter().all(|p| p[3] == 255));
    }

    #[test]
    fn different_numbers_draw_differently_and_100_shows_as_99() {
        assert_ne!(digits_to_rgba(58), digits_to_rgba(59));
        assert_eq!(digits_to_rgba(99), digits_to_rgba(100));
    }

    #[test]
    fn digits_are_drawn_in_the_ink_color() {
        let img = digits_to_rgba(10);
        let bg = color_for_percent(10);
        let ink_pixels = img
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[..3] != bg[..])
            .count();
        assert!(ink_pixels > 0);
    }
}
