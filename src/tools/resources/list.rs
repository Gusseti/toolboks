//! The Overview resource list: one row per resource with a small line, its name, one plain detail
//! line and the current value. The selected row has a lighter fill.

use std::borrow::Cow;

use eframe::egui::{self, Color32, FontId, Pos2, Sense, Shape, Stroke, Vec2};

use super::overview::{peak_ceiling, DISK_FLOOR, GPU_MISSING_AFTER, NET_FLOOR, NONE};
use super::{power, Resource, Resources};
use crate::format::{clock, rate, size_plain};
use crate::ui::{
    BAR_DIVIDER, DATA_CPU, DATA_DISK, DATA_GPU, DATA_MEMORY, DATA_NETWORK, DATA_SECOND, MUTED,
    SELECTED_BG, SPACE_M, SPACE_XS, TEXT, TEXT_BODY, TEXT_SECTION,
};

const ROW_HEIGHT: f32 = 56.0;
const ROW_RADIUS: f32 = 8.0;
const SPARK_W: f32 = 60.0;
const SPARK_H: f32 = 24.0;

/// What one row shows.
struct Row<'a> {
    resource: Resource,
    name: &'static str,
    detail: String,
    value: String,
    color: Color32,
    values: Cow<'a, [f32]>,
    /// The value at the top of the small line.
    max: f32,
}

/// "Mostly reading 1.2 MB/s"-style line for two rates; names the bigger one.
fn dominant(a: (&str, f64), b: (&str, f64)) -> String {
    let (name, v) = if b.1 > a.1 { b } else { a };
    format!("{name} {}", rate(v))
}

impl Resources {
    fn list_rows(&self) -> Vec<Row<'_>> {
        let range = self.range;
        let s = self.last.as_ref();
        let g = self.gpu.as_ref();
        let total_mem = self.info.as_ref().map_or(0, |i| i.mem_total);

        let cpu_detail = match (s, self.info.as_ref()) {
            (Some(s), Some(i)) => format!("{}, {}", clock(s.cpu_mhz), short_cpu_name(&i.cpu_name)),
            (Some(s), None) => clock(s.cpu_mhz),
            (None, Some(i)) => short_cpu_name(&i.cpu_name),
            (None, None) => String::new(),
        };
        let cpu = Row {
            resource: Resource::Cpu,
            name: "CPU",
            detail: cpu_detail,
            value: s.map_or(NONE.to_string(), |s| format!("{:.0}%", s.cpu)),
            color: DATA_CPU,
            values: Cow::Borrowed(self.cpu.view(range)),
            max: 100.0,
        };

        let mem = Row {
            resource: Resource::Memory,
            name: "Memory",
            detail: match (s, total_mem) {
                (Some(s), t) if t > 0 => {
                    format!("{} of {}", size_plain(s.mem_used), size_plain(t))
                }
                (Some(s), _) => size_plain(s.mem_used),
                _ => String::new(),
            },
            value: self
                .mem
                .last()
                .map_or(NONE.to_string(), |p| format!("{p:.0}%")),
            color: DATA_MEMORY,
            values: Cow::Borrowed(self.mem.view(range)),
            max: 100.0,
        };

        let gpu_detail = match g {
            Some(g) => match g.temp_c {
                Some(t) => format!("{}, {t:.0} °C", g.name),
                None => g.name.clone(),
            },
            None if self.gpu_missing >= GPU_MISSING_AFTER => "No GPU found".to_string(),
            None => "Reading GPU...".to_string(),
        };
        let gpu = Row {
            resource: Resource::Gpu,
            name: "GPU",
            detail: gpu_detail,
            value: g
                .and_then(|g| g.load)
                .map_or(NONE.to_string(), |l| format!("{l:.0}%")),
            color: DATA_GPU,
            values: Cow::Borrowed(self.gpu_load.view(range)),
            max: 100.0,
        };

        let (read, write) = s.map_or((0.0, 0.0), |s| (s.disk_read_bps, s.disk_write_bps));
        let disk = Row {
            resource: Resource::Disk,
            name: "Disk",
            detail: if s.is_some() {
                dominant(("Reading", read), ("Writing", write))
            } else {
                String::new()
            },
            value: s.map_or(NONE.to_string(), |_| rate(read + write)),
            color: DATA_DISK,
            values: Cow::Borrowed(self.disk_read.view(range)),
            max: peak_ceiling(
                &[self.disk_read.view(range), self.disk_write.view(range)],
                DISK_FLOOR,
            ),
        };

        let (down, up) = s.map_or((0.0, 0.0), |s| (s.net_down_bps, s.net_up_bps));
        let net = Row {
            resource: Resource::Network,
            name: "Network",
            detail: if s.is_some() {
                dominant(("Receiving", down), ("Sending", up))
            } else {
                String::new()
            },
            value: s.map_or(NONE.to_string(), |_| rate(down + up)),
            color: DATA_NETWORK,
            values: Cow::Borrowed(self.net_down.view(range)),
            max: peak_ceiling(
                &[self.net_down.view(range), self.net_up.view(range)],
                NET_FLOOR,
            ),
        };

