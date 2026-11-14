//! Turns host egui events into wire events relative to the page rectangle.
//!
//! Positions are in points, so they do not depend on the display scale: the add-on gets
//! `pixels_per_point` in the batch and tessellates for it. A drag that starts inside the page
//! keeps the pointer captured: moves outside are still forwarded and the release outside
//! arrives as [`InputEvent::DragReleasedOutside`].

use eframe::egui::{self, Event, Pos2, Rect};
use toolboks_addon_proto::InputEvent;

/// What one pass of events did to the pointer, for focus handling.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Routed {
    pub pressed_inside: bool,
    pub pressed_outside: bool,
}

#[derive(Default)]
pub(super) struct InputRouter {
    /// The pointer was inside the page at the last pointer event.
    hovered: bool,
    /// The button of a drag that started inside the page.
    capture: Option<egui::PointerButton>,
}

fn relative(pos: Pos2, rect: Rect) -> [f32; 2] {
    [pos.x - rect.min.x, pos.y - rect.min.y]
}

impl InputRouter {
    pub fn hovered(&self) -> bool {
        self.hovered
    }

    pub fn captured(&self) -> bool {
        self.capture.is_some()
    }

    /// Pointer presses in `events`, without routing anything. Focus is settled from this
    /// before keyboard events are routed.
    pub fn presses(events: &[Event], rect: Rect) -> Routed {
        let mut routed = Routed::default();
        for event in events {
            if let Event::PointerButton {
                pos, pressed: true, ..
            } = event
            {
                if rect.contains(*pos) {
                    routed.pressed_inside = true;
                } else {
                    routed.pressed_outside = true;
                }
            }
        }
        routed
    }

    /// Routes `events` into `out`. Keyboard, text, clipboard and IME events go only while the
    /// page has keyboard focus.
    pub fn route(
        &mut self,
        events: &[Event],
        rect: Rect,
        focused: bool,
        out: &mut Vec<InputEvent>,
    ) {
        for event in events {
            match event {
                Event::PointerMoved(pos) => {
                    let inside = rect.contains(*pos);
                    if inside || self.capture.is_some() {
                        out.push(InputEvent::PointerMoved {
                            pos: relative(*pos, rect),
                        });
                    } else if self.hovered {
                        out.push(InputEvent::PointerLeft);
                    }
                    self.hovered = inside;
                }
                Event::PointerGone => {
                    if self.hovered && self.capture.is_none() {
                        out.push(InputEvent::PointerLeft);
                    }
                    self.hovered = false;
                }
                Event::PointerButton {
                    pos,
                    button,
                    pressed,
                    modifiers,
                } => {
                    let inside = rect.contains(*pos);
                    let pos_rel = relative(*pos, rect);
                    if *pressed {
                        if inside {
                            self.capture.get_or_insert(*button);
                            out.push(InputEvent::PointerButton {
                                pos: pos_rel,
                                button: *button,
                                pressed: true,
                                modifiers: *modifiers,
                            });
                        }
                    } else if self.capture == Some(*button) {
                        self.capture = None;
                        if inside {
                            out.push(InputEvent::PointerButton {
                                pos: pos_rel,
                                button: *button,
                                pressed: false,
                                modifiers: *modifiers,
                            });
                        } else {
                            // The add-on ends the drag and treats the pointer as gone.
                            self.hovered = false;
                            out.push(InputEvent::DragReleasedOutside {
                                pos: pos_rel,
                                button: *button,
                                modifiers: *modifiers,
                            });
                        }
                    } else if inside {
                        out.push(InputEvent::PointerButton {
                            pos: pos_rel,
                            button: *button,
                            pressed: false,
                            modifiers: *modifiers,
                        });
                    }
                }
                Event::MouseWheel {
                    unit,
                    delta,
                    modifiers,
                    phase,
                    source,
                } if self.hovered => out.push(InputEvent::Scroll {
                    delta: [delta.x, delta.y],
                    unit: *unit,
                    phase: *phase,
                    source: *source,
                    modifiers: *modifiers,
                }),
                Event::Zoom(factor) if self.hovered => out.push(InputEvent::Zoom(*factor)),
                _ if !focused => {}
                Event::Key {
                    key,
                    physical_key,
                    pressed,
                    repeat,
                    modifiers,
                } => out.push(InputEvent::Key {
                    key: *key,
                    physical_key: *physical_key,
                    pressed: *pressed,
                    repeat: *repeat,
                    modifiers: *modifiers,
                }),
                Event::Text(text) => out.push(InputEvent::Text(text.clone())),
                Event::Paste(text) => out.push(InputEvent::Paste(text.clone())),
                Event::Copy => out.push(InputEvent::Copy),
                Event::Cut => out.push(InputEvent::Cut),
                Event::Ime(egui::ImeEvent::Preedit { text, .. }) => {
                    out.push(InputEvent::ImePreedit(text.clone()))
                }
                Event::Ime(egui::ImeEvent::Commit(text)) => {
                    out.push(InputEvent::ImeCommit(text.clone()))
                }
                Event::Ime(egui::ImeEvent::DeleteSurrounding {
                    before_chars,
                    after_chars,
                }) => out.push(InputEvent::ImeDeleteSurrounding {
                    before_chars: u32::try_from(*before_chars).unwrap_or(u32::MAX),
                    after_chars: u32::try_from(*after_chars).unwrap_or(u32::MAX),
                }),
                _ => {}
            }
        }
    }
}
