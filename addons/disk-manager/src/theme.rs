//! Design tokens and small helpers for Disk Manager, mirroring DESIGN.md.
//!
//! The add-on crate cannot import the host's `src/ui.rs`, so the token values are
//! repeated here. Keep them identical to DESIGN.md (Tokens) and `ui::apply_style`;
//! change them there first. Nothing here draws its own palette, font or icon style.
use egui::{Color32, RichText, Vec2};

pub const WINDOW: Color32 = Color32::from_rgb(0x0e, 0x0f, 0x11);
pub const CARD: Color32 = Color32::from_rgb(0x15, 0x16, 0x1a);
pub const HAIRLINE: Color32 = Color32::from_rgb(0x24, 0x26, 0x2c);
pub const TEXT: Color32 = Color32::from_rgb(0xe8, 0xe9, 0xec);
pub const MUTED: Color32 = Color32::from_rgb(0x9a, 0x9e, 0xa8);
pub const ACCENT: Color32 = Color32::from_rgb(0x8b, 0x5c, 0xf6);

/// Data colours used for ranked tiles and bars. Always shown next to a label.
pub const DATA: [Color32; 6] = [
    Color32::from_rgb(0x8b, 0x5c, 0xf6),
    Color32::from_rgb(0xb3, 0x9d, 0xfa),
    Color32::from_rgb(0x3e, 0xcf, 0x8e),
    Color32::from_rgb(0xf0, 0xa8, 0x3a),
    Color32::from_rgb(0x45, 0xb8, 0xf0),
    Color32::from_rgb(0x9a, 0x9e, 0xa8),
];

pub const SPACE_S: f32 = 4.0;
pub const SPACE_M: f32 = 8.0;
pub const SPACE_L: f32 = 12.0;
pub const SPACE_XL: f32 = 16.0;

/// Card: 10 px radius, 1 px hairline, no shadow, no coloured edge.
pub fn card() -> egui::Frame {
    egui::Frame::new()
        .fill(CARD)
        .stroke(egui::Stroke::new(1.0, HAIRLINE))
        .corner_radius(10)
        .inner_margin(12)
}

/// Page title: 18 px, once per page.
pub fn title(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(18.0).strong());
}

/// Section heading inside a card: 14 px.
pub fn section(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(14.0).strong());
}

/// Muted body text.
pub fn weak(ui: &mut egui::Ui, text: impl Into<String>) {
    ui.label(RichText::new(text.into()).color(MUTED));
}

/// Muted 11 px caption.
pub fn caption(ui: &mut egui::Ui, text: impl Into<String>) {
    ui.label(RichText::new(text.into()).size(11.0).color(MUTED));
}

/// Inline stat: value in strong text, then a small muted label.
pub fn stat(ui: &mut egui::Ui, value: impl Into<String>, label: &str) {
    ui.label(RichText::new(value.into()).strong());
    ui.label(RichText::new(label).size(11.0).color(MUTED));
    ui.add_space(SPACE_M);
}

/// Primary button: accent fill, one per view.
pub fn primary(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(label).color(WINDOW).strong())
            .fill(ACCENT)
            .corner_radius(6)
            .min_size(Vec2::new(75.0, 28.0)),
    )
}

/// Secondary button: outlined, no fill.
pub fn outlined(label: &str) -> egui::Button<'_> {
    egui::Button::new(label)
        .fill(Color32::TRANSPARENT)
        .stroke(egui::Stroke::new(1.0, HAIRLINE))
        .corner_radius(6)
}

/// One-line, muted, dismissible banner. Returns true when dismissed.
pub fn banner(ui: &mut egui::Ui, text: &str) -> bool {
    let mut dismissed = false;
    card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            weak(ui, text);
            if ui.small_button("Dismiss").clicked() {
                dismissed = true;
            }
        });
    });
    dismissed
}

/// Table header cell text: 11 px muted, sentence case.
pub fn header_cell(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(11.0).color(MUTED));
}

