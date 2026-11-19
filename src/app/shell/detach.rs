//! Add-ons in a window of their own. Dragging an add-on's sidebar row past the edge of the Toolboks
//! window opens the add-on as a separate OS window; closing that window puts the add-on back in
//! the sidebar, at the place it had.
//!
//! The add-on is not restarted: the window draws the same [`crate::addon_view::AddonView`] over
//! the same link (an egui immediate viewport, drawn inside the main window's `ui` pass; eframe
//! keeps running that pass while the main window is minimized and a detached one is visible).
//! While it is out, its row is hidden from the sidebar and it counts as on screen for the hide
//! rule. Nothing about it is saved: a restart starts with every add-on in the sidebar.

use super::*;
use std::collections::BTreeMap;

/// The size a detached window opens at, and the smallest it can be made, in points.
pub(in crate::app) const WINDOW_SIZE: [f32; 2] = [960.0, 640.0];
pub(in crate::app) const MIN_SIZE: [f32; 2] = [480.0, 360.0];

/// How close to the window's edge, in points, the pointer counts as past it. The outermost point
/// counts, so a maximized window (whose edges are the screen's) can still give an add-on away.
const EDGE: f32 = 1.0;

/// Where the new window's outer corner goes, relative to the pointer: its title bar lands under
/// the pointer that let go.
const OPEN_OFFSET: Vec2 = Vec2::new(-120.0, -14.0);

/// One add-on in its own window.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::app) struct Detached {
    /// The add-on page the window shows.
    pub page: usize,
    /// The window's outer corner on the screen, in points. `None` lets Windows choose.
    pub at: Option<egui::Pos2>,
    /// Bring the window to the front in its next pass (something asked to open the add-on).
    pub focus: bool,
}

/// Is `pos` (window points) past the edge of a window whose content is `window`?
pub(super) fn outside(window: egui::Rect, pos: egui::Pos2) -> bool {
    pos.x < window.min.x + EDGE
        || pos.x >= window.max.x - EDGE
        || pos.y < window.min.y + EDGE
        || pos.y >= window.max.y - EDGE
}

/// Where the pointer of a sidebar drag is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Place {
    /// In the window: the drag re-orders, as it always did.
    Inside(egui::Pos2),
    /// Past the window's edge: letting go opens the add-on in its own window.
    Outside(egui::Pos2),
    /// No position is known (the pointer left before any was seen).
    Unknown,
}

/// The place of the pointer: its latest position, or the last one seen in this drag when the
/// window has none now (Windows reported the pointer gone, for example right before the release).
pub(super) fn place(
    window: egui::Rect,
    latest: Option<egui::Pos2>,
    seen: Option<egui::Pos2>,
) -> Place {
    match latest.or(seen) {
        None => Place::Unknown,
        Some(pos) if outside(window, pos) => Place::Outside(pos),
        Some(pos) => Place::Inside(pos),
    }
}

/// Has the drag ended? egui says so when it saw the button go up. Outside the window the release
/// can get lost (egui-winit drops a release that comes with no pointer position), so there the
/// button's real state counts too.
pub(super) fn released(egui_down: bool, outside: bool, button_up: bool) -> bool {
    !egui_down || (outside && button_up)
}

/// Where the new window's outer corner goes (screen points) for a release at `pos` in a window
/// whose inner area is `inner`: its title bar lands under the pointer. `None` without `inner`.
pub(super) fn window_corner(inner: Option<egui::Rect>, pos: egui::Pos2) -> Option<egui::Pos2> {
    inner.map(|inner| inner.min + pos.to_vec2() + OPEN_OFFSET)
}

/// Does this add-on count as shown (not to be hidden or suspended)? Its page in the main window,
/// or its own window.
pub(in crate::app) fn on_screen(
    screen: &Screen,
    detached: &BTreeMap<String, Detached>,
    id: &str,
) -> bool {
    matches!(screen, Screen::External(shown) if shown == id) || detached.contains_key(id)
}

/// After a stop the supervisor reports: does the add-on go back to the sidebar? Only for a stop
/// that lasts. A host stop, an add-on that closed itself or a declined administrator prompt start
/// again by themselves while the add-on is shown.
pub(in crate::app) fn back_on_stop(reason: &crate::supervisor::StopReason) -> bool {
    use crate::supervisor::StopReason;
    !matches!(
        reason,
        StopReason::ByHost | StopReason::Closed | StopReason::Declined
    )
}

