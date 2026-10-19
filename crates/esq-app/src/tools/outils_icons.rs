//! Icons of the tools added from SketchUp's `C…Tool` classes (drawn with vector shapes, in
//! the style of `icons.rs`).

use eframe::egui::{Color32, Painter, Pos2, Rect, Stroke, vec2};

use super::ToolId;

const INK: Color32 = Color32::from_rgb(40, 44, 52);
const ACCENT: Color32 = Color32::from_rgb(214, 96, 44);

pub fn outils_icon(p: &Painter, r: Rect, tool: ToolId) {
    let s = r.width() / 24.0;
    let at = |x: f32, y: f32| -> Pos2 { r.min + vec2(x * s, y * s) };
    let ink = Stroke::new(1.6 * s, INK);
    let acc = Stroke::new(2.0 * s, ACCENT);
    match tool {
        ToolId::Divide => {
            // A line with its division points.
            p.line_segment([at(3.0, 18.0), at(21.0, 6.0)], ink);
            for i in 0..=3 {
                let t = i as f32 / 3.0;
                p.circle_filled(at(3.0 + 18.0 * t, 18.0 - 12.0 * t), 1.6 * s, ACCENT);
            }
        }
        ToolId::ShowFaceEdges => {
            // A face with its outline highlighted.
            let quad = vec![at(4.0, 8.0), at(16.0, 4.0), at(20.0, 16.0), at(8.0, 20.0)];
            p.add(eframe::egui::Shape::convex_polygon(quad.clone(), Color32::from_rgb(220, 226, 236), ink));
            p.add(eframe::egui::Shape::closed_line(quad, acc));
        }
        ToolId::Classifier => {
            // A tag with a hole, as on a label.
            let tag = vec![at(3.0, 12.0), at(10.0, 5.0), at(21.0, 5.0), at(21.0, 19.0), at(10.0, 19.0)];
            p.add(eframe::egui::Shape::convex_polygon(tag, Color32::from_rgb(250, 236, 200), ink));
            p.circle_stroke(at(9.0, 12.0), 1.6 * s, acc);
            p.line_segment([at(13.0, 10.0), at(19.0, 10.0)], Stroke::new(1.2 * s, INK));
            p.line_segment([at(13.0, 14.0), at(18.0, 14.0)], Stroke::new(1.2 * s, INK));
        }
        ToolId::PhotoPoint => {
            // A photo frame and a guide ray going through it.
            p.rect_stroke(
                eframe::egui::Rect::from_min_max(at(8.0, 4.0), at(21.0, 14.0)),
                0.0,
                ink,
                eframe::egui::StrokeKind::Middle,
            );
            p.add(eframe::egui::Shape::dashed_line(&[at(2.0, 22.0), at(18.0, 6.0)], acc, 2.0 * s, 1.5 * s));
            p.circle_filled(at(14.0, 10.0), 1.6 * s, ACCENT);
        }
        ToolId::AddSnap => {
            // Two pieces meeting at a snap: a square with an arrow out of it.
            p.rect_stroke(
                eframe::egui::Rect::from_min_max(at(3.0, 8.0), at(11.0, 16.0)),
                0.0,
                ink,
                eframe::egui::StrokeKind::Middle,
            );
            p.line_segment([at(7.0, 12.0), at(19.0, 12.0)], acc);
            p.add(eframe::egui::Shape::convex_polygon(
                vec![at(22.0, 12.0), at(18.0, 9.0), at(18.0, 15.0)],
                ACCENT,
                Stroke::NONE,
            ));
            p.circle_filled(at(7.0, 12.0), 1.6 * s, INK);
        }
        ToolId::Light => {
            let c = at(12.0, 11.0);
            for i in 0..8 {
                let a = i as f32 / 8.0 * std::f32::consts::TAU;
                let d = vec2(a.cos(), a.sin()) * s;
                p.line_segment([c + d * 6.5, c + d * 9.5], Stroke::new(1.3 * s, ACCENT));
            }
            p.circle(c, 4.5 * s, Color32::from_rgb(255, 230, 120), ink);
            p.line_segment([at(10.0, 19.0), at(14.0, 19.0)], ink);
        }
        ToolId::Igloo => {
            // A dome of photo frames around a point of view.
            for (x, y) in [(3.0, 10.0), (9.0, 4.0), (15.0, 10.0)] {
                p.rect_stroke(
                    eframe::egui::Rect::from_min_max(at(x, y), at(x + 6.0, y + 5.0)),
                    0.0,
                    Stroke::new(1.2 * s, INK),
                    eframe::egui::StrokeKind::Middle,
                );
            }
            p.circle_filled(at(12.0, 20.0), 1.8 * s, ACCENT);
            for (x, y) in [(6.0, 15.0), (12.0, 9.0), (18.0, 15.0)] {
                p.line_segment([at(12.0, 20.0), at(x, y)], Stroke::new(1.0 * s, ACCENT));
            }
        }
        _ => {}
    }
}