        // The two power figures are never added: the value is the measured one when there is
        // one, otherwise the processor estimate, and the line says which.
        let cpu_w: Vec<f32> = self
            .cpu
            .view(range)
            .iter()
            .map(|l| power::cpu_estimate_w(*l, self.cpu_rated_w))
            .collect();
        let cpu_now = cpu_w.last().copied();
        let gpu_w = g.and_then(|g| g.power_w);
        let (detail, value, values) = match gpu_w {
            Some(w) => (
                format!(
                    "GPU measured, CPU about {} (estimate)",
                    cpu_now.map_or(NONE.to_string(), power::fmt_watts)
                ),
                power::fmt_watts(w),
                Cow::Borrowed(self.gpu_power.view(range)),
            ),
            None => (
                "CPU estimate, no GPU reading".to_string(),
                cpu_now.map_or(NONE.to_string(), power::fmt_watts),
                Cow::Owned(cpu_w),
            ),
        };
        let max = peak_ceiling(&[&values], 50.0);
        let power = Row {
            resource: Resource::Power,
            name: "Power",
            detail,
            value,
            color: if gpu_w.is_some() {
                DATA_GPU
            } else {
                DATA_SECOND
            },
            values,
            max,
        };

        vec![cpu, mem, gpu, disk, net, power]
    }

    /// The list on the left. Clicking a row selects it.
    pub(super) fn resource_list(&mut self, ui: &mut egui::Ui, width: f32) {
        ui.spacing_mut().item_spacing.y = SPACE_XS;
        let capacity = self.range.points();
        let selected = self.selected;
        let clicked = {
            let rows = self.list_rows();
            let mut clicked = None;
            for row in &rows {
                if draw_row(ui, width, row, row.resource == selected, capacity) {
                    clicked = Some(row.resource);
                }
            }
            clicked
        };
        if let Some(r) = clicked {
            self.selected = r;
        }
    }
}

/// "i7-9700K" out of "Intel(R) Core(TM) i7-9700K CPU @ 3.60GHz"; the full name when unsure.
fn short_cpu_name(name: &str) -> String {
    let before_at = name.split('@').next().unwrap_or(name);
    let cleaned: Vec<&str> = before_at
        .split_whitespace()
        .filter(|w| {
            !matches!(
                *w,
                "Intel(R)" | "Core(TM)" | "CPU" | "AMD" | "Processor" | "(R)" | "(TM)"
            )
        })
        .collect();
    if cleaned.is_empty() {
        name.trim().to_string()
    } else {
        cleaned.join(" ")
    }
}

/// Paints one row; true when it was clicked.
fn draw_row(ui: &mut egui::Ui, width: f32, row: &Row, selected: bool, capacity: usize) -> bool {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, ROW_HEIGHT), Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, row.name));
    if !ui.is_rect_visible(rect) {
        return response.clicked();
    }
    let painter = ui.painter_at(rect);
    let fill = if selected {
        SELECTED_BG
    } else if response.hovered() {
        BAR_DIVIDER
    } else {
        Color32::TRANSPARENT
    };
    painter.rect_filled(rect, ROW_RADIUS, fill);
    if response.has_focus() {
        painter.rect_stroke(
            rect,
            ROW_RADIUS,
            ui.visuals().selection.stroke,
            egui::StrokeKind::Inside,
        );
    }

    // The small line.
    let spark = egui::Rect::from_min_size(
        Pos2::new(rect.left() + SPACE_M, rect.center().y - SPARK_H / 2.0),
        Vec2::new(SPARK_W, SPARK_H),
    );
    paint_spark(&painter, spark, &row.values, capacity, row.max, row.color);

    // Name and detail, then the value at the right.
    let name_x = spark.right() + SPACE_M;
    let value = painter.layout_no_wrap(row.value.clone(), FontId::proportional(TEXT_SECTION), TEXT);
    let value_left = rect.right() - SPACE_M - value.size().x;
    painter.text(
        Pos2::new(name_x, rect.top() + 10.0),
        egui::Align2::LEFT_TOP,
        row.name,
        FontId::proportional(TEXT_SECTION),
        TEXT,
    );
    let text_clip = egui::Rect::from_min_max(
        Pos2::new(name_x, rect.top()),
        Pos2::new(value_left - 8.0, rect.bottom()),
    );
    painter.with_clip_rect(text_clip).text(
        Pos2::new(name_x, rect.top() + 30.0),
        egui::Align2::LEFT_TOP,
        &row.detail,
        FontId::proportional(TEXT_BODY),
        MUTED,
    );
    painter.galley(
        Pos2::new(value_left, rect.center().y - value.size().y / 2.0),
        value,
        TEXT,
    );
    response.clicked()
}

/// A tiny line graph; the newest point is at the right edge, like the big graphs.
fn paint_spark(
    painter: &egui::Painter,
    rect: egui::Rect,
    values: &[f32],
    capacity: usize,
    max: f32,
    color: Color32,
) {
    let n = values.len();
    if n < 2 {
        painter.line_segment(
            [rect.left_bottom(), rect.right_bottom()],
            Stroke::new(1.0, BAR_DIVIDER),
        );
        return;
    }
    let step = rect.width() / capacity.saturating_sub(1).max(1) as f32;
    let max = max.max(f32::EPSILON);
    let points: Vec<Pos2> = values
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let x = rect.right() - (n - 1 - i) as f32 * step;
            let y = rect.bottom() - (v / max).clamp(0.0, 1.0) * rect.height();
            Pos2::new(x.max(rect.left()), y)
        })
        .collect();
    painter.add(Shape::line(points, Stroke::new(1.5, color)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_name_drops_filler_words() {
        assert_eq!(
            short_cpu_name("Intel(R) Core(TM) i7-9700K CPU @ 3.60GHz"),
            "i7-9700K"
        );
        assert_eq!(short_cpu_name("CPU"), "CPU");
    }

    #[test]
    fn dominant_names_the_bigger_rate() {
        assert!(dominant(("Reading", 5.0), ("Writing", 2048.0)).starts_with("Writing"));
        assert!(dominant(("Reading", 2048.0), ("Writing", 5.0)).starts_with("Reading"));
        assert!(dominant(("Reading", 0.0), ("Writing", 0.0)).starts_with("Reading"));
    }
}
