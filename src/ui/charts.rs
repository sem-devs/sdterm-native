//! Line charts for the resource monitors, drawn straight onto the window.

use gpui::prelude::*;
use gpui::{Bounds, Hsla, PathBuilder, Pixels, Window, canvas, fill, point, px, size};

use crate::resources::HISTORY;

pub struct ChartSeries {
    pub values: Vec<Option<f32>>,
    pub color: Hsla,
    /// Fill the area under the line.
    pub area: bool,
}

/// A chart that fills its parent. `max` is the value at the top.
pub fn chart(series: Vec<ChartSeries>, max: f32, grid: Option<Hsla>) -> impl IntoElement {
    canvas(
        |bounds, _, _| bounds,
        move |bounds, _, window, _| paint(window, bounds, &series, max, grid),
    )
    .size_full()
}

fn paint(window: &mut Window, bounds: Bounds<Pixels>, series: &[ChartSeries], max: f32, grid: Option<Hsla>) {
    let width: f32 = bounds.size.width.into();
    let height: f32 = bounds.size.height.into();
    if width <= 2.0 || height <= 2.0 || max <= 0.0 {
        return;
    }
    if let Some(color) = grid {
        for line in 1..4 {
            let y = bounds.origin.y + px(height * line as f32 / 4.0);
            window.paint_quad(fill(
                Bounds { origin: point(bounds.origin.x, y), size: size(bounds.size.width, px(1.0)) },
                color,
            ));
        }
    }
    let step = width / (HISTORY - 1) as f32;
    for s in series {
        let n = s.values.len();
        if n < 2 {
            continue;
        }
        // The newest sample sits at the right edge and the history runs off to the left.
        let x = |i: usize| bounds.origin.x + px(width - (n - 1 - i) as f32 * step);
        let y = |v: f32| bounds.origin.y + px(height - (v / max).clamp(0.0, 1.0) * height);
        let bottom = bounds.origin.y + px(height);

        let points: Vec<_> = s
            .values
            .iter()
            .enumerate()
            .filter_map(|(i, v)| v.map(|v| point(x(i), y(v))))
            .collect();
        if points.len() < 2 {
            continue;
        }

        if s.area {
            let mut area = PathBuilder::fill();
            area.move_to(point(points[0].x, bottom));
            for p in &points {
                area.line_to(*p);
            }
            area.line_to(point(points[points.len() - 1].x, bottom));
            area.close();
            if let Ok(path) = area.build() {
                window.paint_path(path, s.color.opacity(0.16));
            }
        }
        let mut line = PathBuilder::stroke(px(1.5));
        line.move_to(points[0]);
        for p in &points[1..] {
            line.line_to(*p);
        }
        if let Ok(path) = line.build() {
            window.paint_path(path, s.color);
        }
    }
}
