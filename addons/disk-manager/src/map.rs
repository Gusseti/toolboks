use crate::scanner::Node;
use egui::{Color32, Vec2};
const MIN_SIDE: f32 = 48.0;
const HEIGHT: f32 = 300.0;
const GAP: f32 = 5.0;
struct Tile {
    id: usize,
    name: String,
    bytes: u64,
    rect: egui::Rect,
    color: Color32,
}
/// Justified, full-height columns eliminate unused pockets while keeping every entry selectable.
pub struct MapLayout {
    tiles: Vec<Tile>,
    right_edges: Vec<f32>,
    width: f32,
}
impl MapLayout {
    pub fn new(nodes: &[&Node]) -> Self {
        let largest = nodes.iter().map(|n| n.bytes).max().unwrap_or(0).max(1) as f64;
        let mut nodes = nodes.to_vec();
        nodes.sort_by_key(|n| std::cmp::Reverse(n.bytes));
        let areas: Vec<f32> = nodes
            .iter()
            .map(|n| ((n.bytes as f64 / largest * 80_000.0) as f32).max(MIN_SIDE * MIN_SIDE))
            .collect();
        let mut tiles = Vec::with_capacity(nodes.len());
        let mut width = 0.0_f32;
        let mut start = 0;
        while start < nodes.len() {
            let mut end = start + 1;
            let mut column = column_geometry(&areas[start..end]);
            // Grow a full-height column while doing so improves rectangle shapes.
            while end < nodes.len() && end - start < 5 {
                let candidate = column_geometry(&areas[start..=end]);
                if candidate.2 > column.2 {
                    break;
                }
                end += 1;
                column = candidate;
            }
            let (column_width, heights, _) = column;
            let mut y = 0.0;
            for (offset, height) in heights.into_iter().enumerate() {
                let i = start + offset;
                let n = nodes[i];
                let rect = egui::Rect::from_min_size(
                    egui::pos2(width, y),
                    Vec2::new(column_width, height),
                );
                tiles.push(Tile {
                    id: n.id,
                    name: n.name.clone(),
                    bytes: n.bytes,
                    rect,
                    color: crate::theme::DATA[3].gamma_multiply([0.28, 0.22, 0.18, 0.15][i % 4]),
                });
                y += height + GAP;
            }
            width += column_width + GAP;
            start = end;
        }
        width = (width - GAP).max(0.0);
        tiles.sort_by(|a, b| a.rect.left().total_cmp(&b.rect.left()));
        let mut right = 0.0_f32;
        let right_edges = tiles
            .iter()
            .map(|t| {
                right = right.max(t.rect.right());
                right
            })
            .collect();
        Self {
            tiles,
            right_edges,
            width,
        }
    }
}
// Stretch tiles within each column to fill its height, respecting the click-size floor.
fn column_geometry(areas: &[f32]) -> (f32, Vec<f32>, f32) {
    let available = HEIGHT - GAP * (areas.len() - 1) as f32;
    let total: f32 = areas.iter().sum();
    let width = (total / available).max(MIN_SIDE);
    let mut heights = vec![0.0; areas.len()];
    let mut remaining = available;
    let mut weight = total;
    loop {
        let mut fixed = false;
        for (i, area) in areas.iter().enumerate() {
            if heights[i] == 0.0 && remaining * area / weight < MIN_SIDE {
                heights[i] = MIN_SIDE;
                remaining -= MIN_SIDE;
                weight -= area;
                fixed = true;
            }
        }
        if !fixed {
            break;
        }
    }
    for (i, area) in areas.iter().enumerate() {
        if heights[i] == 0.0 {
            heights[i] = remaining * area / weight;
        }
    }
    let worst = heights
        .iter()
        .map(|h| (width / h).max(h / width))
        .fold(1.0_f32, f32::max);
    (width, heights, worst)
}
pub fn show(ui: &mut egui::Ui, layout: &MapLayout) -> Option<usize> {
    if layout.tiles.is_empty() {
        crate::theme::weak(ui, "This folder is empty.");
        return None;
    }
    let mut clicked = None;
    let scale = (ui.available_width() / layout.width).max(1.0);
    egui::ScrollArea::horizontal()
        .id_salt("disk-map-packed")
        .auto_shrink([false, true])
        .show_viewport(ui, |ui, viewport| {
            let (r, _) = ui.allocate_exact_size(
                Vec2::new(layout.width * scale, HEIGHT),
                egui::Sense::hover(),
            );
            let first = layout
                .right_edges
                .partition_point(|right| *right * scale < viewport.left());
            for tile in layout.tiles[first..]
                .iter()
                .take_while(|t| t.rect.left() * scale <= viewport.right())
            {
                let rect = egui::Rect::from_min_max(
                    r.min + Vec2::new(tile.rect.left() * scale, tile.rect.top()),
                    r.min + Vec2::new(tile.rect.right() * scale, tile.rect.bottom()),
                );
                if !ui.is_rect_visible(rect) {
                    continue;
                }
                let response = ui.interact(
                    rect,
                    ui.scope_id().with(("disk-map", tile.id)),
                    egui::Sense::click(),
                );
                ui.painter().rect_filled(rect, 4, crate::theme::WINDOW);
                ui.painter().rect_filled(rect, 4, tile.color);
                if response.hovered() || response.has_focus() {
                    ui.painter().rect_stroke(
                        rect,
                        4,
                        egui::Stroke::new(
                            1.5,
                            if response.has_focus() {
                                crate::theme::ACCENT
                            } else {
                                crate::theme::TEXT
                            },
                        ),
                        egui::StrokeKind::Inside,
                    );
                }
                let painter = ui.painter().with_clip_rect(rect.shrink(5.0));
                painter.text(
                    rect.min + Vec2::new(7.0, 7.0),
                    egui::Align2::LEFT_TOP,
                    &tile.name,
                    egui::FontId::proportional(12.0),
                    crate::theme::TEXT,
                );
                painter.text(
                    rect.min + Vec2::new(7.0, 26.0),
                    egui::Align2::LEFT_TOP,
                    crate::size(tile.bytes),
                    egui::FontId::proportional(11.0),
                    crate::theme::TEXT,
                );
                if response.clicked() {
                    clicked = Some(tile.id);
                }
                let label = format!("{} · {}", tile.name, crate::size(tile.bytes));
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), &label)
                });
                response.on_hover_text(label);
            }
        });
    ui.add_space(5.0);
    let note = if scale == 1.0 && layout.width > ui.available_width() {
        "Scroll sideways for more. Small entries are enlarged. Labels show actual sizes."
    } else {
        "Open a folder from the map. Small entries are enlarged. Labels show actual sizes."
    };
    crate::theme::caption(ui, note);
    clicked
}
#[cfg(test)]
mod tests {
    use super::*;
    fn node(id: usize, bytes: u64) -> Node {
        Node {
            id,
            parent: Some(0),
            name: format!("entry-{id}"),
            path: String::new(),
            dir: false,
            bytes,
            files: 1,
            modified: 0,
        }
    }
    #[test]
    fn packed_tiles_are_clickable_bounded_and_non_overlapping() {
        let nodes = (0..120)
            .map(|i| node(i, if i == 0 { 1_000_000_000_000 } else { i as u64 }))
            .collect::<Vec<_>>();
        let layout = MapLayout::new(&nodes.iter().collect::<Vec<_>>());
        for (i, t) in layout.tiles.iter().enumerate() {
            assert!(t.rect.width() >= MIN_SIDE - 0.01 && t.rect.height() >= MIN_SIDE - 0.01);
            assert!(t.rect.top() >= 0.0 && t.rect.bottom() <= HEIGHT + 0.01);
            for other in &layout.tiles[i + 1..] {
                assert!(!t.rect.intersects(other.rect));
            }
        }
        assert!(layout.tiles.iter().any(|t| t.rect.top() > 0.0));
        assert!(layout.width < 120.0 * MIN_SIDE / 2.0);
    }
    #[test]
    fn area_ratios_are_preserved_above_the_size_floor() {
        let nodes = [node(1, 75), node(2, 25)];
        let layout = MapLayout::new(&nodes.iter().collect::<Vec<_>>());
        let area = |id| {
            layout
                .tiles
                .iter()
                .find(|t| t.id == id)
                .unwrap()
                .rect
                .area()
        };
        assert!((area(1) / area(2) - 3.0).abs() < 0.001);
    }
    #[test]
    fn columns_fill_the_entire_map_without_pockets() {
        for count in [1, 2, 6, 37, 120] {
            let nodes = (0..count)
                .map(|i| node(i, ((i * 7919) % 10007) as u64))
                .collect::<Vec<_>>();
            let layout = MapLayout::new(&nodes.iter().collect::<Vec<_>>());
            let mut x = 0.0;
            while x < layout.width - 0.01 {
                let mut column: Vec<_> = layout
                    .tiles
                    .iter()
                    .filter(|t| (t.rect.left() - x).abs() < 0.01)
                    .collect();
                assert!(!column.is_empty());
                column.sort_by(|a, b| a.rect.top().total_cmp(&b.rect.top()));
                let mut y = 0.0;
                let right = column[0].rect.right();
                for tile in column {
                    assert!((tile.rect.top() - y).abs() < 0.01);
                    assert!((tile.rect.right() - right).abs() < 0.01);
                    assert!(
                        tile.rect.width() >= MIN_SIDE - 0.01
                            && tile.rect.height() >= MIN_SIDE - 0.01
                    );
                    y = tile.rect.bottom() + GAP;
                }
                assert!((y - GAP - HEIGHT).abs() < 0.01);
                x = right + GAP;
            }
            assert!((x - GAP - layout.width).abs() < 0.01);
        }
    }
    #[test]
    fn all_entries_including_empty_ones_remain_accessible() {
        let nodes = (0..1200).map(|i| node(i, 0)).collect::<Vec<_>>();
        let layout = MapLayout::new(&nodes.iter().collect::<Vec<_>>());
        assert_eq!(layout.tiles.len(), 1200);
        let mut ids = layout.tiles.iter().map(|t| t.id).collect::<Vec<_>>();
        ids.sort_unstable();
        assert_eq!(ids, (0..1200).collect::<Vec<_>>());
    }
}
