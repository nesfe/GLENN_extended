//! Lightweight clipped plots; no external chart runtime or per-frame data solving.
use eframe::egui::{self, Color32, Pos2, Rect, Stroke, Vec2};
use glenn_extended::model::Point;
pub const CYAN: Color32 = Color32::from_rgb(70, 210, 216);
pub const AMBER: Color32 = Color32::from_rgb(246, 185, 86);
pub const MUTED: Color32 = Color32::from_rgb(138, 156, 178);
pub fn plot(
    ui: &mut egui::Ui,
    title: &str,
    x_label: &str,
    y_label: &str,
    series: &[(&str, &[Point], Color32)],
    invert_y: bool,
) {
    draw(ui, title, x_label, y_label, series, invert_y, false)
}
pub fn scatter(ui: &mut egui::Ui, title: &str, x_label: &str, y_label: &str, points: &[Point]) {
    draw(
        ui,
        title,
        x_label,
        y_label,
        &[("Грани стенки", points, CYAN)],
        true,
        true,
    )
}
fn tick(value: f64, interval: f64) -> String {
    if value == 0.0 {
        return "0".into();
    }
    if value.abs() >= 1e5 || interval < 1e-3 {
        return format!("{value:.1e}");
    }
    let decimals = (1.0 - interval.log10().floor()).clamp(0.0, 5.0) as usize;
    format!("{value:.decimals$}")
}

fn draw(
    ui: &mut egui::Ui,
    title: &str,
    x_label: &str,
    y_label: &str,
    series: &[(&str, &[Point], Color32)],
    invert_y: bool,
    scatter: bool,
) {
    ui.label(egui::RichText::new(title).strong().size(16.0));
    ui.horizontal_wrapped(|ui| {
        for (name, _, color) in series {
            ui.colored_label(*color, *name);
        }
    });
    let width = ui.available_width().max(180.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 240.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let plot = Rect::from_min_max(
        rect.min + Vec2::new(54.0, 12.0),
        rect.max - Vec2::new(18.0, 38.0),
    );
    let mut xmin = f64::INFINITY;
    let mut xmax = f64::NEG_INFINITY;
    let mut ymin = f64::INFINITY;
    let mut ymax = f64::NEG_INFINITY;
    for (_, points, _) in series {
        for p in *points {
            if p.x.is_finite() && p.y.is_finite() {
                xmin = xmin.min(p.x);
                xmax = xmax.max(p.x);
                ymin = ymin.min(p.y);
                ymax = ymax.max(p.y);
            }
        }
    }
    if !xmin.is_finite() {
        return;
    }
    let dx = if xmax > xmin {
        xmax - xmin
    } else {
        xmin.abs().max(1.0) * 0.02
    };
    let dy = if ymax > ymin {
        ymax - ymin
    } else {
        ymin.abs().max(1.0) * 0.02
    };
    xmin -= 0.03 * dx;
    xmax += 0.03 * dx;
    ymin -= 0.08 * dy;
    ymax += 0.08 * dy;
    let transform = |p: Point| {
        Pos2::new(
            plot.left() + ((p.x - xmin) / (xmax - xmin)) as f32 * plot.width(),
            if invert_y {
                plot.top() + ((p.y - ymin) / (ymax - ymin)) as f32 * plot.height()
            } else {
                plot.bottom() - ((p.y - ymin) / (ymax - ymin)) as f32 * plot.height()
            },
        )
    };
    for i in 0..=4 {
        let t = i as f32 / 4.0;
        let x = plot.left() + t * plot.width();
        let y = plot.top() + t * plot.height();
        painter.line_segment(
            [Pos2::new(x, plot.top()), Pos2::new(x, plot.bottom())],
            Stroke::new(1.0, Color32::from_gray(44)),
        );
        painter.line_segment(
            [Pos2::new(plot.left(), y), Pos2::new(plot.right(), y)],
            Stroke::new(1.0, Color32::from_gray(44)),
        );
        painter.text(
            Pos2::new(x, plot.bottom() + 8.0),
            egui::Align2::CENTER_TOP,
            tick(xmin + (xmax - xmin) * t as f64, (xmax - xmin) / 4.0),
            egui::FontId::monospace(10.0),
            MUTED,
        );
        let value = if invert_y {
            ymin + (ymax - ymin) * t as f64
        } else {
            ymax - (ymax - ymin) * t as f64
        };
        painter.text(
            Pos2::new(plot.left() - 6.0, y),
            egui::Align2::RIGHT_CENTER,
            tick(value, (ymax - ymin) / 4.0),
            egui::FontId::monospace(10.0),
            MUTED,
        );
    }
    let clipped = painter.with_clip_rect(plot);
    for (_, points, color) in series {
        let points: Vec<Pos2> = points
            .iter()
            .filter(|p| p.x.is_finite() && p.y.is_finite())
            .map(|p| transform(*p))
            .collect();
        if scatter {
            for point in points {
                clipped.circle_filled(point, 1.6, *color);
            }
        } else if points.len() == 1 {
            clipped.circle_filled(points[0], 4.0, *color);
        } else {
            clipped.add(egui::Shape::line(points, Stroke::new(1.8, *color)));
        }
    }
    painter.text(
        Pos2::new(plot.center().x, rect.bottom()),
        egui::Align2::CENTER_BOTTOM,
        x_label,
        egui::FontId::proportional(11.0),
        MUTED,
    );
    painter.text(
        rect.min,
        egui::Align2::LEFT_TOP,
        y_label,
        egui::FontId::proportional(11.0),
        MUTED,
    );
}