/// The main window's page after the add-on it showed went into its own window: the page it was
/// opened from, unless that is gone too (Home then).
pub(in crate::app) fn back_to(
    came_from: Option<&Screen>,
    id: &str,
    detached: &BTreeMap<String, Detached>,
) -> Screen {
    match came_from {
        Some(Screen::External(other)) if other == id || detached.contains_key(other) => {
            Screen::Home
        }
        Some(screen) => screen.clone(),
        None => Screen::Home,
    }
}

/// The window's id, one per add-on.
pub(in crate::app) fn viewport_id(id: &str) -> egui::ViewportId {
    egui::ViewportId::from_hash_of(("toolboks-detached", id))
}

/// The real state of the primary mouse button: true when it is up. Asked only while a drag is
/// past the window's edge. Never in tests, where the real mouse has nothing to do with the run.
#[cfg(all(windows, not(test)))]
pub(super) fn primary_button_up() -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON};
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_SWAPBUTTON};
    // GetAsyncKeyState reads the physical buttons; with swapped buttons the primary is the right.
    let key = if unsafe { GetSystemMetrics(SM_SWAPBUTTON) } != 0 {
        VK_RBUTTON
    } else {
        VK_LBUTTON
    };
    (unsafe { GetAsyncKeyState(i32::from(key.0)) } as u16) & 0x8000 == 0
}

#[cfg(not(all(windows, not(test))))]
pub(super) fn primary_button_up() -> bool {
    false
}

/// The Toolboks icon for the detached windows' title bar and taskbar button, read once.
fn app_icon() -> Option<std::sync::Arc<egui::IconData>> {
    static ICON: std::sync::OnceLock<Option<std::sync::Arc<egui::IconData>>> =
        std::sync::OnceLock::new();
    ICON.get_or_init(|| {
        eframe::icon_data::from_png_bytes(include_bytes!("../../../assets/app-icon.png"))
            .ok()
            .map(std::sync::Arc::new)
    })
    .clone()
}

impl App {
    /// The add-on's title and its page names, for the window title and the page switch.
    fn addon_title_pages(&self, id: &str) -> (String, Vec<String>) {
        if let Some(package) = self.registry.packages.iter().find(|p| p.manifest.id == id) {
            return (
                package.manifest.title.clone(),
                package
                    .pages
                    .iter()
                    .map(|page| page.title.clone())
                    .collect(),
            );
        }
        crate::installer::installed_entry(&self.registry.root, id)
            .map_or_else(|| (id.to_owned(), Vec::new()), |e| (e.title, e.pages))
    }

    /// Remembers which page the main window came from, and keeps a detached add-on out of the main
    /// window: anything that opens it there (a link, a Home finding, the add-on browser) brings
    /// its own window to the front instead. Called at the start of every main-window pass.
    pub(in crate::app) fn note_screen(&mut self) {
        if let Screen::External(id) = &self.screen {
            if let Some(window) = self.detached.get_mut(id) {
                window.focus = true;
                self.screen = back_to(self.came_from.as_ref(), id, &self.detached);
            }
        }
        if self.screen != self.shown_screen {
            if matches!(self.screen, Screen::External(_)) {
                self.came_from = Some(self.shown_screen.clone());
            }
            self.shown_screen = self.screen.clone();
        }
    }

    /// Opens the add-on in its own window at `at` (outer corner, screen points). An add-on that is
    /// already out is brought to the front; one that is not installed or is turned off stays.
    pub(in crate::app) fn detach_addon(&mut self, id: &str, at: Option<egui::Pos2>) {
        if let Some(window) = self.detached.get_mut(id) {
            window.focus = true;
            return;
        }
        if self.installed_version(id).is_none() || !self.registry.enabled(id) {
            return;
        }
        let page = if self.screen == Screen::External(id.to_owned()) {
            // It leaves the main window, which goes back to where it was before.
            let page = self.addon_page;
            self.screen = back_to(self.came_from.as_ref(), id, &self.detached);
            self.shown_screen = self.screen.clone();
            page
        } else {
            // A running add-on (a background one) may be on any page: show its first, as a
            // click on its row would.
            crate::addon_view::navigate(self.views.get_mut(id), "0".into(), |message| {
                self.supervisor
                    .as_ref()
                    .is_some_and(|supervisor| supervisor.send(id, message))
            });
            0
        };
        self.detached.insert(
            id.to_owned(),
            Detached {
                page,
                at,
                focus: false,
            },
        );
    }

    /// Closes the add-on's window; its row is back in the sidebar where it was. Returns whether it
    /// was out.
    pub(in crate::app) fn redock(&mut self, id: &str) -> bool {
        self.detached.remove(id).is_some()
    }