pub fn number_cell(ui: &mut egui::Ui, contents: impl FnOnce(&mut egui::Ui)) {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), contents);
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOST_UI: &str = include_str!("../../../src/ui.rs");
    const SDK_THEME: &str = include_str!("../../../sdk/theme.json");

    fn parse_num(s: &str) -> u8 {
        let s = s.trim();
        match s.strip_prefix("0x") {
            Some(h) => u8::from_str_radix(h, 16).expect("hex channel"),
            None => s.parse().expect("decimal channel"),
        }
    }

    /// Resolve a host `pub const NAME: Color32 = ...;` to RGB, following aliases.
    fn host_rgb(name: &str) -> [u8; 3] {
        let needle = format!("pub const {name}: Color32 =");
        let start = HOST_UI
            .find(&needle)
            .unwrap_or_else(|| panic!("{name} not found in src/ui.rs"));
        let rest = &HOST_UI[start + needle.len()..];
        let expr = rest[..rest.find(';').expect("const ends")].trim();
        match expr.strip_prefix("Color32::from_rgb(") {
            Some(args) => {
                let ch: Vec<u8> = args
                    .trim_end_matches(')')
                    .split(',')
                    .map(parse_num)
                    .collect();
                assert_eq!(ch.len(), 3, "{name}");
                [ch[0], ch[1], ch[2]]
            }
            None => host_rgb(expr),
        }
    }

    fn rgb(c: Color32) -> [u8; 3] {
        [c.r(), c.g(), c.b()]
    }

    fn hex(s: &str) -> [u8; 3] {
        let s = s.trim_start_matches('#');
        assert!(s.len() >= 6, "bad hex {s}");
        let p = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).expect("hex");
        [p(0), p(2), p(4)]
    }

    /// Value of the first `"key": "#hex"` in sdk/theme.json (keys used here are unique).
    fn json_rgb(key: &str) -> [u8; 3] {
        let needle = format!("\"{key}\"");
        let start = SDK_THEME
            .find(&needle)
            .unwrap_or_else(|| panic!("{key} not found in sdk/theme.json"));
        let rest = &SDK_THEME[start + needle.len()..];
        let q1 = rest.find('"').expect("value open") + 1;
        let q2 = q1 + rest[q1..].find('"').expect("value close");
        hex(&rest[q1..q2])
    }

    #[test]
    fn addon_tokens_match_host() {
        let pairs: [(&str, Color32); 6] = [
            ("WINDOW_BG", WINDOW),
            ("PANEL_BG", CARD),
            ("BORDER", HAIRLINE),
            ("TEXT", TEXT),
            ("MUTED", MUTED),
            ("ACCENT", ACCENT),
        ];
        for (host, ours) in pairs {
            assert_eq!(
                rgb(ours),
                host_rgb(host),
                "theme.rs drifted from ui::{host}"
            );
        }
        let data = [
            "DATA_CPU",
            "DATA_MEMORY",
            "DATA_GPU",
            "DATA_DISK",
            "DATA_NETWORK",
            "DATA_SECOND",
        ];
        for (i, host) in data.iter().enumerate() {
            assert_eq!(
                rgb(DATA[i]),
                host_rgb(host),
                "DATA[{i}] drifted from ui::{host}"
            );
        }
    }

    #[test]
    fn sdk_theme_json_matches_host() {
        let pairs = [
            ("background", "WINDOW_BG"),
            ("sidebar", "SIDEBAR_BG"),
            ("panel", "PANEL_BG"),
            ("status", "STATUS_BG"),
            ("border", "BORDER"),
            ("text", "TEXT"),
            ("muted", "MUTED"),
            ("host_accent", "HOST_ACCENT"),
            ("selected_row", "SELECTED_BG"),
            ("bar_divider", "BAR_DIVIDER"),
            ("cpu", "DATA_CPU"),
            ("memory", "DATA_MEMORY"),
            ("gpu", "DATA_GPU"),
            ("disk", "DATA_DISK"),
            ("network", "DATA_NETWORK"),
            ("second_line", "DATA_SECOND"),
            ("ok", "GREEN"),
            ("watch", "AMBER"),
            ("problem", "RED"),
        ];
        for (key, host) in pairs {
            assert_eq!(
                json_rgb(key),
                host_rgb(host),
                "sdk/theme.json {key} drifted from ui::{host}"
            );
        }
    }
}