    /// Brings back every add-on that can no longer be shown on its own: removed, turned off, or
    /// being removed.
    pub(in crate::app) fn redock_gone(&mut self) {
        if self.detached.is_empty() {
            return;
        }
        let gone: Vec<String> = self
            .detached
            .keys()
            .filter(|id| {
                self.installed_version(id).is_none()
                    || !self.registry.enabled(id)
                    || self
                        .market
                        .remove
                        .as_ref()
                        .is_some_and(|(removing, _)| removing == *id)
            })
            .cloned()
            .collect();
        for id in gone {
            self.redock(&id);
        }
    }

    /// Draws every detached add-on's window. From `ui`, after the main window.
    pub(in crate::app) fn detached_windows(&mut self, ctx: &egui::Context) {
        self.redock_gone();
        let ids: Vec<String> = self.detached.keys().cloned().collect();
        for id in ids {
            let Some(window) = self.detached.get(&id).cloned() else {
                continue;
            };
            let (title, pages) = self.addon_title_pages(&id);
            let mut builder = egui::ViewportBuilder::default()
                .with_title(title.clone())
                .with_inner_size(WINDOW_SIZE)
                .with_min_inner_size(MIN_SIZE)
                // Hidden with the main window when it goes to the tray, back with it.
                .with_visible(!self.hidden);
            if let Some(at) = window.at {
                builder = builder.with_position(at);
            }
            if let Some(icon) = app_icon() {
                builder = builder.with_icon(icon);
            }
            let viewport = viewport_id(&id);
            if window.focus && !self.hidden {
                ctx.send_viewport_cmd_to(viewport, egui::ViewportCommand::Minimized(false));
                ctx.send_viewport_cmd_to(viewport, egui::ViewportCommand::Focus);
                if let Some(window) = self.detached.get_mut(&id) {
                    window.focus = false;
                }
            }
            let close = ctx.show_viewport_immediate(viewport, builder, |ui, _class| {
                if !self.hidden {
                    self.detached_ui(ui, &id, &title, &pages);
                    toolboks_addon_sdk::copy_hover::host_pass(ui.ctx());
                }
                ui.ctx().input(|i| i.viewport().close_requested())
            });
            if close {
                self.redock(&id);
            }
        }
    }

    /// One detached window: a header with the add-on's name and its pages, and the add-on.
    fn detached_ui(&mut self, ui: &mut egui::Ui, id: &str, title: &str, pages: &[String]) {
        let Some(mut page) = self.detached.get(id).map(|window| window.page) else {
            return;
        };
        let mut chosen = None;
        egui::Panel::top("detached-header")
            .exact_size(HEADER_H)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(WINDOW_BG)
                    .inner_margin(egui::Margin::symmetric(16, 0)),
            )
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    let size = look::TEXT_SECTION;
                    match pages.get(page).filter(|_| pages.len() > 1) {
                        Some(leaf) => {
                            ui.label(RichText::new(title).size(size).color(MUTED));
                            ui.label(RichText::new("/").size(size).color(MUTED));
                            ui.label(RichText::new(leaf).size(size).strong().color(TEXT));
                        }
                        None => {
                            ui.label(RichText::new(title).size(size).strong().color(TEXT));
                        }
                    }
                    if pages.len() > 1 {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if pills_width(ui, pages) > ui.available_width() {
                                egui::ComboBox::from_id_salt("detached-page-selector")
                                    .selected_text("Pages")
                                    .show_ui(ui, |ui| {
                                        for (i, name) in pages.iter().enumerate() {
                                            if ui.selectable_label(page == i, name).clicked() {
                                                chosen = Some(i);
                                            }
                                        }
                                    });
                            } else {
                                for (i, name) in pages.iter().enumerate().rev() {
                                    if look::pill(ui, name, page == i).clicked() {
                                        chosen = Some(i);
                                    }
                                }
                            }
                        });
                    }
                });
            });
        if let Some(index) = chosen.filter(|index| *index != page) {
            page = index;
            if !self.registry.packages.iter().any(|p| p.manifest.id == id) {
                crate::addon_view::navigate(self.views.get_mut(id), index.to_string(), |message| {
                    self.supervisor
                        .as_ref()
                        .is_some_and(|supervisor| supervisor.send(id, message))
                });
            }
        }
        let mut shown = true;
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(WINDOW_BG)
                    .inner_margin(egui::Margin::symmetric(16, 16)),
            )
            .show(ui, |ui| {
                shown = self.addon_workspace(ui, id, &mut page);
            });
        if let Some(window) = self.detached.get_mut(id) {
            window.page = page;
        }
        if !shown {
            self.redock(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window() -> egui::Rect {
        egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(1000.0, 700.0))
    }

    #[test]
    fn past_the_edge_is_outside_and_the_outermost_point_counts() {
        let w = window();
        assert!(!outside(w, egui::pos2(26.0, 100.0)), "in the sidebar");
        assert!(!outside(w, egui::pos2(500.0, 350.0)));
        assert!(outside(w, egui::pos2(-30.0, 100.0)), "left of the window");
        assert!(outside(w, egui::pos2(1040.0, 100.0)), "right of it");
        assert!(outside(w, egui::pos2(200.0, -5.0)), "above (title bar)");
        assert!(outside(w, egui::pos2(200.0, 760.0)), "below");
        // A maximized window: the pointer stops at the screen's edge pixel.
        assert!(outside(w, egui::pos2(0.0, 100.0)));
        assert!(outside(w, egui::pos2(999.4, 100.0)));
    }

    #[test]
    fn the_place_uses_the_last_seen_position_when_none_is_left() {
        let w = window();
        let inside = egui::pos2(30.0, 40.0);
        let out = egui::pos2(-50.0, 40.0);
        assert_eq!(place(w, Some(inside), None), Place::Inside(inside));
        assert_eq!(place(w, Some(out), Some(inside)), Place::Outside(out));
        // Windows said the pointer was gone: the last position seen decides.
        assert_eq!(place(w, None, Some(out)), Place::Outside(out));
        assert_eq!(place(w, None, None), Place::Unknown);
    }

    #[test]
    fn a_lost_release_outside_still_ends_the_drag() {
        // egui saw the release.
        assert!(released(false, false, false));
        assert!(released(false, true, true));
        // Inside, only egui decides.
        assert!(!released(true, false, true));
        // Outside, the real button decides when egui still thinks it is down.
        assert!(released(true, true, true));
        assert!(!released(true, true, false));
    }

    #[test]
    fn the_window_opens_under_the_pointer_in_screen_points() {
        let inner = egui::Rect::from_min_size(egui::pos2(100.0, 50.0), Vec2::new(1000.0, 700.0));
        assert_eq!(
            window_corner(Some(inner), egui::pos2(1100.0, 200.0)),
            Some(egui::pos2(100.0 + 1100.0, 50.0 + 200.0) + OPEN_OFFSET)
        );
        assert_eq!(window_corner(None, egui::pos2(1.0, 1.0)), None);
    }

    #[test]
    fn a_detached_add_on_counts_as_on_screen() {
        let mut detached = BTreeMap::new();
        detached.insert(
            "a".to_owned(),
            Detached {
                page: 0,
                at: None,
                focus: false,
            },
        );
        assert!(on_screen(&Screen::Home, &detached, "a"));
        assert!(on_screen(&Screen::External("b".into()), &detached, "b"));
        assert!(!on_screen(&Screen::Home, &detached, "b"));
        // The hide rule with it: shown unless the whole app is in the tray.
        use crate::app::addon_tick::hide_addon;
        assert!(!hide_addon(on_screen(&Screen::Home, &detached, "a"), false));
        assert!(hide_addon(on_screen(&Screen::Home, &detached, "a"), true));
        assert!(hide_addon(on_screen(&Screen::Home, &detached, "c"), false));
    }

    #[test]
    fn only_a_lasting_stop_brings_the_add_on_back() {
        use crate::supervisor::StopReason;
        assert!(!back_on_stop(&StopReason::ByHost));
        assert!(!back_on_stop(&StopReason::Closed));
        assert!(!back_on_stop(&StopReason::Declined));
        assert!(back_on_stop(&StopReason::Crashed { exit_code: Some(3) }));
        assert!(back_on_stop(&StopReason::WrongFile));
        assert!(back_on_stop(&StopReason::Missing("gone".into())));
        assert!(back_on_stop(&StopReason::Refused("old".into())));
    }

    #[test]
    fn the_main_window_goes_back_where_it_came_from() {
        let mut detached = BTreeMap::new();
        assert!(back_to(Some(&Screen::Settings), "a", &detached) == Screen::Settings);
        assert!(back_to(None, "a", &detached) == Screen::Home);
        assert!(back_to(Some(&Screen::External("a".into())), "a", &detached) == Screen::Home);
        assert!(
            back_to(Some(&Screen::External("b".into())), "a", &detached)
                == Screen::External("b".into())
        );
        detached.insert(
            "b".to_owned(),
            Detached {
                page: 0,
                at: None,
                focus: false,
            },
        );
        assert!(back_to(Some(&Screen::External("b".into())), "a", &detached) == Screen::Home);
    }
}
